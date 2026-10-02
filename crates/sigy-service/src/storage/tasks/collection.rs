//! Atomic finite collection grants on the canonical civil scheduler.

use jiff::{Timestamp, tz::TimeZone};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Store, validate_key};
use crate::{
    Error, Result,
    monitor::MonitorScheduleOwner,
    recognition::sha256_hex,
    storage::schedules::{ScheduleDraft, ScheduleOccurrence, ScheduleRule},
    task::{
        TaskView,
        collection::{
            COLLECTION_TEMPLATE, TaskCaptureSpec, TaskCollectionCapture, TaskCollectionSpec,
            TaskCollectionView,
        },
    },
};

mod admission;
mod audit;
mod read;
#[cfg(test)]
mod tests;

pub(crate) use admission::{check, owns_rule, reserve};

#[derive(Debug)]
struct Grant {
    request_id: String,
    spec: TaskCollectionSpec,
    digest: String,
    scope_sha256: String,
    created_ms: i64,
}

fn grant_hash(id: &str, request: &str, json: &str, scope: &str, now: i64) -> Result<String> {
    Ok(sha256_hex(
        serde_json::to_string(&(COLLECTION_TEMPLATE, id, request, json, scope, now, 0))?.as_bytes(),
    ))
}

fn cancel_hash(grant: &str, request: &str, mask: u32, now: i64) -> Result<String> {
    Ok(sha256_hex(
        serde_json::to_string(&("sigy-collection-cancel-v1", grant, request, 1, mask, now))?
            .as_bytes(),
    ))
}

fn draft(grant: &str, ordinal: usize, capture: &TaskCaptureSpec) -> Result<ScheduleDraft> {
    let clock = Timestamp::from_millisecond(capture.start_ms)
        .map_err(|_| Error::InvalidInput("task capture clock"))?
        .to_zoned(TimeZone::UTC);
    Ok(ScheduleDraft {
        id: format!("tc:{grant}:{ordinal}"),
        source_revision: capture.source_revision.clone(),
        zone: "Etc/UTC".into(),
        recurrence: "once".into(),
        civil_date: Some(clock.date().to_string()),
        weekday: None,
        hour: i64::from(clock.hour()),
        minute: i64::from(clock.minute()),
        second: i64::from(clock.second()),
        duration_seconds: i64::from(capture.duration_seconds),
        maximum_bytes: i64::try_from(capture.maximum_bytes)
            .map_err(|_| Error::InvalidInput("task capture bytes"))?,
    })
}

impl Store {
    /// Accept one lifetime finite collection grant and its new rules in one transaction.
    /// # Errors
    /// Refuses drift, unbounded or unauthorized captures, changed replay and expired windows.
    pub(crate) fn start_task_collection(
        &mut self,
        id: &str,
        request_id: &str,
        spec: &TaskCollectionSpec,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskCollectionView> {
        validate_key(id, "task ID")?;
        validate_key(request_id, "task collection request")?;
        spec.validate()?;
        if let Some(existing) = self.task_collection(id)? {
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
        if !task.scope_current {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        self.validate_collection_scope(&task, spec, now)?;
        let json = serde_json::to_string(spec)?;
        let digest = grant_hash(id, request_id, &json, &task.scope_sha256, now)?;
        let owner = MonitorScheduleOwner {
            monitor_id: task.spec.monitor_id.clone(),
            version: task.spec.monitor_version,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !current_scope(&tx, &task.spec)? {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        if super::audit::checked_task_scope_in(&tx, id)?.1 != task.scope_sha256 {
            return Err(Error::StorageIntegrity);
        }
        if now < latest_task_ms(&tx, id)? {
            return Err(Error::InvalidInput("task collection clock"));
        }
        tx.execute("INSERT INTO task_collections(task_id, request_id, spec_json, grant_sha256, scope_sha256, template, amount_micros, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)", params![id, request_id, json, digest, task.scope_sha256, COLLECTION_TEMPLATE, now])?;
        for (ordinal, capture) in spec.captures.iter().enumerate() {
            let prepared = draft(&digest, ordinal, capture)?;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM schedule_rules WHERE id = ?1)",
                [&prepared.id],
                |row| row.get(0),
            )?;
            if exists {
                return Err(Error::IdempotencyConflict);
            }
            super::super::schedules::create_schedule_in(&tx, &prepared, Some(&owner), now)?;
            tx.execute(
                "UPDATE schedule_rules SET task_owned = 1 WHERE id = ?1",
                [&prepared.id],
            )?;
            tx.execute("INSERT INTO task_collection_rules(task_id, ordinal, rule_id, rule_revision, generation) VALUES (?1, ?2, ?3, 0, 1)", params![id, i64::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?, prepared.id])?;
        }
        tx.commit()?;
        self.task_collection(id)?.ok_or(Error::StorageIntegrity)
    }

    fn validate_collection_scope(
        &self,
        task: &TaskView,
        spec: &TaskCollectionSpec,
        now: i64,
    ) -> Result<()> {
        if now < task.created_ms {
            return Err(Error::InvalidInput("task collection clock"));
        }
        let context = self.task_checkpoint_context(&task.spec)?;
        let bounds = context
            .policy
            .capture
            .ok_or(Error::InvalidInput("monitor capture is disabled"))?;
        let mut seconds = 0_u64;
        let mut bytes = 0_u64;
        for capture in &spec.captures {
            if !context.sources.contains(&capture.source_revision)
                || capture.start_ms < task.spec.from_ms
                || capture.end_ms()? > task.spec.to_ms
                || capture.end_ms()? <= now
            {
                return Err(Error::InvalidInput("task collection scope"));
            }
            seconds += u64::from(capture.duration_seconds);
            bytes += capture.maximum_bytes;
        }
        if seconds > bounds.total_seconds || bytes > bounds.total_bytes {
            return Err(Error::InvalidInput(
                "task collection exceeds monitor capture bounds",
            ));
        }
        Ok(())
    }

    /// Cancel future collection admissions only. Running work keeps its existing authority.
    /// # Errors
    /// Refuses changed replay, a stale generation and a regressed receipt clock.
    pub(crate) fn cancel_task_collection(
        &mut self,
        id: &str,
        request_id: &str,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskCollectionView> {
        validate_key(id, "task ID")?;
        validate_key(request_id, "task collection cancellation")?;
        let view = self.task_collection(id)?.ok_or(Error::NotFound)?;
        if let Some((request, expected)) = self.connection.query_row("SELECT request_id, expected_generation FROM task_collection_cancellations WHERE task_id = ?1", [id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))).optional()? {
            return if request == request_id && expected == expected_generation {
                Ok(view)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        if expected_generation != view.generation {
            return Err(Error::IdempotencyConflict);
        }
        let last_admission: i64 = self.connection.query_row("SELECT coalesce(max(admitted_ms), ?2) FROM task_collection_admissions WHERE task_id = ?1", params![id, view.created_ms], |row| row.get(0))?;
        if now < last_admission {
            return Err(Error::InvalidInput("task collection clock"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if now < latest_task_ms(&tx, id)? {
            return Err(Error::InvalidInput("task collection clock"));
        }
        let mask = admission_mask(&tx, id)?;
        let digest = cancel_hash(&view.grant_sha256, request_id, mask, now)?;
        tx.execute("INSERT INTO task_collection_cancellations(task_id, request_id, expected_generation, generation, admitted_mask, receipt_sha256, created_ms) VALUES (?1, ?2, ?3, 2, ?4, ?5, ?6)", params![id, request_id, expected_generation, mask, digest, now])?;
        tx.commit()?;
        self.task_collection(id)?.ok_or(Error::StorageIntegrity)
    }
}

pub(super) fn current_scope(connection: &Connection, task: &crate::task::TaskSpec) -> Result<bool> {
    Ok(connection.query_row("SELECT (SELECT max(version) FROM monitor_versions WHERE monitor_id = ?1) = ?2 AND (SELECT count(*) FROM monitor_actions WHERE monitor_id = ?1) = ?3", params![task.monitor_id, task.monitor_version, task.monitor_actions], |row| row.get(0))?)
}

/// The latest committed task receipt. A fresh task effect must not precede it.
pub(super) fn latest_task_ms(connection: &Connection, id: &str) -> Result<i64> {
    Ok(connection.query_row("SELECT max(moment) FROM (SELECT created_ms AS moment FROM tasks WHERE id = ?1 UNION ALL SELECT observed_ms FROM task_checkpoints WHERE task_id = ?1 UNION ALL SELECT created_ms FROM task_runs WHERE task_id = ?1 UNION ALL SELECT recorded_ms FROM task_run_events WHERE task_id = ?1 UNION ALL SELECT admitted_ms FROM task_collection_admissions WHERE task_id = ?1 UNION ALL SELECT created_ms FROM task_processing WHERE task_id = ?1 UNION ALL SELECT created_ms FROM task_processing_steps WHERE task_id = ?1 UNION ALL SELECT created_ms FROM task_processing_cancellations WHERE task_id = ?1)", [id], |row| row.get(0))?)
}

fn admission_mask(connection: &Connection, id: &str) -> Result<u32> {
    Ok(connection.query_row(
        "SELECT coalesce(sum(1 << ordinal), 0) FROM task_collection_admissions WHERE task_id = ?1",
        [id],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
pub(in crate::storage) fn remove_collection_schema(store: &Store) -> Result<()> {
    super::processing::remove_processing_schema(store)?;
    store.connection.execute_batch("DROP TRIGGER task_collection_schedule_no_update; DROP TRIGGER task_collection_schedule_no_delete; DROP TABLE task_collection_admissions; DROP TABLE task_collection_cancellations; DROP TABLE task_collection_rules; DROP TABLE task_collections; ALTER TABLE schedule_rules DROP COLUMN task_owned")?;
    Ok(())
}
