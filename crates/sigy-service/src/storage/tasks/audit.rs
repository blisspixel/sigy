//! Reopen checks bind immutable scope and checkpoints to original catalog facts.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};

use super::{Store, scope_hash, validate_key};
use crate::{
    Error, Result,
    monitor::{
        MATCH_PAGE, MAX_SCANNED_TRANSCRIPTS, MAX_SOURCES, MAX_WINDOW_CAPTURES, MonitorSpec,
        Proposal, decide,
    },
    task::{MAX_CHECKPOINTS, MAX_TASKS, TASK_TEMPLATE, TaskCheckpoint, TaskSpec},
};

type ScopeRow = (
    String,
    String,
    String,
    i64,
    String,
    u32,
    u32,
    i64,
    i64,
    String,
    i64,
);

pub(super) struct CheckpointContext {
    pub spec: TaskSpec,
    pub policy: MonitorSpec,
    pub sources: Vec<String>,
    pub paused: bool,
}

pub(super) type CheckedScope = (TaskSpec, String, String, i64);

impl Store {
    pub(super) fn checked_task_scope(&self, id: &str) -> Result<CheckedScope> {
        checked_task_scope_in(&self.connection, id)
    }

    pub(super) fn task_checkpoint_context(&self, spec: &TaskSpec) -> Result<CheckpointContext> {
        let policy = self
            .monitor_version(&spec.monitor_id, spec.monitor_version)?
            .spec;
        let mut sources = policy.sources.clone();
        let mut statement = self.connection.prepare(
            "SELECT proposal_json FROM monitor_actions WHERE monitor_id = ?1 AND policy_version = ?2 AND ordinal <= ?3 AND decision = 'applied' AND kind IN ('add_source', 'remove_source') ORDER BY ordinal",
        )?;
        let actions = statement
            .query_map(
                params![spec.monitor_id, spec.monitor_version, spec.monitor_actions],
                |row| row.get::<_, String>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for json in actions {
            let proposal: Proposal =
                serde_json::from_str(&json).map_err(|_| Error::StorageIntegrity)?;
            if !matches!(
                proposal,
                Proposal::AddSource { .. } | Proposal::RemoveSource { .. }
            ) || !decide(&policy, &sources, &proposal).0
            {
                return Err(Error::StorageIntegrity);
            }
            match proposal {
                Proposal::AddSource { source } if !sources.contains(&source) => {
                    sources.push(source);
                }
                Proposal::RemoveSource { source } => sources.retain(|value| value != &source),
                _ => {}
            }
        }
        let kind: Option<String> = self.connection.query_row(
            "SELECT kind FROM monitor_actions WHERE monitor_id = ?1 AND ordinal <= ?2 AND decision = 'applied' AND kind IN ('pause', 'resume') ORDER BY ordinal DESC LIMIT 1",
            params![spec.monitor_id, spec.monitor_actions], |row| row.get(0),
        ).optional()?;
        Ok(CheckpointContext {
            spec: spec.clone(),
            policy,
            sources,
            paused: kind.as_deref() == Some("pause"),
        })
    }

    pub(super) fn validate_task_checkpoint(
        &self,
        spec: &TaskSpec,
        checkpoint: &TaskCheckpoint,
    ) -> Result<()> {
        let context = self.task_checkpoint_context(spec)?;
        self.validate_checkpoint_context(&context, checkpoint)
    }

    pub(super) fn validate_checkpoint_context(
        &self,
        context: &CheckpointContext,
        checkpoint: &TaskCheckpoint,
    ) -> Result<()> {
        let spec = &context.spec;
        validate_key(&checkpoint.task_id, "task ID").map_err(|_| Error::StorageIntegrity)?;
        validate_key(&checkpoint.request_id, "task request")
            .map_err(|_| Error::StorageIntegrity)?;
        let coverage = &checkpoint.coverage;
        let sources = &context.sources;
        if coverage.id != spec.monitor_id
            || coverage.version != spec.monitor_version
            || coverage.from_ms != spec.from_ms
            || coverage.to_ms != spec.to_ms
            || coverage.daily_audio_seconds != context.policy.daily_audio_seconds
            || coverage
                .sources
                .iter()
                .map(|source| &source.source)
                .ne(sources.iter())
            || sources.len() > MAX_SOURCES
            || checkpoint.monitor_paused != context.paused
            || checkpoint.window_elapsed != (checkpoint.observed_ms >= spec.to_ms)
            || !(1..=MAX_CHECKPOINTS).contains(&checkpoint.ordinal)
            || checkpoint.transcripts_scanned > MAX_SCANNED_TRANSCRIPTS
            || checkpoint.citations.len() > MATCH_PAGE
        {
            return Err(Error::StorageIntegrity);
        }
        let mut pinned = 0_u64;
        for source in &coverage.sources {
            let reasons = source
                .untranslated_reasons
                .iter()
                .try_fold(0_u64, |sum, (_, count)| sum.checked_add(u64::from(*count)))
                .ok_or(Error::StorageIntegrity)?;
            if source.captures as usize > MAX_WINDOW_CAPTURES
                || source.published > source.captures
                || source.pinned > source.published
                || u64::from(source.transcribed) + u64::from(source.no_text)
                    > u64::from(source.pinned)
                || source
                    .transcribed_us
                    .checked_add(source.no_text_us)
                    .is_none_or(|value| value > source.recorded_us)
                || reasons != u64::from(source.untranslated_cues)
                || (source.truncated && !checkpoint.more)
            {
                return Err(Error::StorageIntegrity);
            }
            pinned += u64::from(source.pinned);
        }
        if u64::from(checkpoint.transcripts_scanned) > pinned {
            return Err(Error::StorageIntegrity);
        }
        let mut unique = BTreeSet::new();
        let mut transcripts = BTreeSet::new();
        for citation in &checkpoint.citations {
            if !sources.contains(&citation.source)
                || !unique.insert(serde_json::to_string(citation)?)
            {
                return Err(Error::StorageIntegrity);
            }
            transcripts.insert((&citation.transcript_id, citation.transcript_revision));
            self.validate_task_citation(context, citation)?;
        }
        if transcripts.len() > checkpoint.transcripts_scanned as usize {
            return Err(Error::StorageIntegrity);
        }
        self.validate_task_schedules(context, checkpoint)
    }

    pub(crate) fn audit_tasks(&self) -> Result<()> {
        let count: u32 = self
            .connection
            .query_row("SELECT count(*) FROM tasks", [], |row| row.get(0))?;
        if count > MAX_TASKS {
            return Err(Error::StorageIntegrity);
        }
        let mut statement = self
            .connection
            .prepare("SELECT id FROM tasks ORDER BY id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            validate_key(&id, "task ID").map_err(|_| Error::StorageIntegrity)?;
            let scope = self.checked_task_scope(&id)?;
            let context = self.task_checkpoint_context(&scope.0)?;
            let mut last_time = scope.3;
            let mut checkpoints = self.connection.prepare(
                "SELECT ordinal FROM task_checkpoints WHERE task_id = ?1 ORDER BY ordinal",
            )?;
            let ordinals = checkpoints
                .query_map([&id], |row| row.get::<_, u32>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if ordinals.len() > MAX_CHECKPOINTS as usize {
                return Err(Error::StorageIntegrity);
            }
            for (index, ordinal) in ordinals.iter().enumerate() {
                if *ordinal as usize != index + 1 {
                    return Err(Error::StorageIntegrity);
                }
                let checkpoint =
                    self.task_checkpoint_in_context(&id, *ordinal, &scope, &context)?;
                if checkpoint.observed_ms < last_time {
                    return Err(Error::StorageIntegrity);
                }
                last_time = checkpoint.observed_ms;
            }
        }
        Ok(())
    }
}

pub(super) fn checked_task_scope_in(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<CheckedScope> {
    let row: ScopeRow = connection.query_row(
            "SELECT spec_json, scope_sha256, monitor_spec_sha256, created_ms, monitor_id, monitor_version, monitor_actions, from_ms, to_ms, template, amount_micros FROM tasks WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?)),
        ).optional()?.ok_or(Error::NotFound)?;
    let (
        json,
        digest,
        monitor_digest,
        created,
        monitor,
        version,
        actions,
        from,
        to,
        template,
        cost,
    ) = row;
    let spec: TaskSpec = serde_json::from_str(&json).map_err(|_| Error::StorageIntegrity)?;
    spec.validate().map_err(|_| Error::StorageIntegrity)?;
    let policy = crate::storage::monitors::monitor_version_in(
        connection,
        &spec.monitor_id,
        spec.monitor_version,
    )?;
    let invalid_actions: bool = connection.query_row(
            "SELECT ?2 != (SELECT count(*) FROM monitor_actions WHERE monitor_id = ?1 AND ordinal <= ?2) OR EXISTS (SELECT 1 FROM monitor_actions WHERE monitor_id = ?1 AND ordinal <= ?2 AND (policy_version > ?3 OR created_ms > ?4))",
            params![monitor, actions, version, created], |row| row.get(0),
        )?;
    if json.len() > 8192
        || spec.monitor_id != monitor
        || spec.monitor_version != version
        || spec.monitor_actions != actions
        || spec.from_ms != from
        || spec.to_ms != to
        || template != TASK_TEMPLATE
        || cost != 0
        || created < policy.created_ms
        || policy.spec_sha256 != monitor_digest
        || scope_hash(&json, &monitor_digest) != digest
        || invalid_actions
    {
        return Err(Error::StorageIntegrity);
    }
    Ok((spec, digest, monitor_digest, created))
}
