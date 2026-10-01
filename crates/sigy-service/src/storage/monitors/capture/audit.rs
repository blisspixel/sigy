use rusqlite::Connection;

use super::{admission, days};
use crate::{Error, Result};

pub(super) fn audit(connection: &Connection) -> Result<()> {
    let broken: bool = connection.query_row(include_str!("audit.sql"), [], |row| row.get(0))?;
    if broken {
        return Err(Error::StorageIntegrity);
    }
    let mut statement = connection.prepare(
        "SELECT occurrence_id, start_ms, end_ms FROM monitor_capture_admissions ORDER BY monitor_id, ordinal",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut versions = std::collections::BTreeSet::new();
    let mut policies = connection.prepare("SELECT monitor_id, version, spec_json, spec_sha256 FROM monitor_versions v WHERE version = (SELECT MAX(version) FROM monitor_versions WHERE monitor_id = v.monitor_id) OR EXISTS(SELECT 1 FROM monitor_capture_rules owner WHERE owner.monitor_id = v.monitor_id AND owner.created_version = v.version) OR EXISTS(SELECT 1 FROM monitor_capture_refusals denied WHERE denied.monitor_id = v.monitor_id AND denied.policy_version = v.version)")?;
    for row in policies.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })? {
        let (id, version, json, digest) = row?;
        super::super::checked_spec(&json, &digest)?;
        versions.insert((id, version));
    }
    let mut daily = connection.prepare("SELECT utc_day, seconds FROM monitor_capture_days WHERE occurrence_id = ?1 ORDER BY utc_day")?;
    for row in rows {
        let (id, start, end) = row?;
        let proof = admission(connection, &id)?.ok_or(Error::StorageIntegrity)?;
        if versions.insert((proof.monitor_id.clone(), proof.policy_version)) {
            let (json, digest): (String, String) = connection.query_row("SELECT spec_json, spec_sha256 FROM monitor_versions WHERE monitor_id = ?1 AND version = ?2", rusqlite::params![proof.monitor_id, proof.policy_version], |row| Ok((row.get(0)?, row.get(1)?)))?;
            super::super::checked_spec(&json, &digest)?;
        }
        let actual = daily
            .query_map([id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?.cast_unsigned()))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if actual != days(start, end)? {
            return Err(Error::StorageIntegrity);
        }
    }
    Ok(())
}
