use super::{
    Connection, Error, Grant, OptionalExtension, Result, ScheduleOccurrence, Store, TaskView,
    params,
};

pub(super) fn validate_admission(
    connection: &Connection,
    id: &str,
    task: &TaskView,
    grant: &Grant,
    ordinal: usize,
    occurrence: &ScheduleOccurrence,
    cancelled: Option<i64>,
) -> Result<()> {
    let row: Option<(String, u32, i64)> = connection.query_row("SELECT occurrence_id, generation, admitted_ms FROM task_collection_admissions WHERE task_id = ?1 AND ordinal = ?2", params![id, i64::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
    let Some((occurrence_id, generation, admitted)) = row else {
        return if occurrence.state == "admitted" {
            Err(Error::StorageIntegrity)
        } else {
            Ok(())
        };
    };
    let receipt = occurrence
        .capture_admission
        .as_ref()
        .ok_or(Error::StorageIntegrity)?;
    if occurrence.state != "admitted"
        || occurrence_id != occurrence.id
        || occurrence.recording_id.as_deref() != Some(&occurrence.id)
        || generation != 1
        || admitted < grant.created_ms
        || admitted < occurrence.start_ms.ok_or(Error::StorageIntegrity)?
        || admitted >= occurrence.end_ms.ok_or(Error::StorageIntegrity)?
        || cancelled.is_some_and(|cancel| cancel < admitted)
        || receipt.monitor_id != task.spec.monitor_id
        || receipt.policy_version != task.spec.monitor_version
        || receipt.action_ordinal != task.spec.monitor_actions
        || receipt.policy_sha256 != task.monitor_spec_sha256
        || receipt.admitted_ms != admitted
        || receipt.source_revision != grant.spec.captures[ordinal].source_revision
        || receipt.rule_revision != occurrence.rule_revision
        || receipt.planned_seconds != occurrence.duration_seconds.cast_unsigned()
        || receipt.maximum_bytes != occurrence.maximum_bytes.cast_unsigned()
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

impl Store {
    pub(crate) fn audit_task_collections(&self) -> Result<()> {
        validate_markers(&self.connection)?;
        let mut statement = self
            .connection
            .prepare("SELECT task_id FROM task_collections ORDER BY task_id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            self.task_collection(&id)?.ok_or(Error::StorageIntegrity)?;
        }
        Ok(())
    }
}

pub(super) fn validate_markers(connection: &Connection) -> Result<()> {
    let orphaned: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM schedule_rules r WHERE r.task_owned != EXISTS(SELECT 1 FROM task_collection_rules b WHERE b.rule_id = r.id))", [], |row| row.get(0))?;
    if orphaned {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
