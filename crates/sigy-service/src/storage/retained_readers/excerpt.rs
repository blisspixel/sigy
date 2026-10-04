//! Excerpt identity extends the unchanged whole-object specification.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::RetainedReadSpec;
use crate::{Error, Result, storage::validate_key};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedExcerpt {
    pub version: u32,
    /// Exclusive end on the recording timeline, independent of the object end.
    pub timeline_end_us: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<RetainedCitation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedCitation {
    pub monitor_id: String,
    pub finding_id: String,
    pub transcript_id: String,
    pub transcript_revision: i64,
    pub translation_revision: i64,
    pub cue_ordinal: u32,
}

#[derive(Clone, Copy)]
pub(super) struct BorrowedCitation<'a> {
    pub monitor_id: &'a str,
    pub finding_id: &'a str,
    pub transcript_id: &'a str,
    pub transcript_revision: i64,
    pub translation_revision: i64,
    pub cue_ordinal: u32,
}

#[derive(Clone, Copy)]
pub(super) struct BorrowedExcerpt<'a> {
    pub version: u32,
    pub timeline_end_us: u64,
    pub citation: Option<BorrowedCitation<'a>>,
}

impl<'a> From<&'a RetainedExcerpt> for BorrowedExcerpt<'a> {
    fn from(value: &'a RetainedExcerpt) -> Self {
        Self {
            version: value.version,
            timeline_end_us: value.timeline_end_us,
            citation: value.citation.as_ref().map(|citation| BorrowedCitation {
                monitor_id: &citation.monitor_id,
                finding_id: &citation.finding_id,
                transcript_id: &citation.transcript_id,
                transcript_revision: citation.transcript_revision,
                translation_revision: citation.translation_revision,
                cue_ordinal: citation.cue_ordinal,
            }),
        }
    }
}

impl BorrowedExcerpt<'_> {
    pub(super) fn validate(self, start: u64, seek: u64, end: u64) -> Result<()> {
        if self.version != 2
            || start
                .checked_add(seek)
                .is_none_or(|position| position >= self.timeline_end_us)
            || self.timeline_end_us > end
        {
            return Err(Error::StorageIntegrity);
        }
        if let Some(citation) = self.citation {
            for key in [
                citation.monitor_id,
                citation.finding_id,
                citation.transcript_id,
            ] {
                validate_key(key, "retained citation").map_err(|_| Error::StorageIntegrity)?;
            }
            if !(1..=64).contains(&citation.transcript_revision)
                || !(1..=64).contains(&citation.translation_revision)
                || citation.cue_ordinal > 255
            {
                return Err(Error::StorageIntegrity);
            }
        }
        Ok(())
    }
}

impl RetainedReadSpec {
    /// The checked exclusive decoder end relative to this sealed object's origin.
    /// # Errors
    /// Refuses malformed object or excerpt bounds and unsupported identity versions.
    pub fn playback_end_us(&self) -> Result<u64> {
        if self.timeline_end_us.checked_sub(self.timeline_start_us) != Some(self.file_duration_us)
            || self.file_seek_us >= self.file_duration_us
        {
            return Err(Error::StorageIntegrity);
        }
        if let Some(excerpt) = &self.excerpt {
            BorrowedExcerpt::from(excerpt).validate(
                self.timeline_start_us,
                self.file_seek_us,
                self.timeline_end_us,
            )?;
            return excerpt
                .timeline_end_us
                .checked_sub(self.timeline_start_us)
                .ok_or(Error::StorageIntegrity);
        }
        Ok(self.file_duration_us)
    }

    /// The checked selected duration, without reducing whole-object input bounds.
    /// # Errors
    /// Refuses malformed object or excerpt bounds and unsupported identity versions.
    pub fn playback_duration_us(&self) -> Result<u64> {
        self.playback_end_us()?
            .checked_sub(self.file_seek_us)
            .filter(|duration| *duration > 0)
            .ok_or(Error::StorageIntegrity)
    }
}

pub(super) fn digest(
    texts: [&str; 6],
    numbers: [u64; 7],
    excerpt: Option<BorrowedExcerpt<'_>>,
) -> Result<Sha256> {
    let legacy = super::spec_digest(texts, numbers)?;
    let Some(excerpt) = excerpt else {
        return Ok(legacy);
    };
    excerpt.validate(numbers[3], numbers[5], numbers[4])?;
    let mut digest = Sha256::new();
    digest.update(b"sigy-retained-read-spec-v2");
    digest.update(legacy.finalize());
    digest.update(excerpt.version.to_be_bytes());
    digest.update(excerpt.timeline_end_us.to_be_bytes());
    if let Some(citation) = excerpt.citation {
        digest.update([1]);
        for text in [
            citation.monitor_id,
            citation.finding_id,
            citation.transcript_id,
        ] {
            digest.update(
                u64::try_from(text.len())
                    .map_err(|_| Error::StorageIntegrity)?
                    .to_be_bytes(),
            );
            digest.update(text.as_bytes());
        }
        digest.update(citation.transcript_revision.to_be_bytes());
        digest.update(citation.translation_revision.to_be_bytes());
        digest.update(citation.cue_ordinal.to_be_bytes());
    } else {
        digest.update([0]);
    }
    Ok(digest)
}
