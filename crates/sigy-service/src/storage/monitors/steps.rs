//! Facts for the monitor stage controller and its append-only step rows.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, active_sources, latest_version, paused};
use crate::{
    Error, Result,
    monitor::{
        MonitorProcessing,
        pipeline::{Candidate, DAY_MS, Facts, Recognized},
    },
    storage::validate_key,
};

/// Most candidates or finished recognitions read for one monitor in one pass.
const FACT_ROWS: u32 = 16;

mod atomic;
pub(crate) use atomic::{MonitorJobAdmission, MonitorJobScope};

/// One step to record. Queued steps carry the pin and job they queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepRecord<'a> {
    pub monitor_id: &'a str,
    pub recording_id: &'a str,
    pub stage: &'static str,
    pub policy_version: u32,
    /// `Ok((analysis_id, job_id))` when queued, `Err(reason)` when skipped.
    pub outcome: std::result::Result<(&'a str, &'a str), &'a str>,
    pub audio_us: u64,
}

fn micros(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::StorageIntegrity)
}

fn today(now_ms: i64) -> i64 {
    now_ms.div_euclid(DAY_MS)
}

fn usage(connection: &Connection, id: &str, now_ms: i64) -> Result<(u64, u64)> {
    let (day, total): (i64, i64) = connection.query_row(
        "SELECT coalesce(sum(CASE WHEN charged_day = ?2 THEN audio_us ELSE 0 END), 0), coalesce(sum(audio_us), 0) FROM monitor_steps WHERE monitor_id = ?1 AND stage = 'recognition'",
        params![id, today(now_ms)],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((micros(day)?, micros(total)?))
}

fn candidates(
    connection: &Connection,
    id: &str,
    sources: &[String],
    since_ms: i64,
) -> Result<Vec<Candidate>> {
    let mut statement = connection.prepare(
        "SELECT c.id, r.decoded_microseconds FROM capture_jobs c JOIN recordings r ON r.id = c.id
         WHERE c.source_revision IN (SELECT value FROM json_each(?2)) AND c.state = 'completed'
           AND r.storage_state = 'retained' AND r.decoded_microseconds IS NOT NULL
           AND (c.starts_ms >= ?3 OR EXISTS (SELECT 1 FROM monitor_capture_admissions owned WHERE owned.occurrence_id = c.id AND owned.monitor_id = ?1))
           AND NOT EXISTS (SELECT 1 FROM monitor_steps s WHERE s.monitor_id = ?1 AND s.recording_id = c.id AND s.stage = 'recognition')
         ORDER BY c.starts_ms, c.id LIMIT ?4",
    )?;
    let rows = statement
        .query_map(
            params![id, serde_json::to_string(sources)?, since_ms, FACT_ROWS],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(recording_id, audio)| {
            Ok(Candidate {
                recording_id,
                audio_us: micros(audio)?,
            })
        })
        .collect()
}

fn recognized(connection: &Connection, id: &str) -> Result<Vec<Recognized>> {
    let mut statement = connection.prepare(
        "SELECT s.recording_id, s.analysis_id, j.state, t.revision, t.outcome FROM monitor_steps s
         JOIN analysis_jobs j ON j.id = s.job_id LEFT JOIN transcripts t ON t.job_id = s.job_id
         WHERE s.monitor_id = ?1 AND s.stage = 'recognition' AND s.decision = 'queued'
           AND j.state IN ('succeeded', 'failed', 'cancelled', 'interrupted')
           AND NOT EXISTS (SELECT 1 FROM monitor_steps x WHERE x.monitor_id = s.monitor_id AND x.recording_id = s.recording_id AND x.stage = 'translation')
         ORDER BY s.created_ms, s.recording_id LIMIT ?2",
    )?;
    Ok(statement
        .query_map(params![id, FACT_ROWS], |row| {
            let revision: Option<i64> = row.get(3)?;
            let outcome: Option<String> = row.get(4)?;
            Ok(Recognized {
                recording_id: row.get(0)?,
                analysis_id: row.get(1)?,
                job_state: row.get(2)?,
                transcript: revision.zip(outcome),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

impl Store {
    /// Everything the monitor planner needs, read in one place.
    /// # Errors
    /// Refuses a missing monitor or invalid stored rows.
    pub(crate) fn monitor_facts(&self, id: &str, now_ms: i64) -> Result<Facts> {
        validate_key(id, "monitor ID")?;
        let version = latest_version(&self.connection, id)?.ok_or(Error::NotFound)?;
        let created_ms: i64 = self.connection.query_row(
            "SELECT created_ms FROM monitors WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let sources = active_sources(&self.connection, &version)?;
        let (used_today_us, used_total_us) = usage(&self.connection, id, now_ms)?;
        let spec = &version.spec;
        Ok(Facts {
            monitor_id: id.to_owned(),
            version: version.version,
            action_count: self.connection.query_row(
                "SELECT count(*) FROM monitor_actions WHERE monitor_id = ?1",
                [id],
                |row| row.get(0),
            )?,
            paused: paused(&self.connection, id)?,
            daily_cap_us: u64::from(spec.daily_audio_seconds) * 1_000_000,
            total_cap_us: spec.total_audio_seconds * 1_000_000,
            used_today_us,
            used_total_us,
            recognition_profile: spec.recognition_profile.clone(),
            translation_profile: spec.translation_profile.clone(),
            candidates: candidates(&self.connection, id, &sources, created_ms)?,
            recognized: recognized(&self.connection, id)?,
        })
    }

    /// Record one refusal. Queued work is admitted only by the atomic stage methods.
    /// # Errors
    /// Refuses a step that would exceed a cap, a stale policy version or a changed replay.
    pub(crate) fn record_monitor_skip(&mut self, step: &StepRecord<'_>, now_ms: i64) -> Result<()> {
        if step.outcome.is_ok() {
            return Err(Error::InvalidInput(
                "queued monitor step requires atomic admission",
            ));
        }
        write_step(&self.connection, step, now_ms)?;
        Ok(())
    }

    /// Historical fixtures may stage queued rows directly; production cannot.
    #[cfg(test)]
    pub(crate) fn record_monitor_step(&mut self, step: &StepRecord<'_>, now: i64) -> Result<()> {
        write_step(&self.connection, step, now)?;
        Ok(())
    }

    /// Usage and step counts for one monitor.
    /// # Errors
    /// Fails on a database error.
    pub fn monitor_processing(&self, id: &str, now_ms: i64) -> Result<MonitorProcessing> {
        validate_key(id, "monitor ID")?;
        let (used_today_us, used_total_us) = usage(&self.connection, id, now_ms)?;
        let (recognition_queued, translation_queued): (u32, u32) = self.connection.query_row(
            "SELECT count(*) FILTER (WHERE stage = 'recognition' AND decision = 'queued'), count(*) FILTER (WHERE stage = 'translation' AND decision = 'queued') FROM monitor_steps WHERE monitor_id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut statement = self.connection.prepare(
            "SELECT stage, reason, count(*) FROM monitor_steps WHERE monitor_id = ?1 AND decision = 'skipped' GROUP BY stage, reason ORDER BY stage, reason LIMIT 64",
        )?;
        let skipped = statement
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(MonitorProcessing {
            used_today_us,
            used_total_us,
            recognition_queued,
            translation_queued,
            skipped,
        })
    }
}

type StepReplay = (
    u32,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);

fn step_exists(connection: &Connection, step: &StepRecord<'_>) -> Result<bool> {
    validate_key(step.monitor_id, "monitor ID")?;
    validate_key(step.recording_id, "monitor recording ID")?;
    let audio = i64::try_from(step.audio_us).map_err(|_| Error::InvalidInput("audio"))?;
    let (decision, reason, analysis, job) = match step.outcome {
        Ok((analysis, job)) => ("queued", None, Some(analysis), Some(job)),
        Err(reason) => ("skipped", Some(reason), None, None),
    };
    let existing: Option<StepReplay> = connection.query_row(
        "SELECT policy_version, decision, reason, analysis_id, job_id, audio_us FROM monitor_steps WHERE monitor_id = ?1 AND recording_id = ?2 AND stage = ?3",
        params![step.monitor_id, step.recording_id, step.stage], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    ).optional()?;
    match existing {
        None => Ok(false),
        Some(row)
            if row
                == (
                    step.policy_version,
                    decision.into(),
                    reason.map(str::to_owned),
                    analysis.map(str::to_owned),
                    job.map(str::to_owned),
                    audio,
                ) =>
        {
            Ok(true)
        }
        Some(_) => Err(Error::IdempotencyConflict),
    }
}

fn write_step(connection: &Connection, step: &StepRecord<'_>, now: i64) -> Result<bool> {
    if step_exists(connection, step)? {
        return Ok(false);
    }
    if now < 0 {
        return Err(Error::InvalidInput("monitor step clock"));
    }
    let (decision, reason, analysis, job) = match step.outcome {
        Ok((analysis, job)) => ("queued", None, Some(analysis), Some(job)),
        Err(reason) => ("skipped", Some(reason), None, None),
    };
    connection.execute(
        "INSERT INTO monitor_steps(monitor_id, recording_id, stage, policy_version, decision, reason, analysis_id, job_id, audio_us, charged_day, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![step.monitor_id, step.recording_id, step.stage, step.policy_version, decision, reason, analysis, job, i64::try_from(step.audio_us).map_err(|_| Error::InvalidInput("audio"))?, today(now), now],
    )?;
    if let Some(job) = job {
        // The monitor's interest commits with its receipt and charge, never separately.
        crate::storage::interests::record(
            connection,
            step.stage,
            job,
            crate::storage::interests::Authority::Monitor(step.monitor_id),
            now,
        )?;
    }
    Ok(true)
}
