use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{
    Store,
    job_pool::{self, Family},
    validate_key,
};
use crate::{
    Error, Result,
    processing::{
        InputFile, MAX_INPUT_BYTES, MAX_INPUT_FILES, VerificationInput, VerificationReceipt,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisJob {
    pub id: String,
    pub generation: u32,
    pub analysis_id: String,
    pub analysis_revision: i64,
    pub recording_id: String,
    pub profile: String,
    pub state: String,
    pub expected_bytes: u64,
    pub expected_files: usize,
    pub manifest_sha256: String,
    pub verified_bytes: Option<u64>,
    pub reason: Option<String>,
    pub amount_usd: String,
    pub created_ms: i64,
    pub finished_ms: Option<i64>,
    /// Starts of this job, including the current one. A restart can requeue a job.
    pub attempt: u32,
    /// When the current attempt started; absent while queued.
    pub started_ms: Option<i64>,
}

impl AnalysisJob {
    pub(crate) fn active(&self) -> bool {
        matches!(self.state.as_str(), "running" | "cancelling")
    }
}

/// Whether a failure to rebuild a queued job's input means the input moved on, rather
/// than a catalog fault that must stop the service.
pub(super) fn input_moved_on(error: &Error) -> bool {
    !matches!(error, Error::Database(_) | Error::Io(_))
}

impl Store {
    pub(crate) fn verification_input(&self, id: &str, revision: i64) -> Result<VerificationInput> {
        let pin = self.published_analysis(id, revision)?;
        let recording = self.recording(&pin.recording_id)?;
        let mut input = VerificationInput {
            files: Vec::new(),
            bytes: 0,
        };
        for span in pin.intervals {
            let interval = recording
                .intervals
                .iter()
                .find(|interval| {
                    interval.ordinal == span.ordinal
                        && !interval.released
                        && interval.sha256 == span.sha256
                        && interval.decoded_start_us == span.start_us
                        && interval.decoded_end_us == span.end_us
                })
                .ok_or(Error::StorageIntegrity)?;
            let bytes = interval
                .byte_end
                .checked_sub(interval.byte_start)
                .ok_or(Error::StorageIntegrity)?;
            input.bytes = input
                .bytes
                .checked_add(bytes)
                .ok_or(Error::Analysis("input-limit"))?;
            input.files.push(InputFile {
                key: interval.object_key.clone(),
                sha256: span.sha256,
                bytes,
            });
        }
        if input.files.is_empty()
            || input.files.len() > MAX_INPUT_FILES
            || input.bytes == 0
            || input.bytes > MAX_INPUT_BYTES
        {
            return Err(Error::Analysis("input-limit"));
        }
        Ok(input)
    }

    /// Queue one verification of a published pin. A replay returns the stored job.
    /// # Errors
    /// Refuses a changed replay, an unpublished or oversized input, or a full queue.
    pub(crate) fn enqueue_verification(
        &mut self,
        id: &str,
        pin: &str,
        revision: i64,
        now: i64,
    ) -> Result<(AnalysisJob, bool)> {
        validate_key(id, "analysis job ID")?;
        let existing = match self.find_analysis_job(id) {
            Err(Error::Analysis("recognition-job")) => {
                return Err(Error::IdempotencyConflict);
            }
            result => result?,
        };
        if let Some(existing) = existing {
            if existing.analysis_id != pin || existing.analysis_revision != revision {
                return Err(Error::IdempotencyConflict);
            }
            return Ok((existing, false));
        }
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        let input = self.verification_input(pin, revision)?;
        let digest = crate::processing::manifest_digest(&input.files)?;
        let input_bytes = i64::try_from(input.bytes).map_err(|_| Error::StorageIntegrity)?;
        let input_files = i64::try_from(input.files.len()).map_err(|_| Error::StorageIntegrity)?;
        let record = self.published_analysis(pin, revision)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        job_pool::check_open_bound(&transaction, Family::Analysis)?;
        transaction.execute(
            "INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage) VALUES (?1, 1, ?2, ?3, ?4, 'retained-sha256-v1', 'queued', ?5, ?6, ?7, 0, ?8, ?2)",
            params![id, pin, revision, record.recording_id, input_bytes, input_files, digest, now])?;
        transaction.commit()?;
        Ok((self.analysis_job(id)?, true))
    }

    /// Start one queued verification under this owner, with its input rebuilt from the
    /// current catalog. A job whose input moved on fails without a worker.
    /// # Errors
    /// Returns catalog failures.
    pub(crate) fn claim_verification(
        &mut self,
        id: &str,
        owner: &str,
        now: i64,
    ) -> Result<Option<(AnalysisJob, VerificationInput)>> {
        let job = self.analysis_job(id)?;
        if job.state != "queued" {
            return Ok(None);
        }
        let input = match self.verification_input(&job.analysis_id, job.analysis_revision) {
            Ok(input)
                if input.bytes == job.expected_bytes
                    && input.files.len() == job.expected_files
                    && crate::processing::manifest_digest(&input.files)? == job.manifest_sha256 =>
            {
                Some(input)
            }
            Ok(_) => None,
            Err(error) if input_moved_on(&error) => None,
            Err(error) => return Err(error),
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(input) = input else {
            job_pool::end_queued(
                &transaction,
                Family::Analysis,
                id,
                "failed",
                "input-no-longer-current",
                now,
            )?;
            transaction.commit()?;
            return Ok(None);
        };
        let lease = now + 60_000 + job_pool::LEASE_MARGIN_MS;
        if !job_pool::claim_row(&transaction, Family::Analysis, id, owner, lease, now)? {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some((self.analysis_job(id)?, input)))
    }

    /// Queue and immediately start one verification, as tests did before the pool.
    #[cfg(test)]
    pub(crate) fn admit_verification(
        &mut self,
        id: &str,
        pin: &str,
        revision: i64,
        now: i64,
    ) -> Result<(AnalysisJob, Option<VerificationInput>)> {
        let (job, created) = self.enqueue_verification(id, pin, revision, now)?;
        if !created {
            return Ok((job, None));
        }
        match self.claim_verification(id, TEST_OWNER, now)? {
            Some((job, input)) => Ok((job, Some(input))),
            None => Ok((self.analysis_job(id)?, None)),
        }
    }

    pub(crate) fn analysis_job(&self, id: &str) -> Result<AnalysisJob> {
        validate_key(id, "analysis job ID")?;
        self.find_analysis_job(id)?.ok_or(Error::NotFound)
    }

    fn find_analysis_job(&self, id: &str) -> Result<Option<AnalysisJob>> {
        let kind: Option<String> = self
            .connection
            .query_row(
                "SELECT kind FROM analysis_jobs WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if kind.as_deref().is_some_and(|kind| kind != "verify") {
            return Err(Error::Analysis("recognition-job"));
        }
        read_verification_job(&self.connection, id)
    }

    pub(crate) fn cancel_analysis_job(&mut self, id: &str, generation: u32) -> Result<AnalysisJob> {
        let job = self.analysis_job(id)?;
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
                super::now_ms()?,
            )?;
        } else if job.state == "running" {
            self.connection.execute("UPDATE analysis_jobs SET state = 'cancelling' WHERE id = ?1 AND generation = ?2 AND state = 'running'", params![id, generation])?;
        }
        self.analysis_job(id)
    }

    pub(crate) fn finish_verification(
        &mut self,
        id: &str,
        generation: u32,
        result: Result<VerificationReceipt>,
        now: i64,
    ) -> Result<bool> {
        let job = self.analysis_job(id)?;
        if job.generation != generation || !job.active() {
            return Ok(false);
        }
        let (state, reason, verified) = if job.state == "cancelling" {
            ("cancelled", Some("cancelled"), None)
        } else {
            self.verification_outcome(&job, result)
        };
        let verified = verified
            .map(i64::try_from)
            .transpose()
            .map_err(|_| Error::StorageIntegrity)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE analysis_jobs SET state = ?3, reason = ?4, verified_bytes = ?5, finished_ms = ?6 WHERE id = ?1 AND generation = ?2 AND state IN ('running', 'cancelling')",
            params![id, generation, state, reason, verified, now.max(job.created_ms)])?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    fn verification_outcome(
        &self,
        job: &AnalysisJob,
        result: Result<VerificationReceipt>,
    ) -> (&'static str, Option<&'static str>, Option<u64>) {
        match result {
            Ok(receipt)
                if receipt.bytes != job.expected_bytes
                    || receipt.files != job.expected_files
                    || receipt.manifest_sha256 != job.manifest_sha256 =>
            {
                ("failed", Some("invalid-worker-result"), None)
            }
            Ok(_)
                if self
                    .published_analysis(&job.analysis_id, job.analysis_revision)
                    .is_err() =>
            {
                ("failed", Some("input-no-longer-current"), None)
            }
            Ok(receipt) => ("verified", None, Some(receipt.bytes)),
            Err(Error::Analysis("cancelled")) => ("cancelled", Some("cancelled"), None),
            Err(Error::Analysis(reason)) => ("failed", Some(reason), None),
            Err(_) => ("failed", Some("input-verification-failed"), None),
        }
    }

    /// The stored kind of one analysis job, if it exists.
    /// # Errors
    /// Refuses a malformed ID.
    pub fn analysis_job_kind(&self, id: &str) -> Result<Option<String>> {
        validate_key(id, "analysis job ID")?;
        Ok(self
            .connection
            .query_row(
                "SELECT kind FROM analysis_jobs WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// End every verification and recognition lease an earlier service process held.
    /// A native recognizer runs in a contained group owned by that process; its
    /// kill-on-close handle ended with it. Running zero-cost work is requeued under a new
    /// generation until its attempt limit; cancelling or exhausted work is interrupted.
    /// See decisions 0039 and 0043 for the platforms where that termination is enforced.
    pub(crate) fn recover_analysis_jobs(&mut self) -> Result<()> {
        let now = super::now_ms()?;
        job_pool::recover(&mut self.connection, Family::Analysis, now)
    }

    pub(crate) fn audit_analysis_jobs(&self) -> Result<()> {
        audit(&self.connection)
    }
}

pub(super) fn audit(connection: &rusqlite::Connection) -> Result<()> {
    let invalid: bool = connection.query_row(
        "SELECT (SELECT count(*) FROM analysis_jobs WHERE state IN ('queued', 'running', 'cancelling')) > 1024 OR EXISTS(SELECT 1 FROM analysis_jobs j JOIN analysis_inputs a ON a.id = j.analysis_id AND a.revision = j.analysis_revision WHERE a.state != 'published' OR a.recording_id != j.recording_id)", [], |row| row.get(0))?;
    if invalid {
        return Err(Error::CatalogIntegrity);
    }
    let mut statement =
        connection.prepare("SELECT id, profile, reason, kind FROM analysis_jobs")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, profile, reason, kind) = row?;
        validate_key(&id, "analysis job ID").map_err(|_| Error::CatalogIntegrity)?;
        validate_key(&profile, "analysis profile ID").map_err(|_| Error::CatalogIntegrity)?;
        if kind == "verify" {
            read_audited_job(connection, &id)
                .map_err(|_| Error::CatalogIntegrity)?
                .ok_or(Error::CatalogIntegrity)?;
        }
        if let Some(reason) = reason {
            validate_key(&reason, "analysis failure reason")
                .map_err(|_| Error::CatalogIntegrity)?;
        }
    }
    Ok(())
}

fn read_verification_job(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<AnalysisJob>> {
    connection.query_row(
        "SELECT id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, verified_bytes, reason, created_ms, finished_ms, manifest_sha256, attempt, started_ms FROM analysis_jobs WHERE id = ?1",
        [id], read_job).optional().map_err(Error::from)
}

/// Reads only the columns every catalog since v26 has, so migrations can audit rows
/// before the job pool columns exist.
fn read_audited_job(connection: &rusqlite::Connection, id: &str) -> Result<Option<AnalysisJob>> {
    connection.query_row(
        "SELECT id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, verified_bytes, reason, created_ms, finished_ms, manifest_sha256 FROM analysis_jobs WHERE id = ?1",
        [id], read_job).optional().map_err(Error::from)
}

fn read_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnalysisJob> {
    let pooled = row.as_ref().column_count() > 14;
    Ok(AnalysisJob {
        id: row.get(0)?,
        generation: row.get(1)?,
        analysis_id: row.get(2)?,
        analysis_revision: row.get(3)?,
        recording_id: row.get(4)?,
        profile: row.get(5)?,
        state: row.get(6)?,
        expected_bytes: row.get::<_, u32>(7)?.into(),
        expected_files: usize::from(row.get::<_, u16>(8)?),
        manifest_sha256: row.get(13)?,
        verified_bytes: row.get::<_, Option<u32>>(9)?.map(u64::from),
        reason: row.get(10)?,
        amount_usd: "0.000000".into(),
        created_ms: row.get(11)?,
        finished_ms: row.get(12)?,
        attempt: if pooled { row.get(14)? } else { 1 },
        started_ms: if pooled { row.get(15)? } else { None },
    })
}

/// The lease owner test shims claim with.
#[cfg(test)]
pub(crate) const TEST_OWNER: &str = "local-test";

#[cfg(test)]
mod tests;
