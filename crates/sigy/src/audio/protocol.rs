use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub(crate) const CONFIG_BYTES: usize = 4096;
pub(crate) const MESSAGE_BYTES: usize = 8192;
pub(crate) const PCM_BYTES: usize = 32_768;
pub(crate) const STARTUP_MS: u64 = 5000;
pub(crate) const DRAIN_MS: u64 = 3000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HelperLimits {
    pub duration_us: u64,
}

impl HelperLimits {
    pub(crate) fn validate(self) -> Result<Self, super::Failure> {
        if self.duration_us == 0 || self.duration_us > 1_800_000_000 {
            return Err("audio-duration-bound".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ready {
    pub protocol: u32,
    pub rate_hz: u32,
    pub channels: u16,
    pub sample_format: String,
    pub ring_frames: u32,
    pub callback_frames_estimate: u32,
    pub maximum_pcm_bytes: u64,
}

impl Ready {
    #[cfg(any(windows, test))]
    pub(crate) fn validate(&self, limits: HelperLimits) -> Result<(), super::Failure> {
        let maximum = super::pcm::maximum_bytes(limits.duration_us, self.rate_hz, self.channels)?;
        if self.protocol != 1
            || self.sample_format != "f32le"
            || self.ring_frames != self.rate_hz / 4
            || self.callback_frames_estimate == 0
            || self.callback_frames_estimate > self.ring_frames
            || u64::from(self.ring_frames) * u64::from(self.channels) * 4 > 1_048_576
            || self.maximum_pcm_bytes != maximum
        {
            return Err("audio-handshake-invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Drained,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    pub protocol: u32,
    pub status: Status,
    pub error: Option<String>,
    pub decoded_frames: u64,
    pub content_frames: u64,
    pub clipped_samples: u64,
    pub underrun_frames: u64,
    pub drain_zero_frames: u64,
    pub callbacks: u64,
    pub queue_high_water_frames: u64,
    pub predicted_presentation_us: Option<u64>,
    pub presentation_is_estimated: bool,
    pub audibility_proven: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    Ready(Ready),
    Report(Report),
}

pub(crate) fn read_limits(input: &mut impl Read) -> Result<HelperLimits, super::Failure> {
    let length = read_length(input)?;
    let length = usize::try_from(length)?;
    if length == 0 || length > CONFIG_BYTES {
        return Err("audio-config-bound".into());
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice::<HelperLimits>(&bytes)?.validate()
}

pub(crate) fn read_length(input: &mut impl Read) -> Result<u32, super::Failure> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

pub(crate) fn write_event(output: &mut impl Write, event: &Event) -> Result<(), super::Failure> {
    // All event strings are fixed protocol values/codes; no native strings enter this channel.
    let bytes = serde_json::to_vec(event)?;
    if bytes.len() > MESSAGE_BYTES {
        return Err("audio-message-bound".into());
    }
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
