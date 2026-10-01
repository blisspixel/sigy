//! Bounded reopen checks reconstruct the accepted plan and validate committed artifacts.

use super::{
    Error, Event, Grant, Intent, OptionalExtension, RUN_TEMPLATE, Result, Store, TaskRunSpec,
    TaskRunState, effect_id, grant_hash, intents, params, partial_checkpoint, planned_count,
    scope_current, validate_key,
};
use crate::monitor::FindingOriginal;

struct RawGrant {
    request: String,
    json: String,
    digest: String,
    scope: String,
    checkpoint: u32,
    checkpoint_digest: String,
    maximum: u32,
    planned: u32,
    partial: bool,
    template: String,
    cost: i64,
    created: i64,
}

impl Store {
    pub(super) fn checked_run_grant(&self, id: &str) -> Result<Option<Grant>> {
        let Some(raw) = self.connection.query_row(
            "SELECT request_id, spec_json, grant_sha256, scope_sha256, checkpoint_ordinal, checkpoint_sha256, maximum_findings, planned_findings, initial_partial, template, amount_micros, created_ms FROM task_runs WHERE task_id = ?1",
            [id], |row| Ok(RawGrant {
                request: row.get(0)?, json: row.get(1)?, digest: row.get(2)?, scope: row.get(3)?,
                checkpoint: row.get(4)?, checkpoint_digest: row.get(5)?, maximum: row.get(6)?,
                planned: row.get(7)?, partial: row.get(8)?, template: row.get(9)?, cost: row.get(10)?, created: row.get(11)?,
            }),
        ).optional()? else { return Ok(None); };
        validate_key(&raw.request, "task run request").map_err(|_| Error::StorageIntegrity)?;
        let spec: TaskRunSpec =
            serde_json::from_str(&raw.json).map_err(|_| Error::StorageIntegrity)?;
        spec.validate().map_err(|_| Error::StorageIntegrity)?;
        let scope = self.checked_task_scope(id)?;
        let checkpoint = self.task_checkpoint(id, spec.checkpoint_ordinal)?;
        let digest: String = self.connection.query_row(
            "SELECT payload_sha256 FROM task_checkpoints WHERE task_id = ?1 AND ordinal = ?2",
            params![id, spec.checkpoint_ordinal],
            |row| row.get(0),
        )?;
        let partial = partial_checkpoint(&checkpoint)
            || checkpoint.citations.len() > spec.maximum_findings as usize;
        if raw.json.len() > 1024
            || raw.scope != scope.1
            || raw.checkpoint != spec.checkpoint_ordinal
            || raw.checkpoint_digest != digest
            || raw.maximum != spec.maximum_findings
            || raw.planned != planned_count(&checkpoint, &spec)?
            || raw.partial != partial
            || raw.template != RUN_TEMPLATE
            || raw.cost != 0
            || raw.created < checkpoint.observed_ms
            || checkpoint.monitor_paused
            || raw.digest
                != grant_hash(
                    id,
                    &raw.request,
                    &raw.json,
                    &raw.scope,
                    &digest,
                    raw.created,
                )?
        {
            return Err(Error::StorageIntegrity);
        }
        let grant = Grant {
            request_id: raw.request,
            spec,
            sha256: raw.digest,
            created_ms: raw.created,
            planned_findings: raw.planned,
            initial_partial: partial,
            checkpoint,
        };
        let mut statement = self.connection.prepare("SELECT ordinal, kind, effect_id, citation_ordinal FROM task_run_intents WHERE task_id = ?1 ORDER BY ordinal LIMIT 66")?;
        let stored = statement
            .query_map([id], |row| {
                Ok(Intent {
                    ordinal: row.get(0)?,
                    kind: row.get(1)?,
                    effect_id: row.get(2)?,
                    citation_ordinal: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if stored != intents(&grant) {
            return Err(Error::StorageIntegrity);
        }
        Ok(Some(grant))
    }

    pub(super) fn validate_run_history(
        &self,
        id: &str,
        grant: &Grant,
        history: &[Event],
    ) -> Result<()> {
        if history.len() > grant.planned_findings as usize + 1 {
            return Err(Error::StorageIntegrity);
        }
        let plan = intents(grant);
        let monitor = self.checked_task_scope(id)?.0.monitor_id;
        let mut previous_ms = grant.created_ms;
        let mut selected = Vec::new();
        let mut partial = grant.initial_partial;
        for (index, event) in history.iter().enumerate() {
            let ordinal = u32::try_from(index + 1).map_err(|_| Error::StorageIntegrity)?;
            if event.step.ordinal != ordinal
                || event.generation != ordinal + 1
                || event.step.recorded_ms < previous_ms
                || (event.state != TaskRunState::Running && index + 1 != history.len())
            {
                return Err(Error::StorageIntegrity);
            }
            previous_ms = event.step.recorded_ms;
            if matches!(event.step.kind.as_str(), "cancelled" | "revoked") {
                validate_lifecycle(grant, event)?;
                if event.state == TaskRunState::Revoked
                    && scope_current(self, &self.checked_task_scope(id)?.0)?
                {
                    return Err(Error::StorageIntegrity);
                }
                continue;
            }
            let intent = plan.get(index).ok_or(Error::StorageIntegrity)?;
            if event.step.kind != intent.kind
                || event.step.effect_id != intent.effect_id
                || event.step.citation_ordinal != intent.citation_ordinal
                || event.request_id.is_some()
                || event.expected_generation.is_some()
            {
                return Err(Error::StorageIntegrity);
            }
            match intent.kind.as_str() {
                "finding" => {
                    self.validate_finding_event(&monitor, grant, event)?;
                    if let Some(finding) = &event.step.finding_id {
                        selected.push(finding.clone());
                    }
                    partial |= event.step.reason.is_some();
                }
                "briefing" => {
                    self.validate_briefing_event(&monitor, grant, event, &selected, partial)?;
                }
                _ => return Err(Error::StorageIntegrity),
            }
        }
        Ok(())
    }

    fn validate_finding_event(&self, monitor: &str, grant: &Grant, event: &Event) -> Result<()> {
        if event.state != TaskRunState::Running {
            return Err(Error::StorageIntegrity);
        }
        let Some(finding) = &event.step.finding_id else {
            return if self.valid_refused_artifact(monitor, event)? {
                Ok(())
            } else {
                Err(Error::StorageIntegrity)
            };
        };
        let citation = grant
            .checkpoint
            .citations
            .get(event.step.ordinal as usize - 1)
            .ok_or(Error::StorageIntegrity)?;
        let page = self.finding(monitor, finding)?;
        let reason = match page.original {
            FindingOriginal::Retained => None,
            FindingOriginal::Expired => Some("original-expired"),
            FindingOriginal::Missing => Some("original-missing"),
        };
        if finding != &event.step.effect_id
            || page.transcript_id != citation.transcript_id
            || page.transcript_revision != citation.transcript_revision
            || Some(page.translation_revision) != citation.translation_revision
            || page.cue_ordinal != citation.cue_ordinal
            || page.recording_id != citation.recording_id
            || event.step.reason.as_deref() != reason
            || (page.original == FindingOriginal::Retained
                && (page.start_us != Some(citation.start_us)
                    || page.end_us != Some(citation.end_us)))
        {
            return Err(Error::StorageIntegrity);
        }
        let created: i64 = self.connection.query_row(
            "SELECT created_ms FROM monitor_findings WHERE monitor_id = ?1 AND id = ?2",
            params![monitor, finding],
            |row| row.get(0),
        )?;
        if created != event.step.recorded_ms {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }

    fn validate_briefing_event(
        &self,
        monitor: &str,
        grant: &Grant,
        event: &Event,
        selected: &[String],
        partial: bool,
    ) -> Result<()> {
        if event.step.finding_id.is_some() {
            return Err(Error::StorageIntegrity);
        }
        if !matches!(
            event.step.reason.as_deref(),
            None | Some("coverage-partial")
        ) {
            return if event.state == TaskRunState::Partial
                && self.valid_refused_artifact(monitor, event)?
            {
                Ok(())
            } else {
                Err(Error::StorageIntegrity)
            };
        }
        let expected_state = if partial {
            TaskRunState::Partial
        } else {
            TaskRunState::Completed
        };
        if event.state != expected_state
            || event.step.reason.as_deref() != partial.then_some("coverage-partial")
        {
            return Err(Error::StorageIntegrity);
        }
        let page = self.briefing(monitor, &event.step.effect_id)?;
        let mut actual = page
            .members
            .iter()
            .map(|member| member.finding_id.clone())
            .collect::<Vec<_>>();
        actual.sort();
        let mut expected = selected.to_vec();
        expected.sort();
        let created: i64 = self.connection.query_row(
            "SELECT created_ms FROM monitor_briefings WHERE monitor_id = ?1 AND id = ?2",
            params![monitor, event.step.effect_id],
            |row| row.get(0),
        )?;
        if actual != expected
            || page.coverage != grant.checkpoint.coverage
            || created != event.step.recorded_ms
        {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }

    pub(crate) fn audit_task_runs(&self) -> Result<()> {
        let mut statement = self
            .connection
            .prepare("SELECT task_id FROM task_runs ORDER BY task_id LIMIT 257")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if ids.len() > crate::task::MAX_TASKS as usize {
            return Err(Error::StorageIntegrity);
        }
        for id in ids {
            self.task_run(&id)?.ok_or(Error::StorageIntegrity)?;
        }
        Ok(())
    }

    fn valid_refused_artifact(&self, monitor: &str, event: &Event) -> Result<bool> {
        let sql = match event.step.kind.as_str() {
            "finding" => {
                "SELECT created_ms FROM monitor_findings WHERE monitor_id = ?1 AND id = ?2"
            }
            "briefing" => {
                "SELECT created_ms FROM monitor_briefings WHERE monitor_id = ?1 AND id = ?2"
            }
            _ => return Err(Error::StorageIntegrity),
        };
        let created: Option<i64> = self
            .connection
            .query_row(sql, params![monitor, event.step.effect_id], |row| {
                row.get(0)
            })
            .optional()?;
        // Independent commands may share a millisecond with the earlier refusal. Their
        // artifacts do not turn a skipped receipt into publication or invalidate history.
        Ok(if event.step.reason.as_deref() == Some("effect-conflict") {
            created.is_some_and(|time| time <= event.step.recorded_ms)
        } else {
            allowed_refusal(event.step.reason.as_deref())
                && created.is_none_or(|time| time >= event.step.recorded_ms)
        })
    }
}

pub(super) fn allowed_refusal(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some(
            "task-translation-unavailable"
                | "task-citation-conflict"
                | "task-cue-unsupported"
                | "task-original-unavailable"
                | "finding-limit"
                | "finding-conflict"
                | "finding-range"
                | "finding-original"
                | "briefing-limit"
                | "briefing-conflict"
                | "invalid-evidence"
                | "missing-evidence"
                | "effect-conflict"
                | "evidence-conflict"
        )
    )
}

fn validate_lifecycle(grant: &Grant, event: &Event) -> Result<()> {
    let step = &event.step;
    let cancelled = step.kind == "cancelled";
    let expected_state = if cancelled {
        TaskRunState::Cancelled
    } else {
        TaskRunState::Revoked
    };
    let reason = if cancelled {
        "user-cancelled"
    } else {
        "monitor-scope-changed"
    };
    if event.state != expected_state
        || step.citation_ordinal.is_some()
        || step.finding_id.is_some()
        || step.reason.as_deref() != Some(reason)
        || step.effect_id != effect_id(&grant.sha256, &step.kind, step.ordinal)
        || (cancelled
            && (event.expected_generation != Some(step.ordinal) || event.request_id.is_none()))
        || (!cancelled && (event.expected_generation.is_some() || event.request_id.is_some()))
    {
        return Err(Error::StorageIntegrity);
    }
    if let Some(request) = &event.request_id {
        validate_key(request, "task cancellation request").map_err(|_| Error::StorageIntegrity)?;
    }
    Ok(())
}
