//! Concurrent encoded input, checked PCM output and the existing progress protocol.

use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::mpsc,
};

use super::{ProgressLimits, copy_range, read_progress, require_range_progress, timestamp};
use crate::{
    Error, Result,
    recordings::{
        RetainedPlaybackReport,
        audio::{PcmDecoded, PcmReaderRequest},
    },
};

pub(in crate::recordings) async fn decode_pcm(
    request: PcmReaderRequest<'_>,
    input: impl AsyncRead + Unpin,
    output: mpsc::Sender<Vec<u8>>,
) -> Result<PcmDecoded> {
    let spec = request.spec;
    let duration_us = spec.file_duration_us - spec.file_seek_us;
    let maximum_pcm = request.format.maximum_bytes(duration_us)?;
    let work_seconds = spec.file_duration_us / 1_000_000 + 30;
    let limits = ProgressLimits::playback(duration_us, work_seconds * 1_000_000)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(work_seconds);
    let mut child = request.group.spawn(command(request))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or(Error::Acquisition("PCM decoder stdin missing"))?;
    let pcm = child
        .stdout
        .take()
        .ok_or(Error::Acquisition("PCM decoder stdout missing"))?;
    let progress = child
        .stderr
        .take()
        .ok_or(Error::Acquisition("PCM decoder progress missing"))?;
    let work = async {
        let pump = async {
            let result = copy_range(input, &mut stdin, spec.bytes).await;
            drop(stdin);
            result
        };
        let ((), pcm_bytes, progress) = tokio::try_join!(
            pump,
            copy_pcm(pcm, output, maximum_pcm),
            read_progress(progress, limits),
        )?;
        let status = child.wait().await?;
        if !status.success() || !progress.finished || progress.max_out_us == 0 {
            return Err(Error::Acquisition("PCM decoder did not complete"));
        }
        require_range_progress(&progress, duration_us)?;
        if pcm_bytes % (u64::from(request.format.channels) * 4) != 0 {
            return Err(Error::Acquisition("PCM decoder ended inside a frame"));
        }
        Ok(PcmDecoded {
            decoder: RetainedPlaybackReport {
                file_playhead_us: spec
                    .file_seek_us
                    .checked_add(progress.max_out_us)
                    .ok_or(Error::Acquisition("PCM progress overflow"))?,
                reported_elapsed_us: progress.max_out_us,
                boundary_tolerance_us: super::PROGRESS_TOLERANCE_US,
                progress_advanced: progress.advanced,
            },
            pcm_bytes,
        })
    };
    let result = tokio::time::timeout_at(deadline, work)
        .await
        .unwrap_or(Err(Error::Acquisition("PCM decoder deadline exceeded")));
    if result.is_err() {
        let _ = request.group.kill();
        request.group.finish().await?;
    }
    result
}

async fn copy_pcm(
    mut input: impl AsyncRead + Unpin,
    output: mpsc::Sender<Vec<u8>>,
    maximum: u64,
) -> Result<u64> {
    let mut chunk = [0_u8; 8192];
    let mut total = 0_u64;
    loop {
        let read = input.read(&mut chunk).await?;
        if read == 0 {
            return if total == 0 {
                Err(Error::Acquisition("PCM decoder emitted no samples"))
            } else {
                Ok(total)
            };
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| Error::Acquisition("PCM byte limit"))?)
            .filter(|bytes| *bytes <= maximum)
            .ok_or(Error::Acquisition("PCM byte limit"))?;
        output
            .send(chunk[..read].to_vec())
            .await
            .map_err(|_| Error::Acquisition("PCM consumer stopped"))?;
    }
}

fn command(request: PcmReaderRequest<'_>) -> tokio::process::Command {
    let spec = request.spec;
    let mut command = tokio::process::Command::new(request.executable);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "quiet",
            "-nostats",
            "-xerror",
            "-max_alloc",
            "16777216",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-protocol_whitelist",
            "pipe",
            "-stats_period",
            "0.1",
            "-f",
            &spec.format,
            "-i",
            "pipe:0",
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-threads",
            "1",
            "-progress",
            "pipe:2",
            "-ss",
            &timestamp(spec.file_seek_us),
            "-t",
            &timestamp(spec.file_duration_us - spec.file_seek_us),
            "-ar",
            &request.format.rate_hz.to_string(),
            "-ac",
            &request.format.channels.to_string(),
            "-acodec",
            "pcm_f32le",
            "-f",
            "f32le",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
