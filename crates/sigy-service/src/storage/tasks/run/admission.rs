//! User-selected delegation and cancellation. Text and checkpoints grant no authority.

use crate::storage::query_work::{Limits, QueryWork};
use rusqlite::{Transaction, TransactionBehavior, params};

use super::{
    Error, Event, OptionalExtension, Result, Store, TaskRunSelection, TaskRunSpec, TaskRunState,
    TaskRunStep, TaskRunView, TaskSnapshotRunSpec, effect_id, insert_event, validate_key,
};
mod start;

impl Store {
    /// Accept one finite materialization run. Exact request replay changes nothing.
    /// # Errors
    /// Refuses stale scope, missing checkpoint, paused policy, changed replay or clock.
    pub(crate) fn start_task_run(
        &mut self,
        id: &str,
        request_id: &str,
        spec: &TaskRunSpec,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskRunView> {
        self.start_selected_run(
            id,
            request_id,
            &TaskRunSelection::Checkpoint(spec.clone()),
            expected_generation,
            now,
        )
    }

    /// Accept one exact frozen snapshot without substituting monitor-wide coverage.
    /// # Errors
    /// Refuses changed replay, prior lifetime delegation, scope drift or resource bounds.
    pub(crate) fn start_task_snapshot_run(
        &mut self,
        id: &str,
        request_id: &str,
        spec: &TaskSnapshotRunSpec,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskRunView> {
        self.start_selected_run(
            id,
            request_id,
            &TaskRunSelection::Snapshot(spec.clone()),
            expected_generation,
            now,
        )
    }

    /// Append cancellation without stopping independently authorized capture or jobs.
    /// # Errors
    /// Refuses a changed replay, stale generation, terminal state or backward clock.
    pub(crate) fn cancel_task_run(
        &mut self,
        id: &str,
        request_id: &str,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskRunView> {
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let result = self.cancel_task_run_in_work(id, request_id, expected_generation, now, &work);
        work.finish()?;
        result
    }

    fn cancel_task_run_in_work(
        &self,
        id: &str,
        request_id: &str,
        expected_generation: u32,
        now: i64,
        work: &QueryWork<'_>,
    ) -> Result<TaskRunView> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        validate_key(id, "task ID")?;
        validate_key(request_id, "task cancellation request")?;
        let grant = self.checked_run_grant(id)?.ok_or(Error::NotFound)?;
        let view = self.task_run_with_grant(id, &grant)?;
        let replay: Option<u32> = self.connection.query_row(
            "SELECT expected_generation FROM task_run_events WHERE task_id = ?1 AND request_id = ?2",
            params![id, request_id], |row| row.get(0),
        ).optional()?;
        if let Some(expected) = replay {
            if expected != expected_generation {
                return Err(Error::IdempotencyConflict);
            }
            work.check()?;
            tx.commit()?;
            return Ok(view);
        }
        if view.generation != expected_generation {
            return Err(Error::IdempotencyConflict);
        }
        if view.state != TaskRunState::Running {
            return Err(Error::InvalidInput("task run is terminal"));
        }
        if now < view.updated_ms {
            return Err(Error::InvalidInput("task run clock"));
        }
        let event = Event {
            step: TaskRunStep {
                ordinal: expected_generation,
                kind: "cancelled".into(),
                effect_id: effect_id(&grant.sha256, "cancelled", expected_generation),
                citation_ordinal: None,
                finding_id: None,
                reason: Some("user-cancelled".into()),
                recorded_ms: now,
            },
            generation: expected_generation + 1,
            state: TaskRunState::Cancelled,
            request_id: Some(request_id.into()),
            expected_generation: Some(expected_generation),
        };
        insert_event(&tx, id, &event)?;
        let grant = self
            .checked_run_grant_reusing(id, Some(&grant))?
            .ok_or(Error::StorageIntegrity)?;
        let view = self.task_run_with_grant(id, &grant)?;
        work.check()?;
        tx.commit()?;
        Ok(view)
    }
}
