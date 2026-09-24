use rusqlite::TransactionBehavior;

use super::{Error, LocalAsrJob, Result, Store, find_job, validate_key};
use super::{LocalAsrRequest, LocalAsrWork, current_input, manifest, params, sql_integer};

impl Store {
    /// Admit one bounded local recognition job, with an exact immutable request.
    /// A replay returns history and no work capability, even after media expiry.
    /// # Errors
    /// Refuses changed replay, stale pins/parents, occupied worker slots or invalid bounds.
    pub(crate) fn admit_local_asr(
        &mut self,
        request: &LocalAsrRequest,
        now: i64,
    ) -> Result<(LocalAsrJob, Option<LocalAsrWork>)> {
        request.validate()?;
        if let Some(job) = find_job(&self.connection, &request.id)? {
            if job.request != *request {
                return Err(Error::IdempotencyConflict);
            }
            return Ok((job, None));
        }
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        let input = self.local_asr_input(request)?;
        let job = LocalAsrJob {
            request: request.clone(),
            generation: 1,
            recording_id: input.recording_id.clone(),
            state: "running".into(),
            expected_bytes: input.byte_length,
            manifest_sha256: manifest(&input)?,
            reason: None,
            amount_usd: "0.000000".into(),
            created_ms: now,
            finished_ms: None,
        };
        let work = LocalAsrWork { job, input };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (count, active): (i64, bool) = tx.query_row(
            "SELECT count(*), EXISTS(SELECT 1 FROM analysis_jobs WHERE state IN ('running', 'cancelling')) FROM analysis_jobs", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if count >= 256 {
            return Err(Error::Analysis("job-history-limit"));
        }
        if active {
            return Err(Error::Analysis("worker-busy"));
        }
        if !current_input(&tx, &work)? {
            return Err(Error::Analysis("input-no-longer-current"));
        }
        let parent: i64 = tx.query_row(
            "SELECT coalesce(max(revision), 0) FROM transcripts WHERE id = ?1",
            [&request.analysis_id],
            |row| row.get(0),
        )?;
        if parent != request.parent_revision {
            return Err(Error::Analysis("transcript-parent-conflict"));
        }
        tx.execute(
            "INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms) VALUES (?1, 1, ?2, ?3, ?4, ?5, 'local_asr', ?6, ?7, 'running', ?8, 1, ?9, 0, ?10)",
            params![request.id, request.analysis_id, request.analysis_revision, work.input.recording_id, request.profile, request.profile_sha256, request.parent_revision, sql_integer(work.input.byte_length)?, work.job.manifest_sha256, now],
        )?;
        tx.commit()?;
        Ok((work.job.clone(), Some(work)))
    }

    /// Read one exact local recognition job without dispatch.
    /// # Errors
    /// Refuses missing IDs, other job kinds and invalid storage.
    pub fn local_asr_job(&self, id: &str) -> Result<LocalAsrJob> {
        validate_key(id, "analysis job ID")?;
        find_job(&self.connection, id)?.ok_or(Error::NotFound)
    }

    /// Request cancellation of the exact generation. The durable read lease stays active.
    /// # Errors
    /// Refuses missing IDs, another job kind or a stale generation.
    pub(crate) fn cancel_local_asr(&mut self, id: &str, generation: u32) -> Result<LocalAsrJob> {
        let job = self.local_asr_job(id)?;
        if job.generation != generation {
            return Err(Error::Analysis("stale-worker"));
        }
        if job.state == "running" {
            self.connection.execute("UPDATE analysis_jobs SET state = 'cancelling' WHERE id = ?1 AND generation = ?2 AND state = 'running'", params![id, generation])?;
        }
        self.local_asr_job(id)
    }
}
