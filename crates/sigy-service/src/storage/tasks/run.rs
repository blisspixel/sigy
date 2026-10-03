//! Finite service execution. Each catalog effect and its receipt commit together.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, scope_current, validate_key};
use crate::{
    Error, Result,
    recognition::sha256_hex,
    task::{
        TaskCheckpoint, TaskCitation,
        run::{
            EXACT_RUN_TEMPLATE, RUN_TEMPLATE, TaskRunSelection, TaskRunSpec, TaskRunState,
            TaskRunStep, TaskRunView, TaskSnapshotRunSpec,
        },
        snapshot::TaskEvidenceSnapshot,
    },
};

mod admission;
mod advance;
mod audit;
mod migration;
pub(in crate::storage) use migration::migrate_047;
#[cfg(test)]
pub(in crate::storage) use migration::revert_047_for_tests;
#[cfg(test)]
mod tests;

struct Grant {
    request_id: String,
    spec: TaskRunSelection,
    sha256: String,
    observation_sha256: String,
    created_ms: i64,
    planned_findings: u32,
    initial_partial: bool,
    observation: Observation,
}

#[derive(Clone)]
enum Observation {
    Checkpoint(Box<TaskCheckpoint>),
    Snapshot(Box<TaskEvidenceSnapshot>),
}

impl Observation {
    fn citations(&self) -> &[TaskCitation] {
        match self {
            Self::Checkpoint(c) => &c.citations,
            Self::Snapshot(s) => &s.evidence.citations,
        }
    }
    fn observed_ms(&self) -> i64 {
        match self {
            Self::Checkpoint(c) => c.observed_ms,
            Self::Snapshot(s) => s.observed_ms,
        }
    }
    fn partial(&self, maximum: u32) -> bool {
        self.citations().len() > maximum as usize
            || match self {
                Self::Checkpoint(c) => partial_checkpoint(c),
                Self::Snapshot(s) => !matches!(
                    s.evidence.outcome,
                    crate::task::evidence::TaskOutcome::Cited
                        | crate::task::evidence::TaskOutcome::NoLiteralMatch
                ),
            }
    }
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

fn exact_grant_hash(
    id: &str,
    request: &str,
    json: &str,
    scope: &str,
    observation: &str,
    now: i64,
) -> Result<String> {
    let identity = serde_json::to_string(&(id, request, scope, "snapshot", observation, now))?;
    Ok(sha256_hex(
        format!("[\"sigy-task-run-v2\",\"{EXACT_RUN_TEMPLATE}\",{identity},{json},0]").as_bytes(),
    ))
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
                    kind: stored_text(row, 3, 16)?,
                    effect_id: stored_text(row, 4, 128)?,
                    citation_ordinal: row.get(5)?,
                    finding_id: optional_text(row, 6, 128)?,
                    reason: optional_text(row, 7, 128)?,
                    recorded_ms: row.get(8)?,
                },
                row.get::<_, u32>(1)?,
                stored_text(row, 2, 16)?,
                optional_text(row, 9, 128)?,
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

fn stored_text(row: &rusqlite::Row<'_>, index: usize, maximum: usize) -> rusqlite::Result<String> {
    let value = row.get_ref(index)?.as_str()?;
    if value.is_empty() || value.len() > maximum {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(value.to_owned())
}

fn optional_text(
    row: &rusqlite::Row<'_>,
    index: usize,
    maximum: usize,
) -> rusqlite::Result<Option<String>> {
    if matches!(row.get_ref(index)?, rusqlite::types::ValueRef::Null) {
        return Ok(None);
    }
    stored_text(row, index, maximum).map(Some)
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
    /// Read the run-owned exact artifact. It preserves the selected snapshot's facts.
    /// # Errors
    /// Refuses malformed IDs, inconsistent membership, origin or query-work exhaustion.
    pub fn task_evidence_briefing(
        &self,
        id: &str,
    ) -> Result<Option<crate::task::run::TaskEvidenceBriefing>> {
        let work = crate::storage::query_work::QueryWork::start(
            &self.connection,
            crate::storage::query_work::Limits::TASK_EVIDENCE,
        )?;
        let result = (|| {
            let tx = rusqlite::Transaction::new_unchecked(
                &self.connection,
                rusqlite::TransactionBehavior::Deferred,
            )?;
            validate_key(id, "task ID")?;
            let Some(grant) = self.checked_run_grant(id)? else {
                return Ok(None);
            };
            let run = self.task_run_with_grant(id, &grant)?;
            if !matches!(run.spec, TaskRunSelection::Snapshot(_)) {
                return Ok(None);
            }
            let Some(briefing) = run.briefing_id else {
                return Ok(None);
            };
            let Observation::Snapshot(snapshot) = &grant.observation else {
                return Err(Error::StorageIntegrity);
            };
            let page =
                self.task_evidence_briefing_record(snapshot, &grant.observation_sha256, &briefing)?;
            work.check()?;
            tx.commit()?;
            Ok(Some(page))
        })();
        let result = result.and_then(|page| {
            work.check()?;
            Ok(page)
        });
        work.finish()?;
        result
    }

    /// Inspect immutable delegation, committed effects and terminal lifecycle receipts.
    /// # Errors
    /// Refuses invalid IDs or inconsistent grants, intents or effect references.
    pub fn task_run(&self, id: &str) -> Result<Option<TaskRunView>> {
        let work = crate::storage::query_work::QueryWork::start(
            &self.connection,
            crate::storage::query_work::Limits::TASK_EVIDENCE,
        )?;
        let result = (|| {
            let tx = rusqlite::Transaction::new_unchecked(
                &self.connection,
                rusqlite::TransactionBehavior::Deferred,
            )?;
            let view = self.task_run_in_work(id)?;
            work.check()?;
            tx.commit()?;
            Ok(view)
        })();
        let result = result.and_then(|view| {
            work.check()?;
            Ok(view)
        });
        work.finish()?;
        result
    }

    fn task_run_in_work(&self, id: &str) -> Result<Option<TaskRunView>> {
        validate_key(id, "task ID")?;
        let Some(grant) = self.checked_run_grant(id)? else {
            return Ok(None);
        };
        self.task_run_with_grant(id, &grant).map(Some)
    }

    fn task_run_with_grant(&self, id: &str, grant: &Grant) -> Result<TaskRunView> {
        let history = events(&self.connection, id)?;
        self.validate_run_history(id, grant, &history)?;
        let latest = history.last();
        Ok(TaskRunView {
            task_id: id.into(),
            request_id: grant.request_id.clone(),
            spec: grant.spec.clone(),
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
        })
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
