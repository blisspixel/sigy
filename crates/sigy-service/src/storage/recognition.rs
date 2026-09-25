//! Bounded recognition storage. Mutations are crate-private and used by the supervisor.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::{Store, validate_key};
use crate::recognition::{LocalAsrInput, LocalAsrWork};
use crate::{
    Error, Result,
    recognition::{LocalAsrJob, LocalAsrRequest},
};

mod jobs;
mod profiles;
mod publish;
mod reads;
#[cfg(test)]
mod tests;
mod validation;

fn sha256(bytes: &[u8]) -> String {
    super::dvr::hex(&Sha256::digest(bytes))
}

fn sql_integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::InvalidInput("recognition integer range"))
}

fn manifest(input: &LocalAsrInput) -> Result<String> {
    Ok(sha256(&serde_json::to_vec(&(
        "sigy-local-asr-input-v1",
        input,
    ))?))
}

fn find_job(connection: &Connection, id: &str) -> Result<Option<LocalAsrJob>> {
    let kind: Option<String> = connection
        .query_row(
            "SELECT kind FROM analysis_jobs WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    if kind.as_deref().is_some_and(|kind| kind != "local_asr") {
        return Err(Error::IdempotencyConflict);
    }
    Ok(connection.query_row(
        "SELECT id, analysis_id, analysis_revision, profile, profile_sha256, expected_parent_revision, generation, recording_id, state, expected_bytes, manifest_sha256, reason, created_ms, finished_ms, attempt, started_ms FROM analysis_jobs WHERE id = ?1",
        [id], |row| Ok(LocalAsrJob {
            request: LocalAsrRequest { id: row.get(0)?, analysis_id: row.get(1)?, analysis_revision: row.get(2)?, profile: row.get(3)?, profile_sha256: row.get(4)?, parent_revision: row.get(5)? },
            generation: row.get(6)?, recording_id: row.get(7)?, state: row.get(8)?, expected_bytes: row.get::<_, u32>(9)?.into(), manifest_sha256: row.get(10)?, reason: row.get(11)?, created_ms: row.get(12)?, finished_ms: row.get(13)?, attempt: row.get(14)?, started_ms: row.get(15)?, amount_usd: "0.000000".into(),
        }),
    ).optional()?)
}

/// Recheck retained catalog identity inside the publication/admission transaction.
fn current_input(connection: &Connection, work: &LocalAsrWork) -> Result<bool> {
    let input = &work.input;
    let request = &work.job.request;
    let timeline: Option<String> = connection.query_row(
        "SELECT a.timeline_json FROM analysis_inputs a JOIN recordings r ON r.id = a.recording_id JOIN capture_jobs c ON c.id = r.id JOIN recording_intervals s ON s.recording_id = r.id WHERE a.id = ?1 AND a.revision = ?2 AND a.state = 'published' AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id) AND r.id = ?3 AND c.state = 'completed' AND r.storage_state = 'retained' AND r.sha256 = ?4 AND a.media_sha256 = ?4 AND r.media_bytes = ?5 AND s.ordinal = ?6 AND s.decoded_start_us = ?7 AND s.decoded_end_us = ?8 AND s.sha256 = ?9 AND s.object_key = ?10 AND s.byte_end - s.byte_start = ?5 AND NOT EXISTS (SELECT 1 FROM recording_releases x WHERE x.recording_id = s.recording_id AND x.segment_ordinal = s.ordinal)",
        params![request.analysis_id, request.analysis_revision, input.recording_id, input.media_sha256, sql_integer(input.byte_length)?, input.interval_ordinal, sql_integer(input.start_us)?, sql_integer(input.end_us)?, input.source_sha256, input.object_key], |row| row.get(0),
    ).optional()?;
    Ok(timeline.is_some_and(|timeline| sha256(timeline.as_bytes()) == input.timeline_sha256))
}

impl Store {
    fn local_asr_input(&self, request: &LocalAsrRequest) -> Result<LocalAsrInput> {
        let pin = self.published_analysis(&request.analysis_id, request.analysis_revision)?;
        let files = self.verification_input(&request.analysis_id, request.analysis_revision)?;
        if pin.intervals.len() != 1 || files.files.len() != 1 || files.bytes > 67_108_864 {
            return Err(Error::Analysis("recognition-input-limit"));
        }
        let span = pin.intervals.first().ok_or(Error::StorageIntegrity)?;
        if !matches!(span.end_us.checked_sub(span.start_us), Some(1..=60_000_000)) {
            return Err(Error::Analysis("recognition-input-limit"));
        }
        let file = files.files.first().ok_or(Error::StorageIntegrity)?;
        let timeline: String = self.connection.query_row(
            "SELECT timeline_json FROM analysis_inputs WHERE id = ?1 AND revision = ?2",
            params![request.analysis_id, request.analysis_revision],
            |row| row.get(0),
        )?;
        Ok(LocalAsrInput {
            recording_id: pin.recording_id,
            media_sha256: pin.media_sha256,
            timeline_sha256: sha256(timeline.as_bytes()),
            object_key: file.key.clone(),
            byte_length: file.bytes,
            interval_ordinal: span.ordinal,
            start_us: span.start_us,
            end_us: span.end_us,
            source_sha256: span.sha256.clone(),
        })
    }
}
