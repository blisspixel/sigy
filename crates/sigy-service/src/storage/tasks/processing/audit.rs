//! Reopen and read checks re-establish every insert-time binding from the raw rows.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Error, Grant, Result};
use crate::task::processing::TaskProcessingStep;

pub(super) fn job_state(connection: &Connection, stage: &str, job: &str) -> Result<String> {
    let sql = match stage {
        "recognition" => "SELECT state FROM analysis_jobs WHERE id = ?1 AND kind = 'local_asr'",
        "translation" => "SELECT state FROM translation_jobs WHERE id = ?1",
        _ => return Err(Error::StorageIntegrity),
    };
    connection
        .query_row(sql, [job], |row| row.get(0))
        .optional()?
        .ok_or(Error::StorageIntegrity)
}

fn recognition_bound(
    connection: &Connection,
    grant: &Grant,
    step: &TaskProcessingStep,
) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM analysis_jobs j JOIN recordings r ON r.id = j.recording_id WHERE j.id = ?1 AND j.kind = 'local_asr' AND j.recording_id = ?2 AND j.analysis_id = ?3 AND j.analysis_revision = ?4 AND j.profile = ?5 AND j.profile_sha256 = ?6 AND j.created_ms <= ?7 AND r.decoded_microseconds = ?8)",
        params![step.job_id, step.recording_id, step.input_id, step.input_revision, grant.spec.recognition_profile, grant.recognition_sha256, step.created_ms, i64::try_from(step.audio_us).map_err(|_| Error::StorageIntegrity)?],
        |row| row.get(0),
    )?)
}

fn translation_bound(
    connection: &Connection,
    grant: &Grant,
    step: &TaskProcessingStep,
    recognition_job: Option<&str>,
) -> Result<bool> {
    let (Some(profile), Some(sha), Some(recognition_job)) = (
        grant.spec.translation_profile.as_deref(),
        grant.translation_sha256.as_deref(),
        recognition_job,
    ) else {
        return Ok(false);
    };
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM translation_jobs j JOIN transcripts t ON t.id = j.transcript_id AND t.revision = j.transcript_revision WHERE j.id = ?1 AND j.transcript_id = ?2 AND j.transcript_revision = ?3 AND j.profile = ?4 AND j.profile_sha256 = ?5 AND j.created_ms <= ?6 AND t.job_id = ?7 AND t.recording_id = ?8 AND t.kind = 'recognition' AND t.outcome = 'text')",
        params![step.job_id, step.input_id, step.input_revision, profile, sha, step.created_ms, recognition_job, step.recording_id],
        |row| row.get(0),
    )?)
}

/// Bind receipts to exact collected recordings, granted profile revisions and stage order.
pub(super) fn validate_steps(
    connection: &Connection,
    id: &str,
    grant: &Grant,
    steps: &[TaskProcessingStep],
) -> Result<()> {
    let mut recognition: [Option<(Option<&str>, i64)>; 2] = [None, None];
    for step in steps {
        let slot = recognition
            .get_mut(step.ordinal as usize)
            .ok_or(Error::StorageIntegrity)?;
        let admitted: Option<(String, i64)> = connection
            .query_row(
                "SELECT occurrence_id, admitted_ms FROM task_collection_admissions WHERE task_id = ?1 AND ordinal = ?2",
                params![id, step.ordinal],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((occurrence, admitted_ms)) = admitted else {
            return Err(Error::StorageIntegrity);
        };
        crate::storage::validate_key(&step.recording_id, "recording ID")
            .map_err(|_| Error::StorageIntegrity)?;
        if occurrence != step.recording_id
            || step.created_ms < grant.created_ms
            || step.created_ms < admitted_ms
            || step
                .reason
                .as_deref()
                .is_some_and(|reason| crate::storage::validate_key(reason, "reason").is_err())
        {
            return Err(Error::StorageIntegrity);
        }
        let queued = step.decision == "queued";
        let bound = match (step.stage.as_str(), queued) {
            ("recognition", true) => recognition_bound(connection, grant, step)?,
            ("recognition", false) => true,
            ("translation", _) => {
                let Some((job, recognized_ms)) = *slot else {
                    return Err(Error::StorageIntegrity);
                };
                step.created_ms >= recognized_ms
                    && (!queued || translation_bound(connection, grant, step, job)?)
            }
            _ => false,
        };
        if !bound {
            return Err(Error::StorageIntegrity);
        }
        if step.stage == "recognition" {
            *slot = Some((step.job_id.as_deref(), step.created_ms));
        }
    }
    Ok(())
}
