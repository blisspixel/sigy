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

/// Bytes assigned to the one open segment. The radio cap stays 256 MiB.
pub(crate) const OPEN_SEGMENT_CEILING: u64 = 32 * 1024 * 1024;
/// Candidate uncommitted receive window. Not a measured durability result.
pub(crate) const SEGMENT_RECEIVE_WINDOW: std::time::Duration =
    std::time::Duration::from_millis(5_000);

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

/// Radio attempts stay within 15 minutes and 256 MiB. An episode reserves its full ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingProfile {
    Radio,
    Episode,
}

impl RecordingProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Radio => "radio",
            Self::Episode => "episode",
        }
    }
}

impl std::str::FromStr for RecordingProfile {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "radio" => Ok(Self::Radio),
            "episode" => Ok(Self::Episode),
            _ => Err(Error::StorageIntegrity),
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
    pub profile: RecordingProfile,
    pub escrow_bytes: u64,
    pub open_ceiling: u64,
    pub open_object_key: Option<String>,
    pub lease_renewals: u64,
    pub intervals: Vec<RecordingInterval>,
    pub gaps: Vec<RecordingGap>,
    #[serde(default)]
    pub holds: Vec<RecordingHold>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapCause {
    Disconnect,
    Recovery,
    CodecChange,
    RefusedRenewal,
    CapturePause,
    BackwardClock,
    LateStart,
}

impl GapCause {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disconnect => "disconnect",
            Self::Recovery => "recovery",
            Self::CodecChange => "codec_change",
            Self::RefusedRenewal => "refused_renewal",
            Self::CapturePause => "capture_pause",
            Self::BackwardClock => "backward_clock",
            Self::LateStart => "late_start",
        }
    }

    #[must_use]
    pub const fn seek_denial(self) -> &'static str {
        match self {
            Self::Disconnect => "seek is inside a disconnect gap",
            Self::Recovery => "seek is inside a recovery gap",
            Self::CodecChange => "seek is inside a codec change gap",
            Self::RefusedRenewal => "seek is inside a refused renewal gap",
            Self::CapturePause => "seek is inside a capture pause gap",
            Self::BackwardClock => "seek is inside a backward clock gap",
            Self::LateStart => "seek is inside a late start gap",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "disconnect" => Ok(Self::Disconnect),
            "recovery" => Ok(Self::Recovery),
            "codec_change" => Ok(Self::CodecChange),
            "refused_renewal" => Ok(Self::RefusedRenewal),
            "capture_pause" => Ok(Self::CapturePause),
            "backward_clock" => Ok(Self::BackwardClock),
            "late_start" => Ok(Self::LateStart),
            _ => Err(Error::StorageIntegrity),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingGap {
    pub ordinal: u32,
    pub cause: GapCause,
    pub start_us: u64,
    pub end_us: u64,
}

#[must_use]
pub fn blocking_gap(gaps: &[RecordingGap], seek_us: u64) -> Option<&RecordingGap> {
    gaps.iter()
        .find(|gap| seek_us >= gap.start_us && seek_us < gap.end_us)
}

pub(in crate::storage) fn journal_suffix_gap(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    cause: GapCause,
) -> Result<bool> {
    let planned_seconds: Option<i64> = tx
        .query_row(
            "SELECT duration_seconds FROM recordings WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(planned_seconds) = planned_seconds else {
        return Ok(false);
    };
    let planned_us = planned_seconds
        .checked_mul(1_000_000)
        .ok_or(Error::StorageIntegrity)?;
    let interval_end: i64 = tx.query_row(
        "SELECT COALESCE(MAX(decoded_end_us), 0) FROM recording_intervals WHERE recording_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    let gap_end: i64 = tx.query_row(
        "SELECT COALESCE(MAX(end_us), 0) FROM recording_gaps WHERE recording_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    let start_us = interval_end.max(gap_end);
    if planned_us <= start_us {
        return Ok(false);
    }
    let ordinal: i64 = tx.query_row(
        "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM recording_gaps WHERE recording_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO recording_gaps(recording_id, ordinal, cause, start_us, end_us) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, ordinal, cause.as_str(), start_us, planned_us],
    )?;
    Ok(true)
}

pub(in crate::storage) fn journal_prefix_gap(
    tx: &rusqlite::Connection,
    id: &str,
    late_us: i64,
) -> Result<()> {
    if late_us <= 0 {
        return Ok(());
    }
    let planned_seconds: i64 = tx.query_row(
        "SELECT duration_seconds FROM recordings WHERE id = ?1",
        [id],
        |row| row.get(0),
    )?;
    let planned_us = planned_seconds
        .checked_mul(1_000_000)
        .ok_or(Error::StorageIntegrity)?;
    if late_us >= planned_us {
        return Err(Error::InvalidInput(
            "capture is outside its acquisition window",
        ));
    }
    tx.execute(
        "INSERT INTO recording_gaps(recording_id, ordinal, cause, start_us, end_us) VALUES (?1, 0, ?2, 0, ?3)",
        params![id, GapCause::LateStart.as_str(), late_us],
    )?;
    Ok(())
}

fn prefix_end_us(tx: &rusqlite::Connection, id: &str) -> Result<i64> {
    tx.query_row(
        "SELECT COALESCE((SELECT end_us FROM recording_gaps WHERE recording_id = ?1 AND start_us = 0), 0)",
        [id],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn unreleased_bytes(tx: &rusqlite::Transaction<'_>, id: &str, sealed: i64) -> Result<i64> {
    let released: i64 = tx.query_row(
        "SELECT COALESCE(SUM(byte_length), 0) FROM recording_releases WHERE recording_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    sealed.checked_sub(released).ok_or(Error::StorageIntegrity)
}

fn note_segment_clock(tx: &rusqlite::Transaction<'_>, id: &str, ordinal: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO recording_segment_clocks(recording_id, ordinal, sealed_ms) VALUES (?1, ?2, ?3)",
        params![id, ordinal, now_ms()?],
    )?;
    Ok(())
}

fn end_with_gap(
    tx: &rusqlite::Transaction<'_>,
    job: CaptureJob,
    cause: GapCause,
    event: CaptureEvent,
    reason: &str,
    recorded_ms: i64,
) -> Result<()> {
    journal_suffix_gap(tx, job.version.id(), cause)?;
    tx.execute(
        "UPDATE recordings SET escrow_bytes = escrow_bytes + open_ceiling, open_ceiling = 0, open_object_key = NULL WHERE id = ?1 AND open_ceiling > 0",
        [job.version.id()],
    )?;
    journal::transition(tx, job, event, reason, recorded_ms)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingInterval {
    pub ordinal: u32,
    pub decoded_start_us: u64,
    pub decoded_end_us: u64,
    pub byte_start: u64,
    pub byte_end: u64,
    pub object_key: String,
    pub sha256: String,
    pub format: String,
    pub ceiling_bytes: u64,
    /// The file has been deleted. The interval row remains.
    #[serde(default)]
    pub released: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingHold {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub segments: Vec<u32>,
    pub gaps: Vec<u32>,
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
    pub segments_sealed: bool,
}

pub(crate) enum SegmentOpen {
    Opened {
        version: CaptureVersion,
        object_key: String,
        ceiling: u64,
        ordinal: u32,
    },
    BudgetHeld,
}

pub(crate) struct SegmentRelease {
    pub id: String,
    pub ordinal: i64,
    pub object_key: String,
    pub byte_length: i64,
    pub storage_state: String,
}

pub(crate) struct SegmentSeal {
    pub bytes: u64,
    pub sha256: String,
    pub format: &'static str,
    pub decoded_microseconds: u64,
}

impl Store {
    pub(crate) fn clock_ms() -> Result<i64> {
        now_ms()
    }

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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = admit_recording_in(
            &tx,
            &RecordingInsert {
                id,
                source,
                seconds,
                maximum,
                retention,
                metadata,
            },
        )?;
        tx.commit()?;
        Ok(job)
    }

    /// # Errors
    /// Rejects malformed identifiers or corrupt stored records.
    pub fn recording(&self, id: &str) -> Result<Recording> {
        validate_key(id, "recording ID")?;
        let mut statement = self.connection.prepare("SELECT c.source_revision, c.state, r.object_key, r.duration_seconds, c.maximum_bytes, r.retention, r.storage_state, r.charged_bytes, r.media_bytes, r.sha256, r.format, r.decoded_microseconds, r.end_reason, r.processing_receipt, r.failure_detail, r.profile, r.escrow_bytes, r.open_ceiling, r.open_object_key, r.lease_renewals FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.id = ?1")?;
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
            profile: row.get::<_, String>(15)?.parse()?,
            escrow_bytes: unsigned(row, 16)?,
            open_ceiling: unsigned(row, 17)?,
            open_object_key: row.get(18)?,
            lease_renewals: unsigned(row, 19)?,
            intervals: Vec::new(),
            gaps: Vec::new(),
            holds: Vec::new(),
        };
        validate_object_key(&record.object_key)?;
        if let Some(key) = &record.open_object_key {
            validate_object_key(key)?;
        }
        drop(rows);
        drop(statement);
        let mut record = record;
        record.intervals = self.recording_intervals(id)?;
        record.gaps = self.recording_gaps(id)?;
        record.holds = self.recording_holds(id)?;
        if record.failure_detail.as_ref().is_some_and(|text| {
            text.len() > 256 || !text.is_ascii() || text.chars().any(char::is_control)
        }) {
            return Err(Error::StorageIntegrity);
        }
        Ok(record)
    }

    fn recording_intervals(&self, id: &str) -> Result<Vec<RecordingInterval>> {
        let mut statement = self.connection.prepare("SELECT i.ordinal, i.decoded_start_us, i.decoded_end_us, i.byte_start, i.byte_end, i.object_key, i.sha256, i.format, i.ceiling_bytes, EXISTS(SELECT 1 FROM recording_releases AS x WHERE x.recording_id = i.recording_id AND x.segment_ordinal = i.ordinal) FROM recording_intervals AS i WHERE i.recording_id = ?1 ORDER BY i.ordinal")?;
        let rows = statement
            .query_map([id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut intervals = Vec::with_capacity(rows.len());
        for (index, row) in rows.into_iter().enumerate() {
            let ordinal = u32::try_from(row.0).map_err(|_| Error::StorageIntegrity)?;
            if usize::try_from(ordinal).map_err(|_| Error::StorageIntegrity)? != index {
                return Err(Error::StorageIntegrity);
            }
            let interval = RecordingInterval {
                ordinal,
                decoded_start_us: u64::try_from(row.1).map_err(|_| Error::StorageIntegrity)?,
                decoded_end_us: u64::try_from(row.2).map_err(|_| Error::StorageIntegrity)?,
                byte_start: u64::try_from(row.3).map_err(|_| Error::StorageIntegrity)?,
                byte_end: u64::try_from(row.4).map_err(|_| Error::StorageIntegrity)?,
                object_key: row.5,
                sha256: row.6,
                format: row.7,
                ceiling_bytes: u64::try_from(row.8).map_err(|_| Error::StorageIntegrity)?,
                released: row.9 != 0,
            };
            validate_object_key(&interval.object_key)?;
            if interval.decoded_end_us <= interval.decoded_start_us
                || interval.byte_end <= interval.byte_start
                || interval.byte_end - interval.byte_start > interval.ceiling_bytes
                || !matches!(
                    interval.format.as_str(),
                    "mp3" | "aac" | "flac" | "ogg" | "wav"
                )
            {
                return Err(Error::StorageIntegrity);
            }
            intervals.push(interval);
        }
        Ok(intervals)
    }

    fn recording_gaps(&self, id: &str) -> Result<Vec<RecordingGap>> {
        let mut statement = self.connection.prepare(
            "SELECT ordinal, cause, start_us, end_us FROM recording_gaps WHERE recording_id = ?1 ORDER BY ordinal",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut gaps = Vec::with_capacity(rows.len());
        for (index, row) in rows.into_iter().enumerate() {
            let ordinal = u32::try_from(row.0).map_err(|_| Error::StorageIntegrity)?;
            if usize::try_from(ordinal).map_err(|_| Error::StorageIntegrity)? != index {
                return Err(Error::StorageIntegrity);
            }
            let start_us = u64::try_from(row.2).map_err(|_| Error::StorageIntegrity)?;
            let end_us = u64::try_from(row.3).map_err(|_| Error::StorageIntegrity)?;
            if end_us <= start_us {
                return Err(Error::StorageIntegrity);
            }
            gaps.push(RecordingGap {
                ordinal,
                cause: GapCause::parse(&row.1)?,
                start_us,
                end_us,
            });
        }
        Ok(gaps)
    }

    fn recording_holds(&self, id: &str) -> Result<Vec<RecordingHold>> {
        let mut statement = self.connection.prepare(
            "SELECT ordinal, start_us, end_us FROM recording_holds WHERE recording_id = ?1 ORDER BY ordinal",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut holds = Vec::with_capacity(rows.len());
        for (index, row) in rows.into_iter().enumerate() {
            let ordinal = u32::try_from(row.0).map_err(|_| Error::StorageIntegrity)?;
            if usize::try_from(ordinal).map_err(|_| Error::StorageIntegrity)? != index {
                return Err(Error::StorageIntegrity);
            }
            holds.push(RecordingHold {
                ordinal,
                start_us: u64::try_from(row.1).map_err(|_| Error::StorageIntegrity)?,
                end_us: u64::try_from(row.2).map_err(|_| Error::StorageIntegrity)?,
                segments: self.hold_ordinals(
                    "SELECT segment_ordinal FROM recording_hold_segments WHERE recording_id = ?1 AND hold_ordinal = ?2 ORDER BY segment_ordinal",
                    id,
                    row.0,
                )?,
                gaps: self.hold_ordinals(
                    "SELECT gap_ordinal FROM recording_hold_gaps WHERE recording_id = ?1 AND hold_ordinal = ?2 ORDER BY gap_ordinal",
                    id,
                    row.0,
                )?,
            });
        }
        Ok(holds)
    }

    fn hold_ordinals(&self, sql: &str, id: &str, hold: i64) -> Result<Vec<u32>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement
            .query_map(params![id, hold], |row| row.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|value| u32::try_from(value).map_err(|_| Error::StorageIntegrity))
            .collect()
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
        if publication.segments_sealed {
            return self.complete_segmented_recording(expected, publication);
        }
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
        let bytes = i64::try_from(publication.bytes).map_err(|_| Error::StorageIntegrity)?;
        let decoded =
            i64::try_from(publication.decoded_microseconds).map_err(|_| Error::StorageIntegrity)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, expected.id())?.ok_or(Error::NotFound)?;
        if job.version != *expected {
            return Err(Error::StaleCapture);
        }
        let profile: String = tx.query_row(
            "SELECT profile FROM recordings WHERE id = ?1",
            [expected.id()],
            |row| row.get(0),
        )?;
        if profile == RecordingProfile::Episode.as_str() && publication.end_reason != "end_of_body"
        {
            return Err(Error::StorageIntegrity);
        }
        let changed = tx.execute("UPDATE recordings SET storage_state = 'retained', charged_bytes = ?2, media_bytes = ?2, sha256 = ?3, format = ?4, decoded_microseconds = ?5, end_reason = ?6, http_route_json = ?7, escrow_bytes = 0, open_ceiling = 0, open_object_key = NULL WHERE id = ?1 AND storage_state = 'reserved' AND charged_bytes >= ?2 AND open_ceiling = 0", params![expected.id(), bytes, publication.sha256, publication.format, decoded, publication.end_reason, route_json])?;
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
        let origin = prefix_end_us(&tx, expected.id())?;
        let timeline_end = origin.checked_add(decoded).ok_or(Error::StorageIntegrity)?;
        tx.execute(
            "INSERT INTO recording_intervals(recording_id, ordinal, decoded_start_us, decoded_end_us, byte_start, byte_end, object_key, sha256, format, ceiling_bytes) SELECT ?1, 0, ?2, ?3, 0, ?4, object_key, ?5, ?6, byte_ceiling FROM recordings WHERE id = ?1",
            params![
                expected.id(),
                origin,
                timeline_end,
                bytes,
                publication.sha256,
                publication.format
            ],
        )?;
        note_segment_clock(&tx, expected.id(), 0)?;
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

    pub(crate) fn connect_recording(
        &mut self,
        expected: &CaptureVersion,
    ) -> Result<CaptureVersion> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = locked_job(&tx, expected)?;
        if job.state != CaptureState::Starting {
            return Err(Error::StorageIntegrity);
        }
        let now = now_ms()?;
        let job = journal::transition(&tx, job, CaptureEvent::Connected, "socket_open", now)?;
        let lease = lease_deadline(now, job.plan.ends_ms())?;
        if tx.execute(
            "UPDATE recordings SET lease_expires_ms = ?2 WHERE id = ?1 AND open_ceiling = 0",
            params![job.version.id(), lease],
        )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        tx.commit()?;
        Ok(job.version)
    }

    pub(crate) fn open_segment(&mut self, expected: &CaptureVersion) -> Result<SegmentOpen> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = locked_job(&tx, expected)?;
        if job.state != CaptureState::Running {
            return Err(Error::StorageIntegrity);
        }
        let (escrow, open_ceiling, object_key): (i64, i64, String) = tx.query_row(
            "SELECT escrow_bytes, open_ceiling, object_key FROM recordings WHERE id = ?1 AND storage_state = 'reserved'",
            [job.version.id()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if open_ceiling != 0 {
            return Err(Error::StorageIntegrity);
        }
        let ceiling = i64::try_from(OPEN_SEGMENT_CEILING).map_err(|_| Error::StorageIntegrity)?;
        if escrow < ceiling {
            return Ok(SegmentOpen::BudgetHeld);
        }
        let ordinal: i64 = tx.query_row(
            "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM recording_intervals WHERE recording_id = ?1",
            [job.version.id()],
            |row| row.get(0),
        )?;
        let segment_key = if ordinal == 0 {
            object_key
        } else {
            fresh_object_key(&tx)?
        };
        if tx.execute(
            "UPDATE recordings SET escrow_bytes = escrow_bytes - ?2, open_ceiling = ?2, open_object_key = ?3 WHERE id = ?1 AND open_ceiling = 0 AND escrow_bytes >= ?2",
            params![job.version.id(), ceiling, segment_key],
        )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        let ordinal = u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?;
        tx.commit()?;
        Ok(SegmentOpen::Opened {
            version: job.version,
            object_key: segment_key,
            ceiling: OPEN_SEGMENT_CEILING,
            ordinal,
        })
    }

    pub(crate) fn seal_segment(
        &mut self,
        expected: &CaptureVersion,
        segment: &SegmentSeal,
    ) -> Result<CaptureVersion> {
        self.note_clock(expected.id(), now_ms()?)?;
        if segment.bytes == 0
            || segment.decoded_microseconds == 0
            || segment.sha256.len() != 64
            || !segment
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || !matches!(segment.format, "mp3" | "aac" | "flac" | "ogg" | "wav")
        {
            return Err(Error::StorageIntegrity);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = locked_job(&tx, expected)?;
        if job.state != CaptureState::Running {
            return Err(Error::StorageIntegrity);
        }
        let (open_ceiling, open_key): (i64, Option<String>) = tx.query_row(
            "SELECT open_ceiling, open_object_key FROM recordings WHERE id = ?1 AND storage_state = 'reserved'",
            [job.version.id()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let bytes = i64::try_from(segment.bytes).map_err(|_| Error::StorageIntegrity)?;
        let decoded =
            i64::try_from(segment.decoded_microseconds).map_err(|_| Error::StorageIntegrity)?;
        if open_ceiling < bytes || open_key.is_none() {
            return Err(Error::StorageIntegrity);
        }
        let (byte_start, interval_end, ordinal): (i64, i64, i64) = tx.query_row(
            "SELECT COALESCE(MAX(byte_end), 0), COALESCE(MAX(decoded_end_us), 0), COALESCE(MAX(ordinal) + 1, 0) FROM recording_intervals WHERE recording_id = ?1",
            [job.version.id()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let prefix = if ordinal == 0 {
            prefix_end_us(&tx, job.version.id())?
        } else {
            0
        };
        let decoded_start = interval_end.max(prefix);
        let byte_end = byte_start
            .checked_add(bytes)
            .ok_or(Error::StorageIntegrity)?;
        let decoded_end = decoded_start
            .checked_add(decoded)
            .ok_or(Error::StorageIntegrity)?;
        let previous_format: Option<String> = tx
            .query_row(
                "SELECT format FROM recording_intervals WHERE recording_id = ?1 ORDER BY ordinal DESC LIMIT 1",
                [job.version.id()],
                |row| row.get(0),
            )
            .optional()?;
        let covered_by_gap: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM recording_gaps WHERE recording_id = ?1 AND start_us < ?3 AND ?2 < end_us)",
            params![job.version.id(), decoded_start, decoded_end],
            |row| row.get(0),
        )?;
        if covered_by_gap {
            return Err(Error::StorageIntegrity);
        }
        if previous_format.is_some_and(|format| format != segment.format) {
            let recorded = job.updated_ms;
            end_with_gap(
                &tx,
                job,
                GapCause::CodecChange,
                CaptureEvent::Fail,
                "codec_change",
                recorded,
            )?;
            tx.commit()?;
            return Err(Error::InvalidInput("codec change is a gap"));
        }
        tx.execute(
            "INSERT INTO recording_intervals(recording_id, ordinal, decoded_start_us, decoded_end_us, byte_start, byte_end, object_key, sha256, format, ceiling_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                job.version.id(),
                ordinal,
                decoded_start,
                decoded_end,
                byte_start,
                byte_end,
                open_key,
                segment.sha256,
                segment.format,
                open_ceiling,
            ],
        )?;
        note_segment_clock(&tx, job.version.id(), ordinal)?;
        if tx.execute(
            "UPDATE recordings SET escrow_bytes = escrow_bytes + open_ceiling - ?2, open_ceiling = 0, open_object_key = NULL WHERE id = ?1 AND open_ceiling >= ?2",
            params![job.version.id(), bytes],
        )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        tx.commit()?;
        Ok(job.version)
    }

    pub(crate) fn release_open_segment(
        &mut self,
        expected: &CaptureVersion,
    ) -> Result<CaptureVersion> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = locked_job(&tx, expected)?;
        if job.state != CaptureState::Running {
            return Err(Error::StorageIntegrity);
        }
        if tx.execute(
            "UPDATE recordings SET escrow_bytes = escrow_bytes + open_ceiling, open_ceiling = 0, open_object_key = NULL WHERE id = ?1 AND open_ceiling > 0",
            [job.version.id()],
        )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        tx.commit()?;
        Ok(job.version)
    }

    pub(crate) fn renew_segment_lease(
        &mut self,
        expected: &CaptureVersion,
    ) -> Result<CaptureVersion> {
        self.note_clock(expected.id(), now_ms()?)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = locked_job(&tx, expected)?;
        if job.state != CaptureState::Running {
            return Err(Error::StorageIntegrity);
        }
        let current: Option<i64> = tx.query_row(
            "SELECT lease_expires_ms FROM recordings WHERE id = ?1",
            [job.version.id()],
            |row| row.get(0),
        )?;
        let now = now_ms()?;
        if current.is_some_and(|expires| expires < now) {
            let recorded = job.updated_ms;
            end_with_gap(
                &tx,
                job,
                GapCause::RefusedRenewal,
                CaptureEvent::Fail,
                "refused_renewal",
                recorded,
            )?;
            tx.commit()?;
            return Err(Error::InvalidInput("segment lease renewal was refused"));
        }
        let proposed = lease_deadline(now, job.plan.ends_ms())?;
        if current.is_some_and(|current| proposed <= current) {
            return Err(Error::RequestState);
        }
        if tx.execute(
            "UPDATE recordings SET lease_expires_ms = ?2, lease_renewals = lease_renewals + 1 WHERE id = ?1",
            params![job.version.id(), proposed],
        )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        tx.commit()?;
        Ok(job.version)
    }

    fn complete_segmented_recording(
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
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
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
        let bytes = i64::try_from(publication.bytes).map_err(|_| Error::StorageIntegrity)?;
        let decoded =
            i64::try_from(publication.decoded_microseconds).map_err(|_| Error::StorageIntegrity)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = locked_job(&tx, expected)?;
        if job.state != CaptureState::Running {
            return Err(Error::StorageIntegrity);
        }
        let (escrow, open_ceiling, open_key, maximum): (i64, i64, Option<String>, i64) = tx
            .query_row(
                "SELECT r.escrow_bytes, r.open_ceiling, r.open_object_key, c.maximum_bytes FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.id = ?1 AND r.storage_state = 'reserved'",
                [job.version.id()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        if open_ceiling != 0 || open_key.is_some() {
            return Err(Error::StorageIntegrity);
        }
        let (sealed, measured, mismatched): (i64, i64, bool) = tx.query_row(
            "SELECT COALESCE(SUM(byte_end - byte_start), 0), COALESCE(SUM(decoded_end_us - decoded_start_us), 0), EXISTS(SELECT 1 FROM recording_intervals WHERE recording_id = ?1 AND format != ?2) FROM recording_intervals WHERE recording_id = ?1",
            params![job.version.id(), publication.format],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if mismatched
            || sealed != bytes
            || measured != decoded
            || escrow.checked_add(sealed) != Some(maximum)
        {
            return Err(Error::StorageIntegrity);
        }
        let charge = unreleased_bytes(&tx, job.version.id(), sealed)?;
        if tx.execute(
            "UPDATE recordings SET storage_state = 'retained', charged_bytes = ?2, media_bytes = ?2, sha256 = ?3, format = ?4, decoded_microseconds = ?5, end_reason = ?6, http_route_json = ?7, escrow_bytes = 0, open_ceiling = 0, open_object_key = NULL WHERE id = ?1 AND storage_state = 'reserved' AND open_ceiling = 0",
            params![
                job.version.id(),
                charge,
                publication.sha256,
                publication.format,
                decoded,
                publication.end_reason,
                route_json,
            ],
        )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        let requested: i64 = tx.query_row(
            "SELECT metadata_requested FROM recordings WHERE id = ?1",
            [job.version.id()],
            |row| row.get(0),
        )?;
        if requested == 0 && !publication.observations.is_empty() {
            return Err(Error::StorageIntegrity);
        }
        store_observations(
            &tx,
            job.version.id(),
            publication.bytes,
            &publication.observations,
        )?;
        let now = now_ms()?;
        let stopped = journal::transition(&tx, job, CaptureEvent::Stop, "media_received", now)?;
        journal::transition(
            &tx,
            stopped,
            CaptureEvent::Finalized,
            "segments_sealed",
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

    pub(crate) fn note_clock(&mut self, id: &str, observed_ms: i64) -> Result<()> {
        validate_key(id, "capture ID")?;
        let updated: Option<i64> = self
            .connection
            .query_row(
                "SELECT updated_ms FROM capture_jobs WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(updated) = updated else {
            return Err(Error::NotFound);
        };
        if observed_ms >= updated {
            return Ok(());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, id)?.ok_or(Error::NotFound)?;
        if !job.state.is_active() {
            return Err(Error::RequestState);
        }
        end_with_gap(
            &tx,
            job,
            GapCause::BackwardClock,
            CaptureEvent::Fail,
            "backward_clock",
            updated,
        )?;
        tx.commit()?;
        Err(Error::InvalidInput("clock moved backward"))
    }

    pub(crate) fn pause_capture(&mut self, id: &str) -> Result<()> {
        validate_key(id, "capture ID")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, id)?.ok_or(Error::NotFound)?;
        if job.state != CaptureState::Running {
            return Err(Error::RequestState);
        }
        end_with_gap(
            &tx,
            job,
            GapCause::CapturePause,
            CaptureEvent::Lost,
            "capture_pause",
            now_ms()?,
        )?;
        tx.commit()?;
        Ok(())
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
            "UPDATE recordings SET failure_detail = ?2, escrow_bytes = escrow_bytes + open_ceiling, open_ceiling = 0 WHERE id = ?1",
            params![expected.id(), detail],
        )?;
        if detail == "body interrupted; partial body is unverified" {
            journal_suffix_gap(&tx, expected.id(), GapCause::Disconnect)?;
        }
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

    /// Protects every published segment the range intersects. The open tail is not a segment.
    /// # Errors
    /// Rejects an empty range, a missing recording, or a range with no retained segment.
    pub fn hold_range(&mut self, id: &str, start_us: u64, end_us: u64) -> Result<()> {
        validate_key(id, "recording ID")?;
        if start_us >= end_us {
            return Err(Error::InvalidInput("hold range is empty"));
        }
        let record = self.recording(id)?;
        if record.storage_state == "deleted" || record.storage_state == "deleting" {
            return Err(Error::RequestState);
        }
        let segments: Vec<u32> = record
            .intervals
            .iter()
            .filter(|interval| {
                !interval.released
                    && interval.decoded_start_us < end_us
                    && start_us < interval.decoded_end_us
            })
            .map(|interval| interval.ordinal)
            .collect();
        if segments.is_empty() {
            return Err(Error::InvalidInput(
                "hold does not intersect retained audio",
            ));
        }
        let gaps: Vec<u32> = record
            .gaps
            .iter()
            .filter(|gap| gap.start_us < end_us && start_us < gap.end_us)
            .map(|gap| gap.ordinal)
            .collect();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ordinal: i64 = tx.query_row(
            "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM recording_holds WHERE recording_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO recording_holds(recording_id, ordinal, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
            params![id, ordinal, i64::try_from(start_us).map_err(|_| Error::StorageIntegrity)?, i64::try_from(end_us).map_err(|_| Error::StorageIntegrity)?],
        )?;
        for segment in segments {
            tx.execute(
                "INSERT INTO recording_hold_segments(recording_id, hold_ordinal, segment_ordinal) VALUES (?1, ?2, ?3)",
                params![id, ordinal, segment],
            )?;
        }
        for gap in gaps {
            tx.execute(
                "INSERT INTO recording_hold_gaps(recording_id, hold_ordinal, gap_ordinal) VALUES (?1, ?2, ?3)",
                params![id, ordinal, gap],
            )?;
        }
        tx.commit()?;
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
        if tx.execute("UPDATE recordings SET storage_state = 'deleting' WHERE id = ?1 AND storage_state != 'deleted' AND NOT EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.recording_id = recordings.id AND j.state IN ('running', 'cancelling')) AND (NOT ?2 OR (retention = 'temporary' AND storage_state IN ('reserved', 'retained')))", params![id, pruning])? != 1 {
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
        if tx.execute("UPDATE recordings SET storage_state = 'deleted', charged_bytes = 0, escrow_bytes = 0, open_ceiling = 0 WHERE id = ?1 AND storage_state IN ('deleting', 'deleted')", [id])? != 1 { return Err(Error::RequestState); }
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

    pub(crate) fn next_segment_release(
        &self,
        pressure: bool,
        now: i64,
    ) -> Result<Option<SegmentRelease>> {
        let cutoff = now.saturating_sub(i64::from(self.dvr_status()?.retention_days) * 86_400_000);
        let row = self.connection.query_row(
            "SELECT r.id, i.ordinal, i.object_key, i.byte_end - i.byte_start, r.storage_state FROM recording_intervals AS i JOIN recordings AS r ON r.id = i.recording_id JOIN recording_segment_clocks AS k ON k.recording_id = i.recording_id AND k.ordinal = i.ordinal WHERE r.retention = 'temporary' AND r.storage_state IN ('reserved', 'retained') AND NOT EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.recording_id = r.id AND j.state IN ('running', 'cancelling')) AND NOT EXISTS (SELECT 1 FROM recording_releases AS x WHERE x.recording_id = i.recording_id AND x.segment_ordinal = i.ordinal) AND NOT EXISTS (SELECT 1 FROM recording_hold_segments AS h WHERE h.recording_id = i.recording_id AND h.segment_ordinal = i.ordinal) AND (r.open_object_key IS NULL OR r.open_object_key != i.object_key) AND ((?1 AND r.storage_state = 'retained') OR (NOT ?1 AND k.sealed_ms <= ?2)) ORDER BY k.sealed_ms, r.id, i.ordinal LIMIT 1",
            params![pressure, cutoff],
            |row| {
                Ok(SegmentRelease {
                    id: row.get(0)?,
                    ordinal: row.get(1)?,
                    object_key: row.get(2)?,
                    byte_length: row.get(3)?,
                    storage_state: row.get(4)?,
                })
            },
        );
        match row {
            Ok(release) => Ok(Some(release)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn mark_segment_released(&mut self, release: &SegmentRelease) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "INSERT INTO recording_releases(recording_id, segment_ordinal, byte_length) VALUES (?1, ?2, ?3)",
            params![release.id, release.ordinal, release.byte_length],
        )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        if release.storage_state == "retained"
            && tx.execute(
                "UPDATE recordings SET charged_bytes = charged_bytes - ?2, media_bytes = media_bytes - ?2 WHERE id = ?1 AND storage_state = 'retained' AND retention = 'temporary' AND charged_bytes >= ?2 AND media_bytes >= ?2",
                params![release.id, release.byte_length],
            )? != 1
        {
            return Err(Error::StorageIntegrity);
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn prune_candidates(&self, pressure: bool) -> Result<Vec<String>> {
        self.prune_candidates_at(pressure, now_ms()?)
    }

    fn prune_candidates_at(&self, pressure: bool, now: i64) -> Result<Vec<String>> {
        let cutoff = now.saturating_sub(i64::from(self.dvr_status()?.retention_days) * 86_400_000);
        let mut query = self.connection.prepare("SELECT r.id FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.retention = 'temporary' AND r.storage_state IN ('reserved', 'retained') AND NOT EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.recording_id = r.id AND j.state IN ('running', 'cancelling')) AND c.state IN ('completed', 'failed', 'interrupted', 'cancelled') AND (?1 OR c.created_ms <= ?2 OR r.processing_receipt IS NOT NULL) ORDER BY c.created_ms, r.id LIMIT 64")?;
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
        let ceiling_mismatch: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.byte_ceiling != c.maximum_bytes)",
            [],
            |row| row.get(0),
        )?;
        if ceiling_mismatch {
            return Err(Error::StorageIntegrity);
        }
        self.published_intervals_match()?;
        self.recorded_gaps_match()?;
        self.segment_retention_matches()?;
        Ok(())
    }

    fn segment_retention_matches(&self) -> Result<()> {
        let broken: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM recording_releases AS x LEFT JOIN recording_intervals AS i ON i.recording_id = x.recording_id AND i.ordinal = x.segment_ordinal WHERE i.recording_id IS NULL OR x.byte_length != i.byte_end - i.byte_start) OR EXISTS(SELECT 1 FROM recording_hold_segments AS h LEFT JOIN recording_intervals AS i ON i.recording_id = h.recording_id AND i.ordinal = h.segment_ordinal WHERE i.recording_id IS NULL) OR EXISTS(SELECT 1 FROM recording_hold_gaps AS g LEFT JOIN recording_gaps AS q ON q.recording_id = g.recording_id AND q.ordinal = g.gap_ordinal WHERE q.recording_id IS NULL)",
            [],
            |row| row.get(0),
        )?;
        if broken {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }

    fn recorded_gaps_match(&self) -> Result<()> {
        let broken: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM recording_gaps AS g LEFT JOIN recordings AS r ON r.id = g.recording_id WHERE r.id IS NULL OR g.end_us <= g.start_us OR g.end_us > r.duration_seconds * 1000000 OR EXISTS(SELECT 1 FROM recording_intervals AS i WHERE i.recording_id = g.recording_id AND i.decoded_start_us < g.end_us AND g.start_us < i.decoded_end_us))",
            [],
            |row| row.get(0),
        )?;
        if broken {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }

    fn published_intervals_match(&self) -> Result<()> {
        let ceiling = i64::try_from(OPEN_SEGMENT_CEILING).map_err(|_| Error::StorageIntegrity)?;
        let broken: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE (r.storage_state = 'reserved' AND r.escrow_bytes + r.open_ceiling + COALESCE((SELECT SUM(i.byte_end - i.byte_start) FROM recording_intervals i WHERE i.recording_id = r.id), 0) != c.maximum_bytes) OR (r.storage_state = 'retained' AND (r.escrow_bytes != 0 OR r.open_ceiling != 0 OR r.open_object_key IS NOT NULL)) OR (r.open_ceiling != 0 AND (r.open_ceiling != ?1 OR r.open_object_key IS NULL OR c.state != 'running')) OR (r.storage_state = 'deleted' AND (r.escrow_bytes != 0 OR r.open_ceiling != 0)))",
            [ceiling],
            |row| row.get(0),
        )?;
        if broken {
            return Err(Error::StorageIntegrity);
        }
        let intervals: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM recording_intervals i LEFT JOIN recordings r ON r.id = i.recording_id WHERE r.id IS NULL OR (i.ordinal = 0 AND (i.byte_start != 0 OR i.decoded_start_us != COALESCE((SELECT g.end_us FROM recording_gaps g WHERE g.recording_id = i.recording_id AND g.start_us = 0), 0))) OR (i.ordinal > 0 AND (i.byte_start != (SELECT p.byte_end FROM recording_intervals p WHERE p.recording_id = i.recording_id AND p.ordinal = i.ordinal - 1) OR i.decoded_start_us != (SELECT p.decoded_end_us FROM recording_intervals p WHERE p.recording_id = i.recording_id AND p.ordinal = i.ordinal - 1))) OR (r.media_bytes IS NOT NULL AND (r.media_bytes + COALESCE((SELECT SUM(x.byte_length) FROM recording_releases x WHERE x.recording_id = r.id), 0) != (SELECT COALESCE(SUM(q.byte_end - q.byte_start), 0) FROM recording_intervals q WHERE q.recording_id = r.id) OR r.decoded_microseconds != (SELECT COALESCE(SUM(q.decoded_end_us - q.decoded_start_us), 0) FROM recording_intervals q WHERE q.recording_id = r.id))) OR (r.media_bytes IS NULL AND r.storage_state = 'retained') OR (r.decoded_microseconds IS NOT NULL AND NOT EXISTS (SELECT 1 FROM recording_gaps g WHERE g.recording_id = i.recording_id) AND i.decoded_end_us = r.duration_seconds * 1000000 AND i.decoded_end_us != r.decoded_microseconds))",
            [],
            |row| row.get(0),
        )?;
        if intervals {
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

pub(crate) struct RecordingInsert<'a> {
    pub(crate) id: &'a str,
    pub(crate) source: &'a str,
    pub(crate) seconds: u64,
    pub(crate) maximum: u64,
    pub(crate) retention: Retention,
    pub(crate) metadata: bool,
}

pub(crate) fn admit_recording_in(
    tx: &rusqlite::Connection,
    request: &RecordingInsert<'_>,
) -> Result<Option<CaptureJob>> {
    validate_key(request.id, "recording ID")?;
    validate_key(request.source, "source revision")?;
    let (profile, seconds, maximum) =
        profile_limits(request.seconds, request.maximum, request.metadata)?;
    let requested = i64::from(request.metadata);
    let existing: Option<(String, u64, u64, String, i64, String)> = tx.query_row(
        "SELECT c.source_revision, r.duration_seconds, c.maximum_bytes, r.initial_retention, r.metadata_requested, r.profile FROM recordings r JOIN capture_jobs c ON c.id = r.id WHERE r.id = ?1",
        [request.id],
        |row| Ok((row.get(0)?, unsigned(row, 1)?, unsigned(row, 2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    ).optional()?;
    if let Some(parameters) = existing {
        if parameters
            != (
                request.source.into(),
                seconds,
                maximum,
                request.retention.as_str().into(),
                requested,
                profile.as_str().into(),
            )
        {
            return Err(Error::IdempotencyConflict);
        }
        return Ok(None);
    }
    let found: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM source_revisions WHERE id = ?1)",
        [request.source],
        |row| row.get(0),
    )?;
    if !found {
        return Err(Error::NotFound);
    }
    let listening: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM listen_sessions WHERE source_revision = ?1 AND state = 'running')",
        [request.source],
        |row| row.get(0),
    )?;
    if listening {
        return Err(Error::InvalidInput("source revision is in use"));
    }
    recording_capacity(tx, maximum)?;
    let now = now_ms()?;
    let ends = now
        .checked_add(i64::try_from(seconds * 1000).map_err(|_| Error::StorageIntegrity)?)
        .ok_or(Error::StorageIntegrity)?;
    let plan = CapturePlan::new(
        request.source,
        now,
        ends,
        i64::try_from(maximum).map_err(|_| Error::StorageIntegrity)?,
    )?;
    let admission = captures::admit(tx, request.id, &plan)?;
    if !admission.newly_created {
        return Err(Error::IdempotencyConflict);
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| Error::InvalidInput("secure random source unavailable"))?;
    let key = hex(&random);
    tx.execute(
        "INSERT INTO recordings(id, object_key, duration_seconds, initial_retention, retention, storage_state, charged_bytes, metadata_requested, profile, byte_ceiling, escrow_bytes) VALUES (?1, ?2, ?3, ?4, ?4, 'reserved', ?5, ?6, ?7, ?5, ?5)",
        params![
            request.id,
            key,
            i64::try_from(seconds).map_err(|_| Error::StorageIntegrity)?,
            request.retention.as_str(),
            plan.maximum_bytes(),
            requested,
            profile.as_str(),
        ],
    )?;
    let job = journal::transition(
        tx,
        admission.job,
        CaptureEvent::Start,
        "recording_admitted",
        now,
    )?;
    Ok(Some(job))
}

pub(in crate::storage) struct ScheduledRecording<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub planned_start_ms: i64,
    pub planned_end_ms: i64,
    pub seconds: u64,
    pub maximum: u64,
    pub now_ms: i64,
}

/// Admit one scheduled window. The plan keeps the civil bounds. A late `now_ms` records a prefix gap.
/// # Errors
/// Rejects a window that is not open, a missing source, quota, or a second job for the same id.
pub(in crate::storage) fn admit_scheduled_recording(
    tx: &rusqlite::Connection,
    request: &ScheduledRecording<'_>,
) -> Result<CaptureJob> {
    if request.now_ms < request.planned_start_ms || request.now_ms >= request.planned_end_ms {
        return Err(Error::InvalidInput(
            "capture is outside its acquisition window",
        ));
    }
    let span = request
        .planned_end_ms
        .checked_sub(request.planned_start_ms)
        .ok_or(Error::StorageIntegrity)?;
    if span != i64::try_from(request.seconds * 1000).map_err(|_| Error::StorageIntegrity)? {
        return Err(Error::StorageIntegrity);
    }
    let (profile, seconds, maximum) = profile_limits(request.seconds, request.maximum, false)?;
    if profile != RecordingProfile::Radio {
        return Err(Error::InvalidInput("recording duration or byte ceiling"));
    }
    let existing: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM recordings WHERE id = ?1)",
        [request.id],
        |row| row.get(0),
    )?;
    if existing {
        return Err(Error::IdempotencyConflict);
    }
    let found: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM source_revisions WHERE id = ?1)",
        [request.source],
        |row| row.get(0),
    )?;
    if !found {
        return Err(Error::NotFound);
    }
    let listening: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM listen_sessions WHERE source_revision = ?1 AND state = 'running')",
        [request.source],
        |row| row.get(0),
    )?;
    if listening {
        return Err(Error::InvalidInput("source revision is in use"));
    }
    recording_capacity(tx, maximum)?;
    let plan = CapturePlan::new(
        request.source,
        request.planned_start_ms,
        request.planned_end_ms,
        i64::try_from(maximum).map_err(|_| Error::StorageIntegrity)?,
    )?;
    let admission = captures::admit(tx, request.id, &plan)?;
    if !admission.newly_created {
        return Err(Error::IdempotencyConflict);
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| Error::InvalidInput("secure random source unavailable"))?;
    let key = hex(&random);
    tx.execute(
        "INSERT INTO recordings(id, object_key, duration_seconds, initial_retention, retention, storage_state, charged_bytes, metadata_requested, profile, byte_ceiling, escrow_bytes) VALUES (?1, ?2, ?3, 'temporary', 'temporary', 'reserved', ?4, 0, ?5, ?4, ?4)",
        params![
            request.id,
            key,
            i64::try_from(seconds).map_err(|_| Error::StorageIntegrity)?,
            plan.maximum_bytes(),
            profile.as_str(),
        ],
    )?;
    let elapsed_ms = request.now_ms - request.planned_start_ms;
    let prefix_us = elapsed_ms
        .checked_mul(1000)
        .ok_or(Error::StorageIntegrity)?;
    journal_prefix_gap(tx, request.id, prefix_us)?;
    journal::transition(
        tx,
        admission.job,
        CaptureEvent::Start,
        "schedule_admitted",
        request.now_ms,
    )
}

fn locked_job(connection: &rusqlite::Connection, expected: &CaptureVersion) -> Result<CaptureJob> {
    let job = journal::read_job(connection, expected.id())?.ok_or(Error::NotFound)?;
    if job.version != *expected {
        return Err(Error::StaleCapture);
    }
    Ok(job)
}

fn lease_deadline(now: i64, ends_ms: i64) -> Result<i64> {
    let window =
        i64::try_from(SEGMENT_RECEIVE_WINDOW.as_millis()).map_err(|_| Error::StorageIntegrity)?;
    // The close follows the receive cut. A second window lets that close renew in time.
    let horizon = window.saturating_mul(2);
    Ok(now.saturating_add(horizon).min(ends_ms))
}

fn fresh_object_key(connection: &rusqlite::Connection) -> Result<String> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| Error::InvalidInput("secure random source unavailable"))?;
    let key = hex(&random);
    let taken: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM recordings WHERE object_key = ?1 OR open_object_key = ?1) OR EXISTS(SELECT 1 FROM recording_intervals WHERE object_key = ?1)",
        [&key],
        |row| row.get(0),
    )?;
    if taken {
        return Err(Error::StorageIntegrity);
    }
    Ok(key)
}

fn profile_limits(
    seconds: u64,
    maximum: u64,
    metadata: bool,
) -> Result<(RecordingProfile, u64, u64)> {
    if !metadata
        && seconds == crate::sources::http::EPISODE_DURATION.as_secs()
        && maximum == crate::sources::http::EPISODE_BODY_BYTES
    {
        return Ok((RecordingProfile::Episode, seconds, maximum));
    }
    if (1..=crate::sources::http::MAXIMUM_DURATION.as_secs()).contains(&seconds)
        && (1..=crate::sources::http::MAXIMUM_BODY_BYTES).contains(&maximum)
    {
        return Ok((RecordingProfile::Radio, seconds, maximum));
    }
    Err(Error::InvalidInput("recording duration or byte ceiling"))
}

fn recording_capacity(tx: &rusqlite::Connection, maximum: u64) -> Result<()> {
    let (quota, configured): (u64, bool) = tx.query_row(
        "SELECT quota_bytes, decoder IS NOT NULL FROM dvr_policy WHERE singleton = 1",
        [],
        |row| Ok((unsigned(row, 0)?, row.get(1)?)),
    )?;
    let charged: u64 = tx.query_row(
        "SELECT coalesce(sum(charged_bytes), 0) FROM recordings",
        [],
        |row| unsigned(row, 0),
    )?;
    let active: u32 = tx.query_row(
        "SELECT count(*) FROM capture_jobs WHERE state IN ('starting', 'running', 'retrying', 'stopping')",
        [],
        |row| row.get(0),
    )?;
    if active >= captures::MAX_ACTIVE_CAPTURES {
        return Err(Error::CaptureCapacity);
    }
    let pending: u32 = tx.query_row(
        "SELECT count(*) FROM capture_jobs WHERE state IN ('scheduled', 'starting', 'running', 'retrying', 'stopping', 'interrupted')",
        [],
        |row| row.get(0),
    )?;
    if pending >= captures::MAX_PENDING_CAPTURES {
        return Err(Error::CaptureCapacity);
    }
    if !configured || maximum > quota.checked_sub(charged).ok_or(Error::StorageIntegrity)? {
        return Err(Error::StorageQuota);
    }
    Ok(())
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
