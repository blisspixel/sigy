use super::*;
use crate::{
    monitor::{ActionOrigin, MonitorCaptureBounds, MonitorSpec, MonitorTerm, Proposal},
    sources::{HttpSource, NetworkScope},
    task::TaskSpec,
};

mod integrity;
mod recovery;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const START: i64 = 1_790_078_400_000;

fn policy() -> MonitorSpec {
    MonitorSpec {
        name: "Reports".into(),
        goal: "Follow literal reports".into(),
        terms: vec![MonitorTerm {
            language: "und".into(),
            text: "water".into(),
        }],
        sources: vec!["a:v1".into(), "b:v1".into()],
        candidate_sources: vec![],
        schedules: vec![],
        daily_audio_seconds: 120,
        total_audio_seconds: 120,
        recognition_profile: None,
        translation_profile: None,
        capture: Some(MonitorCaptureBounds {
            daily_seconds: 120,
            total_seconds: 120,
            total_bytes: 4096,
        }),
    }
}

fn setup(path: &std::path::Path) -> Result<Store> {
    let mut store = Store::open(path)?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        std::env::current_exe()?
            .to_str()
            .ok_or(Error::StorageIntegrity)?,
    )?;
    for source in ["a:v1", "b:v1", "c:v1"] {
        store.register_source(
            source,
            &HttpSource::new(
                "Public",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
    }
    store.create_monitor("monitor", &policy(), START - 3000)?;
    store.create_task(
        "task",
        &TaskSpec {
            goal: "Follow reports".into(),
            monitor_id: "monitor".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: START,
            to_ms: START + 900_000,
        },
        START - 2000,
    )?;
    Ok(store)
}

fn spec() -> TaskCollectionSpec {
    TaskCollectionSpec {
        captures: vec![
            TaskCaptureSpec {
                source_revision: "a:v1".into(),
                start_ms: START,
                duration_seconds: 30,
                maximum_bytes: 1024,
            },
            TaskCaptureSpec {
                source_revision: "b:v1".into(),
                start_ms: START + 60_000,
                duration_seconds: 30,
                maximum_bytes: 1024,
            },
        ],
    }
}

fn count(store: &Store, table: &str) -> Result<u32> {
    Ok(store
        .connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?)
}

fn assert_no_admission(store: &Store) -> Result<()> {
    for table in [
        "capture_jobs",
        "recordings",
        "task_collection_admissions",
        "monitor_capture_admissions",
        "analysis_jobs",
        "translation_jobs",
    ] {
        assert_eq!(count(store, table)?, 0, "{table}");
    }
    Ok(())
}

#[test]
fn new_rules_and_finite_grant_are_atomic_and_replay_is_read_only() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    let view = store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    assert_eq!(
        (view.generation, view.cancelled, view.captures.len()),
        (1, false, 2)
    );
    assert_eq!(view.paid_allowance_usd, "0.000000");
    assert!(
        view.captures
            .iter()
            .all(|capture| capture.state == "waiting" && capture.recording_id.is_none())
    );
    assert_no_admission(&store)?;
    assert_eq!(count(&store, "schedule_rules")?, 2);
    assert_eq!(count(&store, "monitor_capture_rules")?, 2);
    assert_eq!(
        store.start_task_collection("task", "collect", &spec(), 0, 0)?,
        view
    );
    for (request, generation) in [("other", 0), ("collect", 1)] {
        assert!(matches!(
            store.start_task_collection("task", request, &spec(), generation, START),
            Err(Error::IdempotencyConflict)
        ));
    }
    let mut other = spec();
    other.captures[0].maximum_bytes += 1;
    assert!(matches!(
        store.start_task_collection("task", "collect", &other, 0, START),
        Err(Error::IdempotencyConflict)
    ));
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(reopened.task_collection("task")?, Some(view));
    Ok(())
}

#[test]
fn failed_second_rule_or_commit_rolls_back_the_entire_grant() -> TestResult {
    for fault in [
        "CREATE TRIGGER fixture BEFORE INSERT ON task_collection_rules WHEN NEW.ordinal = 1 BEGIN SELECT RAISE(ABORT, 'fault'); END",
        "CREATE TABLE fixture_parent(id INTEGER PRIMARY KEY); CREATE TABLE fixture_child(id INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fixture AFTER INSERT ON task_collections BEGIN INSERT INTO fixture_child VALUES(1); END",
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = setup(&path)?;
        store.connection.execute_batch(fault)?;
        assert!(
            store
                .start_task_collection("task", "collect", &spec(), 0, START - 1000)
                .is_err()
        );
        for table in [
            "task_collections",
            "task_collection_rules",
            "schedule_rules",
            "schedule_occurrences",
            "monitor_capture_rules",
        ] {
            assert_eq!(count(&store, table)?, 0);
        }
        store.connection.execute_batch("DROP TRIGGER fixture")?;
        drop(store);
        let mut reopened = Store::open(&path)?;
        reopened.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
        assert_eq!(count(&reopened, "schedule_rules")?, 2);
        assert_no_admission(&reopened)?;
    }
    Ok(())
}

#[test]
fn capture_receipt_failure_rolls_back_dvr_and_monitor_reservations() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    store.connection.execute_batch("CREATE TRIGGER fixture BEFORE INSERT ON task_collection_admissions BEGIN SELECT RAISE(ABORT, 'fault'); END")?;
    assert!(store.reconcile_schedules_at(START, true).is_err());
    assert_no_admission(&store)?;
    assert_eq!(
        store.task_collection("task")?.ok_or("collection")?.captures[0].state,
        "waiting"
    );
    store.connection.execute_batch("DROP TRIGGER fixture")?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert_eq!(
        reopened.reconcile_schedules_at(START, true)?.launches.len(),
        1
    );
    assert_eq!(count(&reopened, "task_collection_admissions")?, 1);
    assert_eq!(
        reopened
            .monitor_capture_usage("monitor", START)?
            .used_total_seconds,
        30
    );
    assert!(
        reopened
            .reconcile_schedules_at(START, true)?
            .launches
            .is_empty()
    );
    Ok(())
}

#[test]
fn cancellation_preserves_admitted_capture_and_fences_only_future_task_rules() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    let initial = store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    assert_eq!(store.reconcile_schedules_at(START, true)?.launches.len(), 1);
    let independent = ScheduleDraft {
        id: "independent".into(),
        source_revision: "c:v1".into(),
        ..draft(&initial.grant_sha256, 1, &spec().captures[1])?
    };
    store.create_schedule_at(&independent, START)?;
    let cancelled = store.cancel_task_collection("task", "cancel", 1, START)?;
    assert!(cancelled.cancelled);
    assert_eq!(cancelled.generation, 2);
    assert_eq!(cancelled.hold_reason.as_deref(), Some("cancelled"));
    assert_eq!(
        cancelled.captures[0].recording_state.as_deref(),
        Some("starting")
    );
    assert_eq!(
        store.cancel_task_collection("task", "cancel", 1, 0)?,
        cancelled
    );
    assert!(
        store
            .cancel_task_collection("task", "other", 1, START)
            .is_err()
    );
    assert!(
        store
            .cancel_task_collection("task", "cancel", 2, START)
            .is_err()
    );
    let launches = store.reconcile_schedules_at(START + 60_000, true)?.launches;
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].source_revision, "c:v1");
    assert_eq!(count(&store, "task_collection_admissions")?, 1);
    assert_eq!(
        store
            .monitor_capture_usage("monitor", START)?
            .reserved_total_bytes,
        1024
    );
    let snapshot = directory.path().join("snapshot");
    store.snapshot_catalog(&snapshot)?;
    let restored = Store::open(&snapshot)?;
    assert_eq!(
        restored.task_collection("task")?,
        store.task_collection("task")?
    );
    assert_eq!(
        restored.monitor_capture_usage("monitor", START)?,
        store.monitor_capture_usage("monitor", START)?
    );
    Ok(())
}

#[test]
fn every_action_and_policy_drift_holds_task_collection_without_changing_independent_capture()
-> TestResult {
    for change in ["refused", "pause", "policy"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = setup(&path)?;
        let initial = store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
        let independent = ScheduleDraft {
            id: "independent".into(),
            source_revision: "b:v1".into(),
            ..draft(&initial.grant_sha256, 0, &spec().captures[0])?
        };
        store.create_monitor_schedule_at(
            &independent,
            &MonitorScheduleOwner {
                monitor_id: "monitor".into(),
                version: 1,
            },
            START - 1000,
        )?;
        if change == "policy" {
            let mut revised = policy();
            revised.name = "Updated".into();
            store.revise_monitor("monitor", 1, &revised, START - 500)?;
        } else {
            let proposal = if change == "pause" {
                Proposal::Pause
            } else {
                Proposal::Other {
                    request: "increase allowance".into(),
                }
            };
            store.propose_monitor_action(
                "monitor",
                "change",
                ActionOrigin::User,
                &proposal,
                START - 500,
            )?;
        }
        let launches = store.reconcile_schedules_at(START, true)?.launches;
        assert_eq!(launches.len(), 1);
        assert_eq!(launches[0].source_revision, "b:v1");
        let view = store.task_collection("task")?.ok_or("collection")?;
        assert_eq!(view.hold_reason.as_deref(), Some("scope-changed"));
        assert_eq!(count(&store, "task_collection_admissions")?, 0);
        assert_eq!(
            store.start_task_collection("task", "collect", &spec(), 0, 0)?,
            view
        );
        drop(store);
        assert!(Store::open(&path)?.task_collection("task")?.is_some());
    }
    Ok(())
}

#[test]
fn backward_clock_holds_against_admission_and_task_checkpoint_receipts() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"))?;
    let mut plan = spec();
    plan.captures[0].duration_seconds = 90;
    store.checkpoint_task("task", "observe", 0, START - 100)?;
    assert!(
        store
            .start_task_collection("task", "collect", &plan, 0, START - 101)
            .is_err()
    );
    let grant = store.start_task_collection("task", "collect", &plan, 0, START)?;
    let blocker = ScheduleDraft {
        id: "0-blocker".into(),
        source_revision: "c:v1".into(),
        ..draft(
            &grant.grant_sha256,
            0,
            &TaskCaptureSpec {
                start_ms: START + 60_000,
                ..plan.captures[0].clone()
            },
        )?
    };
    store.create_schedule_at(&blocker, START)?;
    store.checkpoint_task("task", "future", 1, START + 10_000)?;
    assert!(
        store
            .reconcile_schedules_at(START + 5000, true)?
            .launches
            .is_empty()
    );
    let mut launches = store.reconcile_schedules_at(START + 65_000, true)?.launches;
    assert_eq!(launches.len(), 2);
    let blocker = launches.remove(0);
    assert_eq!(blocker.source_revision, "c:v1");
    store.fail_recording(&blocker.job.version, &Error::InvalidInput("fixture"))?;
    assert_eq!(count(&store, "task_collection_admissions")?, 1);
    assert!(
        store
            .reconcile_schedules_at(START + 61_000, true)?
            .launches
            .is_empty()
    );
    assert!(
        store
            .cancel_task_collection("task", "cancel", 1, START + 61_000)
            .is_err()
    );
    assert_eq!(
        store
            .reconcile_schedules_at(START + 65_000, true)?
            .launches
            .len(),
        1
    );
    assert_eq!(count(&store, "task_collection_admissions")?, 2);
    let collection = store.task_collection("task")?.ok_or("collection")?;
    assert_eq!(collection.updated_ms, START + 65_000);
    for capture in collection.captures {
        let gaps = store
            .recording(capture.recording_id.as_deref().ok_or("recording")?)?
            .gaps;
        assert!(
            gaps.iter()
                .any(|gap| gap.cause == crate::storage::dvr::GapCause::LateStart)
        );
    }
    Ok(())
}
