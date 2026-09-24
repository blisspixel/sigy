//! Catalog snapshot and retained-object listing for library backups.

use std::path::Path;

use super::Store;
use crate::{Error, Result};

/// One retained media object the catalog vouches for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetainedObject {
    pub key: String,
    pub sha256: String,
    pub bytes: u64,
}

impl Store {
    /// Write a transactionally consistent copy of the catalog with `VACUUM INTO`.
    /// # Errors
    /// Fails if the destination exists or the snapshot cannot be written.
    pub(crate) fn snapshot_catalog(&self, destination: &Path) -> Result<()> {
        if destination.try_exists()? {
            return Err(Error::InvalidInput("backup catalog destination exists"));
        }
        let text = destination
            .to_str()
            .ok_or(Error::InvalidInput("backup path is not valid Unicode"))?;
        self.connection.execute("VACUUM INTO ?1", [text])?;
        Ok(())
    }

    /// Every published interval whose file is still retained, in key order.
    /// # Errors
    /// Refuses invalid stored rows.
    pub(crate) fn retained_objects(&self) -> Result<Vec<RetainedObject>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT s.object_key, s.sha256, s.byte_end - s.byte_start FROM recording_intervals s JOIN recordings r ON r.id = s.recording_id WHERE r.storage_state = 'retained' AND NOT EXISTS (SELECT 1 FROM recording_releases x WHERE x.recording_id = s.recording_id AND x.segment_ordinal = s.ordinal) ORDER BY s.object_key",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(key, sha256, bytes)| {
                super::dvr::validate_object_key(&key)?;
                Ok(RetainedObject {
                    key,
                    sha256,
                    bytes: u64::try_from(bytes).map_err(|_| Error::StorageIntegrity)?,
                })
            })
            .collect()
    }

    /// `PRAGMA integrity_check` returned exactly `ok`.
    /// # Errors
    /// Fails on a database error.
    pub(crate) fn integrity_ok(&self) -> Result<bool> {
        let result: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        Ok(result == "ok")
    }
}
