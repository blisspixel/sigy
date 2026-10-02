use super::*;

#[test]
fn raw_scope_binding_rejects_changed_bytes_even_with_valid_scalar_fields() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    store.create_task("task", &spec(), 2)?;
    store
        .connection
        .execute_batch("DROP TRIGGER task_no_update")?;
    let mut altered = spec();
    altered.goal = "Changed scope".into();
    store.connection.execute(
        "UPDATE tasks SET spec_json = ?1",
        [serde_json::to_string(&altered)?],
    )?;
    assert!(matches!(store.task("task"), Err(Error::StorageIntegrity)));
    drop(store);
    assert!(Store::open(&path).is_err());
    Ok(())
}

#[test]
fn checkpoint_hash_and_internal_consistency_are_independent_checks() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    store.create_task("task", &spec(), 2)?;
    let mut checkpoint = store.checkpoint_task("task", "first", 0, 3)?;
    store
        .connection
        .execute_batch("DROP TRIGGER task_checkpoint_no_update")?;
    checkpoint.window_elapsed = true;
    let json = serde_json::to_string(&checkpoint)?;
    store
        .connection
        .execute("UPDATE task_checkpoints SET payload_json = ?1", [&json])?;
    assert!(store.task_checkpoint("task", 1).is_err());
    let hash = checkpoint_hash(&json, &store.checked_task_scope("task")?.1);
    store
        .connection
        .execute("UPDATE task_checkpoints SET payload_sha256 = ?1", [hash])?;
    assert!(
        store.task_checkpoint("task", 1).is_err(),
        "a recomputed checksum cannot justify an impossible elapsed flag"
    );
    drop(store);
    assert!(Store::open(&path).is_err());
    Ok(())
}

#[test]
fn hostile_source_counts_citations_and_scope_action_history_fail_closed() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    store.create_task("task", &spec(), 2)?;
    let checkpoint = store.checkpoint_task("task", "first", 0, 3)?;
    let mut bad = checkpoint.clone();
    bad.coverage.sources[0].published = 1;
    assert!(store.validate_task_checkpoint(&spec(), &bad).is_err());
    bad = checkpoint.clone();
    bad.coverage.sources[0].source = "outside:v1".into();
    assert!(store.validate_task_checkpoint(&spec(), &bad).is_err());
    bad = checkpoint.clone();
    bad.monitor_paused = true;
    assert!(store.validate_task_checkpoint(&spec(), &bad).is_err());
    bad = checkpoint;
    bad.citations.push(crate::task::TaskCitation {
        source: "a:v1".into(),
        recording_id: "absent".into(),
        transcript_id: "absent".into(),
        transcript_revision: 1,
        translation_revision: None,
        cue_ordinal: 0,
        start_us: 0,
        end_us: 1,
    });
    assert!(store.validate_task_checkpoint(&spec(), &bad).is_err());
    store
        .connection
        .execute_batch("DROP TRIGGER task_no_update; PRAGMA ignore_check_constraints = ON")?;
    store
        .connection
        .execute("UPDATE tasks SET monitor_actions = 1", [])?;
    assert!(store.audit_tasks().is_err());
    Ok(())
}

#[test]
fn v40_migration_preserves_prior_policy_and_rolls_back_conflicting_ddl() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let store = fixture(&path)?;
    let policy = store.monitor_version("world", 1)?;
    crate::storage::tasks::collection::remove_collection_schema(&store)?;
    store
        .connection
        .execute_batch("DROP TABLE task_run_events; DROP TABLE task_run_intents; DROP TABLE task_runs; DROP TABLE task_checkpoints; DROP TABLE tasks; PRAGMA user_version = 40")?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert_eq!(reopened.monitor_version("world", 1)?, policy);
    reopened.create_task("new", &spec(), 2)?;
    assert_eq!(
        reopened.task("new")?.monitor_spec_sha256,
        policy.spec_sha256
    );
    let conflict_path = root.path().join("conflict");
    let conflict = fixture(&conflict_path)?;
    crate::storage::tasks::collection::remove_collection_schema(&conflict)?;
    conflict.connection.execute_batch("DROP TABLE task_run_events; DROP TABLE task_run_intents; DROP TABLE task_runs; DROP TABLE task_checkpoints; DROP TABLE tasks; CREATE TABLE tasks(conflict INTEGER); PRAGMA user_version = 40")?;
    drop(conflict);
    assert!(Store::open(&conflict_path).is_err());
    let connection = rusqlite::Connection::open(conflict_path)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 40);
    let checkpoints: u32 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'task_checkpoints'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(checkpoints, 0);
    Ok(())
}

#[test]
fn catalog_snapshot_preserves_logical_checkpoint_order_after_physical_reordering() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    store.create_task("task", &spec(), 2)?;
    let first = store.checkpoint_task("task", "first", 0, 3)?;
    let second = store.checkpoint_task("task", "second", 1, 4)?;
    let guard: String = store.connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE name = 'task_checkpoint_no_update'",
        [],
        |row| row.get(0),
    )?;
    store.connection.execute_batch(
        "DROP TRIGGER task_checkpoint_no_update; UPDATE task_checkpoints SET rowid = -ordinal",
    )?;
    store.connection.execute_batch(&guard)?;
    let snapshot = root.path().join("snapshot");
    store.snapshot_catalog(&snapshot)?;
    let reopened = Store::open(&snapshot)?;
    assert_eq!(reopened.task_checkpoint("task", 1)?, first);
    assert_eq!(reopened.task_checkpoint("task", 2)?, second);
    assert_eq!(reopened.task("task")?.checkpoint, 2);
    assert!(
        reopened
            .connection
            .execute("UPDATE task_checkpoints SET observed_ms = 5", [])
            .is_err()
    );
    Ok(())
}

#[test]
fn sql_rejects_stale_scope_and_observation_times_without_inserting() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    store.create_task("task", &spec(), 2)?;
    store.checkpoint_task("task", "first", 0, 3)?;
    store.propose_monitor_action("world", "pause", ActionOrigin::User, &Proposal::Pause, 4)?;
    let scope = serde_json::to_string(&spec())?;
    let digest = scope_hash(&scope, &store.monitor_version("world", 1)?.spec_sha256);
    assert!(store.connection.execute("INSERT INTO tasks SELECT 'stale', ?1, ?2, monitor_id, monitor_version, monitor_actions, monitor_spec_sha256, from_ms, to_ms, template, amount_micros, 5 FROM tasks WHERE id = 'task'", params![scope, digest]).is_err());
    assert_eq!(count(&store, "tasks")?, 1);
    assert_eq!(count(&store, "task_checkpoints")?, 1);
    Ok(())
}
