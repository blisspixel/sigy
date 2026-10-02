use super::{
    COLLECTION_TEMPLATE, Connection, Error, Grant, OptionalExtension, Result, Store,
    TaskCollectionCapture, TaskCollectionSpec, TaskCollectionView, TaskView, admission,
    admission_mask, audit, cancel_hash, grant_hash, params, validate_key,
};

type GrantRow = (String, String, String, String, i64, String, i64);

fn read_grant(connection: &Connection, id: &str) -> Result<Option<Grant>> {
    let row: Option<GrantRow> = connection.query_row("SELECT request_id, spec_json, grant_sha256, scope_sha256, created_ms, template, amount_micros FROM task_collections WHERE task_id = ?1", [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))).optional()?;
    let Some((request_id, json, digest, scope, created, template, amount)) = row else {
        return Ok(None);
    };
    let spec: TaskCollectionSpec =
        serde_json::from_str(&json).map_err(|_| Error::StorageIntegrity)?;
    spec.validate().map_err(|_| Error::StorageIntegrity)?;
    validate_key(&request_id, "task collection request").map_err(|_| Error::StorageIntegrity)?;
    let task: (String, i64) = connection.query_row(
        "SELECT scope_sha256, created_ms FROM tasks WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if template != COLLECTION_TEMPLATE
        || amount != 0
        || created < task.1
        || scope != task.0
        || json.len() > 2048
        || grant_hash(id, &request_id, &json, &scope, created)? != digest
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(Some(Grant {
        request_id,
        spec,
        digest,
        scope_sha256: scope,
        created_ms: created,
    }))
}

pub(super) fn cancellation(
    connection: &Connection,
    id: &str,
    grant: &Grant,
) -> Result<Option<i64>> {
    let row: Option<(String, u32, u32, String, i64, u32)> = connection.query_row("SELECT request_id, expected_generation, generation, receipt_sha256, created_ms, admitted_mask FROM task_collection_cancellations WHERE task_id = ?1", [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))).optional()?;
    let Some((request, expected, generation, digest, created, mask)) = row else {
        return Ok(None);
    };
    validate_key(&request, "task collection cancellation").map_err(|_| Error::StorageIntegrity)?;
    if expected != 1
        || generation != 2
        || created < grant.created_ms
        || mask != admission_mask(connection, id)?
        || digest != cancel_hash(&grant.digest, &request, mask, created)?
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(Some(created))
}

impl Store {
    /// Inspect collection provenance and actual schedule/capture states without new work.
    /// # Errors
    /// Refuses invalid grant, rule, admission or cancellation history.
    pub fn task_collection(&self, id: &str) -> Result<Option<TaskCollectionView>> {
        validate_key(id, "task ID")?;
        audit::validate_markers(&self.connection)?;
        let task = self.task(id)?;
        let Some(grant) = read_grant(&self.connection, id)? else {
            return Ok(None);
        };
        self.validate_collection_scope(&task, &grant.spec, grant.created_ms)
            .map_err(|_| Error::StorageIntegrity)?;
        let cancelled_at = cancellation(&self.connection, id, &grant)?;
        let captures = self.collection_captures(id, &task, &grant, cancelled_at)?;
        let updated_ms: i64 = self.connection.query_row("SELECT coalesce(max(admitted_ms), ?2) FROM task_collection_admissions WHERE task_id = ?1", params![id, grant.created_ms], |row| row.get(0))?;
        Ok(Some(TaskCollectionView {
            id: id.into(),
            request_id: grant.request_id,
            template: COLLECTION_TEMPLATE.into(),
            paid_allowance_usd: "0.000000".into(),
            spec: grant.spec,
            grant_sha256: grant.digest,
            scope_sha256: grant.scope_sha256,
            created_ms: grant.created_ms,
            updated_ms: cancelled_at.unwrap_or(updated_ms),
            generation: if cancelled_at.is_some() { 2 } else { 1 },
            cancelled: cancelled_at.is_some(),
            scope_current: task.scope_current,
            hold_reason: if cancelled_at.is_some() {
                Some("cancelled".into())
            } else if !task.scope_current {
                Some("scope-changed".into())
            } else {
                None
            },
            captures,
        }))
    }

    fn collection_captures(
        &self,
        id: &str,
        task: &TaskView,
        grant: &Grant,
        cancelled: Option<i64>,
    ) -> Result<Vec<TaskCollectionCapture>> {
        let mut statement = self.connection.prepare("SELECT ordinal, rule_id, rule_revision, generation FROM task_collection_rules WHERE task_id = ?1 ORDER BY ordinal")?;
        let rows = statement
            .query_map([id], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.len() != grant.spec.captures.len() {
            return Err(Error::StorageIntegrity);
        }
        let mut captures = Vec::new();
        for (expected, (ordinal, rule_id, revision, generation)) in rows.into_iter().enumerate() {
            let ordinal = usize::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?;
            if expected != ordinal || revision != 0 || generation != 1 {
                return Err(Error::StorageIntegrity);
            }
            let rule = self.schedule(&rule_id)?.ok_or(Error::StorageIntegrity)?;
            admission::validate_rule(task, grant, ordinal, &rule)?;
            let occurrences = self.schedule_occurrences(&rule_id)?;
            if occurrences.len() != 1 {
                return Err(Error::StorageIntegrity);
            }
            let occurrence = &occurrences[0];
            admission::validate_occurrence(grant, ordinal, occurrence)?;
            audit::validate_admission(
                &self.connection,
                id,
                task,
                grant,
                ordinal,
                occurrence,
                cancelled,
            )?;
            let recording_state = occurrence
                .recording_id
                .as_ref()
                .map(|recording| {
                    self.connection.query_row(
                        "SELECT state FROM capture_jobs WHERE id = ?1",
                        [recording],
                        |row| row.get::<_, String>(0),
                    )
                })
                .transpose()?;
            captures.push(TaskCollectionCapture {
                rule_id,
                occurrence_id: Some(occurrence.id.clone()),
                state: occurrence.state.clone(),
                recording_id: occurrence.recording_id.clone(),
                recording_state,
            });
        }
        Ok(captures)
    }
}

pub(super) fn grant(connection: &Connection, id: &str) -> Result<Grant> {
    read_grant(connection, id)?.ok_or(Error::StorageIntegrity)
}
