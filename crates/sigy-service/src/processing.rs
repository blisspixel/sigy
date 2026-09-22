//! Bounded local input preparation. No decoder, recognizer, or source URL is dispatched here.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::{Error, Result, storage::dvr::hex};

pub(crate) const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_INPUT_FILES: usize = 1024;
pub(crate) const VERIFY_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct InputFile {
    pub key: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VerificationInput {
    pub files: Vec<InputFile>,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VerificationReceipt {
    pub bytes: u64,
    pub files: usize,
    pub manifest_sha256: String,
}

pub(crate) fn manifest_digest(files: &[InputFile]) -> Result<String> {
    let bytes = serde_json::to_vec(files)?;
    if bytes.len() > 65_536 {
        return Err(Error::Analysis("input-manifest-limit"));
    }
    Ok(hex(&Sha256::digest(bytes)))
}

struct StopOnDrop(Arc<AtomicBool>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) async fn verify(
    directory: PathBuf,
    input: VerificationInput,
    ownership: Arc<File>,
    signal: watch::Receiver<bool>,
) -> Result<VerificationReceipt> {
    let began = Instant::now();
    supervise(ownership, signal, move |stop| {
        verify_files(&directory, &input, stop, began, VERIFY_DEADLINE)
    })
    .await
}

async fn supervise<F>(
    ownership: Arc<File>,
    mut signal: watch::Receiver<bool>,
    work: F,
) -> Result<VerificationReceipt>
where
    F: FnOnce(&AtomicBool) -> Result<VerificationReceipt> + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(*signal.borrow()));
    let guard = StopOnDrop(Arc::clone(&stop));
    let mut reader = tokio::task::spawn_blocking(move || {
        // Aborting the supervising future cannot abort a blocking filesystem read.
        // This guard keeps the library locked until that read actually returns.
        let _ownership = ownership;
        work(&stop)
    });
    let joined = tokio::select! {
        joined = &mut reader => joined,
        _ = signal.changed() => {
            guard.0.store(true, Ordering::Release);
            reader.await
        }
    };
    joined.unwrap_or(Err(Error::Analysis("worker-panicked")))
}

pub(crate) fn verify_files(
    directory: &Path,
    input: &VerificationInput,
    stop: &AtomicBool,
    began: Instant,
    deadline: Duration,
) -> Result<VerificationReceipt> {
    if input.files.is_empty()
        || input.files.len() > MAX_INPUT_FILES
        || input.bytes == 0
        || input.bytes > MAX_INPUT_BYTES
    {
        return Err(Error::Analysis("input-limit"));
    }
    manifest_digest(&input.files)?;
    let mut verified = 0_u64;
    let mut observed = Vec::new();
    let mut buffer = vec![0; 64 * 1024].into_boxed_slice();
    for input_file in &input.files {
        check_stop(stop, began, deadline)?;
        if input_file.bytes == 0 || input_file.bytes > input.bytes.saturating_sub(verified) {
            return Err(Error::Analysis("input-limit"));
        }
        let path = crate::recordings::media_path(directory, &input_file.key)?;
        let sha256 = hash_file(&path, input_file, &mut buffer, stop, began, deadline)?;
        observed.push(InputFile {
            key: input_file.key.clone(),
            bytes: input_file.bytes,
            sha256,
        });
        verified += input_file.bytes;
    }
    check_stop(stop, began, deadline)?;
    if verified != input.bytes {
        return Err(Error::Analysis("input-limit"));
    }
    Ok(VerificationReceipt {
        bytes: verified,
        files: observed.len(),
        manifest_sha256: manifest_digest(&observed)?,
    })
}

fn hash_file(
    path: &Path,
    input: &InputFile,
    buffer: &mut [u8],
    stop: &AtomicBool,
    began: Instant,
    deadline: Duration,
) -> Result<String> {
    crate::library::reject_link(path)?;
    let mut file = File::open(path).map_err(|_| Error::Analysis("input-unavailable"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != input.bytes {
        return Err(Error::Analysis("input-size-mismatch"));
    }
    let mut hasher = Sha256::new();
    let mut remaining = input.bytes;
    while remaining != 0 {
        check_stop(stop, began, deadline)?;
        let capacity = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| Error::Analysis("input-limit"))?;
        let read = file.read(&mut buffer[..capacity])?;
        if read == 0 {
            return Err(Error::Analysis("input-size-mismatch"));
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    check_stop(stop, began, deadline)?;
    let actual = hex(&hasher.finalize());
    if file.metadata()?.len() != input.bytes || actual != input.sha256 {
        return Err(Error::Analysis("input-checksum-mismatch"));
    }
    Ok(actual)
}

fn check_stop(stop: &AtomicBool, began: Instant, deadline: Duration) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Err(Error::Analysis("cancelled"));
    }
    if began.elapsed() >= deadline {
        return Err(Error::Analysis("deadline"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
