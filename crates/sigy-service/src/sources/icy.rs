//! ICY metadata blocks are timed observations. They are not audio and not authority.

use super::http::MAXIMUM_BODY_BYTES;
use crate::{Error, Result};

pub(crate) const MAX_ICY_OBSERVATIONS: usize = 1024;
pub(crate) const MAX_ICY_BLOCK: usize = 255 * 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetadataPolicy {
    Off,
    Requested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IcyObservation {
    pub audio_offset: u64,
    pub text: String,
}

#[derive(Debug)]
enum Phase {
    Audio,
    Length,
    Metadata(usize),
}

#[derive(Debug)]
pub(crate) struct IcySplitter {
    interval: u64,
    audio_remaining: u64,
    audio_offset: u64,
    phase: Phase,
    block: Vec<u8>,
    observations: Vec<IcyObservation>,
}

impl IcySplitter {
    pub(crate) fn new(interval: u64) -> Result<Self> {
        if interval == 0 || interval > MAXIMUM_BODY_BYTES {
            return Err(Error::Acquisition("ICY metadata interval is invalid"));
        }
        Ok(Self {
            interval,
            audio_remaining: interval,
            audio_offset: 0,
            phase: Phase::Audio,
            block: Vec::new(),
            observations: Vec::new(),
        })
    }

    /// Returns the audio bytes from this chunk. Metadata never appears in the output.
    /// # Errors
    /// Rejects an oversized observation list, text that is not UTF-8, and unsafe display text.
    pub(crate) fn push(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        let mut audio = Vec::new();
        let mut index = 0;
        while index < input.len() {
            match self.phase {
                Phase::Audio => {
                    let available = input.len() - index;
                    let take = usize::try_from(self.audio_remaining)
                        .unwrap_or(usize::MAX)
                        .min(available);
                    audio.extend_from_slice(&input[index..index + take]);
                    let taken = u64::try_from(take).map_err(|_| Error::StorageIntegrity)?;
                    self.audio_offset = self
                        .audio_offset
                        .checked_add(taken)
                        .ok_or(Error::StorageIntegrity)?;
                    self.audio_remaining -= taken;
                    index += take;
                    if self.audio_remaining == 0 {
                        self.phase = Phase::Length;
                        self.audio_remaining = self.interval;
                    }
                }
                Phase::Length => {
                    let blocks = usize::from(input[index]);
                    index += 1;
                    let length = blocks.saturating_mul(16);
                    if length == 0 {
                        self.phase = Phase::Audio;
                    } else {
                        self.block.clear();
                        self.phase = Phase::Metadata(length);
                    }
                }
                Phase::Metadata(remaining) => {
                    let take = remaining.min(input.len() - index);
                    self.block.extend_from_slice(&input[index..index + take]);
                    index += take;
                    let left = remaining - take;
                    if left == 0 {
                        self.store_block()?;
                        self.phase = Phase::Audio;
                    } else {
                        self.phase = Phase::Metadata(left);
                    }
                }
            }
        }
        Ok(audio)
    }

    /// # Errors
    /// A body that ends inside a metadata block is truncated.
    pub(crate) fn finish(&self) -> Result<()> {
        if matches!(self.phase, Phase::Audio) {
            Ok(())
        } else {
            Err(Error::Acquisition("truncated ICY metadata"))
        }
    }

    pub(crate) fn into_observations(self) -> Vec<IcyObservation> {
        self.observations
    }

    pub(crate) fn retain_through(&mut self, bytes: u64) {
        self.observations
            .retain(|observation| observation.audio_offset <= bytes);
    }

    fn store_block(&mut self) -> Result<()> {
        let end = self
            .block
            .iter()
            .rposition(|byte| *byte != 0)
            .map_or(0, |index| index + 1);
        let bytes = &self.block[..end];
        if bytes.is_empty() {
            return Ok(());
        }
        if self.observations.len() == MAX_ICY_OBSERVATIONS {
            return Err(Error::Acquisition("ICY observation limit"));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Acquisition("ICY metadata is not UTF-8"))?;
        if text.chars().any(super::unsafe_display) {
            return Err(Error::Acquisition("ICY metadata is not accepted"));
        }
        self.observations.push(IcyObservation {
            audio_offset: self.audio_offset,
            text: text.to_owned(),
        });
        Ok(())
    }
}

pub(crate) fn interval_header(value: &str) -> Result<u64> {
    if value.is_empty()
        || value.len() > 10
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || value.starts_with('0')
    {
        return Err(Error::Acquisition("ICY metadata interval is invalid"));
    }
    let interval = value
        .parse::<u64>()
        .map_err(|_| Error::Acquisition("ICY metadata interval is invalid"))?;
    if interval == 0 || interval > MAXIMUM_BODY_BYTES {
        return Err(Error::Acquisition("ICY metadata interval is invalid"));
    }
    Ok(interval)
}

#[cfg(test)]
mod tests {
    use super::{IcySplitter, interval_header};

    fn padded(text: &str) -> Vec<u8> {
        let mut block = text.as_bytes().to_vec();
        let size = block.len().div_ceil(16) * 16;
        block.resize(size, 0);
        let mut framed = vec![u8::try_from(size / 16).unwrap_or(255)];
        framed.extend(block);
        framed
    }

    #[test]
    fn metadata_is_removed_and_timed_at_the_audio_offset() -> Result<(), crate::Error> {
        let title = "StreamTitle='Owned';StreamUrl='http://evil.example/secret';";
        let mut body = b"abcd".to_vec();
        body.extend(padded(title));
        body.extend(b"ef");
        let mut splitter = IcySplitter::new(4)?;
        let mut audio = splitter.push(&body[..3])?;
        audio.extend(splitter.push(&body[3..])?);
        splitter.finish()?;
        assert_eq!(audio, b"abcdef");
        let observations = splitter.into_observations();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].audio_offset, 4);
        assert_eq!(observations[0].text, title);
        Ok(())
    }

    #[test]
    fn empty_and_split_blocks_do_not_enter_the_audio() -> Result<(), crate::Error> {
        let mut splitter = IcySplitter::new(2)?;
        let audio = splitter.push(b"ab\x00cd\x00")?;
        splitter.finish()?;
        assert_eq!(audio, b"abcd");
        assert!(splitter.into_observations().is_empty());
        let mut splitter = IcySplitter::new(1)?;
        assert!(splitter.push(b"a").is_ok());
        assert!(splitter.finish().is_err());
        Ok(())
    }

    #[test]
    fn unsafe_or_non_utf8_metadata_fails_closed() -> Result<(), crate::Error> {
        let mut splitter = IcySplitter::new(1)?;
        let mut bad = vec![1_u8];
        bad.extend(b"\xff");
        bad.extend(std::iter::repeat_n(0, 15));
        assert!(splitter.push(b"Z").is_ok());
        assert!(splitter.push(&bad).is_err());
        let mut splitter = IcySplitter::new(1)?;
        let mut control = vec![1_u8];
        control.extend(b"\n");
        control.extend(std::iter::repeat_n(0, 15));
        assert!(splitter.push(b"Z").is_ok());
        assert!(splitter.push(&control).is_err());
        assert!(interval_header("0").is_err());
        assert!(interval_header("00").is_err());
        assert!(interval_header("16").is_ok());
        Ok(())
    }
}
