//! Bounded recognition storage. Mutations are crate-private and used by the supervisor.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::analysis::{AnalysisHole, AnalysisSpan};
use super::dvr::Recording;
use super::{Store, validate_key};
use crate::processing::InputFile;
use crate::recognition::{
    AsrSegment, ChunkHole, ChunkSpan, LocalAsrInput, LocalAsrJob, LocalAsrRequest, LocalAsrWork,
    PlannedChunk, coverages_fit_one_page, plan_chunks,
};
use crate::{Error, Result};

mod arrival;
mod cost;
mod jobs;
mod migrate;
mod pace;
mod queue;
pub(crate) use arrival::{ArrivalReport, describe_arrival};
pub(in crate::storage) use cost::audit_observations;
pub(crate) use cost::{WorkerCost, describe_cost};
pub(crate) use pace::{RecognitionPace, describe};
pub(crate) use queue::{RecognitionQueue, describe_queue, describe_recognition};
mod profiles;
mod publish;
mod reads;
pub(in crate::storage) use migrate::migrate_034;
#[cfg(test)]
pub(in crate::storage) use migrate::revert_034_for_tests;
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
        "sigy-local-asr-input-v2",
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

struct RetainedPin {
    timeline: String,
    recording_sha256: Option<String>,
    media_sha256: String,
    media_bytes: Option<i64>,
    capture_state: String,
    storage_state: String,
}

/// Recheck retained catalog identity inside the publication/admission transaction.
fn current_input(connection: &Connection, work: &LocalAsrWork) -> Result<bool> {
    let Some(row) = retained_pin(connection, work)? else {
        return Ok(false);
    };
    let bytes = sql_integer(work.input.byte_length()?)?;
    if sha256(row.timeline.as_bytes()) != work.input.timeline_sha256
        || row.recording_sha256.as_deref() != Some(work.input.media_sha256.as_str())
        || row.media_sha256 != work.input.media_sha256
        || row.media_bytes != Some(bytes)
        || row.capture_state != "completed"
        || row.storage_state != "retained"
    {
        return Ok(false);
    }
    segments_current(connection, work)
}

fn retained_pin(connection: &Connection, work: &LocalAsrWork) -> Result<Option<RetainedPin>> {
    let request = &work.job.request;
    Ok(connection.query_row(
        "SELECT a.timeline_json, r.sha256, a.media_sha256, r.media_bytes, c.state, r.storage_state FROM analysis_inputs a JOIN recordings r ON r.id = a.recording_id JOIN capture_jobs c ON c.id = r.id WHERE a.id = ?1 AND a.revision = ?2 AND a.state = 'published' AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id) AND r.id = ?3",
        params![request.analysis_id, request.analysis_revision, work.input.recording_id],
        |row| Ok(RetainedPin {
            timeline: row.get(0)?,
            recording_sha256: row.get(1)?,
            media_sha256: row.get(2)?,
            media_bytes: row.get(3)?,
            capture_state: row.get(4)?,
            storage_state: row.get(5)?,
        }),
    ).optional()?)
}

fn segments_current(connection: &Connection, work: &LocalAsrWork) -> Result<bool> {
    let retained: i64 = connection.query_row(
        "SELECT count(*) FROM recording_intervals s WHERE s.recording_id = ?1 AND NOT EXISTS (SELECT 1 FROM recording_releases x WHERE x.recording_id = s.recording_id AND x.segment_ordinal = s.ordinal)",
        [work.input.recording_id.as_str()],
        |row| row.get(0),
    )?;
    let expected = i64::try_from(work.input.segments.len()).map_err(|_| Error::StorageIntegrity)?;
    if retained != expected {
        return Ok(false);
    }
    for segment in &work.input.segments {
        if !segment_current(connection, &work.input.recording_id, segment)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn segment_current(
    connection: &Connection,
    recording_id: &str,
    segment: &AsrSegment,
) -> Result<bool> {
    let matched: Option<i64> = connection.query_row(
        "SELECT 1 FROM recording_intervals s WHERE s.recording_id = ?1 AND s.ordinal = ?2 AND s.decoded_start_us = ?3 AND s.decoded_end_us = ?4 AND s.sha256 = ?5 AND s.object_key = ?6 AND s.byte_end - s.byte_start = ?7 AND s.format = ?8 AND NOT EXISTS (SELECT 1 FROM recording_releases x WHERE x.recording_id = s.recording_id AND x.segment_ordinal = s.ordinal)",
        params![recording_id, segment.ordinal, sql_integer(segment.start_us)?, sql_integer(segment.end_us)?, segment.source_sha256, segment.object_key, sql_integer(segment.byte_length)?, segment.format],
        |row| row.get(0),
    ).optional()?;
    Ok(matched.is_some())
}

impl Store {
    fn local_asr_input(&self, request: &LocalAsrRequest) -> Result<LocalAsrInput> {
        let pin = self.published_analysis(&request.analysis_id, request.analysis_revision)?;
        let files = self.verification_input(&request.analysis_id, request.analysis_revision)?;
        if files.bytes > 67_108_864 {
            return Err(Error::Analysis("recognition-input-limit"));
        }
        let recording = self.recording(&pin.recording_id)?;
        let segments = asr_segments(&recording, &pin.intervals, &files.files)?;
        let chunks = planned_chunks(&pin.gaps, &segments)?;
        let timeline: String = self.connection.query_row(
            "SELECT timeline_json FROM analysis_inputs WHERE id = ?1 AND revision = ?2",
            params![request.analysis_id, request.analysis_revision],
            |row| row.get(0),
        )?;
        Ok(LocalAsrInput {
            recording_id: pin.recording_id,
            media_sha256: pin.media_sha256,
            timeline_sha256: sha256(timeline.as_bytes()),
            segments,
            chunks,
        })
    }
}

fn asr_segments(
    recording: &Recording,
    intervals: &[AnalysisSpan],
    files: &[InputFile],
) -> Result<Vec<AsrSegment>> {
    if intervals.len() != files.len() {
        return Err(Error::StorageIntegrity);
    }
    let mut segments = Vec::with_capacity(intervals.len());
    for (span, file) in intervals.iter().zip(files) {
        if file.sha256 != span.sha256 {
            return Err(Error::StorageIntegrity);
        }
        let interval = recording
            .intervals
            .iter()
            .find(|interval| interval.ordinal == span.ordinal && interval.sha256 == span.sha256)
            .ok_or(Error::StorageIntegrity)?;
        segments.push(AsrSegment {
            ordinal: span.ordinal,
            object_key: file.key.clone(),
            byte_length: file.bytes,
            start_us: span.start_us,
            end_us: span.end_us,
            source_sha256: span.sha256.clone(),
            format: interval.format.clone(),
        });
    }
    Ok(segments)
}

fn planned_chunks(gaps: &[AnalysisHole], segments: &[AsrSegment]) -> Result<Vec<PlannedChunk>> {
    let spans = segments
        .iter()
        .map(|segment| ChunkSpan {
            ordinal: segment.ordinal,
            start_us: segment.start_us,
            end_us: segment.end_us,
            source_sha256: segment.source_sha256.clone(),
        })
        .collect::<Vec<_>>();
    let holes = gaps
        .iter()
        .map(|gap| ChunkHole {
            start_us: gap.start_us,
            end_us: gap.end_us,
        })
        .collect::<Vec<_>>();
    let chunks =
        plan_chunks(&spans, &holes).map_err(|_| Error::Analysis("recognition-input-limit"))?;
    if !coverages_fit_one_page(chunks.len()) {
        return Err(Error::Analysis("recognition-input-limit"));
    }
    Ok(chunks)
}
