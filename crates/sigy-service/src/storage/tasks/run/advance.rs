//! One bounded step per pass. Catalog effect and receipt share the same transaction.

use rusqlite::TransactionBehavior;

use super::{
    Connection, Error, Event, Grant, Intent, Result, Store, TaskRunState, TaskRunStep, TaskRunView,
    artifact_exists, effect_id, insert_event, intents, scope_current,
};
use crate::monitor::FindingOriginal;

fn expected_refusal(error: Error) -> Result<String> {
    match error {
        Error::Analysis(reason) if super::audit::allowed_refusal(Some(reason)) => Ok(reason.into()),
        Error::InvalidInput(_) => Ok("invalid-evidence".into()),
        Error::NotFound => Ok("missing-evidence".into()),
        Error::IdempotencyConflict => Ok("evidence-conflict".into()),
        error => Err(error),
    }
}

fn finding_event(
    connection: &Connection,
    monitor: &str,
    grant: &Grant,
    intent: &Intent,
    now: i64,
) -> Result<Event> {
    let citation = grant
        .checkpoint
        .citations
        .get(intent.ordinal as usize - 1)
        .ok_or(Error::StorageIntegrity)?;
    let mut step = TaskRunStep {
        ordinal: intent.ordinal,
        kind: "finding".into(),
        effect_id: intent.effect_id.clone(),
        citation_ordinal: intent.citation_ordinal,
        finding_id: None,
        reason: None,
        recorded_ms: now,
    };
    if artifact_exists(connection, monitor, &intent.effect_id, "finding")? {
        step.reason = Some("effect-conflict".into());
        return Ok(Event {
            generation: intent.ordinal + 1,
            state: TaskRunState::Running,
            step,
            request_id: None,
            expected_generation: None,
        });
    }
    connection.execute_batch("SAVEPOINT task_effect")?;
    match crate::storage::findings::write_task_finding(
        connection,
        monitor,
        &intent.effect_id,
        citation,
        now,
    ) {
        Ok(page) => {
            step.finding_id = Some(page.id);
            step.reason = match page.original {
                FindingOriginal::Retained => None,
                FindingOriginal::Expired => Some("original-expired".into()),
                FindingOriginal::Missing => Some("original-missing".into()),
            };
        }
        Err(error) => {
            step.reason = Some(expected_refusal(error)?);
            connection.execute_batch("ROLLBACK TO task_effect")?;
        }
    }
    connection.execute_batch("RELEASE task_effect")?;
    Ok(Event {
        generation: intent.ordinal + 1,
        state: TaskRunState::Running,
        step,
        request_id: None,
        expected_generation: None,
    })
}

fn briefing_event(
    connection: &Connection,
    monitor: &str,
    grant: &Grant,
    view: &TaskRunView,
    intent: &Intent,
    now: i64,
) -> Result<Event> {
    let selected = view
        .steps
        .iter()
        .filter_map(|step| step.finding_id.clone())
        .collect::<Vec<_>>();
    let partial = grant.initial_partial || view.steps.iter().any(|step| step.reason.is_some());
    let mut event = Event {
        step: TaskRunStep {
            ordinal: intent.ordinal,
            kind: "briefing".into(),
            effect_id: intent.effect_id.clone(),
            citation_ordinal: None,
            finding_id: None,
            reason: partial.then(|| "coverage-partial".into()),
            recorded_ms: now,
        },
        generation: intent.ordinal + 1,
        state: if partial {
            TaskRunState::Partial
        } else {
            TaskRunState::Completed
        },
        request_id: None,
        expected_generation: None,
    };
    if artifact_exists(connection, monitor, &intent.effect_id, "briefing")? {
        event.step.reason = Some("effect-conflict".into());
        event.state = TaskRunState::Partial;
        return Ok(event);
    }
    connection.execute_batch("SAVEPOINT task_effect")?;
    if let Err(error) = crate::storage::briefings::write_task_briefing(
        connection,
        monitor,
        &intent.effect_id,
        &grant.checkpoint.coverage,
        &selected,
        now,
    ) {
        event.step.reason = Some(expected_refusal(error)?);
        event.state = TaskRunState::Partial;
        connection.execute_batch("ROLLBACK TO task_effect")?;
    }
    connection.execute_batch("RELEASE task_effect")?;
    Ok(event)
}

impl Store {
    /// Advance one accepted intent under the current task scope, or append revocation.
    /// # Errors
    /// Refuses stale generation, backward clock or corrupt storage. Expected evidence
    /// refusals become partial receipts without dispatching other work.
    pub(crate) fn advance_task_run(
        &mut self,
        id: &str,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskRunView> {
        let view = self.task_run(id)?.ok_or(Error::NotFound)?;
        if view.generation != expected_generation {
            return Err(Error::IdempotencyConflict);
        }
        if view.state != TaskRunState::Running {
            return Ok(view);
        }
        if now < view.updated_ms {
            return Err(Error::InvalidInput("task run clock"));
        }
        let grant = self.checked_run_grant(id)?.ok_or(Error::StorageIntegrity)?;
        let task_scope = self.checked_task_scope(id)?.0;
        let current = scope_current(self, &task_scope)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let event = if current {
            let plan = intents(&grant);
            let intent = plan
                .get(expected_generation as usize - 1)
                .ok_or(Error::StorageIntegrity)?;
            if intent.kind == "finding" {
                finding_event(&tx, &task_scope.monitor_id, &grant, intent, now)?
            } else {
                briefing_event(&tx, &task_scope.monitor_id, &grant, &view, intent, now)?
            }
        } else {
            Event {
                step: TaskRunStep {
                    ordinal: expected_generation,
                    kind: "revoked".into(),
                    effect_id: effect_id(&grant.sha256, "revoked", expected_generation),
                    citation_ordinal: None,
                    finding_id: None,
                    reason: Some("monitor-scope-changed".into()),
                    recorded_ms: now,
                },
                generation: expected_generation + 1,
                state: TaskRunState::Revoked,
                request_id: None,
                expected_generation: None,
            }
        };
        insert_event(&tx, id, &event)?;
        tx.commit()?;
        self.task_run(id)?.ok_or(Error::StorageIntegrity)
    }
}
