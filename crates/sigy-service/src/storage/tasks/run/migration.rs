//! Rebuild only the run family and retain every legacy byte and physical row identity.

use crate::{Error, Result};
use rusqlite::Connection;

pub(in crate::storage) fn migrate_047(connection: &Connection) -> Result<()> {
    let enabled: bool = connection.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
    if !enabled {
        return Err(Error::CatalogIntegrity);
    }
    connection.execute_batch("SAVEPOINT migrate_exact_runs")?;
    let result = (|| {
        connection.execute_batch(include_str!("../../047-task-snapshot-runs.sql"))?;
        let violations: i64 =
            connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if violations != 0 {
            return Err(Error::CatalogIntegrity);
        }
        Ok(())
    })();
    if result.is_err() {
        connection.execute_batch("ROLLBACK TO migrate_exact_runs")?;
    }
    connection.execute_batch("RELEASE migrate_exact_runs")?;
    result
}

#[cfg(test)]
const SNAPSHOT_CLOCKS:&str="CREATE TRIGGER task_run_snapshot_clock BEFORE INSERT ON task_runs WHEN NEW.created_ms<coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id=NEW.task_id),0) BEGIN SELECT RAISE(ABORT,'task snapshot clock'); END;
CREATE TRIGGER task_run_event_snapshot_clock BEFORE INSERT ON task_run_events WHEN NEW.recorded_ms<coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id=NEW.task_id),0) BEGIN SELECT RAISE(ABORT,'task snapshot clock'); END; PRAGMA user_version=46;";

/// Only legacy fixture history can be represented in the old schema.
#[cfg(test)]
pub(in crate::storage) fn revert_047_for_tests(connection: &Connection) -> Result<()> {
    let newer: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_runs') WHERE name='origin')",
        [],
        |r| r.get(0),
    )?;
    if !newer {
        return Ok(());
    }
    let exact: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_runs WHERE origin!='checkpoint')",
        [],
        |r| r.get(0),
    )?;
    if exact {
        return Err(Error::InvalidInput("exact run cannot downgrade"));
    }
    connection.execute_batch("SAVEPOINT revert_exact_runs")?;
    let result = revert(connection);
    if result.is_err() {
        connection.execute_batch("ROLLBACK TO revert_exact_runs")?;
    }
    connection.execute_batch("RELEASE revert_exact_runs")?;
    result
}

#[cfg(test)]
fn revert(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TEMP TABLE old_run_copy AS SELECT rowid AS saved_rowid,task_id,request_id,spec_json,grant_sha256,scope_sha256,checkpoint_ordinal,checkpoint_sha256,maximum_findings,planned_findings,initial_partial,template,amount_micros,created_ms FROM task_runs;
CREATE TEMP TABLE old_intent_copy AS SELECT rowid AS saved_rowid,* FROM task_run_intents;
CREATE TEMP TABLE old_event_copy AS SELECT rowid AS saved_rowid,* FROM task_run_events;
DROP TRIGGER monitor_briefing_coverage_exclusive; DROP TABLE task_briefing_evidence;
DROP TABLE task_run_events; DROP TABLE task_run_intents; DROP TABLE task_runs;")?;
    let (tables, triggers) = include_str!("../../042-task-runs.sql")
        .split_once("CREATE TRIGGER task_run_admission")
        .ok_or(Error::StorageIntegrity)?;
    connection.execute_batch(tables)?;
    connection.execute_batch("INSERT INTO task_runs(rowid,task_id,request_id,spec_json,grant_sha256,scope_sha256,checkpoint_ordinal,checkpoint_sha256,maximum_findings,planned_findings,initial_partial,template,amount_micros,created_ms) SELECT * FROM old_run_copy ORDER BY saved_rowid;
INSERT INTO task_run_intents(rowid,task_id,ordinal,kind,effect_id,citation_ordinal) SELECT * FROM old_intent_copy ORDER BY saved_rowid;
INSERT INTO task_run_events(rowid,task_id,ordinal,generation,state,kind,effect_id,citation_ordinal,finding_id,reason,request_id,expected_generation,recorded_ms) SELECT * FROM old_event_copy ORDER BY saved_rowid;
DROP TABLE old_event_copy; DROP TABLE old_intent_copy; DROP TABLE old_run_copy;")?;
    connection.execute_batch(&format!("CREATE TRIGGER task_run_admission{triggers}"))?;
    connection.execute_batch(SNAPSHOT_CLOCKS)?;
    let violations: i64 =
        connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?;
    if violations != 0 {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
