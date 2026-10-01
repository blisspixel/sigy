use std::path::Path;

use rusqlite::params;

use super::*;
use crate::{
    monitor::{ActionOrigin, MonitorSpec, MonitorTerm, Proposal},
    sources::{HttpSource, NetworkScope},
    task::MAX_CHECKPOINTS,
};

mod integrity;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn monitor_spec() -> MonitorSpec {
    MonitorSpec {
        name: "World reports".into(),
        goal: "Watch references to the fair.".into(),
        terms: vec![MonitorTerm {
            language: "und".into(),
            text: "fair".into(),
        }],
        sources: vec!["a:v1".into(), "b:v1".into()],
        candidate_sources: Vec::new(),
        schedules: Vec::new(),
        daily_audio_seconds: 600,
        total_audio_seconds: 1200,
        recognition_profile: None,
        translation_profile: None,
        capture: None,
    }
}

fn fixture(path: &Path) -> Result<Store> {
    let mut store = Store::open(path)?;
    for id in ["a:v1", "b:v1"] {
        store.register_source(
            id,
            &HttpSource::new(
                "Public",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
    }
    store.create_monitor("world", &monitor_spec(), 1)?;
    Ok(store)
}

fn spec() -> TaskSpec {
    TaskSpec {
        goal: "Follow the world fair".into(),
        monitor_id: "world".into(),
        monitor_version: 1,
        monitor_actions: 0,
        from_ms: 10,
        to_ms: 100,
    }
}

fn count(store: &Store, table: &str) -> Result<u32> {
    Ok(store
        .connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?)
}

#[test]
fn immutable_scope_replays_before_policy_drift_and_starts_no_work() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    assert!(store.create_task("task", &spec(), 2)?);
    assert!(!store.create_task("task", &spec(), 3)?);
    let saved = store.task("task")?;
    assert_eq!(saved.checkpoint, 0);
    assert!(saved.latest_checkpoint.is_none());
    assert!(saved.scope_current);
    assert_eq!(saved.created_ms, 2);
    let mut other = spec();
    other.goal = "Different".into();
    assert!(matches!(
        store.create_task("task", &other, 3),
        Err(Error::IdempotencyConflict)
    ));
    let mut revised = monitor_spec();
    revised.daily_audio_seconds = 300;
    store.revise_monitor("world", 1, &revised, 4)?;
    assert!(!store.task("task")?.scope_current);
    assert!(!store.create_task("task", &spec(), 5)?);
    assert!(store.create_task("fresh-stale", &spec(), 5).is_err());
    for table in [
        "capture_jobs",
        "analysis_jobs",
        "translation_jobs",
        "monitor_findings",
        "monitor_briefings",
    ] {
        assert_eq!(count(&store, table)?, 0, "creation mutated {table}");
    }
    assert_eq!(count(&store, "tasks")?, 1);
    drop(store);
    let reopened = Store::open(&path)?;
    let historical = reopened.task("task")?;
    assert_eq!(historical.scope_sha256, saved.scope_sha256);
    assert_eq!(historical.monitor_spec_sha256, saved.monitor_spec_sha256);
    assert!(!historical.scope_current);
    Ok(())
}

#[test]
fn invalid_creation_rolls_back_and_current_action_sequence_is_required() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    assert!(store.create_task("bad/id", &spec(), 2).is_err());
    assert!(store.create_task("bad", &spec(), -1).is_err());
    assert!(store.create_task("bad", &spec(), 0).is_err());
    let mut wrong = spec();
    wrong.monitor_id = "absent".into();
    assert!(store.create_task("missing", &wrong, 2).is_err());
    wrong = spec();
    wrong.to_ms = wrong.from_ms;
    assert!(store.create_task("reversed", &wrong, 2).is_err());
    store.propose_monitor_action("world", "pause", ActionOrigin::User, &Proposal::Pause, 10)?;
    assert!(store.create_task("old-actions", &spec(), 11).is_err());
    wrong = spec();
    wrong.monitor_actions = 1;
    assert!(store.create_task("backwards", &wrong, 9).is_err());
    assert!(store.create_task("paused", &wrong, 11)?);
    let checkpoint = store.checkpoint_task("paused", "observed", 0, 12)?;
    assert!(checkpoint.monitor_paused);
    assert_eq!(count(&store, "tasks")?, 1);
    Ok(())
}

#[test]
fn checkpoints_are_frozen_idempotent_cas_ordered_and_survive_drift() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    store.create_task("task", &spec(), 2)?;
    let first = store.checkpoint_task("task", "first", 0, 20)?;
    assert_eq!(first.coverage.sources.len(), 2);
    assert_eq!(first.transcripts_scanned, 0);
    assert!(first.citations.is_empty());
    assert!(!first.window_elapsed);
    assert_eq!(store.checkpoint_task("task", "first", 0, 99)?, first);
    assert!(matches!(
        store.checkpoint_task("task", "first", 1, 99),
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        store.checkpoint_task("task", "competing", 0, 30),
        Err(Error::IdempotencyConflict)
    ));
    assert!(store.checkpoint_task("task", "backwards", 1, 19).is_err());
    let second = store.checkpoint_task("task", "second", 1, 100)?;
    assert!(second.window_elapsed);
    assert_eq!(store.task("task")?.checkpoint, 2);
    assert_eq!(store.task_checkpoint("task", 1)?, first);
    store.propose_monitor_action(
        "world",
        "remove",
        ActionOrigin::Model,
        &Proposal::RemoveSource {
            source: "b:v1".into(),
        },
        101,
    )?;
    assert!(!store.task("task")?.scope_current);
    assert!(store.checkpoint_task("task", "new", 2, 102).is_err());
    assert_eq!(store.checkpoint_task("task", "first", 0, 102)?, first);
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(reopened.task_checkpoint("task", 2)?, second);
    assert_eq!(reopened.task_checkpoint("task", 1)?, first);
    assert_eq!(count(&reopened, "task_checkpoints")?, 2);
    Ok(())
}

#[test]
fn bounded_pagination_and_lifetime_capacity_are_explicit() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    for number in 0..MAX_TASKS {
        store.create_task(&format!("task-{number:03}"), &spec(), 2)?;
    }
    assert!(store.create_task("overflow", &spec(), 2).is_err());
    assert!(!store.create_task("task-000", &spec(), 2)?);
    let (first, next) = store.task_ids(None, 2)?;
    assert_eq!(first, vec!["task-000", "task-001"]);
    assert_eq!(next.as_deref(), Some("task-001"));
    let (second, _) = store.task_ids(next.as_deref(), 2)?;
    assert_eq!(second, vec!["task-002", "task-003"]);
    assert_eq!(store.task_ids(Some("task-254"), 2)?.0, vec!["task-255"]);
    assert!(store.task_ids(Some("task-254"), 2)?.1.is_none());
    assert!(store.task_ids(None, 0).is_err());
    assert!(store.task_ids(None, 17).is_err());
    assert!(store.task_ids(Some("/"), 2).is_err());
    store.audit_tasks()?;
    Ok(())
}

#[test]
fn checkpoint_limit_is_durable_and_replay_still_works_at_limit() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    store.create_task("task", &spec(), 2)?;
    for number in 0..MAX_CHECKPOINTS {
        store.checkpoint_task(
            "task",
            &format!("request-{number}"),
            number,
            i64::from(number) + 2,
        )?;
    }
    assert!(
        store
            .checkpoint_task("task", "overflow", MAX_CHECKPOINTS, 200)
            .is_err()
    );
    assert_eq!(
        store.checkpoint_task("task", "request-0", 0, 200)?.ordinal,
        1
    );
    assert!(store.task_checkpoint("task", 0).is_err());
    assert!(store.task_checkpoint("task", MAX_CHECKPOINTS + 1).is_err());
    assert!(store.task_checkpoint("absent", 1).is_err());
    drop(store);
    assert_eq!(
        Store::open(&path)?.task("task")?.checkpoint,
        MAX_CHECKPOINTS
    );
    Ok(())
}

#[test]
fn sql_guards_prevent_scope_edits_and_checkpoint_replacement() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    store.create_task("task", &spec(), 2)?;
    store.checkpoint_task("task", "first", 0, 3)?;
    for sql in [
        "UPDATE tasks SET created_ms = 3",
        "DELETE FROM tasks",
        "UPDATE task_checkpoints SET observed_ms = 4",
        "DELETE FROM task_checkpoints",
    ] {
        assert!(store.connection.execute(sql, []).is_err());
    }
    let payload = serde_json::to_string(&store.task_checkpoint("task", 1)?)?;
    assert!(store.connection.execute(
        "INSERT INTO task_checkpoints SELECT task_id, 3, 'skip', 2, 5, ?1, payload_sha256 FROM task_checkpoints WHERE ordinal = 1", [payload],
    ).is_err());
    Ok(())
}
