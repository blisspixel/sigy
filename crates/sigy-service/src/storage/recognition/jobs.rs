use rusqlite::{OptionalExtension, TransactionBehavior};

use super::super::job_pool::{self, Family};
use super::{Error, LocalAsrJob, Result, Store, find_job, validate_key};
use super::{LocalAsrRequest, LocalAsrWork, current_input, manifest, params, sql_integer};

impl Store {
    /// Queue one bounded local recognition job, with an exact immutable request.
    /// A replay returns history, even after media expiry, and never dispatches.
    /// # Errors
    /// Refuses changed replay, stale pins/parents, a full queue or invalid bounds.
    pub(crate) fn enqueue_local_asr(
        &mut self,
        request: &LocalAsrRequest,
        now: i64,
    ) -> Result<(LocalAsrJob, bool)> {
        request.validate()?;
        if let Some(job) = find_job(&self.connection, &request.id)? {
            if job.request != *request {
                return Err(Error::IdempotencyConflict);
            }
            return Ok((job, false));
        }
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        let input = self.local_asr_input(request)?;
        let manifest_sha256 = manifest(&input)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        job_pool::check_open_bound(&tx, Family::Analysis)?;
        let work = LocalAsrWork {
            job: LocalAsrJob {
                request: request.clone(),
                generation: 1,
                recording_id: input.recording_id.clone(),
                state: "queued".into(),
                expected_bytes: input.byte_length,
                manifest_sha256,
                reason: None,
                amount_usd: "0.000000".into(),
                created_ms: now,
                finished_ms: None,
                attempt: 1,
                started_ms: None,
            },
            input,
        };
        if !current_input(&tx, &work)? {
            return Err(Error::Analysis("input-no-longer-current"));
        }
        if latest_parent(&tx, &request.analysis_id)? != request.parent_revision {
            return Err(Error::Analysis("transcript-parent-conflict"));
        }
        tx.execute(
            "INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage) VALUES (?1, 1, ?2, ?3, ?4, ?5, 'local_asr', ?6, ?7, 'queued', ?8, 1, ?9, 0, ?10, ?2)",
            params![request.id, request.analysis_id, request.analysis_revision, work.input.recording_id, request.profile, request.profile_sha256, request.parent_revision, sql_integer(work.input.byte_length)?, work.job.manifest_sha256, now],
        )?;
        tx.commit()?;
        Ok((self.local_asr_job(&request.id)?, true))
    }

    /// Start one queued recognition under this owner. The input and transcript parent are
    /// checked again; a job whose input or parent moved on fails without a worker.
    /// Only a committed claim returns a work token.
    /// # Errors
    /// Returns catalog failures.
    pub(crate) fn claim_local_asr(
        &mut self,
        id: &str,
        owner: &str,
        now: i64,
    ) -> Result<Option<LocalAsrWork>> {
        let job = self.local_asr_job(id)?;
        if job.state != "queued" {
            return Ok(None);
        }
        let input = match self.local_asr_input(&job.request) {
            Ok(input) if manifest(&input)? == job.manifest_sha256 => Some(input),
            Ok(_) => None,
            Err(error) if super::super::analysis_jobs::input_moved_on(&error) => None,
            Err(error) => return Err(error),
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let reason = match input {
            None => Some("input-no-longer-current"),
            Some(input) => {
                let work = LocalAsrWork {
                    job: job.clone(),
                    input,
                };
                if !current_input(&tx, &work)? {
                    Some("input-no-longer-current")
                } else if latest_parent(&tx, &job.request.analysis_id)?
                    != job.request.parent_revision
                {
                    Some("transcript-parent-conflict")
                } else {
                    // A profile row is not a foreign key of the job; bound by the maximum.
                    let deadline: i64 = tx
                        .query_row(
                            "SELECT deadline_ms FROM recognition_profiles WHERE id = ?1",
                            [&job.request.profile],
                            |row| row.get(0),
                        )
                        .optional()?
                        .unwrap_or(3_600_000);
                    let lease = now + deadline + job_pool::LEASE_MARGIN_MS;
                    if !job_pool::claim_row(&tx, Family::Analysis, id, owner, lease, now)? {
                        return Ok(None);
                    }
                    tx.commit()?;
                    return Ok(Some(LocalAsrWork {
                        job: self.local_asr_job(id)?,
                        input: work.input,
                    }));
                }
            }
        };
        if let Some(reason) = reason {
            job_pool::end_queued(&tx, Family::Analysis, id, "failed", reason, now)?;
        }
        tx.commit()?;
        Ok(None)
    }

    /// Queue and immediately start one recognition, as tests did before the pool.
    #[cfg(test)]
    pub(crate) fn admit_local_asr(
        &mut self,
        request: &LocalAsrRequest,
        now: i64,
    ) -> Result<(LocalAsrJob, Option<LocalAsrWork>)> {
        let (job, created) = self.enqueue_local_asr(request, now)?;
        if !created {
            return Ok((job, None));
        }
        match self.claim_local_asr(&request.id, super::super::analysis_jobs::TEST_OWNER, now)? {
            Some(work) => Ok((work.job.clone(), Some(work))),
            None => Ok((self.local_asr_job(&request.id)?, None)),
        }
    }

    /// Read one exact local recognition job without dispatch.
    /// # Errors
    /// Refuses missing IDs, other job kinds and invalid storage.
    pub fn local_asr_job(&self, id: &str) -> Result<LocalAsrJob> {
        validate_key(id, "analysis job ID")?;
        find_job(&self.connection, id)?.ok_or(Error::NotFound)
    }

    /// Request cancellation of the exact generation. A queued job ends at once; a
    /// running job keeps its durable read lease until its worker stops.
    /// # Errors
    /// Refuses missing IDs, another job kind or a stale generation.
    pub(crate) fn cancel_local_asr(&mut self, id: &str, generation: u32) -> Result<LocalAsrJob> {
        let job = self.local_asr_job(id)?;
        if job.generation != generation {
            return Err(Error::Analysis("stale-worker"));
        }
        if job.state == "queued" {
            job_pool::end_queued(
                &self.connection,
                Family::Analysis,
                id,
                "cancelled",
                "cancelled",
                super::super::now_ms()?,
            )?;
        } else if job.state == "running" {
            self.connection.execute("UPDATE analysis_jobs SET state = 'cancelling' WHERE id = ?1 AND generation = ?2 AND state = 'running'", params![id, generation])?;
        }
        self.local_asr_job(id)
    }
}

fn latest_parent(connection: &rusqlite::Connection, analysis_id: &str) -> Result<i64> {
    Ok(connection.query_row(
        "SELECT coalesce(max(revision), 0) FROM transcripts WHERE id = ?1",
        [analysis_id],
        |row| row.get(0),
    )?)
}
