//! Durable protection for a single client-bound retained-object transfer.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    Store,
    query_work::{Limits, QueryWork},
    validate_key,
};
use crate::{Error, Result};

mod read;
#[cfg(test)]
mod tests;
mod write;

pub(crate) const MAX_RETAINED_READ_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const RETAINED_READ_DEADLINE_SECONDS: u64 = 30 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedReadSpec {
    pub request_id: String,
    pub generation: u64,
    pub recording_id: String,
    pub source_revision: String,
    pub ordinal: u32,
    pub object_key: String,
    pub sha256: String,
    pub format: String,
    pub bytes: u64,
    pub timeline_start_us: u64,
    pub timeline_end_us: u64,
    pub file_seek_us: u64,
    pub file_duration_us: u64,
    pub spec_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedReadView {
    pub spec: RetainedReadSpec,
    pub state: String,
    pub seek_us: u64,
    pub admitted_ms: i64,
    pub updated_ms: i64,
    pub completion_reason: Option<String>,
    pub recovery_reason: Option<String>,
}

impl RetainedReadSpec {
    fn digest(&self) -> Result<String> {
        let digest = spec_digest(
            [
                &self.request_id,
                &self.recording_id,
                &self.source_revision,
                &self.object_key,
                &self.sha256,
                &self.format,
            ],
            [
                self.generation,
                u64::from(self.ordinal),
                self.bytes,
                self.timeline_start_us,
                self.timeline_end_us,
                self.file_seek_us,
                self.file_duration_us,
            ],
        )?;
        Ok(super::dvr::hex(&digest.finalize()))
    }
}

fn spec_digest(texts: [&str; 6], numbers: [u64; 7]) -> Result<Sha256> {
    // Fixed stack encoding avoids repeated hashing calls during full-history
    // integrity reads. The byte sequence is unchanged and cannot grow.
    let mut encoded = [0_u8; 1024];
    let mut length = 0;
    append(&mut encoded, &mut length, b"sigy-retained-read-spec-v1")?;
    for text in texts {
        append(
            &mut encoded,
            &mut length,
            &u64::try_from(text.len())
                .map_err(|_| Error::StorageIntegrity)?
                .to_be_bytes(),
        )?;
        append(&mut encoded, &mut length, text.as_bytes())?;
    }
    for value in numbers {
        append(&mut encoded, &mut length, &value.to_be_bytes())?;
    }
    let mut digest = Sha256::new();
    digest.update(&encoded[..length]);
    Ok(digest)
}

fn append(encoded: &mut [u8; 1024], length: &mut usize, bytes: &[u8]) -> Result<()> {
    let end = length
        .checked_add(bytes.len())
        .ok_or(Error::StorageIntegrity)?;
    let destination = encoded
        .get_mut(*length..end)
        .ok_or(Error::StorageIntegrity)?;
    destination.copy_from_slice(bytes);
    *length = end;
    Ok(())
}

impl Store {
    /// Inspect one durable transfer without opening its media object.
    /// # Errors
    /// Refuses unknown identity, malformed stored lineage or exhausted query bounds.
    pub fn retained_reader(&self, id: &str) -> Result<RetainedReadView> {
        validate_key(id, "retained reader ID")?;
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let result = read::find(&self.connection, id)?.ok_or(Error::NotFound)?;
        read::audit(&self.connection, &result)?;
        work.check()?;
        work.finish()?;
        Ok(result)
    }

    /// Inspect at most sixteen identities, unresolved readers first, then recent receipts.
    /// # Errors
    /// Refuses malformed stored lineage or exhausted query bounds.
    pub fn retained_readers(&self) -> Result<Vec<RetainedReadView>> {
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let result = read::list(&self.connection)?;
        for view in &result {
            read::audit(&self.connection, view)?;
        }
        work.check()?;
        work.finish()?;
        Ok(result)
    }

    pub(crate) fn audit_retained_readers(&self) -> Result<()> {
        let work = QueryWork::start(&self.connection, Limits::RETAINED_HISTORY)?;
        read::audit_all(&self.connection)?;
        work.check()?;
        work.finish()?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn revert_049_for_tests(connection: &rusqlite::Connection) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='retained_readers')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let used: bool =
        connection.query_row("SELECT EXISTS(SELECT 1 FROM retained_readers)", [], |row| {
            row.get(0)
        })?;
    if used {
        return Err(Error::RequestState);
    }
    connection.execute_batch("DROP TRIGGER retained_reader_delete_guard; DROP TRIGGER retained_reader_release_guard; DROP TABLE retained_readers;")?;
    Ok(())
}
