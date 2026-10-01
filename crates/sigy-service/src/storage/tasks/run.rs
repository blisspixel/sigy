//! Finite service execution. Each catalog effect and its receipt commit together.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, scope_current, validate_key};
use crate::{
    Error, Result,
    recognition::sha256_hex,
    task::{
        TaskCheckpoint,
        run::{RUN_TEMPLATE, TaskRunSpec, TaskRunState, TaskRunStep, TaskRunView},
    },
};

mod admission;
mod advance;
mod audit;
#[cfg(test)]
mod tests;

struct Grant {
    request_id: String,
    spec: TaskRunSpec,
    sha256: String,
    created_ms: i64,
    planned_findings: u32,
    initial_partial: bool,
    checkpoint: TaskCheckpoint,
}

#[derive(Debug, PartialEq, Eq)]
struct Intent {
    ordinal: u32,
    kind: String,
    effect_id: String,
    citation_ordinal: Option<u32>,
}

struct Event {
    step: TaskRunStep,
    generation: u32,
    state: TaskRunState,
    request_id: Option<String>,
    expected_generation: Option<u32>,
}

fn grant_hash(
    id: &str,
    request: &str,
    json: &str,
    scope: &str,
    checkpoint: &str,
    now: i64,
) -> Result<String> {
    let identity = serde_json::to_string(&(id, request, scope, checkpoint, now))?;
    Ok(sha256_hex(
        format!("[\"sigy-task-run-v1\",\"{RUN_TEMPLATE}\",{identity},{json},0]").as_bytes(),
    ))
}

fn effect_id(grant: &str, kind: &str, ordinal: u32) -> String {
    let digest = sha256_hex(
        format!("[\"sigy-task-effect-v1\",\"{grant}\",\"{kind}\",{ordinal}]").as_bytes(),
    );
    format!("task-{kind}:{digest}")
}

fn partial_checkpoint(checkpoint: &TaskCheckpoint) -> bool {
    checkpoint.more
        || !checkpoint.window_elapsed
        || checkpoint.monitor_paused
        || checkpoint.coverage.sources.is_empty()
        || checkpoint
            .coverage
            .sources
            .iter()
            .any(|source| source.captures == 0)
        || checkpoint.coverage.sources.iter().any(|source| {
            source.truncated
                || source.gaps > 0
                || source.published < source.captures
                || source.pinned < source.published
                || u64::from(source.transcribed) + u64::from(source.no_text)
                    < u64::from(source.pinned)
                || source.cues_without_translation > 0
                || source.untranslated_cues > 0
        })
        || checkpoint.coverage.schedules.iter().any(|schedule| {
            schedule.waiting > 0
                || schedule.missed_elapsed > 0
                || schedule.missed_spring_forward > 0
        })
}

fn planned_count(checkpoint: &TaskCheckpoint, spec: &TaskRunSpec) -> Result<u32> {
    let available =
        u32::try_from(checkpoint.citations.len()).map_err(|_| Error::StorageIntegrity)?;
    Ok(available.min(spec.maximum_findings))
}

fn intents(grant: &Grant) -> Vec<Intent> {
    let mut result = (0..grant.planned_findings)
        .map(|citation| Intent {
            ordinal: citation + 1,
            kind: "finding".into(),
            effect_id: effect_id(&grant.sha256, "finding", citation + 1),
            citation_ordinal: Some(citation),
        })
        .collect::<Vec<_>>();
    result.push(Intent {
        ordinal: grant.planned_findings + 1,
        kind: "briefing".into(),
        effect_id: effect_id(&grant.sha256, "briefing", grant.planned_findings + 1),
        citation_ordinal: None,
    });
    result
}

fn state(value: &str) -> Result<TaskRunState> {
    match value {
        "running" => Ok(TaskRunState::Running),
        "completed" => Ok(TaskRunState::Completed),
        "partial" => Ok(TaskRunState::Partial),
        "cancelled" => Ok(TaskRunState::Cancelled),
        "revoked" => Ok(TaskRunState::Revoked),
        _ => Err(Error::StorageIntegrity),
    }
}

fn state_name(value: TaskRunState) -> &'static str {
    match value {
        TaskRunState::Running => "running",
        TaskRunState::Completed => "completed",
        TaskRunState::Partial => "partial",
        TaskRunState::Cancelled => "cancelled",
        TaskRunState::Revoked => "revoked",
    }
}

fn artifact_exists(connection: &Connection, monitor: &str, id: &str, kind: &str) -> Result<bool> {
    let sql = match kind {
        "finding" => {
            "SELECT EXISTS(SELECT 1 FROM monitor_findings WHERE monitor_id = ?1 AND id = ?2)"
        }
        "briefing" => {
            "SELECT EXISTS(SELECT 1 FROM monitor_briefings WHERE monitor_id = ?1 AND id = ?2)"
        }
        _ => return Err(Error::StorageIntegrity),
    };
    Ok(connection.query_row(sql, params![monitor, id], |row| row.get(0))?)
}

fn events(connection: &Connection, id: &str) -> Result<Vec<Event>> {
    let mut statement = connection.prepare("SELECT ordinal, generation, state, kind, effect_id, citation_ordinal, finding_id, reason, recorded_ms, request_id, expected_generation FROM task_run_events WHERE task_id = ?1 ORDER BY ordinal LIMIT 67")?;
    let rows = statement
        .query_map([id], |row| {
            Ok((
                TaskRunStep {
                    ordinal: row.get(0)?,
                    kind: row.get(3)?,
                    effect_id: row.get(4)?,
                    citation_ordinal: row.get(5)?,
                    finding_id: row.get(6)?,
                    reason: row.get(7)?,
                    recorded_ms: row.get(8)?,
                },
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<u32>>(10)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(
            |(step, generation, status, request_id, expected_generation)| {
                Ok(Event {
                    step,
                    generation,
                    state: state(&status)?,
                    request_id,
                    expected_generation,
                })
            },
        )
        .collect()
}

fn insert_event(connection: &Connection, id: &str, event: &Event) -> Result<()> {
    let step = &event.step;
    connection.execute(
        "INSERT INTO task_run_events(task_id, ordinal, generation, state, kind, effect_id, citation_ordinal, finding_id, reason, request_id, expected_generation, recorded_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![id, step.ordinal, event.generation, state_name(event.state), step.kind, step.effect_id, step.citation_ordinal, step.finding_id, step.reason, event.request_id, event.expected_generation, step.recorded_ms],
    )?;
    Ok(())
}

impl Store {
    /// Inspect immutable delegation, committed effects and terminal lifecycle receipts.
    /// # Errors
    /// Refuses invalid IDs or inconsistent grants, intents or effect references.
    pub fn task_run(&self, id: &str) -> Result<Option<TaskRunView>> {
        validate_key(id, "task ID")?;
        let Some(grant) = self.checked_run_grant(id)? else {
            return Ok(None);
        };
        let history = events(&self.connection, id)?;
        self.validate_run_history(id, &grant, &history)?;
        let latest = history.last();
        Ok(Some(TaskRunView {
            task_id: id.into(),
            request_id: grant.request_id,
            spec: grant.spec,
            generation: latest.map_or(1, |event| event.generation),
            state: latest.map_or(TaskRunState::Running, |event| event.state),
            created_ms: grant.created_ms,
            updated_ms: latest.map_or(grant.created_ms, |event| event.step.recorded_ms),
            planned_findings: grant.planned_findings,
            published_findings: u32::try_from(
                history
                    .iter()
                    .filter(|event| event.step.finding_id.is_some())
                    .count(),
            )
            .map_err(|_| Error::StorageIntegrity)?,
            skipped_findings: u32::try_from(
                history
                    .iter()
                    .filter(|event| event.step.kind == "finding" && event.step.finding_id.is_none())
                    .count(),
            )
            .map_err(|_| Error::StorageIntegrity)?,
            briefing_id: history
                .iter()
                .find(|event| {
                    event.step.kind == "briefing"
                        && matches!(
                            event.step.reason.as_deref(),
                            None | Some("coverage-partial")
                        )
                })
                .map(|event| event.step.effect_id.clone()),
            steps: history.into_iter().map(|event| event.step).collect(),
        }))
    }

    /// Read a bounded page of accepted unfinished runs for the service's existing tick.
    /// # Errors
    /// Refuses malformed cursors or page sizes outside 1 to 16.
    pub(crate) fn pending_task_run_ids(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<String>, Option<String>)> {
        if let Some(cursor) = after {
            validate_key(cursor, "task run cursor")?;
        }
        if !(1..=16).contains(&limit) {
            return Err(Error::InvalidInput("task run page limit"));
        }
        let mut statement = self.connection.prepare("SELECT r.task_id FROM task_runs r WHERE r.task_id > ?1 AND NOT EXISTS (SELECT 1 FROM task_run_events e WHERE e.task_id = r.task_id AND e.state != 'running') ORDER BY r.task_id LIMIT ?2")?;
        let mut ids = statement
            .query_map(params![after.unwrap_or(""), limit + 1], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next = if ids.len() > limit as usize {
            ids.truncate(limit as usize);
            ids.last().cloned()
        } else {
            None
        };
        Ok((ids, next))
    }
}
