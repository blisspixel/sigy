use rusqlite::{Connection, OptionalExtension, params};
use sigy_core::capture::{CaptureEvent, CaptureState};

use super::{CaptureJob, CapturePlan, CaptureVersion};
use crate::{Error, Result, storage::validate_key};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRecord {
    pub revision: i64,
    pub generation: i64,
    pub previous_state: Option<CaptureState>,
    pub state: CaptureState,
    pub event: Option<CaptureEvent>,
    pub reason: String,
    pub recorded_ms: i64,
}

pub(in crate::storage) fn read_job(
    connection: &Connection,
    id: &str,
) -> Result<Option<CaptureJob>> {
    let row = connection.query_row("SELECT source_revision, starts_ms, ends_ms, maximum_bytes, state, revision, generation, created_ms, updated_ms FROM capture_jobs WHERE id = ?1", [id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?, row.get::<_, i64>(8)?))
    }).optional()?;
    row.map(
        |(source, starts, ends, bytes, state, revision, generation, created_ms, updated_ms)| {
            validate_key(id, "capture ID").map_err(|_| Error::CaptureIntegrity)?;
            if revision < 0 || generation < 0 || created_ms < 0 || updated_ms < 0 {
                return Err(Error::CaptureIntegrity);
            }
            Ok(CaptureJob {
                version: CaptureVersion {
                    id: id.into(),
                    revision,
                    generation,
                },
                plan: CapturePlan::new(&source, starts, ends, bytes)
                    .map_err(|_| Error::CaptureIntegrity)?,
                state: state.parse().map_err(|_| Error::CaptureIntegrity)?,
                created_ms,
                updated_ms,
            })
        },
    )
    .transpose()
}

pub(in crate::storage) fn transition(
    connection: &Connection,
    mut job: CaptureJob,
    event: CaptureEvent,
    reason: &str,
    recorded: i64,
) -> Result<CaptureJob> {
    let previous = job.state;
    job.state = previous.transition(event)?;
    job.version.revision = job
        .version
        .revision
        .checked_add(1)
        .ok_or(Error::CaptureIntegrity)?;
    if matches!(event, CaptureEvent::Start | CaptureEvent::Lost) {
        job.version.generation = job
            .version
            .generation
            .checked_add(1)
            .ok_or(Error::CaptureIntegrity)?;
    }
    job.updated_ms = recorded;
    connection.execute("UPDATE capture_jobs SET state = ?2, revision = ?3, generation = ?4, updated_ms = ?5 WHERE id = ?1", params![job.version.id, job.state.as_str(), job.version.revision, job.version.generation, recorded])?;
    connection.execute("INSERT INTO capture_events(job_id, revision, generation, previous_state, state, event, reason, recorded_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![job.version.id, job.version.revision, job.version.generation, previous.as_str(), job.state.as_str(), event.as_str(), reason, recorded])?;
    Ok(job)
}

fn read_record(row: &rusqlite::Row<'_>) -> Result<CaptureRecord> {
    let state: String = row.get(3)?;
    let previous: Option<String> = row.get(2)?;
    let event: Option<String> = row.get(4)?;
    let record = CaptureRecord {
        revision: row.get(0)?,
        generation: row.get(1)?,
        previous_state: previous
            .map(|value| value.parse().map_err(|_| Error::CaptureIntegrity))
            .transpose()?,
        state: state.parse().map_err(|_| Error::CaptureIntegrity)?,
        event: event.map(|value| parse_event(&value)).transpose()?,
        reason: row.get(5)?,
        recorded_ms: row.get(6)?,
    };
    validate_key(&record.reason, "capture reason").map_err(|_| Error::CaptureIntegrity)?;
    if record.revision < 0 || record.generation < 0 || record.recorded_ms < 0 {
        return Err(Error::CaptureIntegrity);
    }
    Ok(record)
}

fn parse_event(value: &str) -> Result<CaptureEvent> {
    match value {
        "start" => Ok(CaptureEvent::Start),
        "connected" => Ok(CaptureEvent::Connected),
        "retry" => Ok(CaptureEvent::Retry),
        "stop" => Ok(CaptureEvent::Stop),
        "finalized" => Ok(CaptureEvent::Finalized),
        "lost" => Ok(CaptureEvent::Lost),
        "cancel" => Ok(CaptureEvent::Cancel),
        "fail" => Ok(CaptureEvent::Fail),
        _ => Err(Error::CaptureIntegrity),
    }
}

pub(super) fn history(
    connection: &Connection,
    id: &str,
    after: i64,
    limit: u32,
) -> Result<Vec<CaptureRecord>> {
    let mut statement = connection.prepare("SELECT revision, generation, previous_state, state, event, reason, recorded_ms FROM capture_events WHERE job_id = ?1 AND revision > ?2 ORDER BY revision LIMIT ?3")?;
    let mut rows = statement.query(params![id, after, limit])?;
    let mut records = Vec::new();
    while let Some(row) = rows.next()? {
        records.push(read_record(row)?);
    }
    Ok(records)
}

pub(super) fn audit(connection: &Connection, job: &CaptureJob) -> Result<()> {
    let mut statement = connection.prepare("SELECT revision, generation, previous_state, state, event, reason, recorded_ms FROM capture_events WHERE job_id = ?1 ORDER BY revision")?;
    let mut rows = statement.query([&job.version.id])?;
    let mut head: Option<CaptureRecord> = None;
    while let Some(row) = rows.next()? {
        let record = read_record(row)?;
        if let Some(previous) = &head {
            let event = record.event.ok_or(Error::CaptureIntegrity)?;
            let generation_step =
                i64::from(matches!(event, CaptureEvent::Start | CaptureEvent::Lost));
            if previous.revision.checked_add(1) != Some(record.revision)
                || previous.generation.checked_add(generation_step) != Some(record.generation)
                || record.previous_state != Some(previous.state)
                || previous.state.transition(event).ok() != Some(record.state)
            {
                return Err(Error::CaptureIntegrity);
            }
        } else if record.revision != 0
            || record.generation != 0
            || record.previous_state.is_some()
            || record.state != CaptureState::Scheduled
            || record.event.is_some()
            || record.reason != "accepted"
            || record.recorded_ms != job.created_ms
        {
            return Err(Error::CaptureIntegrity);
        }
        head = Some(record);
    }
    let head = head.ok_or(Error::CaptureIntegrity)?;
    if head.revision != job.version.revision
        || head.generation != job.version.generation
        || head.state != job.state
        || head.recorded_ms != job.updated_ms
    {
        return Err(Error::CaptureIntegrity);
    }
    Ok(())
}
