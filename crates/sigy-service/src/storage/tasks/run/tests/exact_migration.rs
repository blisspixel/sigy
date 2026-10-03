//! Migration compares every old field and rowid, including completed history after drift.

use super::*;

fn old_rows(store: &Store) -> Result<[String; 3]> {
    let queries = [
        "SELECT json_group_array(json_array(rowid,task_id,request_id,spec_json,grant_sha256,scope_sha256,checkpoint_ordinal,checkpoint_sha256,maximum_findings,planned_findings,initial_partial,template,amount_micros,created_ms)) FROM (SELECT rowid,* FROM task_runs ORDER BY rowid)",
        "SELECT json_group_array(json_array(rowid,task_id,ordinal,kind,effect_id,citation_ordinal)) FROM (SELECT rowid,* FROM task_run_intents ORDER BY rowid)",
        "SELECT json_group_array(json_array(rowid,task_id,ordinal,generation,state,kind,effect_id,citation_ordinal,finding_id,reason,request_id,expected_generation,recorded_ms)) FROM (SELECT rowid,* FROM task_run_events ORDER BY rowid)",
    ];
    Ok([
        store.connection.query_row(queries[0], [], |r| r.get(0))?,
        store.connection.query_row(queries[1], [], |r| r.get(0))?,
        store.connection.query_row(queries[2], [], |r| r.get(0))?,
    ])
}

#[test]
fn v46_upgrade_preserves_exact_legacy_rows_hashes_and_history_after_scope_drift() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = fixture(&path)?;
    accept(&mut store, "task")?;
    let done = store.advance_task_run("task", 1, 102)?;
    store.propose_monitor_action(
        "world",
        "refused",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "No authority".into(),
        },
        103,
    )?;
    let before = old_rows(&store)?;
    revert_047_for_tests(&store.connection)?;
    assert_eq!(old_rows(&store)?, before);
    migrate_047(&store.connection)?;
    assert_eq!(old_rows(&store)?, before);
    assert_eq!(store.task_run("task")?, Some(done.clone()));
    assert_eq!(
        store.start_task_run("task", "accepted", &spec(), 0, 0)?,
        done
    );
    drop(store);
    assert!(Store::open(&path)?.task_run("task")?.is_some());
    Ok(())
}

#[test]
fn later_migration_ddl_failure_restores_the_whole_v46_family_and_allows_retry() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = fixture(&directory.path().join("catalog"))?;
    accept(&mut store, "task")?;
    store.advance_task_run("task", 1, 102)?;
    revert_047_for_tests(&store.connection)?;
    let before = old_rows(&store)?;
    store
        .connection
        .execute_batch("CREATE TABLE task_briefing_evidence(conflict INTEGER)")?;
    assert!(migrate_047(&store.connection).is_err());
    assert_eq!(old_rows(&store)?, before);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))?,
        46
    );
    assert_eq!(count(&store, "pragma_foreign_key_check")?, 0);
    assert_eq!(
        store
            .connection
            .query_row("SELECT 7", [], |r| r.get::<_, i64>(0))?,
        7
    );
    store
        .connection
        .execute_batch("DROP TABLE task_briefing_evidence")?;
    migrate_047(&store.connection)?;
    assert_eq!(old_rows(&store)?, before);
    Ok(())
}
