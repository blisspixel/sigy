//! Exact freeze-now observations, with replay before live reconciliation.

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::{Store, scope_current, validate_key};
use crate::{
    Error, Result,
    recognition::sha256_hex,
    storage::query_work::{Limits, QueryWork},
    task::{
        MAX_CHECKPOINTS,
        snapshot::{SNAPSHOT_MODE, SNAPSHOT_TEMPLATE, TaskEvidenceSnapshot},
    },
};

mod audit;
mod encoding;
mod observations;

#[cfg(test)]
pub(in crate::storage::tasks) use encoding::fit as fit_for_test;

fn snapshot_hash(json: &str) -> String {
    sha256_hex(format!("[\"sigy-task-evidence-snapshot-v1\",{json}]").as_bytes())
}

impl Store {
    /// Freeze the exact currently observable evidence. This grants no work authority.
    /// # Errors
    /// Refuses scope drift, changed replay, stale ordinal, clock or resource exhaustion.
    pub(crate) fn freeze_task_evidence(
        &mut self,
        id: &str,
        request_id: &str,
        expected_snapshot: u32,
        now: i64,
    ) -> Result<TaskEvidenceSnapshot> {
        validate_key(id, "task ID")?;
        validate_key(request_id, "task snapshot request")?;
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let result = self.freeze_evidence_in_work(id, request_id, expected_snapshot, now, &work);
        work.finish()?;
        result
    }

    fn freeze_evidence_in_work(
        &self,
        id: &str,
        request_id: &str,
        expected_snapshot: u32,
        now: i64,
        work: &QueryWork<'_>,
    ) -> Result<TaskEvidenceSnapshot> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let replay: Option<(u32, u32)> = tx.query_row(
            "SELECT ordinal, expected_snapshot FROM task_evidence_snapshots WHERE task_id = ?1 AND request_id = ?2",
            params![id, request_id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        if let Some((ordinal, expected)) = replay {
            if expected != expected_snapshot {
                return Err(Error::IdempotencyConflict);
            }
            let snapshot = self.checked_evidence_snapshot(id, ordinal)?;
            work.check()?;
            tx.commit()?;
            return Ok(snapshot);
        }
        let (scope, scope_sha256, monitor_spec_sha256, _) = self.checked_task_scope(id)?;
        if !scope_current(self, &scope)? {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        let (ordinal, count): (u32, u32) = tx.query_row(
            "SELECT coalesce((SELECT max(ordinal) FROM task_evidence_snapshots WHERE task_id = ?1), 0), (SELECT count(*) FROM task_checkpoints WHERE task_id = ?1) + (SELECT count(*) FROM task_evidence_snapshots WHERE task_id = ?1)",
            [id], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if ordinal != expected_snapshot {
            return Err(Error::IdempotencyConflict);
        }
        if count >= MAX_CHECKPOINTS {
            return Err(Error::InvalidInput("task observation capacity"));
        }
        if now
            < super::collection::latest_task_ms(&tx, id)?
                .max(observations::latest_work_ms(&tx, id)?)
        {
            return Err(Error::InvalidInput("task snapshot clock"));
        }
        let collection = self
            .task_collection(id)?
            .ok_or(Error::InvalidInput("task collection absent"))?;
        let processing = self.task_processing(id)?;
        let evidence = self
            .task_evidence_in_work(id, work)?
            .ok_or(Error::StorageIntegrity)?;
        let mut snapshot = TaskEvidenceSnapshot {
            task_id: id.into(),
            ordinal: ordinal + 1,
            request_id: request_id.into(),
            expected_snapshot,
            observed_ms: now,
            template: SNAPSHOT_TEMPLATE.into(),
            mode: SNAPSHOT_MODE.into(),
            scope,
            scope_sha256,
            monitor_spec_sha256,
            jobs: observations::jobs(&tx, processing.as_ref())?,
            media: observations::media(&tx, &collection)?,
            collection,
            processing,
            evidence,
        };
        let json = encoding::fit(&mut snapshot)?;
        work.check()?;
        tx.execute(
            "INSERT INTO task_evidence_snapshots(task_id, ordinal, request_id, expected_snapshot, observed_ms, scope_sha256, collection_sha256, processing_sha256, template, mode, payload_json, payload_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![id, snapshot.ordinal, request_id, expected_snapshot, now, snapshot.scope_sha256, snapshot.collection.grant_sha256, snapshot.processing.as_ref().map(|p| &p.grant_sha256), SNAPSHOT_TEMPLATE, SNAPSHOT_MODE, json, snapshot_hash(&json)],
        )?;
        work.check()?;
        tx.commit()?;
        Ok(snapshot)
    }

    /// Read one exact frozen observation, including after policy and media changes.
    /// # Errors
    /// Refuses missing ordinals, inconsistent stored lineage or exhausted query work.
    pub fn task_evidence_snapshot(&self, id: &str, ordinal: u32) -> Result<TaskEvidenceSnapshot> {
        validate_key(id, "task ID")?;
        if !(1..=MAX_CHECKPOINTS).contains(&ordinal) {
            return Err(Error::InvalidInput("task snapshot ordinal"));
        }
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let result = self
            .checked_evidence_snapshot(id, ordinal)
            .and_then(|snapshot| {
                work.check()?;
                Ok(snapshot)
            });
        work.finish()?;
        result
    }
}

#[cfg(test)]
pub(in crate::storage) fn remove_snapshot_schema(store: &Store) -> Result<()> {
    store.connection.execute_batch(
        "DROP TRIGGER task_checkpoint_shared_capacity;
         DROP TRIGGER task_capture_snapshot_clock;
         DROP TRIGGER task_collection_cancel_snapshot_clock;
         DROP TRIGGER task_processing_snapshot_clock;
         DROP TRIGGER task_processing_step_snapshot_clock;
         DROP TRIGGER task_processing_cancel_snapshot_clock;
         DROP TRIGGER task_withdrawal_snapshot_clock;
         DROP TRIGGER task_run_snapshot_clock;
         DROP TRIGGER task_run_event_snapshot_clock;
         DROP TABLE task_evidence_snapshots;",
    )?;
    Ok(())
}
