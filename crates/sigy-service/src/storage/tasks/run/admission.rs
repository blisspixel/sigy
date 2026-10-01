//! User-selected delegation and cancellation. Text and checkpoints grant no authority.

use rusqlite::{TransactionBehavior, params};

use super::{
    Error, Event, Grant, OptionalExtension, RUN_TEMPLATE, Result, Store, TaskRunSpec, TaskRunState,
    TaskRunStep, TaskRunView, effect_id, grant_hash, insert_event, intents, partial_checkpoint,
    planned_count, scope_current, validate_key,
};

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
        validate_key(id, "task ID")?;
        validate_key(request_id, "task run request")?;
        spec.validate()?;
        if let Some(existing) = self.task_run(id)? {
            return if expected_generation == 0
                && existing.request_id == request_id
                && existing.spec == *spec
            {
                Ok(existing)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        if expected_generation != 0 {
            return Err(Error::IdempotencyConflict);
        }
        let task = self.task(id)?;
        if !scope_current(self, &task.spec)? {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        let checkpoint = self.task_checkpoint(id, spec.checkpoint_ordinal)?;
        if checkpoint.monitor_paused {
            return Err(Error::InvalidInput("task monitor paused"));
        }
        if now < checkpoint.observed_ms {
            return Err(Error::InvalidInput("task run clock"));
        }
        let checkpoint_sha: String = self.connection.query_row(
            "SELECT payload_sha256 FROM task_checkpoints WHERE task_id = ?1 AND ordinal = ?2",
            params![id, spec.checkpoint_ordinal],
            |row| row.get(0),
        )?;
        let json = serde_json::to_string(spec)?;
        let grant = Grant {
            request_id: request_id.into(),
            spec: spec.clone(),
            sha256: grant_hash(
                id,
                request_id,
                &json,
                &task.scope_sha256,
                &checkpoint_sha,
                now,
            )?,
            created_ms: now,
            planned_findings: planned_count(&checkpoint, spec)?,
            initial_partial: partial_checkpoint(&checkpoint)
                || checkpoint.citations.len() > spec.maximum_findings as usize,
            checkpoint,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO task_runs(task_id, request_id, spec_json, grant_sha256, scope_sha256, checkpoint_ordinal, checkpoint_sha256, maximum_findings, planned_findings, initial_partial, template, amount_micros, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12)",
            params![id, request_id, json, grant.sha256, task.scope_sha256, spec.checkpoint_ordinal, checkpoint_sha, spec.maximum_findings, grant.planned_findings, grant.initial_partial, RUN_TEMPLATE, now],
        )?;
        for intent in intents(&grant) {
            tx.execute(
                "INSERT INTO task_run_intents(task_id, ordinal, kind, effect_id, citation_ordinal) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, intent.ordinal, intent.kind, intent.effect_id, intent.citation_ordinal],
            )?;
        }
        tx.commit()?;
        self.task_run(id)?.ok_or(Error::StorageIntegrity)
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
        validate_key(id, "task ID")?;
        validate_key(request_id, "task cancellation request")?;
        let view = self.task_run(id)?.ok_or(Error::NotFound)?;
        let replay: Option<u32> = self.connection.query_row(
            "SELECT expected_generation FROM task_run_events WHERE task_id = ?1 AND request_id = ?2",
            params![id, request_id], |row| row.get(0),
        ).optional()?;
        if let Some(expected) = replay {
            return if expected == expected_generation {
                Ok(view)
            } else {
                Err(Error::IdempotencyConflict)
            };
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
        let grant = self.checked_run_grant(id)?.ok_or(Error::StorageIntegrity)?;
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
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_event(&tx, id, &event)?;
        tx.commit()?;
        self.task_run(id)?.ok_or(Error::StorageIntegrity)
    }
}
