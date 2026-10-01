//! Opt-in schedule ownership and conservative capture reservations on the shared scheduler.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, active_sources, latest_version};
use crate::{
    Error, Result,
    monitor::{
        MonitorCaptureAdmission, MonitorCaptureBounds, MonitorCaptureUsage, MonitorScheduleOwner,
        MonitorVersion,
    },
    storage::schedules::{ScheduleDraft, ScheduleOccurrence, ScheduleRule},
};

const DAY_MS: i64 = 86_400_000;
mod audit;

pub(crate) fn owner(connection: &Connection, rule: &str) -> Result<Option<MonitorScheduleOwner>> {
    Ok(connection
        .query_row(
            "SELECT monitor_id, created_version FROM monitor_capture_rules WHERE rule_id = ?1",
            [rule],
            |row| {
                Ok(MonitorScheduleOwner {
                    monitor_id: row.get(0)?,
                    version: row.get(1)?,
                })
            },
        )
        .optional()?)
}

pub(crate) fn attach(
    connection: &Connection,
    draft: &ScheduleDraft,
    owner: &MonitorScheduleOwner,
) -> Result<()> {
    let policy = latest_version(connection, &owner.monitor_id)?.ok_or(Error::NotFound)?;
    if policy.version != owner.version {
        return Err(Error::InvalidInput("monitor changed since that version"));
    }
    let bounds = policy
        .spec
        .capture
        .as_ref()
        .ok_or(Error::InvalidInput("monitor capture is disabled"))?;
    if !approved_source(&policy, &draft.source_revision)
        || !active_sources(connection, &policy)?.contains(&draft.source_revision)
    {
        return Err(Error::InvalidInput(
            "monitor capture source is not followed",
        ));
    }
    if draft.duration_seconds.cast_unsigned() > bounds.total_seconds
        || draft.maximum_bytes.cast_unsigned() > bounds.total_bytes
    {
        return Err(Error::InvalidInput(
            "schedule exceeds monitor capture bounds",
        ));
    }
    connection.execute("INSERT INTO monitor_capture_rules(rule_id, monitor_id, created_version) VALUES (?1, ?2, ?3)", params![draft.id, owner.monitor_id, owner.version])?;
    Ok(())
}

fn days(start_ms: i64, end_ms: i64) -> Result<Vec<(i64, u64)>> {
    if start_ms < 0 || end_ms <= start_ms || start_ms % 1000 != 0 || end_ms % 1000 != 0 {
        return Err(Error::StorageIntegrity);
    }
    let mut cursor = start_ms;
    let mut portions = Vec::new();
    while cursor < end_ms {
        let day = cursor / DAY_MS;
        let midnight = day
            .checked_add(1)
            .and_then(|value| value.checked_mul(DAY_MS))
            .unwrap_or(i64::MAX);
        let end = end_ms.min(midnight);
        portions.push((day, (end - cursor).cast_unsigned() / 1000));
        cursor = end;
        if portions.len() > 2 {
            return Err(Error::StorageIntegrity);
        }
    }
    Ok(portions)
}

fn used_day(connection: &Connection, monitor: &str, day: i64) -> Result<u64> {
    let seconds: i64 = connection.query_row("SELECT COALESCE(SUM(d.seconds), 0) FROM monitor_capture_days d JOIN monitor_capture_admissions a ON a.occurrence_id = d.occurrence_id WHERE a.monitor_id = ?1 AND d.utc_day = ?2", params![monitor, day], |row| row.get(0))?;
    u64::try_from(seconds).map_err(|_| Error::StorageIntegrity)
}

fn used_total(connection: &Connection, monitor: &str) -> Result<(u64, u64, u32)> {
    let (seconds, bytes, count): (i64, i64, u32) = connection.query_row("SELECT COALESCE(SUM(planned_seconds), 0), COALESCE(SUM(maximum_bytes), 0), COUNT(*) FROM monitor_capture_admissions WHERE monitor_id = ?1", [monitor], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    Ok((
        u64::try_from(seconds).map_err(|_| Error::StorageIntegrity)?,
        u64::try_from(bytes).map_err(|_| Error::StorageIntegrity)?,
        count,
    ))
}

fn refusal(
    connection: &Connection,
    policy: &MonitorVersion,
    occurrence: &ScheduleOccurrence,
    bounds: Option<&MonitorCaptureBounds>,
    source: &str,
) -> Result<Option<&'static str>> {
    let Some(bounds) = bounds else {
        return Ok(Some("capture-disabled"));
    };
    if !approved_source(policy, source)
        || !active_sources(connection, policy)?
            .iter()
            .any(|followed| followed == source)
    {
        return Ok(Some("source-not-followed"));
    }
    let (seconds, bytes, _) = used_total(connection, &policy.monitor_id)?;
    if occurrence.duration_seconds.cast_unsigned() > bounds.total_seconds.saturating_sub(seconds) {
        return Ok(Some("total-cap"));
    }
    if occurrence.maximum_bytes.cast_unsigned() > bounds.total_bytes.saturating_sub(bytes) {
        return Ok(Some("byte-cap"));
    }
    for (day, planned) in days(
        occurrence.start_ms.ok_or(Error::StorageIntegrity)?,
        occurrence.end_ms.ok_or(Error::StorageIntegrity)?,
    )? {
        if planned
            > u64::from(bounds.daily_seconds).saturating_sub(used_day(
                connection,
                &policy.monitor_id,
                day,
            )?)
        {
            return Ok(Some("daily-cap"));
        }
    }
    Ok(None)
}

fn approved_source(policy: &MonitorVersion, source: &str) -> bool {
    policy
        .spec
        .sources
        .iter()
        .chain(&policy.spec.candidate_sources)
        .any(|approved| approved == source)
}

/// Called inside the occurrence admission transaction. Pause remains processing-only.
pub(crate) fn check(
    connection: &Connection,
    rule: &ScheduleRule,
    occurrence: &ScheduleOccurrence,
    now: i64,
) -> Result<bool> {
    let Some(owner) = &rule.monitor_owner else {
        return Ok(true);
    };
    let policy = latest_version(connection, &owner.monitor_id)?.ok_or(Error::StorageIntegrity)?;
    if let Some(reason) = refusal(
        connection,
        &policy,
        occurrence,
        policy.spec.capture.as_ref(),
        &rule.source_revision,
    )? {
        connection.execute("INSERT OR IGNORE INTO monitor_capture_refusals(occurrence_id, rule_id, rule_revision, source_revision, start_ms, end_ms, planned_seconds, maximum_bytes, monitor_id, policy_version, reason, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)", params![occurrence.id, occurrence.rule_id, occurrence.rule_revision, rule.source_revision, occurrence.start_ms, occurrence.end_ms, occurrence.duration_seconds, occurrence.maximum_bytes, owner.monitor_id, policy.version, reason, now])?;
        return Ok(false);
    }
    Ok(true)
}

/// Called only after the shared DVR admission and occurrence transition, in that transaction.
pub(crate) fn reserve(
    connection: &Connection,
    rule: &ScheduleRule,
    occurrence: &ScheduleOccurrence,
    now: i64,
) -> Result<()> {
    let Some(owner) = &rule.monitor_owner else {
        return Ok(());
    };
    let policy = latest_version(connection, &owner.monitor_id)?.ok_or(Error::StorageIntegrity)?;
    let bounds = policy
        .spec
        .capture
        .as_ref()
        .ok_or(Error::StorageIntegrity)?;
    let action: u32 = connection.query_row(
        "SELECT COALESCE(MAX(ordinal), 0) FROM monitor_actions WHERE monitor_id = ?1",
        [&owner.monitor_id],
        |row| row.get(0),
    )?;
    connection.execute("INSERT INTO monitor_capture_admissions(ordinal, occurrence_id, monitor_id, policy_version, policy_sha256, rule_revision, source_revision, start_ms, end_ms, planned_seconds, maximum_bytes, daily_cap, total_cap, byte_cap, admitted_ms, action_ordinal) VALUES ((SELECT COALESCE(MAX(ordinal) + 1, 1) FROM monitor_capture_admissions WHERE monitor_id = ?2), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)", params![occurrence.id, owner.monitor_id, policy.version, policy.spec_sha256, occurrence.rule_revision, rule.source_revision, occurrence.start_ms, occurrence.end_ms, occurrence.duration_seconds, occurrence.maximum_bytes, bounds.daily_seconds, i64::try_from(bounds.total_seconds).map_err(|_| Error::StorageIntegrity)?, i64::try_from(bounds.total_bytes).map_err(|_| Error::StorageIntegrity)?, now, action])?;
    for (day, seconds) in days(
        occurrence.start_ms.ok_or(Error::StorageIntegrity)?,
        occurrence.end_ms.ok_or(Error::StorageIntegrity)?,
    )? {
        connection.execute(
            "INSERT INTO monitor_capture_days(occurrence_id, utc_day, seconds) VALUES (?1, ?2, ?3)",
            params![
                occurrence.id,
                day,
                i64::try_from(seconds).map_err(|_| Error::StorageIntegrity)?
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn admission(
    connection: &Connection,
    occurrence: &str,
) -> Result<Option<MonitorCaptureAdmission>> {
    Ok(connection.query_row("SELECT occurrence_id, monitor_id, policy_version, policy_sha256, rule_revision, source_revision, planned_seconds, maximum_bytes, admitted_ms, action_ordinal FROM monitor_capture_admissions WHERE occurrence_id = ?1", [occurrence], |row| Ok(MonitorCaptureAdmission {
        occurrence_id: row.get(0)?, monitor_id: row.get(1)?, policy_version: row.get(2)?, policy_sha256: row.get(3)?, rule_revision: row.get(4)?, source_revision: row.get(5)?, planned_seconds: row.get::<_, i64>(6)?.cast_unsigned(), maximum_bytes: row.get::<_, i64>(7)?.cast_unsigned(), admitted_ms: row.get(8)?, action_ordinal: row.get(9)?,
    })).optional()?)
}

impl Store {
    pub(crate) fn monitor_capture_usage(&self, id: &str, now: i64) -> Result<MonitorCaptureUsage> {
        let (seconds, bytes, admissions) = used_total(&self.connection, id)?;
        let mut statement = self.connection.prepare("SELECT reason, COUNT(*) FROM monitor_capture_refusals WHERE monitor_id = ?1 GROUP BY reason ORDER BY reason")?;
        let refusals = statement
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(MonitorCaptureUsage {
            used_today_seconds: used_day(&self.connection, id, now / DAY_MS)?,
            used_total_seconds: seconds,
            reserved_total_bytes: bytes,
            admissions,
            refusals,
        })
    }

    pub(crate) fn audit_monitor_capture(&self) -> Result<()> {
        audit::audit(&self.connection)
    }
}

#[cfg(test)]
mod tests;
