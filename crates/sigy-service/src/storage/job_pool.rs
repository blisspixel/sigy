//! The durable local job pool shared by verification, recognition and translation.
//!
//! Admission enqueues a job. The service scheduler claims the oldest queued job of a kind
//! whose transcript lineage has no active job, up to a per-kind cap, and records a lease
//! owned by this service process. A restart ends every lease: running zero-cost local
//! work returns to the queue under a new generation until its attempt limit, while
//! cancelling or exhausted work becomes interrupted. Every ended attempt is recorded.
//! A queued job holds no read lease; its input is checked again when it is claimed.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::{Store, widen};
use crate::{Error, Result};

/// Queued plus active jobs allowed in one job table. Terminal history is not bounded.
pub const MAX_OPEN_JOBS: i64 = 1024;
/// Attempts one job may start before a restart leaves it interrupted.
pub const MAX_ATTEMPTS: u32 = 3;
/// Added to a task's own deadlines to form the recorded lease expiry.
pub(crate) const LEASE_MARGIN_MS: i64 = 600_000;

/// Native processes are requeued only where the platform ends a dead service's contained
/// groups. Windows Job Objects do this with kill-on-close; that is not yet tested on Linux
/// or macOS, so there a restart still interrupts native work.
pub(crate) const NATIVE_REQUEUE: bool = cfg!(windows);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum JobKind {
    Verification,
    Recognition,
    Translation,
}

impl JobKind {
    pub(crate) const ALL: [Self; 3] = [Self::Verification, Self::Recognition, Self::Translation];

    fn queued_sql(self) -> &'static str {
        match self {
            Self::Verification => {
                "SELECT j.id FROM analysis_jobs j WHERE j.kind = 'verify' AND j.state = 'queued' AND NOT EXISTS (SELECT 1 FROM analysis_jobs r WHERE r.lineage = j.lineage AND r.state IN ('running', 'cancelling')) ORDER BY j.created_ms, j.rowid LIMIT 1"
            }
            Self::Recognition => {
                "SELECT j.id FROM analysis_jobs j WHERE j.kind = 'local_asr' AND j.state = 'queued' AND NOT EXISTS (SELECT 1 FROM analysis_jobs r WHERE r.lineage = j.lineage AND r.state IN ('running', 'cancelling')) ORDER BY j.created_ms, j.rowid LIMIT 1"
            }
            Self::Translation => {
                "SELECT j.id FROM translation_jobs j WHERE j.state = 'queued' AND NOT EXISTS (SELECT 1 FROM translation_jobs r WHERE r.lineage = j.lineage AND r.state IN ('running', 'cancelling')) ORDER BY j.created_ms, j.rowid LIMIT 1"
            }
        }
    }
}

/// Concurrent jobs of each kind. The defaults fit one small host: one of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PoolCaps {
    pub verification: usize,
    pub recognition: usize,
    pub translation: usize,
}

impl Default for PoolCaps {
    fn default() -> Self {
        Self {
            verification: 1,
            recognition: 1,
            translation: 1,
        }
    }
}

impl PoolCaps {
    pub(crate) fn cap(self, kind: JobKind) -> usize {
        match kind {
            JobKind::Verification => self.verification,
            JobKind::Recognition => self.recognition,
            JobKind::Translation => self.translation,
        }
    }
}

/// The lease owner recorded for jobs this service process claims.
pub(crate) fn lease_owner(started_ms: i64) -> String {
    format!("local-{}-{started_ms}", std::process::id())
}

/// Which table and family a claim or recovery touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Analysis,
    Translation,
}

impl Family {
    fn table(self) -> &'static str {
        match self {
            Self::Analysis => "analysis_jobs",
            Self::Translation => "translation_jobs",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Analysis => "analysis",
            Self::Translation => "translation",
        }
    }
}

/// Refuse a new job when the table already holds the maximum open work.
pub(super) fn check_open_bound(connection: &Connection, family: Family) -> Result<()> {
    let open: i64 = connection.query_row(
        &format!(
            "SELECT count(*) FROM {} WHERE state IN ('queued', 'running', 'cancelling')",
            family.table()
        ),
        [],
        |row| row.get(0),
    )?;
    if open >= MAX_OPEN_JOBS {
        return Err(Error::Analysis("queue-full"));
    }
    Ok(())
}

/// Move one queued job to running under this owner. False when it is no longer queued
/// or its lineage became active.
pub(super) fn claim_row(
    tx: &Transaction<'_>,
    family: Family,
    id: &str,
    owner: &str,
    lease_until: i64,
    now: i64,
) -> Result<bool> {
    let table = family.table();
    let changed = tx.execute(
        &format!(
            "UPDATE {table} SET state = 'running', lease_owner = ?2, lease_expires_ms = ?3, started_ms = ?4 WHERE id = ?1 AND state = 'queued' AND NOT EXISTS (SELECT 1 FROM {table} r WHERE r.lineage = {table}.lineage AND r.state IN ('running', 'cancelling'))"
        ),
        params![id, owner, lease_until.max(now), now.max(0)],
    )?;
    Ok(changed == 1)
}

/// End a queued job that can no longer run, without a worker.
pub(super) fn end_queued(
    connection: &Connection,
    family: Family,
    id: &str,
    state: &str,
    reason: &str,
    now: i64,
) -> Result<bool> {
    let changed = connection.execute(
        &format!(
            "UPDATE {} SET state = ?2, reason = ?3, finished_ms = max(created_ms, ?4) WHERE id = ?1 AND state = 'queued'",
            family.table()
        ),
        params![id, state, reason, now],
    )?;
    Ok(changed == 1)
}

struct Held {
    id: String,
    state: String,
    attempt: u32,
    generation: u32,
    owner: String,
    started_ms: i64,
    native: bool,
    zero_cost: bool,
}

/// End every lease held by an earlier service process, in one transaction.
pub(super) fn recover(connection: &mut Connection, family: Family, now: i64) -> Result<()> {
    let table = family.table();
    let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let held = {
        let (native, cost) = match family {
            Family::Analysis => ("kind = 'local_asr'", "amount_micros = 0"),
            Family::Translation => ("1", "1"),
        };
        let mut statement = tx.prepare(&format!(
            "SELECT id, state, attempt, generation, lease_owner, started_ms, {native}, {cost} FROM {table} WHERE state IN ('running', 'cancelling') ORDER BY created_ms, rowid"
        ))?;
        statement
            .query_map([], |row| {
                Ok(Held {
                    id: row.get(0)?,
                    state: row.get(1)?,
                    attempt: row.get(2)?,
                    generation: row.get(3)?,
                    owner: row.get(4)?,
                    started_ms: row.get(5)?,
                    native: row.get(6)?,
                    zero_cost: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for job in held {
        tx.execute(
            "INSERT INTO job_attempts(family, job_id, attempt, generation, lease_owner, started_ms, ended_ms, outcome, reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, max(?6, ?7), 'interrupted', 'service-restarted')",
            params![family.name(), job.id, job.attempt, job.generation, job.owner, job.started_ms, now],
        )?;
        let requeue = job.state == "running"
            && job.zero_cost
            && job.attempt < MAX_ATTEMPTS
            && (NATIVE_REQUEUE || !job.native);
        let changed = if requeue {
            tx.execute(
                &format!(
                    "UPDATE {table} SET state = 'queued', generation = generation + 1, attempt = attempt + 1, lease_owner = NULL, lease_expires_ms = NULL, started_ms = NULL WHERE id = ?1 AND state = 'running'"
                ),
                [&job.id],
            )?
        } else {
            tx.execute(
                &format!(
                    "UPDATE {table} SET state = 'interrupted', generation = generation + 1, reason = 'service-restarted', finished_ms = max(created_ms, ?2) WHERE id = ?1 AND state IN ('running', 'cancelling')"
                ),
                params![job.id, now],
            )?
        };
        if changed != 1 {
            return Err(Error::StorageIntegrity);
        }
    }
    tx.commit()?;
    Ok(())
}

impl Store {
    /// The oldest queued job of this kind whose lineage has no active job.
    /// # Errors
    /// Returns catalog read errors.
    pub(crate) fn next_queued(&self, kind: JobKind) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(kind.queued_sql(), [], |row| row.get(0))
            .optional()?)
    }

    /// Recorded attempts a restart ended for one job, oldest first.
    #[cfg(test)]
    /// # Errors
    /// Returns catalog read errors.
    pub(crate) fn ended_attempts(&self, family: &str, id: &str) -> Result<Vec<(u32, u32)>> {
        let mut statement = self.connection.prepare(
            "SELECT attempt, generation FROM job_attempts WHERE family = ?1 AND job_id = ?2 ORDER BY attempt",
        )?;
        Ok(statement
            .query_map(params![family, id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// Text replacements that rebuild `analysis_jobs` from its v30 definition.
const ANALYSIS_031: [(&str, &str); 3] = [
    (
        "state TEXT NOT NULL CHECK(state IN ('running', 'cancelling', 'verified', 'succeeded', 'cancelled', 'failed', 'interrupted')),",
        "state TEXT NOT NULL CHECK(state IN ('queued', 'running', 'cancelling', 'verified', 'succeeded', 'cancelled', 'failed', 'interrupted')),",
    ),
    (
        "    finished_ms INTEGER CHECK(finished_ms >= created_ms),\n",
        "    finished_ms INTEGER CHECK(finished_ms >= created_ms),\n    lineage TEXT NOT NULL CHECK(lineage = analysis_id),\n    attempt INTEGER NOT NULL DEFAULT 1 CHECK(attempt BETWEEN 1 AND 8),\n    lease_owner TEXT CHECK(length(lease_owner) BETWEEN 1 AND 128),\n    lease_expires_ms INTEGER CHECK(lease_expires_ms >= 0),\n    started_ms INTEGER CHECK(started_ms >= 0),\n",
    ),
    (
        "    CHECK((state IN ('running', 'cancelling')) = (finished_ms IS NULL)),\n",
        "    CHECK((state IN ('queued', 'running', 'cancelling')) = (finished_ms IS NULL)),\n    CHECK((lease_owner IS NULL) = (lease_expires_ms IS NULL) AND (lease_owner IS NULL) = (started_ms IS NULL)),\n    CHECK(state != 'queued' OR lease_owner IS NULL),\n    CHECK(state NOT IN ('running', 'cancelling') OR lease_owner IS NOT NULL),\n",
    ),
];

/// Text replacements that rebuild `translation_jobs` from its v30 definition.
const TRANSLATION_031: [(&str, &str); 3] = [
    (
        "state TEXT NOT NULL CHECK(state IN ('running', 'cancelling', 'succeeded', 'cancelled', 'failed', 'interrupted')),",
        "state TEXT NOT NULL CHECK(state IN ('queued', 'running', 'cancelling', 'succeeded', 'cancelled', 'failed', 'interrupted')),",
    ),
    (
        "    finished_ms INTEGER CHECK(finished_ms >= created_ms),\n",
        "    finished_ms INTEGER CHECK(finished_ms >= created_ms),\n    lineage TEXT NOT NULL CHECK(lineage = transcript_id),\n    attempt INTEGER NOT NULL DEFAULT 1 CHECK(attempt BETWEEN 1 AND 8),\n    lease_owner TEXT CHECK(length(lease_owner) BETWEEN 1 AND 128),\n    lease_expires_ms INTEGER CHECK(lease_expires_ms >= 0),\n    started_ms INTEGER CHECK(started_ms >= 0),\n",
    ),
    (
        "    CHECK((state IN ('running', 'cancelling')) = (finished_ms IS NULL)),\n",
        "    CHECK((state IN ('queued', 'running', 'cancelling')) = (finished_ms IS NULL)),\n    CHECK((lease_owner IS NULL) = (lease_expires_ms IS NULL) AND (lease_owner IS NULL) = (started_ms IS NULL)),\n    CHECK(state != 'queued' OR lease_owner IS NULL),\n    CHECK(state NOT IN ('running', 'cancelling') OR lease_owner IS NOT NULL),\n",
    ),
];

/// Values for the new columns of rows written before v31. Active rows get a lease owned
/// by the earlier catalog so the next service start recovers them like any other.
const ADDED_ANALYSIS: [(&str, &str); 5] = [
    ("lineage", "analysis_id"),
    ("attempt", "1"),
    (
        "lease_owner",
        "CASE WHEN state IN ('running', 'cancelling') THEN 'catalog-v30' END",
    ),
    (
        "lease_expires_ms",
        "CASE WHEN state IN ('running', 'cancelling') THEN created_ms END",
    ),
    (
        "started_ms",
        "CASE WHEN state IN ('running', 'cancelling') THEN created_ms END",
    ),
];
const ADDED_TRANSLATION: [(&str, &str); 5] = [
    ("lineage", "transcript_id"),
    ("attempt", "1"),
    (
        "lease_owner",
        "CASE WHEN state IN ('running', 'cancelling') THEN 'catalog-v30' END",
    ),
    (
        "lease_expires_ms",
        "CASE WHEN state IN ('running', 'cancelling') THEN created_ms END",
    ),
    (
        "started_ms",
        "CASE WHEN state IN ('running', 'cancelling') THEN created_ms END",
    ),
];

/// Migration 031: the durable job pool.
/// # Errors
/// Fails closed when a stored definition differs from the expected v30 text.
pub(super) fn migrate_031(tx: &Transaction<'_>) -> Result<()> {
    // Parent rows are dropped before children point at the rebuilt table again.
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    widen::rebuild(tx, "analysis_jobs", &ANALYSIS_031, &ADDED_ANALYSIS)?;
    widen::rebuild(tx, "translation_jobs", &TRANSLATION_031, &ADDED_TRANSLATION)?;
    tx.execute_batch(include_str!("031-job-pool.sql"))?;
    super::analysis_jobs::audit(tx)?;
    super::transcripts::audit(tx)?;
    let violations: i64 =
        tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations != 0 {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}

/// The v30 indexes and triggers that migration 031 replaced, for test downgrades.
#[cfg(test)]
const V30_OBJECTS: &str = "
CREATE UNIQUE INDEX analysis_one_worker ON analysis_jobs((1)) WHERE state IN ('running', 'cancelling');
CREATE UNIQUE INDEX translation_one_worker ON translation_jobs((1)) WHERE state IN ('running', 'cancelling');
CREATE TRIGGER analysis_job_input
BEFORE INSERT ON analysis_jobs
WHEN NEW.state != 'running' OR NEW.generation != 1 OR NOT EXISTS (
    SELECT 1 FROM analysis_inputs a JOIN recordings r ON r.id = a.recording_id
    WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
      AND a.recording_id = NEW.recording_id AND a.state = 'published'
      AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
      AND r.storage_state = 'retained'
      AND r.sha256 = a.media_sha256 AND r.media_bytes = NEW.expected_bytes
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job input'); END;
CREATE TRIGGER analysis_job_request_immutable
BEFORE UPDATE OF id, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms ON analysis_jobs
BEGIN SELECT RAISE(ABORT, 'analysis request is immutable'); END;
CREATE TRIGGER analysis_job_transition
BEFORE UPDATE OF state, generation, reason, finished_ms, verified_bytes ON analysis_jobs
WHEN NOT (
    (OLD.state = 'running' AND NEW.state = 'cancelling' AND NEW.generation = OLD.generation)
    OR (OLD.state = 'running' AND NEW.state IN ('verified', 'succeeded', 'failed', 'cancelled') AND NEW.generation = OLD.generation)
    OR (OLD.state = 'cancelling' AND NEW.state = 'cancelled' AND NEW.generation = OLD.generation)
    OR (OLD.state IN ('running', 'cancelling') AND NEW.state = 'interrupted' AND NEW.generation = OLD.generation + 1)
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job transition'); END;
CREATE TRIGGER analysis_job_id_retained BEFORE INSERT ON analysis_jobs
WHEN EXISTS (SELECT 1 FROM analysis_jobs WHERE id = NEW.id) OR (SELECT count(*) FROM analysis_jobs) >= 256
BEGIN SELECT RAISE(ABORT, 'analysis job history is retained'); END;
CREATE TRIGGER translation_job_limit BEFORE INSERT ON translation_jobs
WHEN (SELECT count(*) FROM translation_jobs) >= 256 OR NEW.state != 'running' OR NEW.generation != 1
BEGIN SELECT RAISE(ABORT, 'translation job admission'); END;
PRAGMA user_version = 30;
";

/// Returns a v31 test catalog to the v30 definitions so older migration tests can replay
/// their own downgrades. Queued rows cannot be represented in v30 and must not exist.
/// # Errors
/// Fails when a replacement no longer matches or a row cannot be represented.
#[cfg(test)]
pub(crate) fn revert_031_for_tests(connection: &mut Connection) -> Result<()> {
    fn reverse(pairs: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        pairs.iter().map(|(old, new)| (*new, *old)).collect()
    }
    let tx = connection.transaction()?;
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    tx.execute_batch(
        "DROP TABLE job_attempts;
         DROP INDEX analysis_one_per_lineage; DROP INDEX translation_one_per_lineage;
         DROP INDEX analysis_job_queue; DROP INDEX translation_job_queue;
         DROP TRIGGER analysis_job_input; DROP TRIGGER analysis_job_id_retained;
         DROP TRIGGER analysis_job_queue_bound; DROP TRIGGER analysis_job_request_immutable;
         DROP TRIGGER analysis_job_transition; DROP TRIGGER translation_job_admission;
         DROP TRIGGER translation_job_request_immutable; DROP TRIGGER translation_job_transition;",
    )?;
    widen::rebuild(&tx, "analysis_jobs", &reverse(&ANALYSIS_031), &[])?;
    widen::rebuild(&tx, "translation_jobs", &reverse(&TRANSLATION_031), &[])?;
    tx.execute_batch(V30_OBJECTS)?;
    tx.commit()?;
    Ok(())
}
