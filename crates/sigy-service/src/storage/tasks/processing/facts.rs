//! Durable facts for the pure task processing planner, read in one place.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Error, Result, Store, cancellation, current_scope, latest_task_ms, read_grant};
use crate::task::processing::{CaptureFacts, ProcessingFacts, RecognitionFacts};

type CaptureRow = (u32, String, String, Option<String>, Option<i64>);

fn recognition(
    connection: &Connection,
    id: &str,
    ordinal: u32,
) -> Result<Option<RecognitionFacts>> {
    let step: Option<(String, Option<String>, Option<String>)> = connection
        .query_row(
            "SELECT decision, input_id, job_id FROM task_processing_steps WHERE task_id = ?1 AND ordinal = ?2 AND stage = 'recognition'",
            params![id, ordinal],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((decision, analysis_id, job)) = step else {
        return Ok(None);
    };
    let Some(job) = job else {
        return Ok(Some(RecognitionFacts {
            queued: false,
            analysis_id: None,
            job_state: None,
            transcript: None,
        }));
    };
    let job_state: String = connection.query_row(
        "SELECT state FROM analysis_jobs WHERE id = ?1",
        [&job],
        |row| row.get(0),
    )?;
    let transcript = connection
        .query_row(
            "SELECT revision, outcome FROM transcripts WHERE job_id = ?1 AND kind = 'recognition'",
            [&job],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(Some(RecognitionFacts {
        queued: decision == "queued",
        analysis_id,
        job_state: Some(job_state),
        transcript,
    }))
}

impl Store {
    /// Everything the task processing planner needs. Absent without a grant.
    /// # Errors
    /// Refuses invalid stored grants or rows.
    pub(crate) fn task_processing_facts(
        &self,
        id: &str,
        now: i64,
    ) -> Result<Option<ProcessingFacts>> {
        crate::storage::validate_key(id, "task ID")?;
        let Some(grant) = read_grant(&self.connection, id)? else {
            return Ok(None);
        };
        let scope = super::super::audit::checked_task_scope_in(&self.connection, id)?;
        if scope.1 != grant.scope_sha256 {
            return Err(Error::StorageIntegrity);
        }
        let hold = if crate::storage::withdrawals::fenced(&self.connection, id)? {
            Some("task-interest-withdrawn")
        } else if cancellation(&self.connection, id, &grant)?.is_some() {
            Some("cancelled")
        } else if !current_scope(&self.connection, &scope.0)? {
            Some("scope-changed")
        } else if now < latest_task_ms(&self.connection, id)? {
            Some("clock")
        } else {
            None
        };
        let charged: i64 = self.connection.query_row(
            "SELECT coalesce(sum(audio_us), 0) FROM task_processing_steps WHERE task_id = ?1 AND stage = 'recognition'",
            [id],
            |row| row.get(0),
        )?;
        let charged = u64::try_from(charged).map_err(|_| Error::StorageIntegrity)?;
        let mut statement = self.connection.prepare(
            "SELECT a.ordinal, a.occurrence_id, c.state, r.storage_state, r.decoded_microseconds FROM task_collection_admissions a JOIN capture_jobs c ON c.id = a.occurrence_id LEFT JOIN recordings r ON r.id = a.occurrence_id WHERE a.task_id = ?1 ORDER BY a.ordinal LIMIT 2",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<CaptureRow>>>()?;
        let mut captures = Vec::with_capacity(rows.len());
        for (ordinal, recording_id, capture_state, storage, decoded) in rows {
            let translation_recorded: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_processing_steps WHERE task_id = ?1 AND ordinal = ?2 AND stage = 'translation')",
                params![id, ordinal],
                |row| row.get(0),
            )?;
            captures.push(CaptureFacts {
                ordinal,
                recognition: recognition(&self.connection, id, ordinal)?,
                recording_id,
                capture_state,
                retained: storage.as_deref() == Some("retained"),
                decoded_us: decoded
                    .map(|value| u64::try_from(value).map_err(|_| Error::StorageIntegrity))
                    .transpose()?,
                translation_recorded,
            });
        }
        Ok(Some(ProcessingFacts {
            task_id: id.to_owned(),
            hold,
            remaining_audio_us: grant.spec.maximum_audio_us().saturating_sub(charged),
            recognition_profile: grant.spec.recognition_profile,
            translation_profile: grant.spec.translation_profile,
            captures,
        }))
    }
}
