//! Client-local PCM decoding reuses native group limits and protected original reads.

use std::{path::Path, sync::Mutex, time::Duration};

use processkit::{ProcessGroup, ProcessGroupOptions};
use serde::Serialize;
use tokio::{io::AsyncRead, process::Child, sync::mpsc};

use crate::{
    Error, Result, control::RetainedReadSpec, execution, recordings::RetainedPlaybackReport,
};

mod null;
pub use null::{ExcerptNullError, ExcerptNullReport, decode_retained_excerpt_null};

/// One explicitly negotiated interleaved little-endian floating-point profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PcmFormat {
    pub rate_hz: u32,
    pub channels: u16,
}

impl PcmFormat {
    /// Sample onsets in the half-open file-relative interval, after resampling.
    /// # Errors
    /// Refuses unsupported profiles, empty sample selections and overflow.
    pub fn excerpt_frames(self, start_us: u64, end_us: u64) -> Result<(u64, u64)> {
        self.maximum_bytes(
            end_us
                .checked_sub(start_us)
                .ok_or(Error::Acquisition("invalid PCM excerpt range"))?,
        )?;
        let onset = |time: u64| {
            time.checked_mul(u64::from(self.rate_hz))
                .and_then(|scaled| scaled.checked_add(999_999))
                .map(|scaled| scaled / 1_000_000)
                .ok_or(Error::Acquisition("PCM excerpt frame overflow"))
        };
        let first = onset(start_us)?;
        let after = onset(end_us)?;
        if first >= after {
            return Err(Error::Acquisition("PCM excerpt contains no sample onsets"));
        }
        Ok((first, after))
    }

    /// # Errors
    /// Refuses unsupported rates, layouts, durations and arithmetic overflow.
    pub fn maximum_bytes(self, duration_us: u64) -> Result<u64> {
        if !(8_000..=192_000).contains(&self.rate_hz)
            || !(1..=2).contains(&self.channels)
            || duration_us == 0
            || duration_us > 1_800_000_000
        {
            return Err(Error::Acquisition("unsupported bounded PCM profile"));
        }
        duration_us
            .checked_add(100_000)
            .and_then(|time| time.checked_mul(u64::from(self.rate_hz)))
            .and_then(|frames| frames.checked_add(999_999))
            .map(|frames| frames / 1_000_000)
            .and_then(|frames| frames.checked_mul(u64::from(self.channels) * 4))
            .ok_or(Error::Acquisition("PCM bound overflow"))
    }
}

/// One client operation's native group. Dropping it requests a kill, not a proof.
#[derive(Debug)]
pub struct NativeAudioGroup {
    group: ProcessGroup,
    closed: Mutex<bool>,
}

/// The same operating-system snapshot that observed an empty group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AudioClosure {
    pub mechanism: String,
    pub peak_memory_bytes: Option<u64>,
    pub cpu_time_us: Option<u64>,
}

impl NativeAudioGroup {
    /// # Errors
    /// Fails closed if the selected Windows limits cannot be enforced.
    pub fn new() -> Result<Self> {
        if !cfg!(windows) {
            return Err(Error::Acquisition("Windows audio group is unavailable"));
        }
        Self::for_decoder()
    }

    /// Enforce the same finite decoder limits on the selected host mechanism.
    /// # Errors
    /// Refuses a platform or environment that cannot enforce every requested limit.
    pub fn for_decoder() -> Result<Self> {
        let options = ProcessGroupOptions::default()
            .max_processes(2)
            .max_memory(768 * 1024 * 1024)
            .cpu_quota(0.75);
        let group = execution::contained(options)
            .ok_or(Error::Acquisition("bounded audio containment unavailable"))?;
        Ok(Self {
            group,
            closed: Mutex::new(false),
        })
    }

    /// Start one child in the already limited group, without a shell.
    /// Windows raw spawning suspends before assignment; extra creation flags are lost.
    /// Abrupt owner death before assignment can leave an unassigned suspended child.
    /// # Errors
    /// Refuses a failed spawn or exhausted native limits.
    pub fn spawn(&self, mut command: tokio::process::Command) -> Result<Child> {
        let closed = self
            .closed
            .lock()
            .map_err(|_| Error::Acquisition("audio group state unavailable"))?;
        if *closed {
            return Err(Error::Acquisition("audio group is sealed"));
        }
        command.kill_on_drop(true);
        self.group
            .spawn(command)
            .map_err(|_| Error::Acquisition("bounded audio child could not start"))
    }

    /// # Errors
    /// Returns failure if the operating system cannot request termination.
    pub fn kill(&self) -> Result<()> {
        self.seal()?;
        self.group
            .kill_all()
            .map_err(|_| Error::Acquisition("audio termination request failed"))
    }

    /// A repeated child wait never substitutes for this group observation.
    /// # Errors
    /// Reports unproven closure after the existing five-second group drain bound.
    pub async fn finish(&self) -> Result<AudioClosure> {
        self.seal()?;
        let snapshot = execution::drain(&self.group).await?;
        Ok(AudioClosure {
            mechanism: snapshot.mechanism.into(),
            peak_memory_bytes: snapshot.peak_memory_bytes,
            cpu_time_us: snapshot.cpu_time_us,
        })
    }

    fn seal(&self) -> Result<()> {
        *self
            .closed
            .lock()
            .map_err(|_| Error::Acquisition("audio group state unavailable"))? = true;
        Ok(())
    }
}

/// Frozen local decoder configuration; it contains no source URL or audio device.
#[derive(Debug, Clone, Copy)]
pub struct PcmReaderRequest<'a> {
    pub executable: &'a str,
    pub spec: &'a RetainedReadSpec,
    pub format: PcmFormat,
    pub group: &'a NativeAudioGroup,
}

/// The existing protected reader is the only library-file input capability.
#[derive(Debug, Clone, Copy)]
pub struct PcmDecodeRequest<'a> {
    pub reader: PcmReaderRequest<'a>,
    pub directory: &'a Path,
    pub nonce: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmDecoded {
    pub decoder: RetainedPlaybackReport,
    pub pcm_bytes: u64,
}

/// Connect immediately to the existing private protected-reader transport.
/// The resulting stream grants no file path, source URL or native execution.
/// # Errors
/// Refuses unavailable or invalid local capabilities within five seconds.
pub async fn open_protected_pcm(directory: &Path, nonce: &str) -> Result<impl AsyncRead + Unpin> {
    tokio::time::timeout(
        Duration::from_secs(5),
        super::pipe::connect_listen_pipe(directory, nonce),
    )
    .await
    .map_err(|_| Error::Acquisition("protected PCM input connection timed out"))?
}

/// # Errors
/// Refuses invalid bounds, a missing protected pipe or failed bounded decoding.
pub async fn decode_retained_pcm(
    request: PcmDecodeRequest<'_>,
    output: mpsc::Sender<Vec<u8>>,
) -> Result<PcmDecoded> {
    validate(request.reader, &output)?;
    let input = tokio::time::timeout(
        Duration::from_secs(5),
        super::pipe::connect_listen_pipe(request.directory, request.nonce),
    )
    .await
    .map_err(|_| Error::Acquisition("protected PCM input connection timed out"))??;
    Box::pin(decode_pcm_reader(request.reader, input, output)).await
}

/// Decode caller-owned local bytes. The caller retains its own input protection.
/// # Errors
/// Refuses unbounded output queues and invalid or incomplete media.
pub async fn decode_pcm_reader(
    request: PcmReaderRequest<'_>,
    input: impl AsyncRead + Unpin,
    output: mpsc::Sender<Vec<u8>>,
) -> Result<PcmDecoded> {
    validate(request, &output)?;
    super::decoder::decode_pcm(request, input, output).await
}

fn validate(request: PcmReaderRequest<'_>, output: &mpsc::Sender<Vec<u8>>) -> Result<()> {
    let spec = request.spec;
    if output.max_capacity() != 2
        || !crate::storage::dvr::retained_format(&spec.format)
        || spec.file_seek_us >= spec.file_duration_us
        || spec.file_duration_us > 1_800_000_000
        || spec.bytes == 0
        || spec.bytes > 512 * 1024 * 1024
    {
        return Err(Error::Acquisition("invalid protected PCM decode bounds"));
    }
    request.format.maximum_bytes(spec.playback_duration_us()?)?;
    if spec.excerpt.is_some() {
        request
            .format
            .excerpt_frames(spec.file_seek_us, spec.playback_end_us()?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PcmFormat;

    #[test]
    fn declared_frame_caps_preserve_independent_rate_and_channel_arithmetic() {
        let stereo = PcmFormat {
            rate_hz: 48_000,
            channels: 2,
        };
        assert_eq!(stereo.maximum_bytes(250_000).ok(), Some(134_400));
        assert_eq!(stereo.maximum_bytes(1_800_000_000).ok(), Some(691_238_400));
        let mono = PcmFormat {
            rate_hz: 8_000,
            channels: 1,
        };
        assert_eq!(mono.maximum_bytes(1).ok(), Some(3_204));
        for duration in [0, 1_800_000_001, u64::MAX] {
            assert!(stereo.maximum_bytes(duration).is_err());
        }
        for (rate_hz, channels) in [(7_999, 1), (192_001, 2), (48_000, 0), (48_000, 3)] {
            assert!(
                PcmFormat { rate_hz, channels }
                    .maximum_bytes(250_000)
                    .is_err()
            );
        }
    }

    #[test]
    fn excerpt_samples_use_absolute_endpoints_at_each_negotiated_rate() {
        let stereo = PcmFormat {
            rate_hz: 48_000,
            channels: 2,
        };
        assert_eq!(
            stereo.excerpt_frames(125_000, 375_000).ok(),
            Some((6000, 18_000))
        );
        assert_eq!(stereo.excerpt_frames(1, 21).ok(), Some((1, 2)));
        assert_eq!(stereo.excerpt_frames(21, 42).ok(), Some((2, 3)));
        assert!(stereo.excerpt_frames(1, 20).is_err());
        assert!(stereo.excerpt_frames(21, 21).is_err());
        assert!(stereo.excerpt_frames(22, 21).is_err());
        assert!(stereo.excerpt_frames(u64::MAX - 1, u64::MAX).is_err());
        let mono = PcmFormat {
            rate_hz: 44_100,
            channels: 1,
        };
        assert_eq!(mono.excerpt_frames(1, 23).ok(), Some((1, 2)));
        assert!(mono.excerpt_frames(1, 2).is_err());
        assert_eq!(
            mono.excerpt_frames(125_000, 375_000).ok(),
            Some((5513, 16_538))
        );
    }
}
