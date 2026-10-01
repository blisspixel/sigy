//! Compare-and-append checkpoints produced from canonical service observations.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::audit::{CheckedScope, CheckpointContext};
use super::{Store, checkpoint_hash, scope_current, validate_key};
use crate::{
    Error, Result,
    task::{MAX_CHECKPOINT_BYTES, MAX_CHECKPOINTS, TaskCheckpoint, TaskCitation},
};

impl Store {
    /// Freeze current observations without publishing findings or admitting work.
    /// # Errors
    /// Refuses a changed scope, stale ordinal, changed replay or exhausted bound.
    pub(crate) fn checkpoint_task(
        &mut self,
        id: &str,
        request_id: &str,
        expected_checkpoint: u32,
        now: i64,
    ) -> Result<TaskCheckpoint> {
        validate_key(id, "task ID")?;
        validate_key(request_id, "task checkpoint request")?;
        if let Some((ordinal, expected)) = self
            .connection
            .query_row(
                "SELECT ordinal, expected_checkpoint FROM task_checkpoints WHERE task_id = ?1 AND request_id = ?2",
                params![id, request_id],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?)),
            )
            .optional()?
        {
            return if expected == expected_checkpoint {
                self.task_checkpoint(id, ordinal)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        let task = self.task(id)?;
        if task.checkpoint != expected_checkpoint {
            return Err(Error::IdempotencyConflict);
        }
        if !scope_current(self, &task.spec)? {
            return Err(Error::InvalidInput("task monitor scope changed"));
        }
        if task.checkpoint >= MAX_CHECKPOINTS {
            return Err(Error::InvalidInput("task checkpoint capacity"));
        }
        let previous_ms = task
            .latest_checkpoint
            .as_ref()
            .map_or(task.created_ms, |checkpoint| checkpoint.observed_ms);
        if now < previous_ms {
            return Err(Error::InvalidInput("task observation time"));
        }
        let scope = &task.spec;
        let coverage = self.monitor_coverage(&scope.monitor_id, scope.from_ms, scope.to_ms)?;
        let matches = self.monitor_matches(&scope.monitor_id, scope.from_ms, scope.to_ms)?;
        let mut seen = BTreeSet::new();
        let mut citations = Vec::new();
        for passage in matches.matches {
            let citation = TaskCitation {
                source: passage.source,
                recording_id: passage.recording_id,
                transcript_id: passage.transcript_id,
                transcript_revision: passage.transcript_revision,
                translation_revision: passage.translation_revision,
                cue_ordinal: passage.cue_ordinal,
                start_us: passage.start_us,
                end_us: passage.end_us,
            };
            if seen.insert(serde_json::to_string(&citation)?) {
                citations.push(citation);
            }
        }
        let checkpoint = TaskCheckpoint {
            task_id: id.to_owned(),
            ordinal: task.checkpoint + 1,
            request_id: request_id.to_owned(),
            observed_ms: now,
            coverage,
            citations,
            transcripts_scanned: matches.transcripts_scanned,
            more: matches.more,
            window_elapsed: now >= scope.to_ms,
            monitor_paused: self.monitor(&scope.monitor_id)?.paused,
        };
        self.validate_task_checkpoint(&task.spec, &checkpoint)?;
        let json = serde_json::to_string(&checkpoint)?;
        if json.len() > MAX_CHECKPOINT_BYTES {
            return Err(Error::InvalidInput("task checkpoint size"));
        }
        let digest = checkpoint_hash(&json, &task.scope_sha256);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO task_checkpoints(task_id, ordinal, request_id, expected_checkpoint, observed_ms, payload_json, payload_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, checkpoint.ordinal, request_id, expected_checkpoint, now, json, digest],
        )?;
        tx.commit()?;
        Ok(checkpoint)
    }

    /// Read one frozen checkpoint, even after later policy or text changes.
    /// # Errors
    /// Refuses missing observations or inconsistent stored payloads.
    pub fn task_checkpoint(&self, id: &str, ordinal: u32) -> Result<TaskCheckpoint> {
        validate_key(id, "task ID")?;
        if !(1..=MAX_CHECKPOINTS).contains(&ordinal) {
            return Err(Error::InvalidInput("task checkpoint ordinal"));
        }
        let scope = self.checked_task_scope(id)?;
        let context = self.task_checkpoint_context(&scope.0)?;
        self.task_checkpoint_in_context(id, ordinal, &scope, &context)
    }

    pub(super) fn task_checkpoint_in_context(
        &self,
        id: &str,
        ordinal: u32,
        scope: &CheckedScope,
        context: &CheckpointContext,
    ) -> Result<TaskCheckpoint> {
        let (json, digest, request, expected, observed): (String, String, String, u32, i64) = self
            .connection
            .query_row(
                "SELECT payload_json, payload_sha256, request_id, expected_checkpoint, observed_ms FROM task_checkpoints WHERE task_id = ?1 AND ordinal = ?2",
                params![id, ordinal],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let checkpoint: TaskCheckpoint =
            serde_json::from_str(&json).map_err(|_| Error::StorageIntegrity)?;
        if json.len() > MAX_CHECKPOINT_BYTES
            || checkpoint_hash(&json, &scope.1) != digest
            || checkpoint.task_id != id
            || checkpoint.ordinal != ordinal
            || checkpoint.request_id != request
            || expected != ordinal - 1
            || checkpoint.observed_ms != observed
            || observed < scope.3
        {
            return Err(Error::StorageIntegrity);
        }
        self.validate_checkpoint_context(context, &checkpoint)?;
        Ok(checkpoint)
    }
}
