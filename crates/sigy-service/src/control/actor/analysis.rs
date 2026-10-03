//! The service scheduler for the durable local job pool.
//!
//! Admission only enqueues. [`Actor::schedule`] claims queued jobs in source rotation,
//! with every fourth claim reserved for the oldest waiting job, up to each kind's
//! concurrency cap and never two in one transcript lineage, and starts one supervised
//! worker per claim. A monitored recognition job is live while its measured pace still
//! fits after the last seal. Verification and translation stay batch. Capture workers
//! are separate and never wait on the pool.

use std::collections::HashMap;

use super::{Actor, Message, Worker};
use crate::{
    Error, Result, execution,
    fairness::{FairCursor, Pick},
    processing,
    recognition::{LocalAsrRequest, LocalAsrWork, ReapedLocalAsr},
    recognizer::{self, RecognitionTask},
    storage::{
        analysis_jobs::AnalysisJob,
        job_pool::{JobKind, PoolCaps},
        now_ms,
    },
    translation::{TranslationOutcome, TranslationRequest, TranslationWork},
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

/// One rotation cursor for each job kind. It lasts for this service process.
#[derive(Debug, Default)]
struct Fairness {
    verification: FairCursor,
    recognition: FairCursor,
    translation: FairCursor,
}

impl Fairness {
    fn cursor(&self, kind: JobKind) -> &FairCursor {
        match kind {
            JobKind::Verification => &self.verification,
            JobKind::Recognition => &self.recognition,
            JobKind::Translation => &self.translation,
        }
    }

    fn advance(&mut self, kind: JobKind, pick: &Pick) {
        let cursor = match kind {
            JobKind::Verification => &mut self.verification,
            JobKind::Recognition => &mut self.recognition,
            JobKind::Translation => &mut self.translation,
        };
        cursor.advance(pick);
    }
}

/// Running pool workers, by job ID, and the caps and lease owner of this service.
pub(super) struct Pool {
    pub caps: PoolCaps,
    pub owner: String,
    /// False once the service is stopping; no new job starts after that.
    pub accepting: bool,
    fairness: Fairness,
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
            fairness: Fairness::default(),
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

    /// Signal exactly this family, job and generation, if one runs.
    fn signal(&self, kind: JobKind, id: &str, generation: u32) {
        let worker = match kind {
            JobKind::Verification => self
                .verification
                .get(id)
                .map(|active| (active.generation, &active.worker)),
            JobKind::Recognition => self
                .recognition
                .get(id)
                .map(|active| (active.generation, &active.worker)),
            JobKind::Translation => self
                .translation
                .get(id)
                .map(|active| (active.generation, &active.worker)),
        }
        .filter(|(active_generation, _)| *active_generation == generation)
        .map(|(_, worker)| worker);
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

pub(super) struct Prepared<T> {
    pub request: T,
    pub replay: bool,
}

impl Actor {
    /// Start queued jobs until each kind reaches its cap or runs out of eligible work.
    /// # Errors
    /// Returns catalog failures and a worker that could not be started.
    pub(super) fn schedule(&mut self) -> Result<()> {
        self.signal_durable_stops()?;
        for kind in JobKind::ALL {
            if kind != JobKind::Verification && self.library.store().native_completion_unproven()? {
                continue;
            }
            let mut claims = 0;
            while self.pool.accepting
                && self.pool.running(kind) < self.pool.caps.cap(kind)
                && claims < MAX_CLAIMS_PER_PASS
            {
                if kind == JobKind::Recognition && self.decoder().is_err() {
                    // Recognition stays queued until a decoder is configured again.
                    break;
                }
                let cursor = self.pool.fairness.cursor(kind).clone();
                let now = now_ms()?;
                let Some(pick) = self.library.store().next_fair(kind, &cursor, now)? else {
                    break;
                };
                claims += 1;
                let started = match kind {
                    JobKind::Verification => self.launch_verification(&pick.id)?,
                    JobKind::Recognition => self.launch_recognition(&pick.id)?,
                    JobKind::Translation => self.launch_translation(&pick.id)?,
                };
                if started {
                    self.pool.fairness.advance(kind, &pick);
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

    fn launch_verification(&mut self, id: &str) -> Result<bool> {
        let owner = self.pool.owner.clone();
        let Some((job, manifest)) =
            self.library
                .store_mut()
                .claim_verification(id, &owner, now_ms()?)?
        else {
            return Ok(false);
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
                Ok(true)
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
            self.pool.signal(
                if native {
                    JobKind::Recognition
                } else {
                    JobKind::Verification
                },
                id,
                generation,
            );
        }
        Ok(())
    }

    pub(super) fn signal_task_withdrawal(
        &self,
        view: &crate::task::withdrawal::TaskWithdrawalView,
    ) {
        for interest in &view.receipt.interests {
            if interest.decision == "running-cancelling" {
                let kind = match interest.family.as_str() {
                    "recognition" => JobKind::Recognition,
                    "translation" => JobKind::Translation,
                    _ => continue,
                };
                self.pool
                    .signal(kind, &interest.job_id, interest.job_generation);
            }
        }
    }

    fn signal_durable_stops(&self) -> Result<()> {
        for (id, active) in &self.pool.recognition {
            if self
                .library
                .store()
                .native_stop_requested("recognition", id, active.generation)?
            {
                self.pool
                    .signal(JobKind::Recognition, id, active.generation);
            }
        }
        for (id, active) in &self.pool.translation {
            if self
                .library
                .store()
                .native_stop_requested("translation", id, active.generation)?
            {
                self.pool
                    .signal(JobKind::Translation, id, active.generation);
            }
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
        let prepared = self.prepare_recognition(request)?;
        if prepared.replay {
            return Ok(());
        }
        self.library
            .store_mut()
            .enqueue_local_asr(&prepared.request, now_ms()?)?;
        self.schedule()
    }

    pub(super) fn prepare_recognition(
        &self,
        request: TranscribeRequest,
    ) -> Result<Prepared<LocalAsrRequest>> {
        let store = self.library.store();
        let profile = store.recognition_profile(&request.profile)?;
        let existing = match store.local_asr_job(&request.id) {
            Ok(job) => Some(job),
            Err(Error::NotFound) => None,
            Err(error) => return Err(error),
        };
        let parent_revision = match request.parent_revision {
            Some(parent) => parent,
            None => match &existing {
                Some(job) => job.request.parent_revision,
                None => store.latest_transcript_revision(&request.input)?,
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
        if let Some(job) = existing {
            // Replay returns history. It never selects a new model or reconnects.
            return if job.request == request {
                Ok(Prepared {
                    request,
                    replay: true,
                })
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        self.decoder()?;
        Ok(Prepared {
            request,
            replay: false,
        })
    }

    fn launch_recognition(&mut self, id: &str) -> Result<bool> {
        let decoder = self.decoder()?;
        let owner = self.pool.owner.clone();
        let Some(work) = self
            .library
            .store_mut()
            .claim_local_asr(id, &owner, now_ms()?)?
        else {
            return Ok(false);
        };
        let profile = self
            .library
            .store()
            .recognition_profile(&work.job.request.profile);
        let task = match profile {
            Ok(profile) if !work.input.segments.is_empty() && !work.input.chunks.is_empty() => {
                Ok(RecognitionTask {
                    directory: self.library.directory().to_path_buf(),
                    decoder,
                    profile,
                    job: work.job.clone(),
                    input: work.input.clone(),
                })
            }
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
                Ok(true)
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
        if job.state == "succeeded" && !reaped.languages().is_empty() {
            // Best effort after the transcript commit: missing evidence stays visibly absent
            // and never rewrites or fails the published transcript.
            let evidence = recognizer::language_evidence(&job, reaped.languages());
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
        let prepared = self.prepare_translation(id, input, transcript_revision, profile)?;
        if prepared.replay {
            return Ok(());
        }
        self.library
            .store_mut()
            .enqueue_translation(&prepared.request, now_ms()?)?;
        self.schedule()
    }

    pub(super) fn prepare_translation(
        &self,
        id: &str,
        input: &str,
        transcript_revision: Option<i64>,
        profile: &str,
    ) -> Result<Prepared<TranslationRequest>> {
        let store = self.library.store();
        let profile = store.translation_profile(profile)?;
        let existing = match store.translation_job(id) {
            Ok(job) => Some(job),
            Err(Error::NotFound) => None,
            Err(error) => return Err(error),
        };
        let transcript_revision = match transcript_revision {
            Some(revision) => revision,
            None => match &existing {
                Some(job) => job.request.transcript_revision,
                None => store.latest_transcript_revision(input)?,
            },
        };
        let request = TranslationRequest {
            id: id.to_owned(),
            transcript_id: input.to_owned(),
            transcript_revision,
            profile: profile.id.clone(),
            profile_sha256: profile.profile_sha256.clone(),
        };
        if let Some(job) = existing {
            // Replay returns history and never runs the translator again.
            return if job.request == request {
                Ok(Prepared {
                    request,
                    replay: true,
                })
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        Ok(Prepared {
            request,
            replay: false,
        })
    }

    fn launch_translation(&mut self, id: &str) -> Result<bool> {
        let owner = self.pool.owner.clone();
        let Some(work) = self
            .library
            .store_mut()
            .claim_translation(id, &owner, now_ms()?)?
        else {
            return Ok(false);
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
                Ok(true)
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
            self.pool.signal(JobKind::Translation, id, generation);
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
