//! Bounded, fail-closed progress observations. They are not sample-accuracy evidence.

use crate::{Error, Result};
use tokio::io::{AsyncRead, AsyncReadExt};

const MAX_LINE: usize = 4096;
const MAX_FRAME: usize = 8192;
const MAX_TOTAL: usize = 8 * 1024 * 1024;
const TIMELINE_STEP_US: u64 = 1_000_000;

tokio::task_local! {
    static DECODER_TIMELINE: tokio::sync::mpsc::Sender<u64>;
}

/// Observe decoder `out_time_us` while `future` runs on this task.
#[must_use]
pub(super) async fn with_timeline<F: std::future::Future>(
    sender: tokio::sync::mpsc::Sender<u64>,
    future: F,
) -> F::Output {
    DECODER_TIMELINE.scope(sender, future).await
}

/// 100 ms playback cadence over the whole wall deadline plus four boundary
/// frames, or 500 ms verification cadence over its 30-second wall deadline.
/// The output media clock is independent of these wall-work limits.
#[derive(Clone, Copy)]
pub(super) struct Limits {
    maximum_us: u64,
    frames: u64,
    total_bytes: usize,
}

impl Limits {
    pub(super) fn playback(duration_us: u64, work_duration_us: u64) -> Result<Self> {
        if duration_us == 0
            || duration_us > 30 * 60 * 1_000_000
            || work_duration_us < duration_us
            || work_duration_us > (30 * 60 + 30) * 1_000_000
        {
            return Err(Error::Acquisition(
                "invalid decoder progress duration bound",
            ));
        }
        let maximum_us = duration_us
            .checked_add(super::PROGRESS_TOLERANCE_US)
            .ok_or(Error::Acquisition("decoder progress bound overflow"))?;
        let frames = work_duration_us / 100_000 + 4;
        Ok(Self::new(maximum_us, frames))
    }

    pub(super) fn verification() -> Self {
        Self::new(u64::MAX, 30 * 2 + 4)
    }

    fn new(maximum_us: u64, frames: u64) -> Self {
        let total_bytes = usize::try_from(frames)
            .ok()
            .and_then(|frames| frames.checked_mul(MAX_FRAME))
            .unwrap_or(MAX_TOTAL)
            .min(MAX_TOTAL);
        Self {
            maximum_us,
            frames,
            total_bytes,
        }
    }

    #[cfg(test)]
    pub(super) fn exact(maximum_us: u64, frames: u64) -> Self {
        Self::new(maximum_us, frames)
    }
}

#[derive(Debug, Default)]
pub(super) struct Progress {
    pub(super) max_out_us: u64,
    pub(super) advanced: bool,
    pub(super) finished: bool,
    previous: Option<u64>,
    frame_time: Option<Observation>,
    last_reported: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Observation {
    Unknown,
    Measured(u64),
}

impl Progress {
    fn line(&mut self, bytes: &[u8], maximum_us: u64) -> Result<()> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Acquisition("invalid decoder progress"))?;
        let text = text.strip_suffix('\r').unwrap_or(text);
        let (key, value) = text
            .split_once('=')
            .filter(|(key, _)| {
                !key.is_empty()
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
            .ok_or(Error::Acquisition("invalid decoder progress"))?;
        if self.finished || value.chars().any(char::is_control) {
            return Err(Error::Acquisition("invalid decoder progress"));
        }
        match key {
            "out_time_us" => self.time(value, maximum_us),
            "progress" => {
                let observation = self.frame_time.take();
                if observation.is_none()
                    || !matches!(value, "continue" | "end")
                    || (value == "end" && observation == Some(Observation::Unknown))
                {
                    return Err(Error::Acquisition("invalid decoder progress frame"));
                }
                self.finished = value == "end";
                if let Some(Observation::Measured(time)) = observation {
                    self.note(time);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn time(&mut self, value: &str, maximum_us: u64) -> Result<()> {
        if value == "N/A" {
            if self.previous.is_some()
                || self
                    .frame_time
                    .is_some_and(|time| time != Observation::Unknown)
            {
                return Err(Error::Acquisition(
                    "decoder progress became unknown or conflicted",
                ));
            }
            self.frame_time = Some(Observation::Unknown);
            return Ok(());
        }
        let time = value
            .parse::<u64>()
            .ok()
            .filter(|_| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .filter(|time| *time <= maximum_us)
            .ok_or(Error::Acquisition("invalid decoder progress time"))?;
        if self.previous.is_some_and(|previous| time < previous)
            || self
                .frame_time
                .is_some_and(|previous| previous != Observation::Measured(time))
        {
            return Err(Error::Acquisition(
                "decoder progress regressed or conflicted",
            ));
        }
        // The media clock starts at zero even when FFmpeg emits only its final
        // frame. A positive first observation is progress, not a missing delta.
        self.advanced |= time > 0;
        self.previous = Some(time);
        self.frame_time = Some(Observation::Measured(time));
        self.max_out_us = time;
        Ok(())
    }

    /// Publish at most once per second of decoder output, and always publish a later final frame.
    /// A full observer drops the sample. Decoding does not wait.
    fn note(&mut self, time: u64) {
        if time == 0 {
            return;
        }
        if let Some(previous) = self.last_reported {
            let within_step = time < previous.saturating_add(TIMELINE_STEP_US);
            if previous == time || (within_step && !self.finished) {
                return;
            }
        }
        self.last_reported = Some(time);
        let _ = DECODER_TIMELINE.try_with(|sender| {
            let _ = sender.try_send(time);
        });
    }
}

pub(super) async fn read_progress(
    mut input: impl AsyncRead + Unpin,
    limits: Limits,
) -> Result<Progress> {
    let mut progress = Progress::default();
    let mut line = Vec::with_capacity(MAX_LINE);
    let mut chunk = [0_u8; 1024];
    let mut total = 0_usize;
    let mut frame_bytes = 0_usize;
    let mut frames = 0_u64;
    loop {
        let read = input.read(&mut chunk).await?;
        if read == 0 {
            if !line.is_empty() || progress.frame_time.is_some() {
                return Err(Error::Acquisition("incomplete decoder progress"));
            }
            return Ok(progress);
        }
        total += read;
        if total > limits.total_bytes {
            return Err(Error::Acquisition("decoder progress limit"));
        }
        for byte in &chunk[..read] {
            frame_bytes += 1;
            if frame_bytes > MAX_FRAME {
                return Err(Error::Acquisition("decoder progress frame limit"));
            }
            if *byte == b'\n' {
                if line.starts_with(b"progress=") {
                    frames += 1;
                    if frames > limits.frames {
                        return Err(Error::Acquisition("decoder progress frame count limit"));
                    }
                    frame_bytes = 0;
                }
                progress.line(&line, limits.maximum_us)?;
                line.clear();
            } else if line.len() == MAX_LINE {
                return Err(Error::Acquisition("decoder progress line limit"));
            } else {
                line.push(*byte);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Limits, read_progress, with_timeline};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn timeline_samples_keep_one_second_steps_and_the_final_frame() -> TestResult {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        let bytes = b"out_time_us=100000\nprogress=continue\nout_time_us=200000\nprogress=continue\nout_time_us=1100000\nprogress=continue\nout_time_us=1500000\nprogress=end\n";
        let progress = with_timeline(
            sender,
            read_progress(bytes.as_slice(), Limits::exact(1_500_000, 8)),
        )
        .await?;
        assert_eq!(progress.max_out_us, 1_500_000);
        let mut samples = Vec::new();
        while let Ok(time) = receiver.try_recv() {
            samples.push(time);
        }
        assert_eq!(samples, vec![100_000, 1_100_000, 1_500_000]);
        Ok(())
    }

    #[tokio::test]
    async fn timeline_drops_samples_when_the_observer_is_full() -> TestResult {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let bytes = b"out_time_us=100000\nprogress=continue\nout_time_us=1100000\nprogress=continue\nout_time_us=2100000\nprogress=end\n";
        let progress = with_timeline(
            sender,
            read_progress(bytes.as_slice(), Limits::exact(2_100_000, 8)),
        )
        .await?;
        assert_eq!(progress.max_out_us, 2_100_000);
        assert!(progress.finished);
        assert_eq!(receiver.try_recv()?, 100_000);
        assert!(receiver.try_recv().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn timeline_stays_quiet_without_an_observer() -> TestResult {
        let progress = read_progress(
            b"out_time_us=1500000\nprogress=end\n".as_slice(),
            Limits::exact(1_500_000, 4),
        )
        .await?;
        assert_eq!(progress.max_out_us, 1_500_000);
        assert!(progress.finished);
        Ok(())
    }
}
