//! Point-indexed historical station context, independent of current cache membership.

use rusqlite::{OptionalExtension, Row};

use super::super::{Store, validate_key};
use crate::{
    Error, Result,
    discovery::{
        Station,
        linked::{
            LinkedRecording, LinkedSource, LinkedStationContext, MAX_LINKED_RECORDINGS,
            MAX_LINKED_SOURCES,
        },
        ordered::DirectoryCatalog,
        validate_station_id,
    },
};

fn text(row: &Row<'_>, column: usize, maximum: usize) -> rusqlite::Result<String> {
    let rusqlite::types::ValueRef::Text(bytes) = row.get_ref(column)? else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    if bytes.len() > maximum {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| rusqlite::Error::InvalidQuery)?
        .to_owned())
}

impl Store {
    /// Observe at most four exact registrations and four recordings per registration.
    /// # Errors
    /// Refuses stale catalog identities, corrupt lineage and exhausted query work.
    pub fn linked_station_context(
        &self,
        id: &str,
        catalog: Option<&DirectoryCatalog>,
    ) -> Result<LinkedStationContext> {
        self.guarded_directory_read(|store| store.linked_station_context_in_work(id, catalog))
    }

    pub(crate) fn linked_station_context_in_work(
        &self,
        id: &str,
        expected: Option<&DirectoryCatalog>,
    ) -> Result<LinkedStationContext> {
        validate_station_id(id)?;
        let catalog = self.directory_catalog()?;
        if expected.is_some_and(|expected| expected != &catalog) {
            return Err(Error::InvalidInput("linked station catalog changed"));
        }
        let cached = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM directory_stations WHERE provider='radio_browser' AND id=?1)", [id], |row| row.get(0))?;
        let mut statement = self.connection.prepare("SELECT source_revision FROM source_directory_links WHERE provider='radio_browser' AND station_id=?1 ORDER BY source_revision LIMIT 5")?;
        let mut rows = statement.query([id])?;
        let mut sources = Vec::with_capacity(MAX_LINKED_SOURCES);
        let mut more_sources = false;
        while let Some(row) = rows.next()? {
            let revision = text(row, 0, 128)?;
            validate_key(&revision, "source revision ID").map_err(|_| Error::SourceIntegrity)?;
            if sources.len() == MAX_LINKED_SOURCES {
                more_sources = true;
                break;
            }
            sources.push(self.linked_source(id, &revision)?);
        }
        let page = LinkedStationContext {
            provider: "radio_browser".into(),
            station_id: id.into(),
            catalog,
            cached,
            sources,
            more_sources,
        };
        super::ordered::encode(&page, 128 * 1024)?;
        Ok(page)
    }

    fn linked_source(&self, id: &str, revision: &str) -> Result<LinkedSource> {
        let json = self.connection.query_row("SELECT metadata_json FROM source_directory_links WHERE provider='radio_browser' AND station_id=?1 AND source_revision=?2", [id, revision], |row| text(row, 0, 8192))?;
        let registered_station: Station = serde_json::from_str(&json)?;
        registered_station.validate()?;
        if registered_station.provider != "radio_browser" || registered_station.id != id {
            return Err(Error::SourceIntegrity);
        }
        // Cap every raw source string before the existing canonical source decoder allocates.
        self.connection.query_row("SELECT kind,name,endpoint,network_scope,pinned_address,redirect_policy FROM source_revisions WHERE id=?1", [revision], |row| {
            for (column, maximum) in [(0,32),(1,256),(2,2048),(3,32),(5,32)] { text(row,column,maximum)?; }
            if !matches!(row.get_ref(4)?, rusqlite::types::ValueRef::Null) { text(row,4,64)?; }
            Ok(())
        })?;
        let source = self.source(revision)?.ok_or(Error::SourceIntegrity)?;
        if source.source.origin() != registered_station.stream_origin {
            return Err(Error::SourceIntegrity);
        }
        let mut statement = self.connection.prepare("SELECT r.id FROM capture_jobs c JOIN recordings r ON r.id=c.id WHERE c.source_revision=?1 ORDER BY c.id LIMIT 5")?;
        let mut rows = statement.query([revision])?;
        let mut recordings = Vec::with_capacity(MAX_LINKED_RECORDINGS);
        let mut more_recordings = false;
        while let Some(row) = rows.next()? {
            let recording = text(row, 0, 128)?;
            validate_key(&recording, "recording ID").map_err(|_| Error::StorageIntegrity)?;
            if recordings.len() == MAX_LINKED_RECORDINGS {
                more_recordings = true;
                break;
            }
            recordings.push(self.linked_recording(&recording, revision)?);
        }
        Ok(LinkedSource {
            source: source.into(),
            registered_station,
            recordings,
            more_recordings,
        })
    }

    fn linked_recording(&self, id: &str, revision: &str) -> Result<LinkedRecording> {
        let entry = self.connection.query_row("SELECT c.state,r.storage_state,r.retention,r.media_bytes,r.decoded_microseconds,EXISTS(SELECT 1 FROM recording_intervals i WHERE i.recording_id=r.id AND NOT EXISTS(SELECT 1 FROM recording_releases x WHERE x.recording_id=i.recording_id AND x.segment_ordinal=i.ordinal)),EXISTS(SELECT 1 FROM recording_releases x WHERE x.recording_id=r.id),EXISTS(SELECT 1 FROM recording_gaps g WHERE g.recording_id=r.id) FROM recordings r JOIN capture_jobs c ON c.id=r.id WHERE r.id=?1 AND c.source_revision=?2", [id,revision], |row| {
            Ok((text(row,0,32)?,text(row,1,32)?,text(row,2,32)?,row.get::<_,Option<i64>>(3)?,row.get::<_,Option<i64>>(4)?,row.get(5)?,row.get(6)?,row.get(7)?))
        }).optional()?.ok_or(Error::StorageIntegrity)?;
        let unsigned = |value: Option<i64>| -> Result<Option<u64>> {
            value
                .map(|value| u64::try_from(value).map_err(|_| Error::StorageIntegrity))
                .transpose()
        };
        if !matches!(
            entry.0.as_str(),
            "scheduled"
                | "starting"
                | "running"
                | "retrying"
                | "stopping"
                | "completed"
                | "interrupted"
                | "cancelled"
                | "failed"
        ) || !matches!(
            entry.1.as_str(),
            "reserved" | "retained" | "deleting" | "deleted"
        ) || !matches!(entry.2.as_str(), "temporary" | "kept" | "archived")
        {
            return Err(Error::StorageIntegrity);
        }
        Ok(LinkedRecording {
            id: id.into(),
            source_revision: revision.into(),
            state: entry.0,
            storage_state: entry.1.clone(),
            retention: entry.2,
            media_bytes: unsigned(entry.3)?,
            decoded_microseconds: unsigned(entry.4)?,
            retained_segments: entry.5 && entry.1 != "deleted" && entry.1 != "deleting",
            released_segments: entry.6,
            has_gaps: entry.7,
        })
    }
}

#[cfg(test)]
pub(crate) fn revert_050_for_tests(connection: &rusqlite::Connection) -> Result<()> {
    crate::storage::retained_readers::revert_051_for_tests(connection)?;
    connection.execute_batch("DROP INDEX IF EXISTS station_source_links; DROP INDEX IF EXISTS captures_by_source_revision;")?;
    Ok(())
}

#[cfg(test)]
mod tests;
