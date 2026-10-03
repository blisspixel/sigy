//! Small exact immutable identity reads, bounded by the two-entry task grant.

use rusqlite::{Connection, Row, params};

use crate::{
    Error, Result,
    task::{
        collection::TaskCollectionView,
        processing::TaskProcessingView,
        snapshot::{TaskJobObservation, TaskMediaObservation},
    },
};

/// New observations cannot put already committed media/work outcomes in the future.
pub(super) fn latest_work_ms(connection: &Connection, id: &str) -> Result<i64> {
    Ok(connection.query_row(
        "SELECT coalesce(max(moment), 0) FROM (SELECT c.updated_ms AS moment FROM task_collection_admissions a JOIN schedule_occurrences o ON o.id = a.occurrence_id JOIN capture_jobs c ON c.id = o.recording_id WHERE a.task_id = ?1 UNION ALL SELECT max(j.created_ms, coalesce(j.started_ms, 0), coalesce(j.finished_ms, 0)) FROM task_processing_steps s JOIN analysis_jobs j ON j.id = s.job_id WHERE s.task_id = ?1 AND s.stage = 'recognition' UNION ALL SELECT max(j.created_ms, coalesce(j.started_ms, 0), coalesce(j.finished_ms, 0)) FROM task_processing_steps s JOIN translation_jobs j ON j.id = s.job_id WHERE s.task_id = ?1 AND s.stage = 'translation')",
        [id], |row| row.get(0),
    )?)
}

pub(super) fn jobs(
    connection: &Connection,
    processing: Option<&TaskProcessingView>,
) -> Result<Vec<TaskJobObservation>> {
    let mut jobs = Vec::new();
    for step in processing.map(|p| p.steps.as_slice()).unwrap_or_default() {
        let Some(id) = &step.job_id else {
            continue;
        };
        let job = match step.stage.as_str() {
            "recognition" => connection.query_row(
                "SELECT generation, state, analysis_id, analysis_revision, profile, profile_sha256, manifest_sha256, expected_parent_revision FROM analysis_jobs WHERE id = ?1 AND kind = 'local_asr'",
                [id], |row| Ok(TaskJobObservation {
                    ordinal: step.ordinal, stage: step.stage.clone(), job_id: id.clone(),
                    observed_generation: row.get(0)?, observed_state: row.get(1)?,
                    input_id: row.get(2)?, input_revision: row.get(3)?, profile: row.get(4)?,
                    profile_sha256: row.get(5)?, manifest_sha256: Some(row.get(6)?),
                    expected_parent_revision: Some(row.get(7)?), target: None,
                }),
            )?,
            "translation" => connection.query_row(
                "SELECT generation, state, transcript_id, transcript_revision, profile, profile_sha256 FROM translation_jobs WHERE id = ?1",
                [id], |row| Ok(TaskJobObservation {
                    ordinal: step.ordinal, stage: step.stage.clone(), job_id: id.clone(),
                    observed_generation: row.get(0)?, observed_state: row.get(1)?,
                    input_id: row.get(2)?, input_revision: row.get(3)?, profile: row.get(4)?,
                    profile_sha256: row.get(5)?, manifest_sha256: None,
                    expected_parent_revision: None, target: Some("en".into()),
                }),
            )?,
            _ => return Err(Error::StorageIntegrity),
        };
        if Some(&job.input_id) != step.input_id.as_ref()
            || Some(job.input_revision) != step.input_revision
        {
            return Err(Error::StorageIntegrity);
        }
        jobs.push(job);
    }
    if jobs.len() > 4 {
        return Err(Error::StorageIntegrity);
    }
    Ok(jobs)
}

pub(super) fn media(
    connection: &Connection,
    collection: &TaskCollectionView,
) -> Result<Vec<TaskMediaObservation>> {
    let mut media = Vec::new();
    for (ordinal, capture) in collection.captures.iter().enumerate() {
        let Some(id) = &capture.recording_id else {
            continue;
        };
        let ordinal = u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?;
        media.push(connection.query_row(
            "SELECT sha256, media_bytes, decoded_microseconds FROM recordings WHERE id = ?1",
            params![id],
            |row| {
                Ok(TaskMediaObservation {
                    ordinal,
                    recording_id: id.clone(),
                    media_sha256: row.get(0)?,
                    media_bytes: units(row, 1)?,
                    decoded_us: units(row, 2)?,
                })
            },
        )?);
    }
    if media.len() > 2 {
        return Err(Error::StorageIntegrity);
    }
    Ok(media)
}

fn units(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
        })
        .transpose()
}
