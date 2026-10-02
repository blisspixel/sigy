use super::{
    Connection, Error, Grant, MonitorScheduleOwner, OptionalExtension, Result, ScheduleOccurrence,
    ScheduleRule, TaskView, current_scope, draft, latest_task_ms, params, read,
};

pub(crate) fn owns_rule(connection: &Connection, id: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT task_owned = 1 OR EXISTS(SELECT 1 FROM task_collection_rules WHERE rule_id = ?1) FROM schedule_rules WHERE id = ?1",
        [id],
        |row| row.get(0),
    )?)
}

pub(super) fn validate_rule(
    task: &TaskView,
    grant: &Grant,
    ordinal: usize,
    rule: &ScheduleRule,
) -> Result<()> {
    let capture = grant
        .spec
        .captures
        .get(ordinal)
        .ok_or(Error::StorageIntegrity)?;
    let expected = draft(&grant.digest, ordinal, capture).map_err(|_| Error::StorageIntegrity)?;
    if rule.id != expected.id
        || !crate::storage::schedules::same_rule(rule, &expected)
        || rule.revision != 0
        || rule.created_ms != grant.created_ms
        || rule.updated_ms != grant.created_ms
        || !rule.task_owned
        || rule.monitor_owner
            != Some(MonitorScheduleOwner {
                monitor_id: task.spec.monitor_id.clone(),
                version: task.spec.monitor_version,
            })
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

pub(super) fn validate_occurrence(
    grant: &Grant,
    ordinal: usize,
    occurrence: &ScheduleOccurrence,
) -> Result<()> {
    let capture = grant
        .spec
        .captures
        .get(ordinal)
        .ok_or(Error::StorageIntegrity)?;
    let expected = draft(&grant.digest, ordinal, capture).map_err(|_| Error::StorageIntegrity)?;
    let date = expected
        .civil_date
        .as_deref()
        .ok_or(Error::StorageIntegrity)?;
    if occurrence.id != format!("{}:{date}", expected.id)
        || occurrence.rule_id != expected.id
        || occurrence.rule_revision != 0
        || occurrence.start_ms != Some(capture.start_ms)
        || occurrence.end_ms != Some(capture.end_ms().map_err(|_| Error::StorageIntegrity)?)
        || occurrence.duration_seconds != i64::from(capture.duration_seconds)
        || occurrence.maximum_bytes
            != i64::try_from(capture.maximum_bytes).map_err(|_| Error::StorageIntegrity)?
        || occurrence.civil_date != expected.civil_date.ok_or(Error::StorageIntegrity)?
        || occurrence.hour != expected.hour
        || occurrence.minute != expected.minute
        || occurrence.second != expected.second
        || occurrence.offset_seconds != Some(0)
        || occurrence.transition_ms.is_some()
        || occurrence.created_ms != grant.created_ms
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

/// Runs in the same transaction as canonical DVR admission and capture reservations.
pub(crate) fn check(
    connection: &Connection,
    rule: &ScheduleRule,
    occurrence: &ScheduleOccurrence,
    now: i64,
) -> Result<bool> {
    let binding: Option<(String, u32, u32, i64)> = connection.query_row("SELECT task_id, ordinal, generation, rule_revision FROM task_collection_rules WHERE rule_id = ?1", [&rule.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional()?;
    let Some((id, ordinal, generation, revision)) = binding else {
        return if rule.task_owned {
            Err(Error::StorageIntegrity)
        } else {
            Ok(true)
        };
    };
    if generation != 1 || revision != 0 {
        return Err(Error::StorageIntegrity);
    }
    let ordinal = usize::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?;
    let grant = read::grant(connection, &id)?;
    validate_occurrence(&grant, ordinal, occurrence)?;
    let task = checked_scope(connection, &id, &grant)?;
    for capture in &grant.spec.captures {
        if capture.start_ms < task.from_ms
            || capture.end_ms()? > task.to_ms
            || capture.end_ms()? <= grant.created_ms
        {
            return Err(Error::StorageIntegrity);
        }
    }
    let expected = draft(&grant.digest, ordinal, &grant.spec.captures[ordinal])
        .map_err(|_| Error::StorageIntegrity)?;
    if !crate::storage::schedules::same_rule(rule, &expected)
        || rule.id != expected.id
        || rule.revision != revision
        || rule.created_ms != grant.created_ms
        || rule.updated_ms != grant.created_ms
        || !rule.task_owned
        || rule.monitor_owner
            != Some(MonitorScheduleOwner {
                monitor_id: task.monitor_id.clone(),
                version: task.monitor_version,
            })
    {
        return Err(Error::StorageIntegrity);
    }
    let last_admission: i64 = connection.query_row(
        "SELECT coalesce(max(admitted_ms), ?2) FROM task_collection_admissions WHERE task_id = ?1",
        params![id, grant.created_ms],
        |row| row.get(0),
    )?;
    Ok(current_scope(connection, &task)?
        && now >= last_admission.max(latest_task_ms(connection, &id)?)
        && read::cancellation(connection, &id, &grant)?.is_none())
}

fn checked_scope(
    connection: &Connection,
    id: &str,
    grant: &Grant,
) -> Result<crate::task::TaskSpec> {
    let (spec, scope, _, _) = super::super::audit::checked_task_scope_in(connection, id)?;
    if scope != grant.scope_sha256 {
        return Err(Error::StorageIntegrity);
    }
    Ok(spec)
}
/// Writes exact task occurrence provenance after the canonical monitor receipt, in its TX.
pub(crate) fn reserve(
    connection: &Connection,
    rule: &ScheduleRule,
    occurrence: &ScheduleOccurrence,
    now: i64,
) -> Result<()> {
    let binding: Option<(String, u32)> = connection
        .query_row(
            "SELECT task_id, ordinal FROM task_collection_rules WHERE rule_id = ?1",
            [&rule.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((id, ordinal)) = binding {
        connection.execute("INSERT INTO task_collection_admissions(task_id, ordinal, occurrence_id, generation, admitted_ms) VALUES (?1, ?2, ?3, 1, ?4)", params![id, ordinal, occurrence.id, now])?;
    }
    Ok(())
}
