//! Exact task membership and previously observed coverage, without monitor-wide expansion.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use super::{
    Cited, Request, group_ordinals, header, insert_header, insert_members, load_snapshot, members,
    repetition_key, store_snapshot, validate_briefing,
};
use crate::{Error, Result, monitor::MonitorCoverage, storage::validate_key};

/// The caller owns the transaction and has checked task delegation and checkpoint scope.
pub(in crate::storage) fn write_task_briefing(
    connection: &Connection,
    monitor: &str,
    id: &str,
    coverage: &MonitorCoverage,
    finding_ids: &[String],
    now: i64,
) -> Result<()> {
    let request = Request {
        monitor,
        id,
        from_ms: coverage.from_ms,
        to_ms: coverage.to_ms,
        now,
    };
    validate_briefing(&request)?;
    let ids = checked_ids(finding_ids)?;
    if coverage.id != monitor || coverage.version == 0 {
        return Err(Error::Analysis("task-briefing-scope"));
    }
    if let Some(stored) = header(connection, monitor, id)? {
        let saved = load_snapshot(connection, monitor, id, stored.from_ms, stored.to_ms)?;
        let saved_ids = members(connection, monitor, id)?
            .into_iter()
            .map(|member| member.finding_id)
            .collect::<BTreeSet<_>>();
        return if stored.monitor_version == i64::from(coverage.version)
            && stored.classification == "off"
            && saved == *coverage
            && saved_ids == ids
        {
            Ok(())
        } else {
            Err(Error::Analysis("briefing-conflict"))
        };
    }
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM monitor_versions WHERE monitor_id = ?1 AND version = ?2)",
        params![monitor, coverage.version],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::NotFound);
    }
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM monitor_briefings WHERE monitor_id = ?1",
        [monitor],
        |row| row.get(0),
    )?;
    if count >= 1024 {
        return Err(Error::Analysis("briefing-limit"));
    }
    let cited = selected_citations(connection, monitor, &ids)?;
    let groups = group_ordinals(&cited);
    let distinct = groups.iter().max().map_or(0, |ordinal| ordinal + 1);
    let corroboration = i64::try_from(distinct).map_err(|_| Error::StorageIntegrity)?;
    insert_header(
        connection,
        &request,
        count + 1,
        i64::from(coverage.version),
        corroboration,
    )?;
    insert_members(connection, &request, &cited, &groups)?;
    store_snapshot(connection, monitor, id, coverage)
}

fn checked_ids(finding_ids: &[String]) -> Result<BTreeSet<String>> {
    if finding_ids.len() > crate::monitor::MATCH_PAGE {
        return Err(Error::Analysis("task-briefing-limit"));
    }
    let mut ids = BTreeSet::new();
    for id in finding_ids {
        validate_key(id, "finding ID")?;
        if !ids.insert(id.clone()) {
            return Err(Error::Analysis("task-briefing-duplicate"));
        }
    }
    Ok(ids)
}

fn selected_citations(
    connection: &Connection,
    monitor: &str,
    ids: &BTreeSet<String>,
) -> Result<Vec<Cited>> {
    ids.iter()
        .map(|id| {
            let script: String = connection
                .query_row(
                    "SELECT c.script FROM monitor_findings f JOIN transcript_cues c ON c.transcript_id = f.transcript_id AND c.revision = f.transcript_revision AND c.ordinal = f.cue_ordinal WHERE f.monitor_id = ?1 AND f.id = ?2",
                    params![monitor, id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(Error::NotFound)?;
            Ok(Cited {
                id: id.clone(),
                key: repetition_key(&script),
            })
        })
        .collect()
}
