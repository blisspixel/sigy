//! Durable interests in canonical recognition and translation jobs.
//!
//! One shared job may be wanted by a direct request, by monitors and by tasks. Each
//! authority records its own interest with its own receipt, in the transaction that admits
//! or attaches the job. An interest is history, not a cancellation right over shared work.

use rusqlite::{Connection, params};

use super::Store;
use crate::{Error, Result, task::processing::JobSharing};

/// Which authority admitted or attached to a canonical job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Authority<'a> {
    Direct,
    Monitor(&'a str),
    Task(&'a str),
}

impl<'a> Authority<'a> {
    /// The stored authority kind and owner key. Direct interests have no owner key.
    fn parts(self) -> (&'static str, &'a str) {
        match self {
            Self::Direct => ("direct", ""),
            Self::Monitor(id) => ("monitor", id),
            Self::Task(id) => ("task", id),
        }
    }
}

/// Record one admitted interest inside the caller's admission transaction.
/// # Errors
/// Refuses a missing canonical job or receipt, or a duplicate interest.
pub(in crate::storage) fn record(
    connection: &Connection,
    family: &str,
    job_id: &str,
    authority: Authority<'_>,
    now: i64,
) -> Result<()> {
    let (kind, owner) = authority.parts();
    connection.execute(
        "INSERT INTO job_interests(family, job_id, authority, owner_id, origin, created_ms) VALUES (?1, ?2, ?3, ?4, 'admitted', ?5)",
        params![family, job_id, kind, owner, now],
    )?;
    Ok(())
}

/// Count every interest in one canonical job.
/// # Errors
/// Returns catalog read errors.
pub(in crate::storage) fn sharing(
    connection: &Connection,
    family: &str,
    job_id: &str,
) -> Result<JobSharing> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_interests WHERE family = ?1 AND job_id = ?2 AND authority = 'direct'), (SELECT count(*) FROM job_interests WHERE family = ?1 AND job_id = ?2 AND authority = 'monitor'), (SELECT count(*) FROM job_interests WHERE family = ?1 AND job_id = ?2 AND authority = 'task'), (SELECT count(*) FROM job_interest_withdrawals WHERE family = ?1 AND job_id = ?2)",
        params![family, job_id],
        |row| Ok(JobSharing { direct: row.get(0)?, monitors: row.get(1)?, tasks: row.get(2)?, withdrawn_tasks: row.get(3)? }),
    )?)
}

/// Each statement returns true when a stored interest or job breaks an invariant.
const AUDITS: [&str; 7] = [
    // An interest names an existing canonical job of its family.
    "SELECT EXISTS(SELECT 1 FROM job_interests i WHERE (i.family = 'recognition' AND NOT EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.id = i.job_id AND j.kind = 'local_asr')) OR (i.family = 'translation' AND NOT EXISTS (SELECT 1 FROM translation_jobs j WHERE j.id = i.job_id)))",
    // A monitor interest has its queued receipt from the same transaction.
    "SELECT EXISTS(SELECT 1 FROM job_interests i WHERE i.authority = 'monitor' AND NOT EXISTS (SELECT 1 FROM monitor_steps s WHERE s.monitor_id = i.owner_id AND s.stage = i.family AND s.decision = 'queued' AND s.job_id = i.job_id AND s.created_ms = i.created_ms))",
    // A task interest has its queued receipt from the same transaction.
    "SELECT EXISTS(SELECT 1 FROM job_interests i WHERE i.authority = 'task' AND NOT EXISTS (SELECT 1 FROM task_processing_steps s WHERE s.task_id = i.owner_id AND s.stage = i.family AND s.decision = 'queued' AND s.job_id = i.job_id AND s.created_ms = i.created_ms))",
    // Every queued monitor or task receipt carries its interest.
    "SELECT EXISTS(SELECT 1 FROM monitor_steps s WHERE s.decision = 'queued' AND NOT EXISTS (SELECT 1 FROM job_interests i WHERE i.family = s.stage AND i.job_id = s.job_id AND i.authority = 'monitor' AND i.owner_id = s.monitor_id)) OR EXISTS(SELECT 1 FROM task_processing_steps s WHERE s.decision = 'queued' AND NOT EXISTS (SELECT 1 FROM job_interests i WHERE i.family = s.stage AND i.job_id = s.job_id AND i.authority = 'task' AND i.owner_id = s.task_id))",
    // No canonical recognition or translation job exists without some authority.
    "SELECT EXISTS(SELECT 1 FROM analysis_jobs j WHERE j.kind = 'local_asr' AND NOT EXISTS (SELECT 1 FROM job_interests i WHERE i.family = 'recognition' AND i.job_id = j.id)) OR EXISTS(SELECT 1 FROM translation_jobs j WHERE NOT EXISTS (SELECT 1 FROM job_interests i WHERE i.family = 'translation' AND i.job_id = j.id))",
    // A direct interest is written with the job it created.
    "SELECT EXISTS(SELECT 1 FROM job_interests i WHERE i.authority = 'direct' AND i.created_ms IS NOT (CASE i.family WHEN 'recognition' THEN (SELECT created_ms FROM analysis_jobs WHERE id = i.job_id) ELSE (SELECT created_ms FROM translation_jobs WHERE id = i.job_id) END))",
    // Owner keys stay bounded identities; direct interests carry no owner.
    "SELECT EXISTS(SELECT 1 FROM job_interests WHERE (authority = 'direct') != (owner_id = '') OR (authority = 'task' AND origin != 'admitted'))",
];

impl Store {
    /// Reopen checks for interest provenance, beyond the insert-time triggers.
    /// # Errors
    /// Refuses orphaned jobs, receipts or interests.
    pub(crate) fn audit_job_interests(&self) -> Result<()> {
        for sql in AUDITS {
            let broken: bool = self.connection.query_row(sql, [], |row| row.get(0))?;
            if broken {
                return Err(Error::StorageIntegrity);
            }
        }
        Ok(())
    }
}
