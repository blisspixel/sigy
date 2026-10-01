//! Queue admission and exact monitor usage commit before any scheduler can claim work.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::super::{active_sources, latest_version, paused};
use super::{StepRecord, Store, step_exists, usage, write_step};
use crate::{Error, Result, recognition::LocalAsrRequest, translation::TranslationRequest};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MonitorJobScope<'a> {
    pub monitor_id: &'a str,
    pub policy_version: u32,
    pub action_count: u32,
    pub recording_id: &'a str,
    pub audio_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MonitorJobAdmission {
    pub job_created: bool,
    pub step_created: bool,
}

const REPLAY: MonitorJobAdmission = MonitorJobAdmission {
    job_created: false,
    step_created: false,
};

fn step<'a>(
    scope: &MonitorJobScope<'a>,
    stage: &'static str,
    analysis: &'a str,
    job: &'a str,
) -> StepRecord<'a> {
    StepRecord {
        monitor_id: scope.monitor_id,
        recording_id: scope.recording_id,
        stage,
        policy_version: scope.policy_version,
        outcome: Ok((analysis, job)),
        audio_us: scope.audio_us,
    }
}

fn fresh_scope(
    connection: &Connection,
    scope: &MonitorJobScope<'_>,
    profile: &str,
    hash: &str,
    stage: &str,
    now: i64,
) -> Result<()> {
    let policy = latest_version(connection, scope.monitor_id)?.ok_or(Error::NotFound)?;
    if policy.version != scope.policy_version {
        return Err(Error::Analysis("monitor-policy-changed"));
    }
    let (actions, action_time): (u32, i64) = connection.query_row(
        "SELECT count(*), coalesce(max(created_ms), 0) FROM monitor_actions WHERE monitor_id = ?1",
        [scope.monitor_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if actions != scope.action_count {
        return Err(Error::Analysis("monitor-actions-changed"));
    }
    let last_step: i64 = connection.query_row(
        "SELECT coalesce(max(created_ms), 0) FROM monitor_steps WHERE monitor_id = ?1",
        [scope.monitor_id],
        |row| row.get(0),
    )?;
    if now < policy.created_ms || now < action_time || now < last_step {
        return Err(Error::Analysis("monitor-admission-clock"));
    }
    if paused(connection, scope.monitor_id)? {
        return Err(Error::Analysis("monitor-paused"));
    }
    check_profile(connection, &policy.spec, profile, hash, stage)?;
    let (source, starts, decoded, retained): (String, i64, Option<i64>, String) = connection.query_row("SELECT c.source_revision, c.starts_ms, r.decoded_microseconds, r.storage_state FROM capture_jobs c JOIN recordings r ON r.id = c.id WHERE c.id = ?1 AND c.state = 'completed'", [scope.recording_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional()?.ok_or(Error::NotFound)?;
    if !active_sources(connection, &policy)?.contains(&source) {
        return Err(Error::Analysis("monitor-source-not-followed"));
    }
    let created: i64 = connection.query_row(
        "SELECT created_ms FROM monitors WHERE id = ?1",
        [scope.monitor_id],
        |row| row.get(0),
    )?;
    let owned: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM monitor_capture_admissions WHERE monitor_id = ?1 AND occurrence_id = ?2)", params![scope.monitor_id, scope.recording_id], |row| row.get(0))?;
    if starts < created && !owned {
        return Err(Error::Analysis("monitor-recording-ineligible"));
    }
    if stage == "recognition" {
        if retained != "retained"
            || decoded.and_then(|value| u64::try_from(value).ok()) != Some(scope.audio_us)
            || scope.audio_us == 0
        {
            return Err(Error::InvalidInput("monitor recording audio"));
        }
        let (day, total) = usage(connection, scope.monitor_id, now)?;
        if scope.audio_us
            > (u64::from(policy.spec.daily_audio_seconds) * 1_000_000).saturating_sub(day)
        {
            return Err(Error::Analysis("monitor-daily-cap"));
        }
        if scope.audio_us > (policy.spec.total_audio_seconds * 1_000_000).saturating_sub(total) {
            return Err(Error::Analysis("monitor-total-cap"));
        }
    } else if scope.audio_us != 0 {
        return Err(Error::InvalidInput("translation audio charge"));
    }
    Ok(())
}

fn check_profile(
    connection: &Connection,
    policy: &crate::monitor::MonitorSpec,
    profile: &str,
    hash: &str,
    stage: &str,
) -> Result<()> {
    let (configured, sql) = if stage == "recognition" {
        (
            &policy.recognition_profile,
            "SELECT profile_sha256 FROM recognition_profiles WHERE id = ?1",
        )
    } else {
        (
            &policy.translation_profile,
            "SELECT profile_sha256 FROM translation_profiles WHERE id = ?1",
        )
    };
    let stored: Option<String> = connection
        .query_row(sql, [profile], |row| row.get(0))
        .optional()?;
    if configured.as_deref() != Some(profile) || stored.as_deref() != Some(hash) {
        return Err(Error::Analysis("monitor-profile-changed"));
    }
    Ok(())
}

impl Store {
    /// Commit canonical recognition admission and its exact monitor charge together.
    /// # Errors
    /// Refuses changed replay, stale policy/actions, pause, wrong input/profile or caps.
    pub(crate) fn enqueue_monitor_recognition(
        &mut self,
        scope: &MonitorJobScope<'_>,
        request: &LocalAsrRequest,
        now: i64,
    ) -> Result<MonitorJobAdmission> {
        request.validate()?;
        let record = step(scope, "recognition", &request.analysis_id, &request.id);
        if step_exists(&self.connection, &record)? {
            if self.local_asr_job(&request.id)?.request != *request {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(REPLAY);
        }
        let recording: String = self.connection.query_row("SELECT recording_id FROM analysis_inputs WHERE id = ?1 AND revision = ?2 AND state = 'published'", params![request.analysis_id, request.analysis_revision], |row| row.get(0)).optional()?.ok_or(Error::NotFound)?;
        if recording != scope.recording_id {
            return Err(Error::IdempotencyConflict);
        }
        if now < 0 {
            return Err(Error::Analysis("monitor-admission-clock"));
        }
        let prepared = self.prepare_local_asr_job(request, now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        fresh_scope(
            &tx,
            scope,
            &request.profile,
            &request.profile_sha256,
            "recognition",
            now,
        )?;
        let job_created = Self::enqueue_local_asr_in(&tx, request, prepared.as_ref(), now)?;
        let step_created = write_step(&tx, &record, now)?;
        tx.commit()?;
        Ok(MonitorJobAdmission {
            job_created,
            step_created,
        })
    }

    /// Commit canonical translation admission and its zero-audio monitor step together.
    /// # Errors
    /// Refuses changed replay, stale authority, unsupported text or unrelated recognition.
    pub(crate) fn enqueue_monitor_translation(
        &mut self,
        scope: &MonitorJobScope<'_>,
        request: &TranslationRequest,
        now: i64,
    ) -> Result<MonitorJobAdmission> {
        request.validate()?;
        let record = step(scope, "translation", &request.transcript_id, &request.id);
        if step_exists(&self.connection, &record)? {
            if self.translation_job(&request.id)?.request != *request {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(REPLAY);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        fresh_scope(
            &tx,
            scope,
            &request.profile,
            &request.profile_sha256,
            "translation",
            now,
        )?;
        let supported: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM transcripts t JOIN monitor_steps s ON s.monitor_id = ?1 AND s.recording_id = t.recording_id AND s.stage = 'recognition' AND s.decision = 'queued' AND s.analysis_id = t.id AND t.job_id = s.job_id JOIN analysis_jobs j ON j.id = s.job_id AND j.state = 'succeeded' WHERE t.id = ?2 AND t.revision = ?3 AND t.recording_id = ?4 AND t.kind = 'recognition' AND t.outcome = 'text')", params![scope.monitor_id, request.transcript_id, request.transcript_revision, scope.recording_id], |row| row.get(0))?;
        if !supported {
            return Err(Error::Analysis("monitor-translation-unsupported"));
        }
        let job_created = crate::storage::translations::enqueue_translation_in(&tx, request, now)?;
        let step_created = write_step(&tx, &record, now)?;
        tx.commit()?;
        Ok(MonitorJobAdmission {
            job_created,
            step_created,
        })
    }
}
