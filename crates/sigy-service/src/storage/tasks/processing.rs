//! Finite task processing on the canonical job pool. A task job, its immutable receipt,
//! its interest and its lifetime audio charge commit together before scheduling.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{
    Store,
    collection::{current_scope, latest_task_ms},
    validate_key,
};
use crate::{
    Error, Result,
    recognition::sha256_hex,
    task::processing::{
        PROCESSING_TEMPLATE, TaskProcessingSpec, TaskProcessingStep, TaskProcessingView,
    },
};

mod admission;
mod audit;
mod facts;
#[cfg(test)]
mod tests;

pub(crate) use admission::{TaskJobAdmission, TaskJobScope};

/// Test downgrades remove v44 objects before rebuilding tables their triggers name.
#[cfg(test)]
const REVERT_044_FOR_TESTS: &str = "
DROP TRIGGER IF EXISTS task_checkpoint_shared_capacity;
DROP TRIGGER IF EXISTS task_capture_snapshot_clock;
DROP TRIGGER IF EXISTS task_collection_cancel_snapshot_clock;
DROP TRIGGER IF EXISTS task_processing_snapshot_clock;
DROP TRIGGER IF EXISTS task_processing_step_snapshot_clock;
DROP TRIGGER IF EXISTS task_processing_cancel_snapshot_clock;
DROP TRIGGER IF EXISTS task_withdrawal_snapshot_clock;
DROP TRIGGER IF EXISTS task_run_snapshot_clock;
DROP TRIGGER IF EXISTS task_run_event_snapshot_clock;
DROP TABLE IF EXISTS task_evidence_snapshots;
DROP TABLE IF EXISTS native_stop_completions;
DROP TABLE IF EXISTS native_stop_targets;
DROP TABLE IF EXISTS job_interest_withdrawals;
DROP TABLE IF EXISTS task_interest_withdrawals;
DROP TABLE IF EXISTS job_interest_legacy_guards;
DROP TABLE IF EXISTS job_interests;
DROP TABLE IF EXISTS task_processing_cancellations;
DROP TABLE IF EXISTS task_processing_steps;
DROP TABLE IF EXISTS task_processing;";

#[cfg(test)]
pub(in crate::storage) fn revert_for_tests(connection: &Connection) -> Result<()> {
    super::run::revert_047_for_tests(connection)?;
    connection.execute_batch(REVERT_044_FOR_TESTS)?;
    Ok(())
}

#[cfg(test)]
pub(in crate::storage) fn remove_processing_schema(store: &Store) -> Result<()> {
    revert_for_tests(&store.connection)
}

#[derive(Debug, Clone)]
struct Grant {
    request_id: String,
    spec: TaskProcessingSpec,
    recognition_sha256: String,
    translation_sha256: Option<String>,
    collection_sha256: String,
    scope_sha256: String,
    digest: String,
    created_ms: i64,
}

#[derive(Debug, Clone, Copy)]
struct Hashed<'a> {
    id: &'a str,
    request: &'a str,
    json: &'a str,
    recognition: &'a str,
    translation: Option<&'a str>,
    collection: &'a str,
    scope: &'a str,
    now: i64,
}

fn grant_hash(value: Hashed<'_>) -> Result<String> {
    Ok(sha256_hex(
        serde_json::to_string(&(
            "sigy-task-processing-v1",
            PROCESSING_TEMPLATE,
            value.id,
            value.request,
            value.json,
            value.recognition,
            value.translation,
            value.collection,
            value.scope,
            value.now,
            0,
        ))?
        .as_bytes(),
    ))
}

fn cancel_hash(grant: &str, request: &str, mask: u32, now: i64) -> Result<String> {
    Ok(sha256_hex(
        serde_json::to_string(&(
            "sigy-task-processing-cancel-v1",
            grant,
            request,
            1,
            mask,
            now,
        ))?
        .as_bytes(),
    ))
}

type GrantRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
    String,
    String,
    String,
    i64,
    i64,
);

fn read_grant(connection: &Connection, id: &str) -> Result<Option<Grant>> {
    let row: Option<GrantRow> = connection.query_row(
        "SELECT request_id, spec_json, recognition_profile, recognition_profile_sha256, translation_profile, translation_profile_sha256, maximum_audio_us, collection_sha256, scope_sha256, grant_sha256, template, amount_micros, created_ms FROM task_processing WHERE task_id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?, row.get(12)?)),
    ).optional()?;
    let Some((
        request_id,
        json,
        recognition,
        recognition_sha256,
        translation,
        translation_sha256,
        maximum,
        collection_sha256,
        scope_sha256,
        digest,
        template,
        amount,
        created_ms,
    )) = row
    else {
        return Ok(None);
    };
    let spec: TaskProcessingSpec =
        serde_json::from_str(&json).map_err(|_| Error::StorageIntegrity)?;
    spec.validate().map_err(|_| Error::StorageIntegrity)?;
    validate_key(&request_id, "task processing request").map_err(|_| Error::StorageIntegrity)?;
    let (task_scope, task_created): (String, i64) = connection.query_row(
        "SELECT scope_sha256, created_ms FROM tasks WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let (collection, collected_ms): (String, i64) = connection.query_row(
        "SELECT grant_sha256, created_ms FROM task_collections WHERE task_id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let expected = grant_hash(Hashed {
        id,
        request: &request_id,
        json: &json,
        recognition: &recognition_sha256,
        translation: translation_sha256.as_deref(),
        collection: &collection_sha256,
        scope: &scope_sha256,
        now: created_ms,
    })?;
    if template != PROCESSING_TEMPLATE
        || amount != 0
        || json.len() > 1024
        || spec.recognition_profile != recognition
        || spec.translation_profile != translation
        || u64::try_from(maximum).ok() != Some(spec.maximum_audio_us())
        || scope_sha256 != task_scope
        || collection_sha256 != collection
        || created_ms < task_created.max(collected_ms)
        || expected != digest
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(Some(Grant {
        request_id,
        spec,
        recognition_sha256,
        translation_sha256,
        collection_sha256,
        scope_sha256,
        digest,
        created_ms,
    }))
}

fn step_mask(connection: &Connection, id: &str) -> Result<u32> {
    Ok(connection.query_row(
        "SELECT coalesce(sum(1 << (ordinal * 2 + (stage = 'translation'))), 0) FROM task_processing_steps WHERE task_id = ?1",
        [id],
        |row| row.get(0),
    )?)
}

/// The cancellation receipt time, after checking its hash and frozen step set.
fn cancellation(connection: &Connection, id: &str, grant: &Grant) -> Result<Option<i64>> {
    let row: Option<(String, u32, u32, u32, String, i64)> = connection.query_row(
        "SELECT request_id, expected_generation, generation, step_mask, receipt_sha256, created_ms FROM task_processing_cancellations WHERE task_id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    ).optional()?;
    let Some((request, expected, generation, mask, digest, created)) = row else {
        return Ok(None);
    };
    validate_key(&request, "task processing cancellation").map_err(|_| Error::StorageIntegrity)?;
    let latest_step: i64 = connection.query_row(
        "SELECT coalesce(max(created_ms), 0) FROM task_processing_steps WHERE task_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    if expected != 1
        || generation != 2
        || created < grant.created_ms
        || created < latest_step
        || mask != step_mask(connection, id)?
        || digest != cancel_hash(&grant.digest, &request, mask, created)?
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(Some(created))
}

type StepRow = (
    u32,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    i64,
    i64,
);

fn stored_steps(connection: &Connection, id: &str) -> Result<Vec<TaskProcessingStep>> {
    let mut statement = connection.prepare(
        "SELECT ordinal, stage, recording_id, decision, reason, input_id, input_revision, job_id, audio_us, created_ms FROM task_processing_steps WHERE task_id = ?1 ORDER BY ordinal, stage LIMIT 5",
    )?;
    let rows = statement
        .query_map([id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<StepRow>>>()?;
    if rows.len() > 4 {
        return Err(Error::StorageIntegrity);
    }
    rows.into_iter()
        .map(
            |(
                ordinal,
                stage,
                recording,
                decision,
                reason,
                input,
                revision,
                job,
                audio,
                created,
            )| {
                let job_state = job
                    .as_deref()
                    .map(|job| audit::job_state(connection, &stage, job))
                    .transpose()?;
                let sharing = job
                    .as_deref()
                    .map(|job| crate::storage::interests::sharing(connection, &stage, job))
                    .transpose()?;
                Ok(TaskProcessingStep {
                    ordinal,
                    stage,
                    recording_id: recording,
                    decision,
                    reason,
                    input_id: input,
                    input_revision: revision,
                    job_id: job,
                    audio_us: u64::try_from(audio).map_err(|_| Error::StorageIntegrity)?,
                    created_ms: created,
                    job_state,
                    sharing,
                })
            },
        )
        .collect()
}

pub(in crate::storage) struct WithdrawalStep {
    pub ordinal: u32,
    pub stage: String,
    pub job_id: String,
    pub created_ms: i64,
}

pub(in crate::storage) struct WithdrawalContext {
    pub grant_sha256: String,
    pub generation: u32,
    pub steps: Vec<WithdrawalStep>,
}

/// Bounded authority facts for the caller's immediate withdrawal transaction.
pub(in crate::storage) fn withdrawal_context(
    connection: &Connection,
    id: &str,
) -> Result<WithdrawalContext> {
    let grant = read_grant(connection, id)?.ok_or(Error::NotFound)?;
    let cancelled = cancellation(connection, id, &grant)?.is_some();
    let mut statement = connection.prepare("SELECT ordinal, stage, job_id, created_ms FROM task_processing_steps WHERE task_id = ?1 AND decision = 'queued' ORDER BY ordinal, stage LIMIT 5")?;
    let steps = statement
        .query_map([id], |row| {
            Ok(WithdrawalStep {
                ordinal: row.get(0)?,
                stage: row.get(1)?,
                job_id: row.get(2)?,
                created_ms: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if steps.len() > 4 {
        return Err(Error::StorageIntegrity);
    }
    Ok(WithdrawalContext {
        grant_sha256: grant.digest,
        generation: if cancelled { 2 } else { 1 },
        steps,
    })
}

impl Store {
    /// Accept one lifetime finite processing grant over this task's collected recordings.
    /// # Errors
    /// Refuses drift, a missing collection or profile, changed replay or a regressed clock.
    pub(crate) fn start_task_processing(
        &mut self,
        id: &str,
        request_id: &str,
        spec: &TaskProcessingSpec,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskProcessingView> {
        validate_key(id, "task ID")?;
        validate_key(request_id, "task processing request")?;
        spec.validate()?;
        if let Some(existing) = self.task_processing(id)? {
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
        let collection = self
            .task_collection(id)?
            .ok_or(Error::InvalidInput("task processing requires a collection"))?;
        let recognition = self
            .recognition_profile(&spec.recognition_profile)?
            .profile_sha256;
        let translation = spec
            .translation_profile
            .as_deref()
            .map(|profile| self.translation_profile(profile))
            .transpose()?
            .map(|profile| profile.profile_sha256);
        let json = serde_json::to_string(spec)?;
        let digest = grant_hash(Hashed {
            id,
            request: request_id,
            json: &json,
            recognition: &recognition,
            translation: translation.as_deref(),
            collection: &collection.grant_sha256,
            scope: &task.scope_sha256,
            now,
        })?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !current_scope(&tx, &task.spec)? {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        if super::audit::checked_task_scope_in(&tx, id)?.1 != task.scope_sha256 {
            return Err(Error::StorageIntegrity);
        }
        if now < latest_task_ms(&tx, id)? || now < collection.created_ms {
            return Err(Error::InvalidInput("task processing clock"));
        }
        tx.execute(
            "INSERT INTO task_processing(task_id, request_id, spec_json, recognition_profile, recognition_profile_sha256, translation_profile, translation_profile_sha256, maximum_audio_us, collection_sha256, scope_sha256, grant_sha256, template, amount_micros, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, ?13)",
            params![id, request_id, json, spec.recognition_profile, recognition, spec.translation_profile, translation, i64::try_from(spec.maximum_audio_us()).map_err(|_| Error::StorageIntegrity)?, collection.grant_sha256, task.scope_sha256, digest, PROCESSING_TEMPLATE, now],
        )?;
        tx.commit()?;
        self.task_processing(id)?.ok_or(Error::StorageIntegrity)
    }

    /// Fence future task processing admissions. Admitted jobs and other interests continue.
    /// # Errors
    /// Refuses changed replay, a stale generation and a regressed receipt clock.
    pub(crate) fn cancel_task_processing(
        &mut self,
        id: &str,
        request_id: &str,
        expected_generation: u32,
        now: i64,
    ) -> Result<TaskProcessingView> {
        validate_key(id, "task ID")?;
        validate_key(request_id, "task processing cancellation")?;
        let view = self.task_processing(id)?.ok_or(Error::NotFound)?;
        if let Some((request, expected)) = self
            .connection
            .query_row(
                "SELECT request_id, expected_generation FROM task_processing_cancellations WHERE task_id = ?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
            )
            .optional()?
        {
            return if request == request_id && expected == expected_generation {
                Ok(view)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        if expected_generation != view.generation {
            return Err(Error::IdempotencyConflict);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if now < latest_task_ms(&tx, id)? {
            return Err(Error::InvalidInput("task processing clock"));
        }
        let mask = step_mask(&tx, id)?;
        let digest = cancel_hash(&view.grant_sha256, request_id, mask, now)?;
        tx.execute(
            "INSERT INTO task_processing_cancellations(task_id, request_id, expected_generation, generation, step_mask, receipt_sha256, created_ms) VALUES (?1, ?2, ?3, 2, ?4, ?5, ?6)",
            params![id, request_id, expected_generation, mask, digest, now],
        )?;
        tx.commit()?;
        self.task_processing(id)?.ok_or(Error::StorageIntegrity)
    }

    /// Inspect the grant, immutable receipts and live shared job states without new work.
    /// # Errors
    /// Refuses an invalid grant, receipt, binding or cancellation history.
    pub fn task_processing(&self, id: &str) -> Result<Option<TaskProcessingView>> {
        validate_key(id, "task ID")?;
        let Some(grant) = read_grant(&self.connection, id)? else {
            return Ok(None);
        };
        let task = super::evidence::evidence_task(self, id)?;
        let cancelled = cancellation(&self.connection, id, &grant)?;
        let steps = stored_steps(&self.connection, id)?;
        audit::validate_steps(&self.connection, id, &grant, &steps)?;
        let charged_audio_us = steps
            .iter()
            .filter(|step| step.stage == "recognition")
            .try_fold(0_u64, |sum, step| sum.checked_add(step.audio_us))
            .ok_or(Error::StorageIntegrity)?;
        if charged_audio_us > grant.spec.maximum_audio_us() {
            return Err(Error::StorageIntegrity);
        }
        let updated_ms = steps
            .iter()
            .map(|step| step.created_ms)
            .chain(cancelled)
            .fold(grant.created_ms, i64::max);
        Ok(Some(TaskProcessingView {
            id: id.into(),
            request_id: grant.request_id,
            template: PROCESSING_TEMPLATE.into(),
            paid_allowance_usd: "0.000000".into(),
            spec: grant.spec,
            recognition_profile_sha256: grant.recognition_sha256,
            translation_profile_sha256: grant.translation_sha256,
            grant_sha256: grant.digest,
            collection_sha256: grant.collection_sha256,
            scope_sha256: grant.scope_sha256,
            created_ms: grant.created_ms,
            updated_ms,
            generation: if cancelled.is_some() { 2 } else { 1 },
            cancelled: cancelled.is_some(),
            scope_current: task.scope_current,
            hold_reason: if crate::storage::withdrawals::fenced(&self.connection, id)? {
                Some("task-interest-withdrawn".into())
            } else if cancelled.is_some() {
                Some("cancelled".into())
            } else if !task.scope_current {
                Some("scope-changed".into())
            } else {
                None
            },
            charged_audio_us,
            steps,
        }))
    }

    /// A bounded page of uncancelled grants with an admitted recording not yet settled.
    /// The service tick visits these; the cursor carries no authority.
    /// # Errors
    /// Refuses malformed cursors or page sizes outside 1 to 16.
    pub(crate) fn pending_task_processing_ids(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<String>, Option<String>)> {
        if let Some(cursor) = after {
            validate_key(cursor, "task processing cursor")?;
        }
        if !(1..=16).contains(&limit) {
            return Err(Error::InvalidInput("task processing page limit"));
        }
        let mut statement = self.connection.prepare(
            "SELECT p.task_id FROM task_processing p WHERE p.task_id > ?1
               AND NOT EXISTS (SELECT 1 FROM task_processing_cancellations c WHERE c.task_id = p.task_id)
               AND NOT EXISTS (SELECT 1 FROM task_interest_withdrawals w WHERE w.task_id = p.task_id)
               AND EXISTS (SELECT 1 FROM task_collection_admissions a WHERE a.task_id = p.task_id
                 AND NOT EXISTS (SELECT 1 FROM task_processing_steps s WHERE s.task_id = a.task_id AND s.ordinal = a.ordinal AND (s.stage = 'translation' OR s.decision = 'skipped')))
             ORDER BY p.task_id LIMIT ?2",
        )?;
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

    pub(crate) fn audit_task_processing(&self) -> Result<()> {
        let mut statement = self
            .connection
            .prepare("SELECT task_id FROM task_processing ORDER BY task_id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            self.task_processing(&id)?.ok_or(Error::StorageIntegrity)?;
        }
        Ok(())
    }
}
