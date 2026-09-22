//! Exact storage reservations and retained-media publication share the capture journal.

#[cfg(test)]
mod tests;

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sigy_core::capture::{CaptureEvent, CaptureState};

use super::{
    Store,
    captures::{self, CaptureJob, CapturePlan, CaptureVersion, journal},
    now_ms, validate_key,
};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    Temporary,
    Kept,
    Archived,
}

impl Retention {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Temporary => "temporary",
            Self::Kept => "kept",
            Self::Archived => "archived",
        }
    }
}

impl std::str::FromStr for Retention {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "temporary" => Ok(Self::Temporary),
            "kept" => Ok(Self::Kept),
            "archived" => Ok(Self::Archived),
            _ => Err(Error::InvalidInput(
                "retention: use temporary, kept, or archived",
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DvrStatus {
    pub quota_bytes: u64,
    pub charged_bytes: u64,
    pub reserved_bytes: u64,
    pub available_bytes: u64,
    pub minimum_free_bytes: u64,
    pub retention_days: u32,
    pub decoder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub id: String,
    pub source_revision: String,
    pub state: String,
    pub object_key: String,
    pub duration_seconds: u64,
    pub maximum_bytes: u64,
    pub retention: Retention,
    pub storage_state: String,
    pub charged_bytes: u64,
    pub media_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub format: Option<String>,
    pub decoded_microseconds: Option<u64>,
    pub end_reason: Option<String>,
    pub processing_receipt: Option<String>,
    pub failure_detail: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Publication {
    pub bytes: u64,
    pub sha256: String,
    pub format: &'static str,
    pub decoded_microseconds: u64,
    pub end_reason: &'static str,
    pub http_route: Vec<crate::sources::HttpHop>,
    pub observations: Vec<crate::sources::icy::IcyObservation>,
}

impl Store {
    /// # Errors
    /// Rejects invalid policy or inconsistent accounting.
    pub fn dvr_status(&self) -> Result<DvrStatus> {
        let (quota, floor, days, decoder): (u64, u64, u32, Option<String>) = self.connection.query_row(
            "SELECT quota_bytes, minimum_free_bytes, retention_days, decoder FROM dvr_policy WHERE singleton = 1", [],
            |row| Ok((unsigned(row, 0)?, unsigned(row, 1)?, row.get(2)?, row.get(3)?)))?;
        let charged: u64 = self.connection.query_row(
            "SELECT coalesce(sum(charged_bytes), 0) FROM recordings",
            [],
            |row| unsigned(row, 0),
        )?;
        let reserved = self.connection.query_row("SELECT coalesce(sum(charged_bytes), 0) FROM recordings WHERE storage_state = 'reserved'", [], |row| unsigned(row, 0))?;
        Ok(DvrStatus {
            quota_bytes: quota,
            charged_bytes: charged,
            reserved_bytes: reserved,
            available_bytes: quota.checked_sub(charged).ok_or(Error::StorageIntegrity)?,
            minimum_free_bytes: floor,
            retention_days: days,
            decoder,
        })
    }

    /// Configure a finite logical quota. Kept and archived recordings count toward it.
    /// # Errors
    /// Rejects quotas below existing liabilities or unusable decoder paths.
    pub fn configure_dvr(
        &mut self,
        quota: u64,
        minimum_free: u64,
        retention_days: u32,
        decoder: &str,
    ) -> Result<()> {
        let quota = i64::try_from(quota).map_err(|_| Error::InvalidInput("storage quota"))?;
        let floor =
            i64::try_from(minimum_free).map_err(|_| Error::InvalidInput("free-space floor"))?;
        if floor < 64 * 1024 * 1024 {
            return Err(Error::InvalidInput(
                "free-space floor must be at least 64 MiB",
            ));
        }
        if !(1..=36500).contains(&retention_days) {
            return Err(Error::InvalidInput("retention days"));
        }
        let path = std::path::Path::new(decoder);
        if !path.is_absolute()
            || !path.is_file()
            || decoder.len() > 4096
            || decoder.chars().any(char::is_control)
        {
            return Err(Error::InvalidInput(
                "absolute path to an installed FFmpeg executable",
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let charged: i64 = tx.query_row(
            "SELECT coalesce(sum(charged_bytes), 0) FROM recordings",
            [],
            |r| r.get(0),
        )?;
        if quota < charged {
            return Err(Error::StorageQuota);
        }
        tx.execute("UPDATE dvr_policy SET quota_bytes = ?1, minimum_free_bytes = ?2, decoder = ?3, retention_days = ?4 WHERE singleton = 1", params![quota, floor, decoder, retention_days])?;
        tx.commit()?;
        Ok(())
    }

    /// Admit immediate recording and reserve its complete byte ceiling atomically.
    /// An exact replay never starts another worker, including after a crash.
    pub(crate) fn admit_recording(
        &mut self,
        id: &str,
        source: &str,
        seconds: u64,
        maximum: u64,
        retention: Retention,
        metadata: bool,
    ) -> Result<Option<CaptureJob>> {
        validate_key(id, "recording ID")?;
        validate_key(source, "source revision")?;
        if !(1..=900).contains(&seconds)
            || !(1..=crate::sources::http::MAXIMUM_BODY_BYTES).contains(&maximum)
        {
            return Err(Error::InvalidInput("recording duration or byte ceiling"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let requested = i64::from(metadata);
        let existing: Option<(String, u64, u64, String, i64)> = tx.query_row(
            "SELECT c.source_revision, r.duration_seconds, c.maximum_bytes, r.initial_retention, r.metadata_requested FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.id = ?1", [id],
            |r| Ok((r.get(0)?, unsigned(r, 1)?, unsigned(r, 2)?, r.get(3)?, r.get(4)?))).optional()?;
        if let Some(parameters) = existing {
            if parameters
                != (
                    source.into(),
                    seconds,
                    maximum,
                    retention.as_str().into(),
                    requested,
                )
            {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(None);
        }
        let found: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_revisions WHERE id = ?1)",
            [source],
            |r| r.get(0),
        )?;
        if !found {
            return Err(Error::NotFound);
        }
        let listening: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM listen_sessions WHERE source_revision = ?1 AND state = 'running')",
            [source],
            |row| row.get(0),
        )?;
        if listening {
            return Err(Error::InvalidInput("source revision is in use"));
        }
        let (quota, configured): (u64, bool) = tx.query_row(
            "SELECT quota_bytes, decoder IS NOT NULL FROM dvr_policy WHERE singleton = 1",
            [],
            |r| Ok((unsigned(r, 0)?, r.get(1)?)),
        )?;
        let charged: u64 = tx.query_row(
            "SELECT coalesce(sum(charged_bytes), 0) FROM recordings",
            [],
            |r| unsigned(r, 0),
        )?;
        let active: u32 = tx.query_row("SELECT count(*) FROM capture_jobs WHERE state IN ('starting', 'running', 'retrying', 'stopping')", [], |r| r.get(0))?;
        if active >= captures::MAX_ACTIVE_CAPTURES {
            return Err(Error::CaptureCapacity);
        }
        let pending: u32 = tx.query_row("SELECT count(*) FROM capture_jobs WHERE state IN ('scheduled', 'starting', 'running', 'retrying', 'stopping', 'interrupted')", [], |r| r.get(0))?;
        if pending >= captures::MAX_PENDING_CAPTURES {
            return Err(Error::CaptureCapacity);
        }
        if !configured || maximum > quota.checked_sub(charged).ok_or(Error::StorageIntegrity)? {
            return Err(Error::StorageQuota);
        }
        let now = now_ms()?;
        let ends = now
            .checked_add(i64::try_from(seconds * 1000).map_err(|_| Error::StorageIntegrity)?)
            .ok_or(Error::StorageIntegrity)?;
        let plan = CapturePlan::new(
            source,
            now,
            ends,
            i64::try_from(maximum).map_err(|_| Error::StorageIntegrity)?,
        )?;
        let admission = captures::admit(&tx, id, &plan)?;
        if !admission.newly_created {
            return Err(Error::IdempotencyConflict);
        }
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| Error::InvalidInput("secure random source unavailable"))?;
        let key = hex(&random);
        tx.execute("INSERT INTO recordings(id, object_key, duration_seconds, initial_retention, retention, storage_state, charged_bytes, metadata_requested) VALUES (?1, ?2, ?3, ?4, ?4, 'reserved', ?5, ?6)", params![id, key, i64::try_from(seconds).map_err(|_| Error::StorageIntegrity)?, retention.as_str(), plan.maximum_bytes(), i64::from(metadata)])?;
        let job = journal::transition(
            &tx,
            admission.job,
            CaptureEvent::Start,
            "recording_admitted",
            now,
        )?;
        tx.commit()?;
        Ok(Some(job))
    }

    /// # Errors
    /// Rejects malformed identifiers or corrupt stored records.
    pub fn recording(&self, id: &str) -> Result<Recording> {
        validate_key(id, "recording ID")?;
        let mut statement = self.connection.prepare("SELECT c.source_revision, c.state, r.object_key, r.duration_seconds, c.maximum_bytes, r.retention, r.storage_state, r.charged_bytes, r.media_bytes, r.sha256, r.format, r.decoded_microseconds, r.end_reason, r.processing_receipt, r.failure_detail FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.id = ?1")?;
        let mut rows = statement.query([id])?;
        let row = rows.next()?.ok_or(Error::NotFound)?;
        let record = Recording {
            id: id.into(),
            source_revision: row.get(0)?,
            state: row.get(1)?,
            object_key: row.get(2)?,
            duration_seconds: unsigned(row, 3)?,
            maximum_bytes: unsigned(row, 4)?,
            retention: row.get::<_, String>(5)?.parse()?,
            storage_state: row.get(6)?,
            charged_bytes: unsigned(row, 7)?,
            media_bytes: optional_unsigned(row, 8)?,
            sha256: row.get(9)?,
            format: row.get(10)?,
            decoded_microseconds: optional_unsigned(row, 11)?,
            end_reason: row.get(12)?,
            processing_receipt: row.get(13)?,
            failure_detail: row.get(14)?,
        };
        validate_object_key(&record.object_key)?;
        if record.failure_detail.as_ref().is_some_and(|text| {
            text.len() > 256 || !text.is_ascii() || text.chars().any(char::is_control)
        }) {
            return Err(Error::StorageIntegrity);
        }
        Ok(record)
    }

    /// # Errors
    /// Rejects invalid pagination or catalog records.
    pub fn recordings(&self, after: Option<&str>, limit: u32) -> Result<Vec<Recording>> {
        if !(1..=64).contains(&limit) {
            return Err(Error::InvalidInput("recording page size"));
        }
        if let Some(after) = after {
            validate_key(after, "recording cursor")?;
        }
        let mut statement = self
            .connection
            .prepare("SELECT id FROM recordings WHERE id > ?1 ORDER BY id LIMIT ?2")?;
        let ids = statement
            .query_map(params![after.unwrap_or(""), limit], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.iter().map(|id| self.recording(id)).collect()
    }

    pub(crate) fn publish_recording(
        &mut self,
        expected: &CaptureVersion,
        publication: &Publication,
    ) -> Result<()> {
        if publication.bytes == 0
            || publication.decoded_microseconds == 0
            || publication.sha256.len() != 64
            || !publication
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::StorageIntegrity);
        }
        let source_id = self
            .capture(expected.id())?
            .ok_or(Error::NotFound)?
            .plan
            .source_revision()
            .to_owned();
        self.source(&source_id)?
            .ok_or(Error::SourceIntegrity)?
            .source
            .validate_route(&publication.http_route)?;
        let route_json = serde_json::to_string(&publication.http_route)?;
        if route_json.len() > 8192 {
            return Err(Error::StorageIntegrity);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, expected.id())?.ok_or(Error::NotFound)?;
        if job.version != *expected {
            return Err(Error::StaleCapture);
        }
        let changed = tx.execute("UPDATE recordings SET storage_state = 'retained', charged_bytes = ?2, media_bytes = ?2, sha256 = ?3, format = ?4, decoded_microseconds = ?5, end_reason = ?6, http_route_json = ?7 WHERE id = ?1 AND storage_state = 'reserved' AND charged_bytes >= ?2", params![expected.id(), i64::try_from(publication.bytes).map_err(|_| Error::StorageIntegrity)?, publication.sha256, publication.format, i64::try_from(publication.decoded_microseconds).map_err(|_| Error::StorageIntegrity)?, publication.end_reason, route_json])?;
        if changed != 1 {
            return Err(Error::StorageIntegrity);
        }
        let requested: i64 = tx.query_row(
            "SELECT metadata_requested FROM recordings WHERE id = ?1",
            [expected.id()],
            |row| row.get(0),
        )?;
        if requested == 0 && !publication.observations.is_empty() {
            return Err(Error::StorageIntegrity);
        }
        store_observations(
            &tx,
            expected.id(),
            publication.bytes,
            &publication.observations,
        )?;
        let now = now_ms()?;
        let stopped = journal::transition(&tx, job, CaptureEvent::Stop, "media_received", now)?;
        journal::transition(
            &tx,
            stopped,
            CaptureEvent::Finalized,
            "media_decoded_and_synced",
            now,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Returns publication provenance, or None for unfinished and legacy recordings.
    /// # Errors
    /// Rejects malformed or policy-inconsistent observations from the catalog.
    pub fn recording_route(&self, id: &str) -> Result<Option<Vec<crate::sources::HttpHop>>> {
        validate_key(id, "recording ID")?;
        let (json, source_id): (Option<String>, String) = self.connection.query_row(
            "SELECT r.http_route_json, c.source_revision FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.id = ?1", [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?.ok_or(Error::NotFound)?;
        let Some(json) = json else {
            return Ok(None);
        };
        if json.len() > 8192 {
            return Err(Error::StorageIntegrity);
        }
        let route = serde_json::from_str::<Vec<crate::sources::HttpHop>>(&json)?;
        self.source(&source_id)?
            .ok_or(Error::SourceIntegrity)?
            .source
            .validate_route(&route)?;
        Ok(Some(route))
    }

    pub(crate) fn fail_recording(
        &mut self,
        expected: &CaptureVersion,
        error: &Error,
    ) -> Result<()> {
        let detail = match error {
            Error::Acquisition(detail) => *detail,
            Error::DestinationDenied => "source destination denied",
            Error::StorageQuota => "storage quota or free-space floor reached",
            Error::Io(_) => "filesystem operation failed; partial bytes retained",
            _ => "recording worker failed; partial bytes retained",
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, expected.id())?.ok_or(Error::NotFound)?;
        if job.version != *expected {
            return Err(Error::StaleCapture);
        }
        tx.execute(
            "UPDATE recordings SET failure_detail = ?2 WHERE id = ?1",
            params![expected.id(), detail],
        )?;
        journal::transition(
            &tx,
            job,
            CaptureEvent::Fail,
            "acquisition_or_decode_failed",
            now_ms()?,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// # Errors
    /// Rejects missing, deleting or deleted recordings.
    pub fn retain_recording(&mut self, id: &str, retention: Retention) -> Result<()> {
        self.recording(id)?;
        if self.connection.execute("UPDATE recordings SET retention = ?2 WHERE id = ?1 AND storage_state IN ('reserved', 'retained')", params![id, retention.as_str()])? != 1 { return Err(Error::RequestState); }
        Ok(())
    }

    /// Records explicit user acknowledgment, not a claim of automatic analysis.
    /// # Errors
    /// Requires a retained verified recording and a bounded receipt identifier.
    pub fn acknowledge_processing(&mut self, id: &str, receipt: &str) -> Result<()> {
        validate_key(id, "recording ID")?;
        validate_key(receipt, "processing receipt")?;
        if self.connection.execute("UPDATE recordings SET processing_receipt = ?2 WHERE id = ?1 AND storage_state = 'retained'", params![id, receipt])? != 1 { return Err(Error::RequestState); }
        Ok(())
    }

    pub(crate) fn begin_delete(&mut self, id: &str, pruning: bool) -> Result<Recording> {
        let record = self.recording(id)?;
        if record.storage_state == "deleted" && !pruning {
            return Ok(record);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, id)?.ok_or(Error::NotFound)?;
        if job.state.is_active() {
            return Err(Error::RequestState);
        }
        // Recheck protection in the mutation, including competing catalog users.
        if tx.execute("UPDATE recordings SET storage_state = 'deleting' WHERE id = ?1 AND storage_state != 'deleted' AND (NOT ?2 OR (retention = 'temporary' AND storage_state IN ('reserved', 'retained')))", params![id, pruning])? != 1 {
            return Err(Error::RequestState);
        }
        tx.commit()?;
        Ok(record)
    }

    pub(crate) fn finish_delete(&mut self, id: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, id)?.ok_or(Error::NotFound)?;
        if job.state.is_active() {
            return Err(Error::RequestState);
        }
        if tx.execute("UPDATE recordings SET storage_state = 'deleted', charged_bytes = 0 WHERE id = ?1 AND storage_state IN ('deleting', 'deleted')", [id])? != 1 { return Err(Error::RequestState); }
        if matches!(
            job.state,
            CaptureState::Scheduled | CaptureState::Interrupted
        ) {
            journal::transition(
                &tx,
                job,
                CaptureEvent::Cancel,
                "unverified_media_deleted",
                now_ms()?,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn pending_deletions(&self) -> Result<Vec<String>> {
        let mut query = self.connection.prepare(
            "SELECT id FROM recordings WHERE storage_state = 'deleting' ORDER BY id LIMIT 64",
        )?;
        Ok(query
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?)
    }

    pub(crate) fn prune_candidates(&self, pressure: bool) -> Result<Vec<String>> {
        self.prune_candidates_at(pressure, now_ms()?)
    }

    fn prune_candidates_at(&self, pressure: bool, now: i64) -> Result<Vec<String>> {
        let cutoff = now.saturating_sub(i64::from(self.dvr_status()?.retention_days) * 86_400_000);
        let mut query = self.connection.prepare("SELECT r.id FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.retention = 'temporary' AND r.storage_state IN ('reserved', 'retained') AND c.state IN ('completed', 'failed', 'interrupted', 'cancelled') AND (?1 OR c.created_ms <= ?2 OR r.processing_receipt IS NOT NULL) ORDER BY c.created_ms, r.id LIMIT 64")?;
        Ok(query
            .query_map(params![pressure, cutoff], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?)
    }

    pub(crate) fn audit_dvr(&self) -> Result<()> {
        self.dvr_status()?;
        let invalid: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.charged_bytes > c.maximum_bytes OR (r.storage_state = 'reserved' AND r.charged_bytes != c.maximum_bytes) OR (r.media_bytes IS NOT NULL AND c.state != 'completed')) OR EXISTS(SELECT 1 FROM capture_jobs c WHERE c.state = 'completed' AND NOT EXISTS(SELECT 1 FROM recordings r WHERE r.id = c.id AND r.media_bytes IS NOT NULL)) OR EXISTS(SELECT 1 FROM recording_observations o LEFT JOIN recordings r ON r.id = o.recording_id WHERE r.id IS NULL OR r.metadata_requested != 1 OR r.media_bytes IS NULL OR o.audio_offset > r.media_bytes)", [], |r| r.get(0))?;
        if invalid {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }

    /// Ordered untrusted ICY text. An empty list means none was published.
    /// # Errors
    /// Rejects a malformed identifier or a corrupt observation row.
    pub fn recording_observations(&self, id: &str) -> Result<Vec<(u64, String)>> {
        validate_key(id, "recording ID")?;
        let mut statement = self.connection.prepare(
            "SELECT audio_offset, text FROM recording_observations WHERE recording_id = ?1 ORDER BY ordinal",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let media_limit = if rows.is_empty() {
            None
        } else {
            let (requested, media): (i64, Option<i64>) = self.connection.query_row(
                "SELECT metadata_requested, media_bytes FROM recordings WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if requested != 1 {
                return Err(Error::StorageIntegrity);
            }
            Some(
                u64::try_from(media.ok_or(Error::StorageIntegrity)?)
                    .map_err(|_| Error::StorageIntegrity)?,
            )
        };
        let mut previous = None;
        let mut observations = Vec::with_capacity(rows.len());
        for (offset, text) in rows {
            let offset = u64::try_from(offset).map_err(|_| Error::StorageIntegrity)?;
            if text.is_empty()
                || text.len() > crate::sources::icy::MAX_ICY_BLOCK
                || text.chars().any(crate::sources::unsafe_display)
                || media_limit.is_some_and(|limit| offset > limit)
                || previous.is_some_and(|previous: u64| offset <= previous)
            {
                return Err(Error::StorageIntegrity);
            }
            previous = Some(offset);
            observations.push((offset, text));
        }
        Ok(observations)
    }
}

fn store_observations(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    bytes: u64,
    observations: &[crate::sources::icy::IcyObservation],
) -> Result<()> {
    if observations.len() > crate::sources::icy::MAX_ICY_OBSERVATIONS {
        return Err(Error::StorageIntegrity);
    }
    let mut previous = None;
    for (ordinal, observation) in observations.iter().enumerate() {
        if observation.text.is_empty()
            || observation.text.len() > crate::sources::icy::MAX_ICY_BLOCK
            || observation.text.chars().any(crate::sources::unsafe_display)
            || observation.audio_offset > bytes
            || previous.is_some_and(|previous: u64| observation.audio_offset <= previous)
        {
            return Err(Error::StorageIntegrity);
        }
        previous = Some(observation.audio_offset);
        tx.execute(
            "INSERT INTO recording_observations(recording_id, ordinal, audio_offset, text) VALUES (?1, ?2, ?3, ?4)",
            params![
                id,
                i64::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?,
                i64::try_from(observation.audio_offset).map_err(|_| Error::StorageIntegrity)?,
                observation.text,
            ],
        )?;
    }
    Ok(())
}

fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(column)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn optional_unsigned(row: &rusqlite::Row<'_>, column: usize) -> Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|value| u64::try_from(value).map_err(|_| Error::StorageIntegrity))
        .transpose()
}

pub(crate) fn validate_object_key(key: &str) -> Result<()> {
    if key.len() != 32
        || !key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        },
    )
}
