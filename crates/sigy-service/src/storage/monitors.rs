//! Monitor versions and the action log. Versions and actions are append-only.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Store, validate_key};
use crate::{
    Error, Result,
    monitor::{
        ActionOrigin, MonitorAction, MonitorSpec, MonitorVersion, MonitorView, Proposal, decide,
    },
};

pub const ACTION_PAGE: u32 = 16;

/// Undo migration 032 so a test can build an older catalog from the current one.
#[cfg(test)]
pub(crate) fn revert_032_for_tests(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "DROP TABLE monitor_actions; DROP TABLE monitor_versions; DROP TABLE monitors; PRAGMA user_version = 31;",
    )?;
    Ok(())
}

/// Whether a create or revise wrote a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionWrite {
    Created,
    Unchanged,
}

fn parse_spec(json: &str) -> Result<MonitorSpec> {
    let spec: MonitorSpec = serde_json::from_str(json).map_err(|_| Error::StorageIntegrity)?;
    spec.validate().map_err(|_| Error::StorageIntegrity)?;
    Ok(spec)
}

fn latest_version(connection: &Connection, id: &str) -> Result<Option<MonitorVersion>> {
    let row: Option<(u32, String, String, i64)> = connection
        .query_row(
            "SELECT version, spec_json, spec_sha256, created_ms FROM monitor_versions WHERE monitor_id = ?1 ORDER BY version DESC LIMIT 1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    row.map(|(version, json, spec_sha256, created_ms)| {
        Ok(MonitorVersion {
            monitor_id: id.to_owned(),
            version,
            spec: parse_spec(&json)?,
            spec_sha256,
            created_ms,
        })
    })
    .transpose()
}

fn exists(connection: &Connection, sql: &str, key: &str) -> Result<bool> {
    Ok(connection.query_row(sql, [key], |row| row.get(0))?)
}

/// Every referenced source, schedule and profile must already exist.
fn check_references(connection: &Connection, spec: &MonitorSpec) -> Result<()> {
    for source in spec.sources.iter().chain(&spec.candidate_sources) {
        if !exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM source_revisions WHERE id = ?1)",
            source,
        )? {
            return Err(Error::InvalidInput(
                "monitor source revision does not exist",
            ));
        }
    }
    for schedule in &spec.schedules {
        if !exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM schedule_rules WHERE id = ?1)",
            schedule,
        )? {
            return Err(Error::InvalidInput("monitor schedule does not exist"));
        }
    }
    if let Some(profile) = &spec.recognition_profile
        && !exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM recognition_profiles WHERE id = ?1)",
            profile,
        )?
    {
        return Err(Error::InvalidInput(
            "monitor recognition profile does not exist",
        ));
    }
    if let Some(profile) = &spec.translation_profile
        && !exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM translation_profiles WHERE id = ?1)",
            profile,
        )?
    {
        return Err(Error::InvalidInput(
            "monitor translation profile does not exist",
        ));
    }
    Ok(())
}

fn applied_actions(connection: &Connection, id: &str, version: u32) -> Result<Vec<Proposal>> {
    let mut statement = connection.prepare(
        "SELECT proposal_json FROM monitor_actions WHERE monitor_id = ?1 AND policy_version = ?2 AND decision = 'applied' ORDER BY ordinal",
    )?;
    let rows = statement
        .query_map(params![id, version], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.iter()
        .map(|json| serde_json::from_str(json).map_err(|_| Error::StorageIntegrity))
        .collect()
}

/// Sources followed under the latest version: its sources plus applied changes since.
fn active_sources(connection: &Connection, version: &MonitorVersion) -> Result<Vec<String>> {
    let mut active = version.spec.sources.clone();
    for proposal in applied_actions(connection, &version.monitor_id, version.version)? {
        match proposal {
            Proposal::AddSource { source } if !active.contains(&source) => active.push(source),
            Proposal::RemoveSource { source } => active.retain(|existing| *existing != source),
            _ => {}
        }
    }
    Ok(active)
}

fn paused(connection: &Connection, id: &str) -> Result<bool> {
    let kind: Option<String> = connection
        .query_row(
            "SELECT kind FROM monitor_actions WHERE monitor_id = ?1 AND decision = 'applied' AND kind IN ('pause', 'resume') ORDER BY ordinal DESC LIMIT 1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(kind.as_deref() == Some("pause"))
}

fn action_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActionRow> {
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
    ))
}

type ActionRow = (
    String,
    u32,
    String,
    u32,
    String,
    String,
    String,
    String,
    i64,
);

fn to_action(row: ActionRow) -> Result<MonitorAction> {
    let (
        monitor_id,
        ordinal,
        _action_id,
        policy_version,
        origin,
        proposal,
        decision,
        reason,
        created_ms,
    ) = row;
    let origin = match origin.as_str() {
        "user" => ActionOrigin::User,
        "schedule" => ActionOrigin::Schedule,
        "rule" => ActionOrigin::Rule,
        "model" => ActionOrigin::Model,
        _ => return Err(Error::StorageIntegrity),
    };
    Ok(MonitorAction {
        monitor_id,
        ordinal,
        policy_version,
        origin,
        proposal: serde_json::from_str(&proposal).map_err(|_| Error::StorageIntegrity)?,
        decision,
        reason,
        amount_usd: "0.000000".into(),
        created_ms,
    })
}

const ACTION_COLUMNS: &str = "monitor_id, ordinal, action_id, policy_version, origin, proposal_json, decision, reason, created_ms";

impl Store {
    /// Create a monitor with its first version. An identical replay is unchanged.
    /// # Errors
    /// Refuses an invalid specification, a missing reference, or a changed replay.
    pub(crate) fn create_monitor(
        &mut self,
        id: &str,
        spec: &MonitorSpec,
        now: i64,
    ) -> Result<VersionWrite> {
        validate_key(id, "monitor ID")?;
        spec.validate()?;
        let digest = spec.digest()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(first) = tx
            .query_row(
                "SELECT spec_sha256 FROM monitor_versions WHERE monitor_id = ?1 AND version = 1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return if first == digest {
                Ok(VersionWrite::Unchanged)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        check_references(&tx, spec)?;
        tx.execute(
            "INSERT INTO monitors(id, created_ms) VALUES (?1, ?2)",
            params![id, now],
        )?;
        tx.execute(
            "INSERT INTO monitor_versions(monitor_id, version, spec_json, spec_sha256, created_ms) VALUES (?1, 1, ?2, ?3, ?4)",
            params![id, serde_json::to_string(spec)?, digest, now],
        )?;
        tx.commit()?;
        Ok(VersionWrite::Created)
    }

    /// Append a new user version. It must follow the version the user last saw.
    /// # Errors
    /// Refuses a stale expected version, an invalid specification or a missing reference.
    pub(crate) fn revise_monitor(
        &mut self,
        id: &str,
        expected_version: u32,
        spec: &MonitorSpec,
        now: i64,
    ) -> Result<VersionWrite> {
        validate_key(id, "monitor ID")?;
        spec.validate()?;
        let digest = spec.digest()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let latest = latest_version(&tx, id)?.ok_or(Error::NotFound)?;
        if latest.spec_sha256 == digest
            && (latest.version == expected_version
                || latest.version == expected_version.saturating_add(1))
        {
            return Ok(VersionWrite::Unchanged);
        }
        if latest.version != expected_version {
            return Err(Error::InvalidInput(
                "monitor changed since that version; show it and revise again",
            ));
        }
        check_references(&tx, spec)?;
        tx.execute(
            "INSERT INTO monitor_versions(monitor_id, version, spec_json, spec_sha256, created_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, latest.version + 1, serde_json::to_string(spec)?, digest, now],
        )?;
        tx.commit()?;
        Ok(VersionWrite::Created)
    }

    /// Record one proposal and its policy decision against the latest version.
    /// Replaying the same action ID returns the stored action; changing it is refused.
    /// # Errors
    /// Refuses a missing monitor, an invalid action ID or proposal, or a changed replay.
    pub(crate) fn propose_monitor_action(
        &mut self,
        id: &str,
        action_id: &str,
        origin: ActionOrigin,
        proposal: &Proposal,
        now: i64,
    ) -> Result<MonitorAction> {
        validate_key(id, "monitor ID")?;
        validate_key(action_id, "monitor action ID")?;
        let proposal_json = serde_json::to_string(proposal)?;
        if proposal_json.len() > 8192 {
            return Err(Error::InvalidInput("monitor proposal is too large"));
        }
        match proposal {
            Proposal::AddSource { source } | Proposal::RemoveSource { source } => {
                validate_key(source, "monitor source")?;
            }
            Proposal::Other { request } if request.is_empty() || request.chars().count() > 1000 => {
                return Err(Error::InvalidInput("monitor proposal text"));
            }
            _ => {}
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = tx
            .query_row(
                &format!("SELECT {ACTION_COLUMNS} FROM monitor_actions WHERE monitor_id = ?1 AND action_id = ?2"),
                params![id, action_id],
                action_row,
            )
            .optional()?
        {
            let action = to_action(existing)?;
            return if action.origin == origin && action.proposal == *proposal {
                Ok(action)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        let latest = latest_version(&tx, id)?.ok_or(Error::NotFound)?;
        let active = active_sources(&tx, &latest)?;
        let (applied, reason) = decide(&latest.spec, &active, proposal);
        let ordinal: u32 = tx.query_row(
            "SELECT coalesce(max(ordinal), 0) + 1 FROM monitor_actions WHERE monitor_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO monitor_actions(monitor_id, ordinal, action_id, policy_version, origin, kind, proposal_json, decision, reason, amount_micros, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10)",
            params![
                id, ordinal, action_id, latest.version, origin.as_str(), proposal.kind(),
                proposal_json, if applied { "applied" } else { "refused" }, reason, now
            ],
        )?;
        let action = tx.query_row(
            &format!("SELECT {ACTION_COLUMNS} FROM monitor_actions WHERE monitor_id = ?1 AND ordinal = ?2"),
            params![id, ordinal],
            action_row,
        )?;
        tx.commit()?;
        to_action(action)
    }

    /// The current state of one monitor.
    /// # Errors
    /// Refuses a missing monitor or invalid stored rows.
    pub fn monitor(&self, id: &str) -> Result<MonitorView> {
        validate_key(id, "monitor ID")?;
        let version = latest_version(&self.connection, id)?.ok_or(Error::NotFound)?;
        let actions: u32 = self.connection.query_row(
            "SELECT count(*) FROM monitor_actions WHERE monitor_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(MonitorView {
            id: id.to_owned(),
            active_sources: active_sources(&self.connection, &version)?,
            paused: paused(&self.connection, id)?,
            version,
            actions,
        })
    }

    /// One exact historical version.
    /// # Errors
    /// Refuses a missing version or invalid stored rows.
    pub fn monitor_version(&self, id: &str, version: u32) -> Result<MonitorVersion> {
        validate_key(id, "monitor ID")?;
        let (json, spec_sha256, created_ms): (String, String, i64) = self
            .connection
            .query_row(
                "SELECT spec_json, spec_sha256, created_ms FROM monitor_versions WHERE monitor_id = ?1 AND version = ?2",
                params![id, version],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(MonitorVersion {
            monitor_id: id.to_owned(),
            version,
            spec: parse_spec(&json)?,
            spec_sha256,
            created_ms,
        })
    }

    /// Up to sixteen actions after an ordinal cursor, oldest first.
    /// # Errors
    /// Refuses a malformed ID or invalid stored rows.
    pub fn monitor_actions(&self, id: &str, after: Option<u32>) -> Result<Vec<MonitorAction>> {
        validate_key(id, "monitor ID")?;
        let mut statement = self.connection.prepare(&format!(
            "SELECT {ACTION_COLUMNS} FROM monitor_actions WHERE monitor_id = ?1 AND ordinal > ?2 ORDER BY ordinal LIMIT ?3"
        ))?;
        let rows = statement
            .query_map(params![id, after.unwrap_or(0), ACTION_PAGE], action_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(to_action).collect()
    }

    /// Every monitor ID, at most 256.
    /// # Errors
    /// Fails on a database error.
    pub fn monitor_ids(&self) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM monitors ORDER BY id LIMIT 256")?;
        Ok(statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

mod coverage;

#[cfg(test)]
mod tests;
