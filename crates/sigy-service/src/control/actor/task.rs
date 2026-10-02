//! Task passes on the existing service tick: bounded processing admissions, then one
//! publication effect per visited task. Intent and receipts live in the catalog; cursors
//! are only fairness hints and carry no authority.

use super::Actor;
use crate::{
    Error, Result,
    storage::{
        now_ms,
        tasks::processing::{TaskJobAdmission, TaskJobScope},
    },
    task::processing::{self, ProcessingFacts, ProcessingStep},
};

const TASKS_PER_PASS: u32 = 4;
/// Minimum time between task processing passes. Schedule ticks arrive every second.
const PROCESSING_INTERVAL_MS: i64 = 5_000;

/// Where the bounded processing pass resumes, and when it may run next.
#[derive(Debug, Default)]
pub(super) struct ProcessingPass {
    pub(super) cursor: Option<String>,
    pub(super) next_ms: i64,
}

enum Attempt {
    Admitted(TaskJobAdmission),
    Refused(String),
    /// A temporary hold, such as a full queue or stale authority: retry on a later pass.
    Later,
}

/// Permanent refusals become receipts; temporary holds wait; catalog faults propagate.
fn classify(error: Error) -> Result<Attempt> {
    match error {
        Error::Analysis(
            "queue-full"
            | "native-worker-active"
            | "task-processing-cancelled"
            | "task-scope-changed"
            | "task-processing-clock"
            | "task-profile-changed",
        ) => Ok(Attempt::Later),
        Error::Analysis(reason) => Ok(Attempt::Refused(reason.into())),
        Error::InvalidInput(_) => Ok(Attempt::Refused("invalid-input".into())),
        Error::NotFound => Ok(Attempt::Refused("not-found".into())),
        Error::IdempotencyConflict => Ok(Attempt::Refused("idempotency-conflict".into())),
        error => Err(error),
    }
}

impl Actor {
    pub(super) fn reconcile_tasks(&mut self) -> Result<()> {
        self.reconcile_task_processing()?;
        let store = self.library.store();
        let (mut ids, _) =
            store.pending_task_run_ids(self.task_cursor.as_deref(), TASKS_PER_PASS)?;
        if ids.is_empty() && self.task_cursor.is_some() {
            (ids, _) = store.pending_task_run_ids(None, TASKS_PER_PASS)?;
        }
        self.task_cursor = ids.last().cloned();
        for id in ids {
            let run = self
                .library
                .store()
                .task_run(&id)?
                .ok_or(Error::StorageIntegrity)?;
            let now = now_ms()?;
            if now < run.updated_ms {
                continue;
            }
            self.library
                .store_mut()
                .advance_task_run(&id, run.generation, now)?;
        }
        Ok(())
    }

    /// Admit bounded task-owned recognition and translation under each finite grant.
    pub(super) fn reconcile_task_processing(&mut self) -> Result<()> {
        let now = now_ms()?;
        if now < self.task_processing.next_ms {
            return Ok(());
        }
        self.task_processing.next_ms = now + PROCESSING_INTERVAL_MS;
        let can_recognize = self.decoder().is_ok();
        let store = self.library.store();
        let cursor = self.task_processing.cursor.clone();
        let (mut ids, _) = store.pending_task_processing_ids(cursor.as_deref(), TASKS_PER_PASS)?;
        if ids.is_empty() && cursor.is_some() {
            (ids, _) = store.pending_task_processing_ids(None, TASKS_PER_PASS)?;
        }
        self.task_processing.cursor = ids.last().cloned();
        for id in ids {
            let Some(facts) = self.library.store().task_processing_facts(&id, now)? else {
                continue;
            };
            for step in processing::plan(&facts) {
                if matches!(step, ProcessingStep::Recognize { .. }) && !can_recognize {
                    continue;
                }
                if !self.take_task_step(&facts, &step, now)? {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Returns false when the rest of this task's pass should wait.
    fn take_task_step(
        &mut self,
        facts: &ProcessingFacts,
        step: &ProcessingStep,
        now: i64,
    ) -> Result<bool> {
        let (ordinal, recording_id, stage, attempt) = match step {
            ProcessingStep::Recognize {
                ordinal,
                recording_id,
                audio_us,
            } => {
                let scope = TaskJobScope {
                    task_id: &facts.task_id,
                    ordinal: *ordinal,
                    recording_id,
                    audio_us: *audio_us,
                };
                let attempt = self
                    .recognize(recording_id, &facts.recognition_profile, now)
                    .and_then(|request| {
                        self.library
                            .store_mut()
                            .enqueue_task_recognition(&scope, &request, now)
                    })
                    .map(Attempt::Admitted)
                    .or_else(classify)?;
                (*ordinal, recording_id, "recognition", attempt)
            }
            ProcessingStep::Translate {
                ordinal,
                recording_id,
                analysis_id,
                transcript_revision,
            } => {
                let scope = TaskJobScope {
                    task_id: &facts.task_id,
                    ordinal: *ordinal,
                    recording_id,
                    audio_us: 0,
                };
                let profile = facts
                    .translation_profile
                    .as_deref()
                    .ok_or(Error::StorageIntegrity)?;
                let attempt = self
                    .translate(analysis_id, *transcript_revision, profile)
                    .and_then(|request| {
                        self.library
                            .store_mut()
                            .enqueue_task_translation(&scope, &request, now)
                    })
                    .map(Attempt::Admitted)
                    .or_else(classify)?;
                (*ordinal, recording_id, "translation", attempt)
            }
            ProcessingStep::SkipRecognition {
                ordinal,
                recording_id,
                reason,
            } => (
                *ordinal,
                recording_id,
                "recognition",
                Attempt::Refused(reason.clone()),
            ),
            ProcessingStep::SkipTranslation {
                ordinal,
                recording_id,
                reason,
            } => (
                *ordinal,
                recording_id,
                "translation",
                Attempt::Refused(reason.clone()),
            ),
        };
        match attempt {
            Attempt::Admitted(admission) => {
                if admission.job_created {
                    self.schedule()?;
                }
                Ok(true)
            }
            Attempt::Refused(reason) => {
                let scope = TaskJobScope {
                    task_id: &facts.task_id,
                    ordinal,
                    recording_id,
                    audio_us: 0,
                };
                match self
                    .library
                    .store_mut()
                    .record_task_processing_skip(&scope, stage, &reason, now)
                {
                    Ok(_) => Ok(true),
                    // An unrecordable refusal holds this task until a later pass.
                    Err(error) => classify(error).map(|_| false),
                }
            }
            Attempt::Later => Ok(false),
        }
    }
}
