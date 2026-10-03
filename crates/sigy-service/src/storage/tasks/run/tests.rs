use super::*;
mod exact_migration;
use crate::{
    monitor::{ActionOrigin, MonitorSpec, MonitorTerm, Proposal},
    sources::{HttpSource, NetworkScope},
    task::TaskSpec,
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn fixture(path: &std::path::Path) -> Result<Store> {
    let mut store = Store::open(path)?;
    store.register_source(
        "a:v1",
        &HttpSource::new(
            "Public",
            "https://example.com/audio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    store.create_monitor(
        "world",
        &MonitorSpec {
            name: "Water".into(),
            goal: "Observe water".into(),
            terms: vec![MonitorTerm {
                language: "und".into(),
                text: "water".into(),
            }],
            sources: vec!["a:v1".into()],
            candidate_sources: vec![],
            schedules: vec![],
            daily_audio_seconds: 60,
            total_audio_seconds: 60,
            recognition_profile: None,
            translation_profile: None,
            capture: None,
        },
        1,
    )?;
    Ok(store)
}

fn scope() -> TaskSpec {
    TaskSpec {
        goal: "Observe water".into(),
        monitor_id: "world".into(),
        monitor_version: 1,
        monitor_actions: 0,
        from_ms: 0,
        to_ms: 100,
    }
}

fn spec() -> TaskRunSpec {
    TaskRunSpec {
        checkpoint_ordinal: 1,
        maximum_findings: 64,
    }
}

fn accept(store: &mut Store, id: &str) -> Result<TaskRunView> {
    store.create_task(id, &scope(), 2)?;
    store.checkpoint_task(id, "observed", 0, 100)?;
    store.start_task_run(id, "accepted", &spec(), 0, 101)
}

fn count(store: &Store, table: &str) -> Result<u32> {
    Ok(store
        .connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?)
}

#[test]
fn empty_checkpoint_materializes_one_frozen_partial_briefing_and_reopens() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    assert!(store.task_run("absent")?.is_none());
    let accepted = accept(&mut store, "task")?;
    assert_eq!(accepted.generation, 1);
    assert_eq!(accepted.planned_findings, 0);
    assert!(accepted.steps.is_empty());
    assert_eq!(count(&store, "monitor_briefings")?, 0);
    let done = store.advance_task_run("task", 1, 102)?;
    assert_eq!(done.generation, 2);
    assert_eq!(done.state, TaskRunState::Partial);
    let briefing = store.briefing(
        "world",
        done.briefing_id.as_deref().ok_or(Error::StorageIntegrity)?,
    )?;
    assert_eq!(
        briefing.coverage,
        store.task_checkpoint("task", 1)?.coverage
    );
    assert!(briefing.members.is_empty());
    assert_eq!(store.advance_task_run("task", 2, 103)?, done);
    assert!(matches!(
        store.advance_task_run("task", 1, 103),
        Err(Error::IdempotencyConflict)
    ));
    for table in [
        "capture_jobs",
        "analysis_jobs",
        "translation_jobs",
        "provider_attempts",
        "ledger_events",
    ] {
        assert_eq!(count(&store, table)?, 0);
    }
    assert_eq!(
        store.pending_task_run_ids(None, 16)?.0,
        Vec::<String>::new()
    );
    drop(store);
    assert_eq!(Store::open(&path)?.task_run("task")?, Some(done));
    Ok(())
}

#[test]
fn immutable_start_replays_after_policy_drift_and_changed_requests_conflict() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    let accepted = accept(&mut store, "task")?;
    store.propose_monitor_action(
        "world",
        "note",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "Untrusted change".into(),
        },
        102,
    )?;
    assert_eq!(
        store.start_task_run("task", "accepted", &spec(), 0, -1)?,
        accepted
    );
    for (request, run_spec, generation) in [
        ("other", spec(), 0),
        (
            "accepted",
            TaskRunSpec {
                maximum_findings: 1,
                ..spec()
            },
            0,
        ),
        ("accepted", spec(), 1),
    ] {
        assert!(matches!(
            store.start_task_run("task", request, &run_spec, generation, 103),
            Err(Error::IdempotencyConflict)
        ));
    }
    assert!(
        store
            .start_task_run("other", "new", &spec(), 0, 103)
            .is_err()
    );
    let revoked = store.advance_task_run("task", 1, 103)?;
    assert_eq!(revoked.state, TaskRunState::Revoked);
    assert_eq!(
        revoked.steps[0].reason.as_deref(),
        Some("monitor-scope-changed")
    );
    assert!(revoked.briefing_id.is_none());
    assert_eq!(count(&store, "monitor_findings")?, 0);
    assert_eq!(count(&store, "monitor_briefings")?, 0);
    Ok(())
}

#[test]
fn cancellation_is_versioned_replayable_and_survives_catalog_restore() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    accept(&mut store, "task")?;
    assert!(store.cancel_task_run("task", "cancel", 0, 102).is_err());
    assert!(store.cancel_task_run("task", "cancel", 1, 100).is_err());
    let cancelled = store.cancel_task_run("task", "cancel", 1, 102)?;
    assert_eq!(cancelled.state, TaskRunState::Cancelled);
    assert_eq!(cancelled.generation, 2);
    assert_eq!(store.cancel_task_run("task", "cancel", 1, -1)?, cancelled);
    assert!(store.cancel_task_run("task", "cancel", 2, 103).is_err());
    assert!(store.cancel_task_run("task", "another", 2, 103).is_err());
    assert_eq!(store.advance_task_run("task", 2, 103)?, cancelled);
    let snapshot = root.path().join("snapshot");
    store.snapshot_catalog(&snapshot)?;
    let restored = Store::open(&snapshot)?;
    assert_eq!(restored.task_run("task")?, Some(cancelled));
    assert_eq!(count(&restored, "monitor_briefings")?, 0);
    Ok(())
}

#[test]
fn paused_scope_missing_checkpoint_and_clock_bounds_admit_nothing() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    store.create_task("task", &scope(), 2)?;
    assert!(
        store
            .start_task_run("task", "run", &spec(), 0, 101)
            .is_err()
    );
    store.checkpoint_task("task", "first", 0, 100)?;
    assert!(store.start_task_run("task", "run", &spec(), 0, 99).is_err());
    assert!(
        store
            .start_task_run(
                "task",
                "run",
                &TaskRunSpec {
                    maximum_findings: 65,
                    ..spec()
                },
                0,
                101
            )
            .is_err()
    );
    store.propose_monitor_action("world", "pause", ActionOrigin::User, &Proposal::Pause, 101)?;
    assert!(
        store
            .start_task_run("task", "run", &spec(), 0, 102)
            .is_err()
    );
    let paused = TaskSpec {
        monitor_actions: 1,
        ..scope()
    };
    store.create_task("paused", &paused, 102)?;
    store.checkpoint_task("paused", "first", 0, 103)?;
    assert!(
        store
            .start_task_run("paused", "run", &spec(), 0, 104)
            .is_err()
    );
    assert_eq!(count(&store, "task_runs")?, 0);
    assert_eq!(count(&store, "task_run_intents")?, 0);
    Ok(())
}

#[test]
fn receipt_failure_rolls_back_the_effect_and_next_pass_retries_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    accept(&mut store, "task")?;
    store.connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON task_run_events BEGIN SELECT RAISE(ABORT, 'fixture receipt failure'); END")?;
    assert!(store.advance_task_run("task", 1, 102).is_err());
    assert_eq!(count(&store, "monitor_briefings")?, 0);
    assert_eq!(count(&store, "monitor_briefing_coverage")?, 0);
    assert_eq!(count(&store, "task_run_events")?, 0);
    assert_eq!(
        store
            .task_run("task")?
            .ok_or(Error::StorageIntegrity)?
            .generation,
        1
    );
    store
        .connection
        .execute_batch("DROP TRIGGER fail_receipt")?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    let done = reopened.advance_task_run("task", 1, 103)?;
    assert_eq!(count(&reopened, "monitor_briefings")?, 1);
    assert_eq!(count(&reopened, "task_run_events")?, 1);
    assert_eq!(reopened.advance_task_run("task", 2, 104)?, done);
    Ok(())
}

#[test]
fn pending_run_pages_are_bounded_and_terminal_runs_leave_the_queue() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    for id in ["a", "b", "c"] {
        accept(&mut store, id)?;
    }
    assert_eq!(
        store.pending_task_run_ids(None, 2)?,
        (vec!["a".into(), "b".into()], Some("b".into()))
    );
    assert_eq!(
        store.pending_task_run_ids(Some("b"), 2)?,
        (vec!["c".into()], None)
    );
    store.cancel_task_run("b", "cancel", 1, 102)?;
    assert_eq!(store.pending_task_run_ids(None, 16)?.0, vec!["a", "c"]);
    assert!(store.pending_task_run_ids(None, 0).is_err());
    assert!(store.pending_task_run_ids(None, 17).is_err());
    assert!(store.pending_task_run_ids(Some("../bad"), 1).is_err());
    Ok(())
}

#[test]
fn corrupted_grant_and_intent_identity_fail_reopen_independently() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    accept(&mut store, "task")?;
    assert!(
        store
            .connection
            .execute("UPDATE task_runs SET created_ms = 102", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM task_run_intents", [])
            .is_err()
    );
    store.connection.execute_batch(
        "DROP TRIGGER task_run_intent_no_update; UPDATE task_run_intents SET effect_id = 'forged'",
    )?;
    assert!(store.task_run("task").is_err());
    drop(store);
    assert!(Store::open(&path).is_err());
    let second = root.path().join("second");
    let mut store = fixture(&second)?;
    accept(&mut store, "task")?;
    store.connection.execute_batch(
        "DROP TRIGGER task_run_no_update; UPDATE task_runs SET grant_sha256 = printf('%064d', 0)",
    )?;
    assert!(store.audit_task_runs().is_err());
    Ok(())
}

#[test]
fn v41_migration_preserves_scopes_and_conflicting_ddl_rolls_back() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    store.create_task("task", &scope(), 2)?;
    let before = store.task("task")?;
    crate::storage::tasks::collection::remove_collection_schema(&store)?;
    store
        .connection
        .execute_batch(include_str!("../../037-findings.sql"))?;
    store.connection.execute_batch("DROP TABLE task_run_events; DROP TABLE task_run_intents; DROP TABLE task_runs; PRAGMA user_version = 41")?;
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(reopened.task("task")?, before);
    crate::storage::tasks::collection::remove_collection_schema(&reopened)?;
    reopened.connection.execute_batch("DROP TABLE task_run_events; DROP TABLE task_run_intents; DROP TABLE task_runs; CREATE TABLE task_run_events(conflict INTEGER); PRAGMA user_version = 41")?;
    drop(reopened);
    assert!(Store::open(&path).is_err());
    let connection = Connection::open(&path)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 41);
    let grants: u32 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'task_runs'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(grants, 0);
    Ok(())
}

#[test]
fn manually_published_briefing_collision_is_partial_without_claiming_ownership() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    accept(&mut store, "task")?;
    let grant = store
        .checked_run_grant("task")?
        .ok_or(Error::StorageIntegrity)?;
    let id = effect_id(&grant.sha256, "briefing", 1);
    let manual = store.publish_briefing("world", &id, 0, 100, 101)?;
    let done = store.advance_task_run("task", 1, 102)?;
    assert_eq!(done.state, TaskRunState::Partial);
    assert_eq!(done.steps[0].reason.as_deref(), Some("effect-conflict"));
    assert!(done.briefing_id.is_none());
    assert_eq!(store.briefing("world", &id)?, manual);
    assert_eq!(count(&store, "monitor_briefings")?, 1);
    drop(store);
    assert_eq!(Store::open(&path)?.task_run("task")?, Some(done));
    Ok(())
}

#[test]
fn later_independent_artifact_does_not_rewrite_or_invalidate_a_skipped_receipt() -> TestResult {
    for independent_time in [102, 103] {
        let root = tempfile::tempdir()?;
        let path = root.path().join("catalog");
        let mut store = fixture(&path)?;
        accept(&mut store, "task")?;
        store.connection.execute_batch("CREATE TRIGGER transient_briefing_limit BEFORE INSERT ON monitor_briefings BEGIN SELECT RAISE(ABORT, 'briefing limit'); END")?;
        let skipped = store.advance_task_run("task", 1, 102)?;
        assert_eq!(skipped.state, TaskRunState::Partial);
        assert!(skipped.briefing_id.is_none());
        assert_eq!(skipped.steps[0].reason.as_deref(), Some("briefing-limit"));
        assert_eq!(count(&store, "monitor_briefings")?, 0);
        store
            .connection
            .execute_batch("DROP TRIGGER transient_briefing_limit")?;
        store.publish_briefing(
            "world",
            &skipped.steps[0].effect_id,
            0,
            100,
            independent_time,
        )?;
        assert_eq!(store.task_run("task")?, Some(skipped.clone()));
        drop(store);
        assert_eq!(Store::open(&path)?.task_run("task")?, Some(skipped));
    }
    Ok(())
}
