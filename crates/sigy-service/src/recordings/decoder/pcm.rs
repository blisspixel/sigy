//! Concurrent encoded input, checked PCM output and the existing progress protocol.

#[cfg(test)]
mod tests;

use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::mpsc,
};

use super::progress::Progress;
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
    let duration_us = spec.playback_duration_us()?;
    let frames = excerpt_frames(request)?;
    let maximum_pcm = frames.map_or_else(
        || request.format.maximum_bytes(duration_us),
        |(first, after)| frame_bytes(request, after - first),
    )?;
    let work_seconds = spec.file_duration_us / 1_000_000 + 30;
    let limits = ProgressLimits::playback(duration_us, work_seconds * 1_000_000)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(work_seconds);
    let mut child = request.group.spawn(command(request, frames))?;
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
            copy_pcm(pcm, output, maximum_pcm, frames.is_some()),
            read_progress(progress, limits),
        )?;
        let status = child.wait().await?;
        if !status.success() || !progress.finished || progress.max_out_us == 0 {
            return Err(Error::Acquisition("PCM decoder did not complete"));
        }
        completion(request, frames, pcm_bytes, &progress)
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

fn completion(
    request: PcmReaderRequest<'_>,
    frames: Option<(u64, u64)>,
    pcm_bytes: u64,
    progress: &Progress,
) -> Result<PcmDecoded> {
    if !pcm_bytes.is_multiple_of(u64::from(request.format.channels) * 4) {
        return Err(Error::Acquisition("PCM decoder ended inside a frame"));
    }
    if let Some((first, after)) = frames {
        if pcm_bytes != frame_bytes(request, after - first)? {
            return Err(Error::Acquisition(
                "PCM excerpt ended before requested samples",
            ));
        }
        let scaled = (after - first)
            .checked_mul(1_000_000)
            .ok_or(Error::Acquisition("PCM excerpt clock overflow"))?;
        let rate = u64::from(request.format.rate_hz);
        if !(scaled / rate..=scaled.div_ceil(rate)).contains(&progress.max_out_us) {
            return Err(Error::Acquisition(
                "PCM excerpt progress contradicts sample count",
            ));
        }
    } else {
        require_range_progress(progress, request.spec.playback_duration_us()?)?;
    }
    Ok(PcmDecoded {
        decoder: RetainedPlaybackReport {
            file_playhead_us: request
                .spec
                .file_seek_us
                .checked_add(progress.max_out_us)
                .ok_or(Error::Acquisition("PCM progress overflow"))?,
            reported_elapsed_us: progress.max_out_us,
            boundary_tolerance_us: if frames.is_some() {
                1_000_000_u64.div_ceil(u64::from(request.format.rate_hz))
            } else {
                super::PROGRESS_TOLERANCE_US
            },
            progress_advanced: progress.advanced,
        },
        pcm_bytes,
    })
}

pub(super) async fn copy_pcm(
    mut input: impl AsyncRead + Unpin,
    output: mpsc::Sender<Vec<u8>>,
    maximum: u64,
    finite: bool,
) -> Result<u64> {
    let mut chunk = [0_u8; 8192];
    let mut total = 0_u64;
    let mut samples = Samples::default();
    loop {
        let read = input.read(&mut chunk).await?;
        if read == 0 {
            return if total == 0 || (finite && samples.used != 0) {
                Err(Error::Acquisition("PCM decoder emitted no samples"))
            } else {
                Ok(total)
            };
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| Error::Acquisition("PCM byte limit"))?)
            .filter(|bytes| *bytes <= maximum)
            .ok_or(Error::Acquisition("PCM byte limit"))?;
        if finite {
            samples.check(&chunk[..read])?;
        }
        output
            .send(chunk[..read].to_vec())
            .await
            .map_err(|_| Error::Acquisition("PCM consumer stopped"))?;
    }
}

#[derive(Default)]
struct Samples {
    bytes: [u8; 4],
    used: usize,
}

impl Samples {
    fn check(&mut self, bytes: &[u8]) -> Result<()> {
        for byte in bytes {
            self.bytes[self.used] = *byte;
            self.used += 1;
            if self.used == 4 {
                if !f32::from_le_bytes(self.bytes).is_finite() {
                    return Err(Error::Acquisition("PCM excerpt contains nonfinite samples"));
                }
                self.used = 0;
            }
        }
        Ok(())
    }
}

fn excerpt_frames(request: PcmReaderRequest<'_>) -> Result<Option<(u64, u64)>> {
    request
        .spec
        .excerpt
        .as_ref()
        .map(|_| {
            request
                .format
                .excerpt_frames(request.spec.file_seek_us, request.spec.playback_end_us()?)
        })
        .transpose()
}

fn frame_bytes(request: PcmReaderRequest<'_>, frames: u64) -> Result<u64> {
    frames
        .checked_mul(u64::from(request.format.channels) * 4)
        .ok_or(Error::Acquisition("PCM excerpt byte overflow"))
}

fn command(request: PcmReaderRequest<'_>, frames: Option<(u64, u64)>) -> tokio::process::Command {
    let spec = request.spec;
    let mut command = tokio::process::Command::new(request.executable);
    command.args([
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
    ]);
    if let Some((first, after)) = frames {
        command.args(["-af", &format!(
            "aresample={}:async=0,atrim=start_sample={first}:end_sample={after},asetpts=PTS-STARTPTS",
            request.format.rate_hz,
        )]);
    } else {
        command.args([
            "-ss",
            &timestamp(spec.file_seek_us),
            "-t",
            &timestamp(spec.file_duration_us - spec.file_seek_us),
        ]);
    }
    command
        .args([
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
