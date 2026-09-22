use super::{Actor, Message, Worker};
use crate::{
    Error, Result, processing,
    storage::{analysis_jobs::AnalysisJob, now_ms},
};

pub(super) struct AnalysisWorker {
    pub id: String,
    pub generation: u32,
    pub worker: Worker,
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
        let job = self
            .library
            .store_mut()
            .cancel_analysis_job(id, generation)?;
        if job.active()
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
        if active.id != id || active.generation != generation {
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

#[cfg(test)]
mod tests;
