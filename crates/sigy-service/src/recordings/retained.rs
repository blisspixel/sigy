//! Cooperative encoded reads. Protection outlives a blocked filesystem operation.
//!
//! Hashing finishes before any bytes are sent. Both passes use one opened handle;
//! the second hash detects subsequent mutation but cannot retract delivered bytes.
//! Cancellation and deadlines request a stop, then wait for actual close and join.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, watch},
};

use crate::{
    Error, Result,
    storage::{
        dvr::hex,
        retained_readers::{
            MAX_RETAINED_READ_BYTES, RETAINED_READ_DEADLINE_SECONDS, RetainedReadSpec,
        },
    },
};

use super::pipe::{ListenPipe, bind_listen_pipe};

const DEADLINE: Duration = Duration::from_secs(RETAINED_READ_DEADLINE_SECONDS);
const CHUNK_BYTES: usize = 16 * 1024;
const QUEUED_CHUNKS: usize = 2;

/// Only this worker can construct proof that its original-file reader finished.
/// A transport completion is separate from client decoding or audible completion.
#[derive(Debug)]
pub(crate) struct RetainedReadReceipt {
    spec: RetainedReadSpec,
    reason: &'static str,
}

impl RetainedReadReceipt {
    pub(crate) fn id(&self) -> &str {
        &self.spec.request_id
    }
    pub(crate) const fn generation(&self) -> u64 {
        self.spec.generation
    }
    pub(crate) const fn reason(&self) -> &str {
        self.reason
    }
    pub(crate) fn successful(&self) -> bool {
        self.reason == "completed"
    }
    pub(crate) fn matches(&self, spec: &RetainedReadSpec) -> bool {
        self.spec == *spec
    }
}

struct ReadCompletion(Result<()>);

struct StopReader(watch::Sender<bool>);

impl Drop for StopReader {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

pub(crate) async fn stream_retained<F, Fut>(
    directory: PathBuf,
    spec: RetainedReadSpec,
    ownership: Arc<File>,
    mut signal: watch::Receiver<bool>,
    ready: F,
) -> Result<RetainedReadReceipt>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let began = Instant::now();
    if let Err(error) = validate(&spec) {
        return Ok(receipt(spec, &Err(error)));
    }
    let prepared = prepare(&directory, &mut signal, ready).await;
    let pipe = match prepared {
        Ok(pipe) => pipe,
        Err(error) => return Ok(receipt(spec, &Err(error))),
    };
    let (sender, receiver) = mpsc::channel(QUEUED_CHUNKS);
    let (reader_stop, reader_signal) = watch::channel(*signal.borrow());
    let read_spec = spec.clone();
    let read = crate::processing::supervise(ownership, reader_signal, move |stop| {
        // Even failure carries close proof only after this closure has returned.
        Ok(ReadCompletion(read_file(
            &directory, &read_spec, &sender, stop, began,
        )))
    });
    let transfer = transfer(pipe, receiver, reader_stop, &mut signal, began);
    let result = join_transfer(read, transfer).await?;
    Ok(receipt(spec, &result))
}

async fn join_transfer<R, T>(read: R, transfer: T) -> Result<Result<()>>
where
    R: std::future::Future<Output = Result<ReadCompletion>>,
    T: std::future::Future<Output = Result<()>>,
{
    let mut read = Box::pin(read);
    let mut transfer = Box::pin(transfer);
    tokio::select! {
        joined = &mut read => {
            // A failed joined reader needs no client. Closing the transport also
            // closes its bounded receiver; no detached reader is introduced.
            let ReadCompletion(result) = joined?;
            if result.is_err() { drop(transfer); return Ok(result); }
            Ok(result.and(transfer.await))
        }
        transferred = &mut transfer => {
            drop(transfer);
            // A panic or unknown join has no release capability. Stop requests
            // never replace waiting for actual file closure.
            let ReadCompletion(result) = read.await?;
            Ok(result.and(transferred))
        }
    }
}

async fn prepare<F, Fut>(
    directory: &Path,
    signal: &mut watch::Receiver<bool>,
    ready: F,
) -> Result<ListenPipe>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    if *signal.borrow() {
        return Err(Error::Analysis("cancelled"));
    }
    let pipe = bind_listen_pipe(directory)?;
    tokio::select! {
        biased;
        _ = signal.changed() => return Err(Error::Analysis("cancelled")),
        result = tokio::time::timeout(DEADLINE, ready(pipe.nonce().to_owned())) => {
            result.map_err(|_| Error::Analysis("deadline"))??;
        }
    }
    Ok(pipe)
}

async fn transfer(
    pipe: ListenPipe,
    mut receiver: mpsc::Receiver<Vec<u8>>,
    reader_stop: watch::Sender<bool>,
    signal: &mut watch::Receiver<bool>,
    began: Instant,
) -> Result<()> {
    let _stop = StopReader(reader_stop);
    let mut cancellation = signal.clone();
    if *cancellation.borrow() {
        return Err(Error::Analysis("cancelled"));
    }
    let send = async {
        let mut writer = pipe.accept(signal).await?;
        while let Some(chunk) = receiver.recv().await {
            writer.write_all(&chunk).await?;
        }
        // LocalStream closes here without a detached native flush owner.
        Ok(())
    };
    tokio::select! {
        biased;
        _ = cancellation.changed() => Err(Error::Analysis("cancelled")),
        result = tokio::time::timeout(DEADLINE.saturating_sub(began.elapsed()), send) => {
            result.map_err(|_| Error::Analysis("deadline"))?
        }
    }
}

fn validate(spec: &RetainedReadSpec) -> Result<()> {
    crate::storage::dvr::validate_object_key(&spec.object_key)?;
    let hash_valid = |hash: &str| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    if spec.generation != 1
        || !(1..=MAX_RETAINED_READ_BYTES).contains(&spec.bytes)
        || !hash_valid(&spec.sha256)
        || !hash_valid(&spec.spec_sha256)
        || !crate::storage::dvr::retained_format(&spec.format)
        || spec.timeline_end_us.checked_sub(spec.timeline_start_us) != Some(spec.file_duration_us)
        || spec.file_duration_us > RETAINED_READ_DEADLINE_SECONDS * 1_000_000
        || spec.file_seek_us >= spec.file_duration_us
        || spec.playback_duration_us().is_err()
    {
        return Err(Error::Analysis("retained-read-spec-invalid"));
    }
    Ok(())
}

fn read_file(
    directory: &Path,
    spec: &RetainedReadSpec,
    sender: &mpsc::Sender<Vec<u8>>,
    stop: &AtomicBool,
    began: Instant,
) -> Result<()> {
    check_stop(stop, began)?;
    let path = super::media_path(directory, &spec.object_key)?;
    crate::library::reject_link(&path)?;
    let mut file = File::open(path).map_err(|_| Error::Analysis("input-unavailable"))?;
    check_size(&file, spec.bytes)?;
    let mut buffer = [0_u8; CHUNK_BYTES];
    let observed = read_pass(&mut file, spec.bytes, &mut buffer, stop, began, None)?;
    check_size(&file, spec.bytes)?;
    if observed != spec.sha256 {
        return Err(Error::Analysis("input-checksum-mismatch"));
    }
    check_stop(stop, began)?;
    file.seek(SeekFrom::Start(0))?;
    let streamed = read_pass(
        &mut file,
        spec.bytes,
        &mut buffer,
        stop,
        began,
        Some(sender),
    )?;
    check_size(&file, spec.bytes)?;
    if streamed != observed {
        return Err(Error::Analysis("input-checksum-mismatch"));
    }
    check_stop(stop, began)
}

fn read_pass(
    file: &mut File,
    bytes: u64,
    buffer: &mut [u8],
    stop: &AtomicBool,
    began: Instant,
    sender: Option<&mpsc::Sender<Vec<u8>>>,
) -> Result<String> {
    let mut remaining = bytes;
    let mut hasher = Sha256::new();
    while remaining != 0 {
        check_stop(stop, began)?;
        let capacity = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| Error::Analysis("retained-read-spec-invalid"))?;
        let read = file.read(&mut buffer[..capacity])?;
        if read == 0 {
            return Err(Error::Analysis("input-size-mismatch"));
        }
        hasher.update(&buffer[..read]);
        if let Some(sender) = sender {
            sender
                .blocking_send(buffer[..read].to_vec())
                .map_err(|_| Error::Analysis("retained-pipe-failed"))?;
        }
        remaining -= read as u64;
    }
    check_stop(stop, began)?;
    Ok(hex(&hasher.finalize()))
}

fn check_size(file: &File, bytes: u64) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != bytes {
        return Err(Error::Analysis("input-size-mismatch"));
    }
    Ok(())
}

fn check_stop(stop: &AtomicBool, began: Instant) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Err(Error::Analysis("cancelled"));
    }
    if began.elapsed() >= DEADLINE {
        return Err(Error::Analysis("deadline"));
    }
    Ok(())
}

fn receipt(spec: RetainedReadSpec, result: &Result<()>) -> RetainedReadReceipt {
    let reason = match result {
        Ok(()) => "completed",
        Err(Error::Analysis(reason)) => reason,
        Err(_) => "retained-read-failed",
    };
    RetainedReadReceipt { spec, reason }
}

#[cfg(test)]
mod tests;
