//! Atomic admission of a typed frozen observation under one lifetime run slot.

use super::super::{
    Error, Grant, Observation, Result, Store, TaskRunSelection, TaskRunView, exact_grant_hash,
    grant_hash, intents, scope_current, validate_key,
};
use crate::storage::query_work::{Limits, QueryWork};
use rusqlite::{Transaction, TransactionBehavior, params};

impl Store {
    pub(super) fn start_selected_run(
        &self,
        id: &str,
        request: &str,
        spec: &TaskRunSelection,
        expected: u32,
        now: i64,
    ) -> Result<TaskRunView> {
        validate_key(id, "task ID")?;
        validate_key(request, "task run request")?;
        spec.validate()?;
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let result = self.admit_selected_run(id, request, spec, expected, now, &work);
        work.finish()?;
        result
    }

    fn admit_selected_run(
        &self,
        id: &str,
        request: &str,
        spec: &TaskRunSelection,
        expected: u32,
        now: i64,
        work: &QueryWork<'_>,
    ) -> Result<TaskRunView> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        if let Some(existing) = self.task_run_in_work(id)? {
            if expected != 0 || existing.request_id != request || existing.spec != *spec {
                return Err(Error::IdempotencyConflict);
            }
            work.check()?;
            tx.commit()?;
            return Ok(existing);
        }
        if expected != 0 {
            return Err(Error::IdempotencyConflict);
        }
        let (scope, scope_sha, _, _) = self.checked_task_scope(id)?;
        if !scope_current(self, &scope)? {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        let paused:bool=tx.query_row("SELECT coalesce((SELECT kind = 'pause' FROM monitor_actions WHERE monitor_id = ?1 AND decision = 'applied' AND kind IN ('pause','resume') ORDER BY ordinal DESC LIMIT 1),0)",[&scope.monitor_id],|r|r.get(0))?;
        if paused {
            return Err(Error::InvalidInput("task monitor paused"));
        }
        let observation = match spec {
            TaskRunSelection::Checkpoint(s) => {
                Observation::Checkpoint(Box::new(self.task_checkpoint(id, s.checkpoint_ordinal)?))
            }
            TaskRunSelection::Snapshot(s) => Observation::Snapshot(Box::new(
                self.checked_evidence_snapshot(id, s.snapshot_ordinal)?,
            )),
        };
        if matches!(&observation,Observation::Checkpoint(c) if c.monitor_paused) {
            return Err(Error::InvalidInput("task monitor paused"));
        }
        if now < observation.observed_ms()
            || now < crate::storage::tasks::collection::latest_task_ms(&tx, id)?
        {
            return Err(Error::InvalidInput("task run clock"));
        }
        let digest:String=tx.query_row(match spec {
            TaskRunSelection::Checkpoint(_)=>"SELECT payload_sha256 FROM task_checkpoints WHERE task_id=?1 AND ordinal=?2",
            TaskRunSelection::Snapshot(_)=>"SELECT payload_sha256 FROM task_evidence_snapshots WHERE task_id=?1 AND ordinal=?2",
        },params![id,spec.ordinal()],|row|row.get(0))?;
        let json = serde_json::to_string(spec)?;
        let sha256 = match spec {
            TaskRunSelection::Checkpoint(_) => {
                grant_hash(id, request, &json, &scope_sha, &digest, now)?
            }
            TaskRunSelection::Snapshot(_) => {
                exact_grant_hash(id, request, &json, &scope_sha, &digest, now)?
            }
        };
        let grant = Grant {
            request_id: request.into(),
            spec: spec.clone(),
            sha256,
            observation_sha256: digest.clone(),
            created_ms: now,
            planned_findings: u32::try_from(observation.citations().len())
                .map_err(|_| Error::StorageIntegrity)?
                .min(spec.maximum_findings()),
            initial_partial: observation.partial(spec.maximum_findings()),
            observation,
        };
        let (checkpoint, snapshot, checkpoint_digest) = match spec {
            TaskRunSelection::Checkpoint(s) => (Some(s.checkpoint_ordinal), None, Some(&digest)),
            TaskRunSelection::Snapshot(s) => (None, Some(s.snapshot_ordinal), None),
        };
        tx.execute("INSERT INTO task_runs(task_id,request_id,spec_json,grant_sha256,scope_sha256,checkpoint_ordinal,checkpoint_sha256,snapshot_ordinal,origin,observation_sha256,maximum_findings,planned_findings,initial_partial,template,amount_micros,created_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,0,?15)",params![id,request,json,grant.sha256,scope_sha,checkpoint,checkpoint_digest,snapshot,spec.origin(),digest,spec.maximum_findings(),grant.planned_findings,grant.initial_partial,spec.template(),now])?;
        for intent in intents(&grant) {
            tx.execute("INSERT INTO task_run_intents(task_id,ordinal,kind,effect_id,citation_ordinal) VALUES (?1,?2,?3,?4,?5)",params![id,intent.ordinal,intent.kind,intent.effect_id,intent.citation_ordinal])?;
        }
        let grant = self
            .checked_run_grant_reusing(id, Some(&grant))?
            .ok_or(Error::StorageIntegrity)?;
        let view = self.task_run_with_grant(id, &grant)?;
        work.check()?;
        tx.commit()?;
        Ok(view)
    }
}
