use super::{Actor, Message, Worker};
use crate::{
    Error, Result, processing,
    recognition::{LocalAsrRequest, LocalAsrWork, ReapedLocalAsr},
    recognizer::{self, RecognitionTask},
    storage::{analysis_jobs::AnalysisJob, now_ms},
};

pub(super) struct AnalysisWorker {
    pub id: String,
    pub generation: u32,
    pub worker: Worker,
    /// Present for a native recognition job; the admitted work token stays here.
    pub recognition: Option<LocalAsrWork>,
}

pub(crate) struct TranscribeRequest {
    pub id: String,
    pub input: String,
    pub revision: i64,
    pub profile: String,
    pub parent_revision: Option<i64>,
}

impl Actor {
    pub(super) fn start_verification(
        &mut self,
        id: &str,
        input: &str,
        revision: i64,
    ) -> Result<()> {
        let (job, manifest) =
            self.library
                .store_mut()
                .admit_verification(id, input, revision, now_ms()?)?;
        let Some(manifest) = manifest else {
            return Ok(());
        };
        if self.analysis_worker.is_some() {
            self.fail_analysis_spawn(&job)?;
            return Err(Error::Analysis("worker-busy"));
        }
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
                self.analysis_worker = Some(AnalysisWorker {
                    id: job.id,
                    generation,
                    worker,
                    recognition: None,
                });
            }
            Err(error) => {
                self.fail_analysis_spawn(&job)?;
                return Err(error);
            }
        }
        Ok(())
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
        if active_job
            && let Some(active) = &self.analysis_worker
            && active.id == id
            && active.generation == generation
        {
            active.worker.stop.send_replace(true);
        }
        Ok(())
    }

    pub(super) fn finish_verification(
        &mut self,
        id: &str,
        generation: u32,
        result: Result<processing::VerificationReceipt>,
    ) -> Result<()> {
        let Some(active) = &self.analysis_worker else {
            return Ok(());
        };
        if active.id != id || active.generation != generation || active.recognition.is_some() {
            return Ok(());
        }
        // Only actual worker completion arrives here. A failed commit retains the durable
        // lease and stops the actor, but must not leave shutdown waiting on a finished task.
        let outcome = now_ms().and_then(|now| {
            self.library
                .store_mut()
                .finish_verification(id, generation, result, now)
        });
        self.analysis_worker.take();
        outcome?;
        Ok(())
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
            Op::Cancel { id, generation } => {
                self.cancel_verification(&id, generation)?;
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
        let decoder = self.decoder()?;
        if self.analysis_worker.is_some() {
            return Err(Error::Analysis("worker-busy"));
        }
        let (_, work) = self
            .library
            .store_mut()
            .admit_local_asr(&request, now_ms()?)?;
        let Some(work) = work else {
            return Ok(());
        };
        let format = self
            .library
            .store()
            .recording(&work.input.recording_id)
            .ok()
            .and_then(|recording| {
                recording
                    .intervals
                    .into_iter()
                    .find(|interval| interval.ordinal == work.input.interval_ordinal)
                    .map(|interval| interval.format)
            });
        let task = format.map(|format| RecognitionTask {
            directory: self.library.directory().to_path_buf(),
            decoder,
            format,
            profile,
            job: work.job.clone(),
            input: work.input.clone(),
        });
        let spawned = task.ok_or(Error::StorageIntegrity).and_then(|task| {
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
                self.analysis_worker = Some(AnalysisWorker {
                    id: work.job.request.id.clone(),
                    generation: work.job.generation,
                    worker,
                    recognition: Some(work),
                });
                Ok(())
            }
            Err(error) => {
                let reaped = recognizer::not_started(&work.job);
                self.library
                    .store_mut()
                    .finish_local_asr(&work, &reaped, now_ms()?)?;
                Err(error)
            }
        }
    }

    /// An error result means the process tree could not be proven drained. The read
    /// lease stays active and the caller stops the service; restart interrupts the job.
    pub(super) fn finish_recognition(
        &mut self,
        id: &str,
        generation: u32,
        result: Result<ReapedLocalAsr>,
    ) -> Result<()> {
        let Some(active) = &self.analysis_worker else {
            return Ok(());
        };
        if active.id != id || active.generation != generation || active.recognition.is_none() {
            return Ok(());
        }
        let Some(active) = self.analysis_worker.take() else {
            return Ok(());
        };
        let Some(work) = active.recognition else {
            return Err(Error::StorageIntegrity);
        };
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
        Ok(())
    }
}

#[cfg(test)]
mod tests;
