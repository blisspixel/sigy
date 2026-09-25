//! The service scheduler for the durable local job pool.
//!
//! Admission only enqueues. [`Actor::schedule`] claims queued jobs, oldest first, up to
//! each kind's concurrency cap and never two in one transcript lineage, and starts one
//! supervised worker per claim. Capture workers are separate and never wait on the pool.

use std::collections::HashMap;

use super::{Actor, Message, Worker};
use crate::{
    Error, Result, execution, processing,
    recognition::{LocalAsrRequest, LocalAsrWork, ReapedLocalAsr},
    recognizer::{self, RecognitionTask},
    storage::{
        analysis_jobs::AnalysisJob,
        job_pool::{JobKind, PoolCaps},
        now_ms,
    },
    translation::{TranslationOutcome, TranslationWork},
};

/// Claims attempted for one kind in one scheduling pass. Each failed claim ends a job,
/// so this only bounds the work one pass can do.
const MAX_CLAIMS_PER_PASS: usize = 64;

pub(super) struct VerificationWorker {
    pub generation: u32,
    pub worker: Worker,
}

pub(super) struct RecognitionWorker {
    pub generation: u32,
    pub worker: Worker,
    /// The claimed work token stays here until the worker finishes.
    pub work: LocalAsrWork,
}

pub(super) struct TranslationWorker {
    pub generation: u32,
    pub worker: Worker,
    pub work: TranslationWork,
}

/// Running pool workers, by job ID, and the caps and lease owner of this service.
pub(super) struct Pool {
    pub caps: PoolCaps,
    pub owner: String,
    /// False once the service is stopping; no new job starts after that.
    pub accepting: bool,
    pub verification: HashMap<String, VerificationWorker>,
    pub recognition: HashMap<String, RecognitionWorker>,
    pub translation: HashMap<String, TranslationWorker>,
}

impl Pool {
    pub(super) fn new(owner: String) -> Self {
        Self {
            caps: PoolCaps::default(),
            owner,
            accepting: true,
            verification: HashMap::new(),
            recognition: HashMap::new(),
            translation: HashMap::new(),
        }
    }

    fn running(&self, kind: JobKind) -> usize {
        match kind {
            JobKind::Verification => self.verification.len(),
            JobKind::Recognition => self.recognition.len(),
            JobKind::Translation => self.translation.len(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.verification.is_empty() && self.recognition.is_empty() && self.translation.is_empty()
    }

    pub(super) fn stop(&mut self) {
        self.accepting = false;
        let workers = self
            .verification
            .values()
            .map(|active| &active.worker)
            .chain(self.recognition.values().map(|active| &active.worker))
            .chain(self.translation.values().map(|active| &active.worker));
        for worker in workers {
            worker.stop.send_replace(true);
        }
    }

    /// Signal the worker of exactly this job generation, if one runs.
    fn signal(&self, id: &str, generation: u32) {
        let worker = match (
            self.verification.get(id),
            self.recognition.get(id),
            self.translation.get(id),
        ) {
            (Some(active), _, _) if active.generation == generation => Some(&active.worker),
            (_, Some(active), _) if active.generation == generation => Some(&active.worker),
            (_, _, Some(active)) if active.generation == generation => Some(&active.worker),
            _ => None,
        };
        if let Some(worker) = worker {
            worker.stop.send_replace(true);
        }
    }
}

pub(crate) struct TranscribeRequest {
    pub id: String,
    pub input: String,
    pub revision: i64,
    pub profile: String,
    pub parent_revision: Option<i64>,
}

impl Actor {
    /// Start queued jobs until each kind reaches its cap or runs out of eligible work.
    /// # Errors
    /// Returns catalog failures and a worker that could not be started.
    pub(super) fn schedule(&mut self) -> Result<()> {
        for kind in JobKind::ALL {
            let mut claims = 0;
            while self.pool.accepting
                && self.pool.running(kind) < self.pool.caps.cap(kind)
                && claims < MAX_CLAIMS_PER_PASS
            {
                if kind == JobKind::Recognition && self.decoder().is_err() {
                    // Recognition stays queued until a decoder is configured again.
                    break;
                }
                let Some(id) = self.library.store().next_queued(kind)? else {
                    break;
                };
                claims += 1;
                match kind {
                    JobKind::Verification => self.launch_verification(&id)?,
                    JobKind::Recognition => self.launch_recognition(&id)?,
                    JobKind::Translation => self.launch_translation(&id)?,
                }
            }
        }
        Ok(())
    }

    pub(super) fn start_verification(
        &mut self,
        id: &str,
        input: &str,
        revision: i64,
    ) -> Result<()> {
        self.library
            .store_mut()
            .enqueue_verification(id, input, revision, now_ms()?)?;
        self.schedule()
    }

    fn launch_verification(&mut self, id: &str) -> Result<()> {
        let owner = self.pool.owner.clone();
        let Some((job, manifest)) =
            self.library
                .store_mut()
                .claim_verification(id, &owner, now_ms()?)?
        else {
            return Ok(());
        };
        let directory = self.library.directory().to_path_buf();
        let ownership = self.library.hold_ownership();
        let worker_id = job.id.clone();
        let generation = job.generation;
        let worker = self.spawn_worker(
            move |signal| processing::verify(directory, manifest, ownership, signal),
            move |result| Message::AnalysisFinished {
                id: worker_id,
                generation,
                result,
            },
        );
        match worker {
            Ok(worker) => {
                self.pool
                    .verification
                    .insert(job.id, VerificationWorker { generation, worker });
                Ok(())
            }
            Err(error) => {
                self.fail_analysis_spawn(&job)?;
                Err(error)
            }
        }
    }

    fn fail_analysis_spawn(&mut self, job: &AnalysisJob) -> Result<()> {
        self.library.store_mut().finish_verification(
            &job.id,
            job.generation,
            Err(Error::Analysis("worker-start-failed")),
            now_ms()?,
        )?;
        Ok(())
    }

    pub(super) fn cancel_verification(&mut self, id: &str, generation: u32) -> Result<()> {
        let native = self.library.store().analysis_job_kind(id)?.as_deref() == Some("local_asr");
        let active_job = if native {
            let job = self.library.store_mut().cancel_local_asr(id, generation)?;
            matches!(job.state.as_str(), "running" | "cancelling")
        } else {
            self.library
                .store_mut()
                .cancel_analysis_job(id, generation)?
                .active()
        };
        if active_job {
            self.pool.signal(id, generation);
        }
        Ok(())
    }

    pub(super) fn finish_verification(
        &mut self,
        id: &str,
        generation: u32,
        result: Result<processing::VerificationReceipt>,
    ) -> Result<()> {
        if self
            .pool
            .verification
            .get(id)
            .is_none_or(|active| active.generation != generation)
        {
            return Ok(());
        }
        // Only actual worker completion arrives here. A failed commit retains the durable
        // lease and stops the actor, but must not leave shutdown waiting on a finished task.
        let outcome = now_ms().and_then(|now| {
            self.library
                .store_mut()
                .finish_verification(id, generation, result, now)
        });
        self.pool.verification.remove(id);
        outcome?;
        self.schedule()
    }
}

impl Actor {
    /// Route analysis commands that start or stop service-owned workers.
    /// Everything else is a catalog operation.
    pub(super) fn analysis(
        &mut self,
        command: super::super::AnalysisOperation,
    ) -> Result<super::super::Operation> {
        use super::super::AnalysisOperation as Op;
        let job = match command {
            Op::Verify {
                id,
                input,
                revision,
            } => {
                self.start_verification(&id, &input, revision)?;
                id
            }
            Op::Transcribe {
                id,
                input,
                revision,
                profile,
                parent_revision,
            } => {
                self.start_recognition(TranscribeRequest {
                    id: id.clone(),
                    input,
                    revision,
                    profile,
                    parent_revision,
                })?;
                id
            }
            Op::Translate {
                id,
                input,
                transcript_revision,
                profile,
            } => {
                self.start_translation(&id, &input, transcript_revision, &profile)?;
                id
            }
            Op::Cancel { id, generation } => {
                if self.library.store().analysis_job_kind(&id)?.is_none()
                    && self.library.store().translation_job(&id).is_ok()
                {
                    self.cancel_translation(&id, generation)?;
                } else {
                    self.cancel_verification(&id, generation)?;
                }
                id
            }
            command => return Ok(super::super::Operation::Analysis { command }),
        };
        Ok(super::super::Operation::Analysis {
            command: Op::Job { id: job },
        })
    }

    pub(super) fn start_recognition(&mut self, request: TranscribeRequest) -> Result<()> {
        let store = self.library.store();
        let profile = store.recognition_profile(&request.profile)?;
        let parent_revision = match request.parent_revision {
            Some(parent) => parent,
            None => match store.local_asr_job(&request.id) {
                Ok(job) => job.request.parent_revision,
                Err(Error::NotFound) => store.latest_transcript_revision(&request.input)?,
                Err(error) => return Err(error),
            },
        };
        let request = LocalAsrRequest {
            id: request.id,
            analysis_id: request.input,
            analysis_revision: request.revision,
            profile: profile.id.clone(),
            profile_sha256: profile.profile_sha256.clone(),
            parent_revision,
        };
        if let Ok(job) = store.local_asr_job(&request.id) {
            // Replay returns history. It never selects a new model or reconnects.
            return if job.request == request {
                Ok(())
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        self.decoder()?;
        self.library
            .store_mut()
            .enqueue_local_asr(&request, now_ms()?)?;
        self.schedule()
    }

    fn launch_recognition(&mut self, id: &str) -> Result<()> {
        let decoder = self.decoder()?;
        let owner = self.pool.owner.clone();
        let Some(work) = self
            .library
            .store_mut()
            .claim_local_asr(id, &owner, now_ms()?)?
        else {
            return Ok(());
        };
        let store = self.library.store();
        let format = store
            .recording(&work.input.recording_id)
            .ok()
            .and_then(|recording| {
                recording
                    .intervals
                    .into_iter()
                    .find(|interval| interval.ordinal == work.input.interval_ordinal)
                    .map(|interval| interval.format)
            });
        let profile = store.recognition_profile(&work.job.request.profile);
        let task = match (format, profile) {
            (Some(format), Ok(profile)) => Ok(RecognitionTask {
                directory: self.library.directory().to_path_buf(),
                decoder,
                format,
                profile,
                job: work.job.clone(),
                input: work.input.clone(),
            }),
            _ => Err(Error::StorageIntegrity),
        };
        let spawned = task.and_then(|task| {
            let worker_id = work.job.request.id.clone();
            let generation = work.job.generation;
            self.spawn_worker(
                move |signal| recognizer::run(task, signal),
                move |result| Message::RecognitionFinished {
                    id: worker_id,
                    generation,
                    result,
                },
            )
        });
        match spawned {
            Ok(worker) => {
                self.pool.recognition.insert(
                    work.job.request.id.clone(),
                    RecognitionWorker {
                        generation: work.job.generation,
                        worker,
                        work,
                    },
                );
                Ok(())
            }
            Err(error) => {
                let reaped = recognizer::not_started(&work.job)?;
                self.library
                    .store_mut()
                    .finish_local_asr(&work, &reaped, now_ms()?)?;
                Err(error)
            }
        }
    }

    /// An error result means the process tree could not be proven drained. The read
    /// lease stays active and the caller stops the service; restart recovers the job.
    pub(super) fn finish_recognition(
        &mut self,
        id: &str,
        generation: u32,
        result: Result<ReapedLocalAsr>,
    ) -> Result<()> {
        if self
            .pool
            .recognition
            .get(id)
            .is_none_or(|active| active.generation != generation)
        {
            return Ok(());
        }
        let Some(active) = self.pool.recognition.remove(id) else {
            return Ok(());
        };
        let work = active.work;
        let reaped = result?;
        let now = now_ms()?;
        let job = self
            .library
            .store_mut()
            .finish_local_asr(&work, &reaped, now)?;
        if job.state == "succeeded"
            && let Some(code) = reaped.language()
        {
            // Best effort after the transcript commit: missing evidence stays visibly absent
            // and never rewrites or fails the published transcript.
            let evidence = recognizer::language_evidence(&job, &work.input, code);
            let _ = self
                .library
                .store_mut()
                .publish_language_evidence(evidence, now);
        }
        self.schedule()
    }
}

impl Actor {
    pub(super) fn start_translation(
        &mut self,
        id: &str,
        input: &str,
        transcript_revision: Option<i64>,
        profile: &str,
    ) -> Result<()> {
        use crate::translation::TranslationRequest;
        let store = self.library.store();
        let profile = store.translation_profile(profile)?;
        let transcript_revision = match transcript_revision {
            Some(revision) => revision,
            None => match store.translation_job(id) {
                Ok(job) => job.request.transcript_revision,
                Err(Error::NotFound) => store.latest_transcript_revision(input)?,
                Err(error) => return Err(error),
            },
        };
        let request = TranslationRequest {
            id: id.to_owned(),
            transcript_id: input.to_owned(),
            transcript_revision,
            profile: profile.id.clone(),
            profile_sha256: profile.profile_sha256.clone(),
        };
        if let Ok(job) = store.translation_job(id) {
            // Replay returns history and never runs the translator again.
            return if job.request == request {
                Ok(())
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        self.library
            .store_mut()
            .enqueue_translation(&request, now_ms()?)?;
        self.schedule()
    }

    fn launch_translation(&mut self, id: &str) -> Result<()> {
        let owner = self.pool.owner.clone();
        let Some(work) = self
            .library
            .store_mut()
            .claim_translation(id, &owner, now_ms()?)?
        else {
            return Ok(());
        };
        let spawned = self
            .library
            .store()
            .translation_profile(&work.job.request.profile)
            .and_then(|profile| {
                let task = recognizer::translate::TranslationTask {
                    directory: self.library.directory().to_path_buf(),
                    profile,
                    job: work.job.clone(),
                    cues: work.cues.clone(),
                    language: work.language.clone(),
                };
                let worker_id = work.job.request.id.clone();
                let generation = work.job.generation;
                self.spawn_worker(
                    move |signal| recognizer::translate::run(task, signal),
                    move |result| Message::TranslationFinished {
                        id: worker_id,
                        generation,
                        result,
                    },
                )
            });
        match spawned {
            Ok(worker) => {
                self.pool.translation.insert(
                    work.job.request.id.clone(),
                    TranslationWorker {
                        generation: work.job.generation,
                        worker,
                        work,
                    },
                );
                Ok(())
            }
            Err(error) => {
                let envelope = execution::refuse_translation(
                    &work.job.request.id,
                    work.job.generation,
                    "worker-start-failed",
                );
                let outcome = TranslationOutcome::from_envelope(&work.job, envelope, None)?;
                self.library
                    .store_mut()
                    .finish_translation(&work, &outcome, now_ms()?)?;
                Err(error)
            }
        }
    }

    fn cancel_translation(&mut self, id: &str, generation: u32) -> Result<()> {
        let job = self
            .library
            .store_mut()
            .cancel_translation(id, generation)?;
        if matches!(job.state.as_str(), "running" | "cancelling") {
            self.pool.signal(id, generation);
        }
        Ok(())
    }

    /// An error result means process cleanup was not proven; the caller stops the service.
    pub(super) fn finish_translation(
        &mut self,
        id: &str,
        generation: u32,
        result: Result<TranslationOutcome>,
    ) -> Result<()> {
        if self
            .pool
            .translation
            .get(id)
            .is_none_or(|active| active.generation != generation)
        {
            return Ok(());
        }
        let Some(active) = self.pool.translation.remove(id) else {
            return Ok(());
        };
        let outcome = result?;
        self.library
            .store_mut()
            .finish_translation(&active.work, &outcome, now_ms()?)?;
        self.schedule()
    }
}

#[cfg(test)]
mod tests;
