//! Immutable task scopes. These records grant no execution authority.

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::{Store, validate_key};
use crate::{
    Error, Result,
    recognition::sha256_hex,
    task::{MAX_TASKS, TASK_TEMPLATE, TaskSpec, TaskView},
};

mod audit;
mod checkpoint;
pub(crate) mod collection;
mod evidence;
pub(crate) mod processing;
pub(in crate::storage) mod run;
pub(crate) mod snapshot;
#[cfg(test)]
mod tests;
mod validation;

fn scope_hash(json: &str, monitor_sha256: &str) -> String {
    sha256_hex(
        format!("[\"sigy-task-scope-v1\",\"{TASK_TEMPLATE}\",\"{monitor_sha256}\",{json},0]")
            .as_bytes(),
    )
}

fn checkpoint_hash(json: &str, scope_sha256: &str) -> String {
    sha256_hex(format!("[\"sigy-task-checkpoint-v1\",\"{scope_sha256}\",{json}]").as_bytes())
}

fn scope_current(store: &Store, spec: &TaskSpec) -> Result<bool> {
    Ok(store.connection.query_row(
        "SELECT (SELECT max(version) FROM monitor_versions WHERE monitor_id = ?1) = ?2 AND (SELECT count(*) FROM monitor_actions WHERE monitor_id = ?1) = ?3",
        params![spec.monitor_id, spec.monitor_version, spec.monitor_actions], |row| row.get(0),
    )?)
}

impl Store {
    /// Store a bounded task without admitting work. Exact replay changes nothing.
    /// # Errors
    /// Refuses invalid or stale scope, changed replay, or exhausted task capacity.
    pub(crate) fn create_task(&mut self, id: &str, spec: &TaskSpec, now: i64) -> Result<bool> {
        validate_key(id, "task ID")?;
        spec.validate()?;
        if now < 0 {
            return Err(Error::InvalidInput("task creation time"));
        }
        if let Some(existing) = self
            .connection
            .query_row("SELECT spec_json FROM tasks WHERE id = ?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
        {
            let old: TaskSpec =
                serde_json::from_str(&existing).map_err(|_| Error::StorageIntegrity)?;
            self.task(id)?;
            return if old == *spec {
                Ok(false)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        let current = self.monitor(&spec.monitor_id)?;
        let action_time: i64 = self.connection.query_row(
            "SELECT coalesce(max(created_ms), 0) FROM monitor_actions WHERE monitor_id = ?1",
            [&spec.monitor_id],
            |row| row.get(0),
        )?;
        if current.version.version != spec.monitor_version
            || current.actions != spec.monitor_actions
            || now < current.version.created_ms
            || now < action_time
        {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        let count: u32 = self
            .connection
            .query_row("SELECT count(*) FROM tasks", [], |row| row.get(0))?;
        if count >= MAX_TASKS {
            return Err(Error::InvalidInput("task capacity"));
        }
        let json = serde_json::to_string(spec)?;
        let monitor_sha = current.version.spec_sha256;
        let digest = scope_hash(&json, &monitor_sha);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO tasks(id, spec_json, scope_sha256, monitor_id, monitor_version, monitor_actions, monitor_spec_sha256, from_ms, to_ms, template, amount_micros, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)",
            params![id, json, digest, spec.monitor_id, spec.monitor_version, spec.monitor_actions, monitor_sha, spec.from_ms, spec.to_ms, TASK_TEMPLATE, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Inspect one accepted scope and its most recent frozen observations.
    /// # Errors
    /// Refuses a missing task or invalid stored scope and checkpoints.
    pub fn task(&self, id: &str) -> Result<TaskView> {
        validate_key(id, "task ID")?;
        let (spec, scope_sha256, monitor_spec_sha256, created_ms) = self.checked_task_scope(id)?;
        let ordinal: u32 = self.connection.query_row(
            "SELECT coalesce(max(ordinal), 0) FROM task_checkpoints WHERE task_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let latest_checkpoint = if ordinal == 0 {
            None
        } else {
            Some(Box::new(self.task_checkpoint(id, ordinal)?))
        };
        let snapshots = self.connection.query_row(
            "SELECT coalesce(max(ordinal), 0) FROM task_evidence_snapshots WHERE task_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(TaskView {
            id: id.to_owned(),
            scope_current: scope_current(self, &spec)?,
            spec,
            scope_sha256,
            monitor_spec_sha256,
            created_ms,
            checkpoint: ordinal,
            snapshots,
            latest_checkpoint,
            template: TASK_TEMPLATE.into(),
            paid_allowance_usd: "0.000000".into(),
        })
    }

    /// Read a bounded lexicographic page. The cursor is the final returned identity.
    /// # Errors
    /// Refuses malformed cursors or limits outside 1 to 16.
    pub fn task_ids(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<String>, Option<String>)> {
        if let Some(cursor) = after {
            validate_key(cursor, "task cursor")?;
        }
        if !(1..=16).contains(&limit) {
            return Err(Error::InvalidInput("task page limit"));
        }
        let mut statement = self
            .connection
            .prepare("SELECT id FROM tasks WHERE id > ?1 ORDER BY id LIMIT ?2")?;
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
