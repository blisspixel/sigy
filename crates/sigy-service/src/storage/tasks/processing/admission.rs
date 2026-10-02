//! Task job admission. The canonical job (or an exact shared job), the immutable task
//! receipt, the task's job interest and its lifetime audio charge commit in one immediate
//! transaction before any scheduler can claim work. Historical replay never dispatches.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Error, Grant, Result, Store, cancellation, current_scope, latest_task_ms, read_grant};
use crate::{
    recognition::LocalAsrRequest,
    storage::interests::{self, Authority},
    translation::TranslationRequest,
};

/// One collected recording of one task, as inspected by the planner.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TaskJobScope<'a> {
    pub task_id: &'a str,
    pub ordinal: u32,
    pub recording_id: &'a str,
    /// Decoded audio charged to the task allowance; zero for translation and refusals.
    pub audio_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskJobAdmission {
    pub job_created: bool,
    pub step_created: bool,
}

const REPLAY: TaskJobAdmission = TaskJobAdmission {
    job_created: false,
    step_created: false,
};

/// `Ok((input, revision, job))` for queued work, `Err(reason)` for a recorded refusal.
type Outcome<'a> = std::result::Result<(&'a str, i64, &'a str), &'a str>;

struct Receipt<'a> {
    scope: &'a TaskJobScope<'a>,
    stage: &'static str,
    outcome: Outcome<'a>,
}

type Stored = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    i64,
);

/// True for an exact stored receipt, false when absent, and a conflict when it differs.
fn replayed(connection: &Connection, receipt: &Receipt<'_>) -> Result<bool> {
    let scope = receipt.scope;
    crate::storage::validate_key(scope.task_id, "task ID")?;
    crate::storage::validate_key(scope.recording_id, "task recording")?;
    let stored: Option<Stored> = connection
        .query_row(
            "SELECT recording_id, decision, reason, input_id, input_revision, job_id, audio_us FROM task_processing_steps WHERE task_id = ?1 AND ordinal = ?2 AND stage = ?3",
            params![scope.task_id, scope.ordinal, receipt.stage],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(false);
    };
    let audio = i64::try_from(scope.audio_us).map_err(|_| Error::InvalidInput("audio"))?;
    let expected: Stored = match receipt.outcome {
        Ok((input, revision, job)) => (
            scope.recording_id.into(),
            "queued".into(),
            None,
            Some(input.into()),
            Some(revision),
            Some(job.into()),
            audio,
        ),
        Err(reason) => (
            scope.recording_id.into(),
            "skipped".into(),
            Some(reason.into()),
            None,
            None,
            None,
            audio,
        ),
    };
    if stored == expected {
        Ok(true)
    } else {
        Err(Error::IdempotencyConflict)
    }
}

fn write(connection: &Connection, receipt: &Receipt<'_>, now: i64) -> Result<()> {
    let scope = receipt.scope;
    let (decision, reason, input, revision, job) = match receipt.outcome {
        Ok((input, revision, job)) => ("queued", None, Some(input), Some(revision), Some(job)),
        Err(reason) => ("skipped", Some(reason), None, None, None),
    };
    connection.execute(
        "INSERT INTO task_processing_steps(task_id, ordinal, stage, recording_id, decision, reason, input_id, input_revision, job_id, audio_us, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![scope.task_id, scope.ordinal, receipt.stage, scope.recording_id, decision, reason, input, revision, job, i64::try_from(scope.audio_us).map_err(|_| Error::InvalidInput("audio"))?, now],
    )?;
    if let Some(job) = job {
        interests::record(
            connection,
            receipt.stage,
            job,
            Authority::Task(scope.task_id),
            now,
        )?;
    }
    Ok(())
}

/// Current task authority, rechecked inside the admission transaction.
fn fresh(connection: &Connection, scope: &TaskJobScope<'_>, now: i64) -> Result<Grant> {
    let grant = read_grant(connection, scope.task_id)?.ok_or(Error::NotFound)?;
    let task = super::super::audit::checked_task_scope_in(connection, scope.task_id)?;
    if task.1 != grant.scope_sha256 {
        return Err(Error::StorageIntegrity);
    }
    if cancellation(connection, scope.task_id, &grant)?.is_some() {
        return Err(Error::Analysis("task-processing-cancelled"));
    }
    if !current_scope(connection, &task.0)? {
        return Err(Error::Analysis("task-scope-changed"));
    }
    if now < 0 || now < latest_task_ms(connection, scope.task_id)? {
        return Err(Error::Analysis("task-processing-clock"));
    }
    let collected: Option<String> = connection
        .query_row(
            "SELECT occurrence_id FROM task_collection_admissions WHERE task_id = ?1 AND ordinal = ?2",
            params![scope.task_id, scope.ordinal],
            |row| row.get(0),
        )
        .optional()?;
    if collected.as_deref() != Some(scope.recording_id) {
        return Err(Error::InvalidInput("task recording is not collected"));
    }
    Ok(grant)
}

/// An existing shared job created after this clock would precede its own creation.
fn job_after(connection: &Connection, sql: &str, job: &str, now: i64) -> Result<bool> {
    Ok(connection
        .query_row(sql, [job], |row| row.get::<_, i64>(0))
        .optional()?
        .is_some_and(|created| created > now))
}

fn profile_current(connection: &Connection, sql: &str, id: &str, sha256: &str) -> Result<bool> {
    let stored: Option<String> = connection
        .query_row(sql, [id], |row| row.get(0))
        .optional()?;
    Ok(stored.as_deref() == Some(sha256))
}

impl Store {
    /// Commit canonical recognition admission with its task receipt, interest and charge.
    /// # Errors
    /// Refuses changed replay, cancelled or stale authority, a regressed clock, another
    /// profile, uncollected or changed audio, and an exhausted lifetime allowance.
    pub(crate) fn enqueue_task_recognition(
        &mut self,
        scope: &TaskJobScope<'_>,
        request: &LocalAsrRequest,
        now: i64,
    ) -> Result<TaskJobAdmission> {
        request.validate()?;
        let receipt = Receipt {
            scope,
            stage: "recognition",
            outcome: Ok((&request.analysis_id, request.analysis_revision, &request.id)),
        };
        if replayed(&self.connection, &receipt)? {
            if self.local_asr_job(&request.id)?.request != *request {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(REPLAY);
        }
        let pinned: String = self
            .connection
            .query_row(
                "SELECT recording_id FROM analysis_inputs WHERE id = ?1 AND revision = ?2 AND state = 'published'",
                params![request.analysis_id, request.analysis_revision],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if pinned != scope.recording_id {
            return Err(Error::IdempotencyConflict);
        }
        if now < 0 {
            return Err(Error::Analysis("task-processing-clock"));
        }
        let prepared = self.prepare_local_asr_job(request, now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let grant = fresh(&tx, scope, now)?;
        if request.profile != grant.spec.recognition_profile
            || request.profile_sha256 != grant.recognition_sha256
            || !profile_current(
                &tx,
                "SELECT profile_sha256 FROM recognition_profiles WHERE id = ?1",
                &request.profile,
                &grant.recognition_sha256,
            )?
        {
            return Err(Error::Analysis("task-profile-changed"));
        }
        let audio: Option<(String, String, Option<i64>)> = tx
            .query_row(
                "SELECT c.state, r.storage_state, r.decoded_microseconds FROM capture_jobs c JOIN recordings r ON r.id = c.id WHERE c.id = ?1",
                [scope.recording_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let decoded = match audio {
            Some((state, storage, Some(decoded)))
                if state == "completed" && storage == "retained" =>
            {
                u64::try_from(decoded).ok()
            }
            _ => None,
        };
        if decoded != Some(scope.audio_us) || scope.audio_us == 0 {
            return Err(Error::InvalidInput("task recording audio"));
        }
        let charged: i64 = tx.query_row(
            "SELECT coalesce(sum(audio_us), 0) FROM task_processing_steps WHERE task_id = ?1 AND stage = 'recognition'",
            [scope.task_id],
            |row| row.get(0),
        )?;
        let charged = u64::try_from(charged).map_err(|_| Error::StorageIntegrity)?;
        if scope.audio_us > grant.spec.maximum_audio_us().saturating_sub(charged) {
            return Err(Error::Analysis("task-audio-allowance"));
        }
        if job_after(
            &tx,
            "SELECT created_ms FROM analysis_jobs WHERE id = ?1",
            &request.id,
            now,
        )? {
            return Err(Error::Analysis("task-processing-clock"));
        }
        let job_created = Self::enqueue_local_asr_in(&tx, request, prepared.as_ref(), now)?;
        write(&tx, &receipt, now)?;
        tx.commit()?;
        Ok(TaskJobAdmission {
            job_created,
            step_created: true,
        })
    }

    /// Commit canonical translation of this task's own recognized transcript revision.
    /// # Errors
    /// Refuses changed replay, stale authority, an ungranted profile or unrelated text.
    pub(crate) fn enqueue_task_translation(
        &mut self,
        scope: &TaskJobScope<'_>,
        request: &TranslationRequest,
        now: i64,
    ) -> Result<TaskJobAdmission> {
        request.validate()?;
        if scope.audio_us != 0 {
            return Err(Error::InvalidInput("translation audio charge"));
        }
        let receipt = Receipt {
            scope,
            stage: "translation",
            outcome: Ok((
                &request.transcript_id,
                request.transcript_revision,
                &request.id,
            )),
        };
        if replayed(&self.connection, &receipt)? {
            if self.translation_job(&request.id)?.request != *request {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(REPLAY);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let grant = fresh(&tx, scope, now)?;
        if grant.spec.translation_profile.as_deref() != Some(request.profile.as_str())
            || grant.translation_sha256.as_deref() != Some(request.profile_sha256.as_str())
            || !profile_current(
                &tx,
                "SELECT profile_sha256 FROM translation_profiles WHERE id = ?1",
                &request.profile,
                &request.profile_sha256,
            )?
        {
            return Err(Error::Analysis("task-profile-changed"));
        }
        let supported: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM transcripts t JOIN task_processing_steps s ON s.task_id = ?1 AND s.ordinal = ?2 AND s.stage = 'recognition' AND s.decision = 'queued' AND s.job_id = t.job_id JOIN analysis_jobs j ON j.id = s.job_id AND j.state = 'succeeded' WHERE t.id = ?3 AND t.revision = ?4 AND t.recording_id = ?5 AND t.kind = 'recognition' AND t.outcome = 'text')",
            params![scope.task_id, scope.ordinal, request.transcript_id, request.transcript_revision, scope.recording_id],
            |row| row.get(0),
        )?;
        if !supported {
            return Err(Error::Analysis("task-translation-unsupported"));
        }
        if job_after(
            &tx,
            "SELECT created_ms FROM translation_jobs WHERE id = ?1",
            &request.id,
            now,
        )? {
            return Err(Error::Analysis("task-processing-clock"));
        }
        let job_created = crate::storage::translations::enqueue_translation_in(&tx, request, now)?;
        write(&tx, &receipt, now)?;
        tx.commit()?;
        Ok(TaskJobAdmission {
            job_created,
            step_created: true,
        })
    }

    /// Record one permanent task processing refusal under current task authority.
    /// # Errors
    /// Refuses a changed replay, stale or cancelled authority, a malformed reason, or a
    /// translation refusal before any recognition receipt.
    pub(crate) fn record_task_processing_skip(
        &mut self,
        scope: &TaskJobScope<'_>,
        stage: &'static str,
        reason: &str,
        now: i64,
    ) -> Result<bool> {
        crate::storage::validate_key(reason, "task processing reason")?;
        if reason.len() > 64 || scope.audio_us != 0 {
            return Err(Error::InvalidInput("task processing refusal"));
        }
        if !matches!(stage, "recognition" | "translation") {
            return Err(Error::InvalidInput("task processing stage"));
        }
        let receipt = Receipt {
            scope,
            stage,
            outcome: Err(reason),
        };
        if replayed(&self.connection, &receipt)? {
            return Ok(false);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        fresh(&tx, scope, now)?;
        if stage == "translation" {
            let recognized: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_processing_steps WHERE task_id = ?1 AND ordinal = ?2 AND stage = 'recognition')",
                params![scope.task_id, scope.ordinal],
                |row| row.get(0),
            )?;
            if !recognized {
                return Err(Error::InvalidInput("task translation before recognition"));
            }
        }
        write(&tx, &receipt, now)?;
        tx.commit()?;
        Ok(true)
    }
}
