//! Durable capture intent and generation-fenced lifecycle. No media is dispatched here.

mod journal;

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sigy_core::capture::{CaptureEvent, CaptureState};

use super::{Store, now_ms, validate_key};
use crate::{Error, Result};

pub use journal::CaptureRecord;

/// Conservative admission ceilings, not measured simultaneous capture capacity.
pub const MAX_PENDING_CAPTURES: u32 = 256;
pub const MAX_ACTIVE_CAPTURES: u32 = 2;
pub const MAX_CAPTURE_PAGE: u32 = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturePlan {
    source_revision: String,
    starts_ms: i64,
    ends_ms: i64,
    maximum_bytes: i64,
}

impl CapturePlan {
    /// `source_revision` names an immutable adapter configuration, never a URL or secret.
    /// Resolving and authorizing that configuration precedes worker dispatch.
    /// # Errors
    /// Rejects malformed keys, empty/unbounded windows, or nonpositive byte limits.
    pub fn new(
        source_revision: &str,
        starts_ms: i64,
        ends_ms: i64,
        maximum_bytes: i64,
    ) -> Result<Self> {
        validate_key(source_revision, "source revision")?;
        if starts_ms < 0 || ends_ms <= starts_ms || maximum_bytes <= 0 {
            return Err(Error::InvalidInput("finite capture window and byte limit"));
        }
        Ok(Self {
            source_revision: source_revision.into(),
            starts_ms,
            ends_ms,
            maximum_bytes,
        })
    }

    #[must_use]
    pub fn source_revision(&self) -> &str {
        &self.source_revision
    }
    #[must_use]
    pub const fn starts_ms(&self) -> i64 {
        self.starts_ms
    }
    #[must_use]
    pub const fn ends_ms(&self) -> i64 {
        self.ends_ms
    }
    #[must_use]
    pub const fn maximum_bytes(&self) -> i64 {
        self.maximum_bytes
    }
}

/// Compare-and-swap token. Reading a token does not grant permission to dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureVersion {
    id: String,
    revision: i64,
    generation: i64,
}

impl CaptureVersion {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    #[must_use]
    pub const fn revision(&self) -> i64 {
        self.revision
    }
    #[must_use]
    pub const fn generation(&self) -> i64 {
        self.generation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureJob {
    pub version: CaptureVersion,
    pub plan: CapturePlan,
    pub state: CaptureState,
    pub created_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureAdmission {
    pub job: CaptureJob,
    pub newly_created: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureCounts {
    pub scheduled: u64,
    pub active: u64,
    pub interrupted: u64,
    pub terminal: u64,
}

impl Store {
    /// Accepts finite capture intent once. A replay cannot dispatch or restart work.
    /// # Errors
    /// Rejects key reuse, a full pending queue, or a failed atomic journal write.
    pub fn create_capture(&mut self, id: &str, plan: &CapturePlan) -> Result<CaptureAdmission> {
        validate_key(id, "capture ID")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(job) = journal::read_job(&tx, id)? {
            if job.plan != *plan {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(CaptureAdmission {
                job,
                newly_created: false,
            });
        }
        let pending: u32 = tx.query_row("SELECT count(*) FROM capture_jobs WHERE state NOT IN ('completed', 'cancelled', 'failed')", [], |row| row.get(0))?;
        if pending >= MAX_PENDING_CAPTURES {
            return Err(Error::CaptureCapacity);
        }
        let recorded = now_ms()?;
        tx.execute("INSERT INTO capture_jobs(id, source_revision, starts_ms, ends_ms, maximum_bytes, state, revision, generation, created_ms, updated_ms) VALUES (?1, ?2, ?3, ?4, ?5, 'scheduled', 0, 0, ?6, ?6)", params![id, plan.source_revision, plan.starts_ms, plan.ends_ms, plan.maximum_bytes, recorded])?;
        tx.execute("INSERT INTO capture_events(job_id, revision, generation, state, reason, recorded_ms) VALUES (?1, 0, 0, 'scheduled', 'accepted', ?2)", params![id, recorded])?;
        let job = journal::read_job(&tx, id)?.ok_or(Error::CaptureIntegrity)?;
        tx.commit()?;
        Ok(CaptureAdmission {
            job,
            newly_created: true,
        })
    }

    /// # Errors
    /// Rejects malformed IDs, invalid persisted values, or catalog failures.
    pub fn capture(&self, id: &str) -> Result<Option<CaptureJob>> {
        validate_key(id, "capture ID")?;
        journal::read_job(&self.connection, id)
    }

    /// Keyset pagination keeps reads finite as the historical library grows.
    /// # Errors
    /// Rejects malformed cursors and page sizes outside 1..=64.
    pub fn captures(&self, after_id: Option<&str>, limit: u32) -> Result<Vec<CaptureJob>> {
        validate_page(limit)?;
        if let Some(id) = after_id {
            validate_key(id, "capture cursor")?;
        }
        let mut query = self
            .connection
            .prepare("SELECT id FROM capture_jobs WHERE id > ?1 ORDER BY id LIMIT ?2")?;
        let ids = query
            .query_map(params![after_id.unwrap_or(""), limit], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.capture(&id)?.ok_or(Error::CaptureIntegrity))
            .collect()
    }

    /// Records a lifecycle transition only against the exact observed revision.
    /// Worker acknowledgments must carry the token from their admitted attempt.
    /// # Errors
    /// Rejects stale workers, invalid transitions, missed windows, excess active
    /// work and finalization without a future verified-media publication operation.
    pub fn transition_capture(
        &mut self,
        expected: &CaptureVersion,
        event: CaptureEvent,
        reason: &str,
    ) -> Result<CaptureJob> {
        validate_key(reason, "capture transition reason")?;
        if event == CaptureEvent::Finalized {
            return Err(Error::InvalidInput(
                "capture finalization requires verified media",
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = journal::read_job(&tx, &expected.id)?.ok_or(Error::NotFound)?;
        if job.version != *expected {
            return Err(Error::StaleCapture);
        }
        let recorded = now_ms()?;
        if event == CaptureEvent::Start {
            if recorded < job.plan.starts_ms || recorded >= job.plan.ends_ms {
                return Err(Error::InvalidInput(
                    "capture is outside its acquisition window",
                ));
            }
            if !job.state.is_active() {
                let active: u32 = tx.query_row("SELECT count(*) FROM capture_jobs WHERE state IN ('starting', 'running', 'retrying', 'stopping')", [], |row| row.get(0))?;
                if active >= MAX_ACTIVE_CAPTURES {
                    return Err(Error::CaptureCapacity);
                }
            }
        }
        let changed = journal::transition(&tx, job, event, reason, recorded)?;
        tx.commit()?;
        Ok(changed)
    }

    /// Marks abandoned active attempts interrupted and revokes their generations.
    /// Call only while holding exclusive library ownership and before starting workers.
    /// # Errors
    /// Returns catalog or journal errors. A failed batch makes no partial changes.
    pub fn recover_captures(&mut self) -> Result<usize> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = {
            let mut query = tx.prepare("SELECT id FROM capture_jobs WHERE state IN ('starting', 'running', 'retrying', 'stopping') ORDER BY id LIMIT ?1")?;
            query
                .query_map([MAX_CAPTURE_PAGE], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let recorded = now_ms()?;
        for id in &ids {
            let job = journal::read_job(&tx, id)?.ok_or(Error::CaptureIntegrity)?;
            journal::transition(&tx, job, CaptureEvent::Lost, "service_recovery", recorded)?;
        }
        tx.commit()?;
        Ok(ids.len())
    }

    /// # Errors
    /// Rejects missing jobs, invalid cursors, and malformed journal entries.
    pub fn capture_history(
        &self,
        id: &str,
        after_revision: Option<i64>,
        limit: u32,
    ) -> Result<Vec<CaptureRecord>> {
        validate_key(id, "capture ID")?;
        validate_page(limit)?;
        if after_revision.is_some_and(|revision| revision < 0) {
            return Err(Error::InvalidInput("capture history cursor"));
        }
        if self.capture(id)?.is_none() {
            return Err(Error::NotFound);
        }
        journal::history(&self.connection, id, after_revision.unwrap_or(-1), limit)
    }

    /// Checks journal continuity, transitions and projected state without loading
    /// the historical event set into memory.
    /// # Errors
    /// Returns an integrity error for a malformed or inconsistent journal.
    pub fn audit_captures(&self) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let mut query = tx.prepare("SELECT id FROM capture_jobs ORDER BY id")?;
        let ids = query.query_map([], |row| row.get::<_, String>(0))?;
        for id in ids {
            let job = journal::read_job(&tx, &id?)?.ok_or(Error::CaptureIntegrity)?;
            journal::audit(&tx, &job)?;
        }
        let orphan: Option<i64> = tx.query_row("SELECT 1 FROM capture_events WHERE NOT EXISTS (SELECT 1 FROM capture_jobs WHERE id = capture_events.job_id) LIMIT 1", [], |row| row.get(0)).optional()?;
        if orphan.is_some() {
            return Err(Error::CaptureIntegrity);
        }
        Ok(())
    }

    /// # Errors
    /// Returns catalog errors or unknown persisted lifecycle states.
    pub fn capture_counts(&self) -> Result<CaptureCounts> {
        let mut query = self
            .connection
            .prepare("SELECT state, count(*) FROM capture_jobs GROUP BY state")?;
        let mut rows = query.query([])?;
        let mut counts = CaptureCounts::default();
        while let Some(row) = rows.next()? {
            let state: CaptureState = row
                .get::<_, String>(0)?
                .parse()
                .map_err(|_| Error::CaptureIntegrity)?;
            let count =
                u64::try_from(row.get::<_, i64>(1)?).map_err(|_| Error::CaptureIntegrity)?;
            let target = if state.is_active() {
                &mut counts.active
            } else if state.is_terminal() {
                &mut counts.terminal
            } else if state == CaptureState::Scheduled {
                &mut counts.scheduled
            } else {
                &mut counts.interrupted
            };
            *target = target.checked_add(count).ok_or(Error::CaptureIntegrity)?;
        }
        Ok(counts)
    }
}

fn validate_page(limit: u32) -> Result<()> {
    if !(1..=MAX_CAPTURE_PAGE).contains(&limit) {
        return Err(Error::InvalidInput("capture page size"));
    }
    Ok(())
}
