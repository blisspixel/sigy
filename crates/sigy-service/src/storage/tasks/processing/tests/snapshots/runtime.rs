//! Exact publication uses real immutable storage outputs, not broad checkpoint coverage.

use super::*;
use crate::task::run::{TaskRunSelection, TaskRunSpec, TaskRunState, TaskSnapshotRunSpec};

fn selection() -> TaskSnapshotRunSpec {
    TaskSnapshotRunSpec {
        snapshot_ordinal: 1,
        maximum_findings: 64,
    }
}

#[test]
fn a_task_frozen_under_an_already_paused_monitor_cannot_publish() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    store.propose_monitor_action(
        "monitor",
        "pause",
        crate::monitor::ActionOrigin::User,
        &crate::monitor::Proposal::Pause,
        START - 900,
    )?;
    let paused = TaskSpec {
        monitor_actions: 1,
        ..store.task("task")?.spec
    };
    store.create_task("paused", &paused, START - 800)?;
    store.start_task_collection("paused", "collect", &collection(), 0, START - 700)?;
    store.freeze_task_evidence("paused", "freeze", 0, START - 600)?;
    assert!(matches!(
        store.start_task_snapshot_run("paused", "publish", &selection(), 0, START - 500),
        Err(Error::InvalidInput("task monitor paused"))
    ));
    assert!(store.task_run("paused")?.is_none());
    Ok(())
}

fn complete(path: &std::path::Path) -> Result<Store> {
    let (mut store, recordings) = recognized(path)?;
    let now = clock(&store)?;
    for (ordinal, recording) in recordings.iter().enumerate() {
        let request = translation(&store, recording, 1)?;
        store.enqueue_task_translation(
            &scope(
                u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?,
                recording,
                0,
            ),
            &request,
            now + 92_000,
        )?;
        translate(&mut store, &request.id, "No target term", now + 94_000)?;
    }
    store.freeze_task_evidence("task", "freeze", 0, now + 95_000)?;
    Ok(store)
}

#[test]
fn exact_run_publishes_canonical_members_and_distinct_frozen_coverage() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = complete(&path)?;
    let now = clock(&store)?;
    let frozen = store.task_evidence_snapshot("task", 1)?;
    assert_eq!(frozen.evidence.outcome, TaskOutcome::Cited);
    let accepted =
        store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 96_000)?;
    assert_eq!(accepted.spec, TaskRunSelection::Snapshot(selection()));
    assert_eq!(accepted.planned_findings, 1);
    assert!(store.task_evidence_briefing("task")?.is_none());
    let first = store.advance_task_run("task", 1, now + 97_000)?;
    assert_eq!(first.published_findings, 1);
    let done = store.advance_task_run("task", 2, now + 98_000)?;
    assert_eq!(done.state, TaskRunState::Completed);
    let page = store.task_evidence_briefing("task")?.ok_or("briefing")?;
    assert_eq!(page.snapshot.as_ref(), &frozen);
    assert_eq!(page.members.len(), 1);
    assert_eq!(page.members[0].finding_id, done.steps[0].effect_id);
    assert_eq!(
        count(&store, "SELECT count(*) FROM monitor_briefing_coverage")?,
        0
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM task_briefing_evidence")?,
        1
    );
    assert!(matches!(
        store.briefing("monitor", &page.id),
        Err(Error::Analysis("task-evidence-briefing-required"))
    ));
    drop(store);
    let restored = Store::open(&path)?;
    assert_eq!(restored.task_run("task")?, Some(done));
    assert_eq!(restored.task_evidence_briefing("task")?, Some(page));
    Ok(())
}

#[test]
fn an_exact_briefing_without_its_atomic_receipt_is_rejected_on_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = complete(&path)?;
    let now = clock(&store)?;
    store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 96_000)?;
    store.advance_task_run("task", 1, now + 97_000)?;
    store.advance_task_run("task", 2, now + 98_000)?;
    store.audit_briefings()?;
    store.connection.execute_batch(
        "DROP TRIGGER task_run_event_no_delete;
         DELETE FROM task_run_events WHERE task_id='task' AND kind='briefing';",
    )?;
    assert!(matches!(
        store.audit_briefings(),
        Err(Error::CatalogIntegrity)
    ));
    drop(store);
    assert!(matches!(Store::open(&path), Err(Error::CatalogIntegrity)));
    Ok(())
}

#[test]
fn original_only_snapshot_stays_partial_even_when_translation_finishes_later() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, recordings) = recognized(&directory.path().join("catalog"))?;
    let now = clock(&store)?;
    let frozen = store.freeze_task_evidence("task", "freeze", 0, now + 93_000)?;
    let request = translation(&store, &recordings[0], 1)?;
    store.enqueue_task_translation(&scope(0, &recordings[0], 0), &request, now + 94_000)?;
    translate(&mut store, &request.id, "Report about water", now + 95_000)?;
    store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 96_000)?;
    let first = store.advance_task_run("task", 1, now + 97_000)?;
    assert_eq!(
        first.steps[0].reason.as_deref(),
        Some("task-translation-unavailable")
    );
    assert_eq!(first.published_findings, 0);
    let done = store.advance_task_run("task", 2, now + 98_000)?;
    assert_eq!(done.state, TaskRunState::Partial);
    let page = store.task_evidence_briefing("task")?.ok_or("briefing")?;
    assert_eq!(page.snapshot.as_ref(), &frozen);
    assert!(page.members.is_empty());
    assert_eq!(count(&store, "SELECT count(*) FROM monitor_findings")?, 0);
    Ok(())
}

#[test]
fn exact_effect_and_briefing_receipt_faults_rollback_then_resume_once() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = complete(&directory.path().join("catalog"))?;
    let now = clock(&store)?;
    store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 96_000)?;
    for (generation, kind, findings) in [(1, "finding", 0), (2, "briefing", 1)] {
        store.connection.execute_batch(&format!("CREATE TRIGGER fail_receipt BEFORE INSERT ON task_run_events WHEN NEW.kind='{kind}' BEGIN SELECT RAISE(ABORT,'fixture fault'); END"))?;
        assert!(
            store
                .advance_task_run("task", generation, now + 97_000 + i64::from(generation))
                .is_err()
        );
        assert_eq!(
            count(&store, "SELECT count(*) FROM monitor_findings")?,
            findings
        );
        assert_eq!(count(&store, "SELECT count(*) FROM monitor_briefings")?, 0);
        assert_eq!(
            count(&store, "SELECT count(*) FROM task_briefing_evidence")?,
            0
        );
        store
            .connection
            .execute_batch("DROP TRIGGER fail_receipt")?;
        store.advance_task_run("task", generation, now + 97_000 + i64::from(generation))?;
    }
    assert_eq!(count(&store, "SELECT count(*) FROM monitor_findings")?, 1);
    assert_eq!(count(&store, "SELECT count(*) FROM monitor_briefings")?, 1);
    assert_eq!(
        count(&store, "SELECT count(*) FROM task_briefing_evidence")?,
        1
    );
    Ok(())
}

#[test]
fn both_origins_share_one_lifetime_run_slot_and_replay_precedes_scope_drift() -> TestResult {
    for exact_first in [false, true] {
        let directory = tempfile::tempdir()?;
        let mut store = complete(&directory.path().join("catalog"))?;
        let now = clock(&store)?;
        store.checkpoint_task("task", "legacy", 0, now + 96_000)?;
        let legacy = TaskRunSpec {
            checkpoint_ordinal: 1,
            maximum_findings: 64,
        };
        let accepted = if exact_first {
            store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 97_000)?
        } else {
            store.start_task_run("task", "publish", &legacy, 0, now + 97_000)?
        };
        let refusal = if exact_first {
            store.start_task_run("task", "publish", &legacy, 0, now + 98_000)
        } else {
            store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 98_000)
        };
        assert!(matches!(refusal, Err(Error::IdempotencyConflict)));
        let mut changed = policy(false);
        changed.name = "Changed".into();
        store.revise_monitor("monitor", 1, &changed, now + 98_000)?;
        let replay = if exact_first {
            store.start_task_snapshot_run("task", "publish", &selection(), 0, 0)?
        } else {
            store.start_task_run("task", "publish", &legacy, 0, 0)?
        };
        assert_eq!(replay, accepted);
        assert_eq!(
            store.advance_task_run("task", 1, now + 99_000)?.state,
            TaskRunState::Revoked
        );
    }
    Ok(())
}

#[test]
fn exact_cancellation_retains_committed_finding_and_stops_only_future_publication() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = complete(&path)?;
    let now = clock(&store)?;
    store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 96_000)?;
    store.advance_task_run("task", 1, now + 97_000)?;
    let stopped = store.cancel_task_run("task", "stop", 2, now + 98_000)?;
    assert_eq!(stopped.state, TaskRunState::Cancelled);
    assert_eq!(stopped.published_findings, 1);
    assert!(store.task_evidence_briefing("task")?.is_none());
    assert_eq!(store.advance_task_run("task", 3, now + 99_000)?, stopped);
    assert_eq!(count(&store, "SELECT count(*) FROM monitor_findings")?, 1);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM capture_jobs WHERE state='completed'"
        )?,
        2
    );
    drop(store);
    assert_eq!(Store::open(&path)?.task_run("task")?, Some(stopped));
    Ok(())
}

#[test]
fn full_exact_membership_measures_guarded_admission_read_and_tick() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    super::super::bounded_evidence::populated(&mut store, 64, "agua", "No target term")?;
    let now = clock(&store)?;
    let snapshot = store.freeze_task_evidence("task", "freeze", 0, now + 1)?;
    assert_eq!(snapshot.evidence.citations.len(), 64);
    assert!(!snapshot.evidence.more);
    let began = std::time::Instant::now();
    store.start_task_snapshot_run("task", "publish", &selection(), 0, now + 2)?;
    let admission = began.elapsed();
    let mut maximum_tick = std::time::Duration::ZERO;
    for generation in 1..=65 {
        let began = std::time::Instant::now();
        let page = store.advance_task_run("task", generation, now + i64::from(generation) + 2)?;
        maximum_tick = maximum_tick.max(began.elapsed());
        if generation == 65 {
            assert_eq!(page.state, TaskRunState::Completed);
            assert_eq!(page.published_findings, 64);
        }
    }
    let began = std::time::Instant::now();
    let page = store.task_evidence_briefing("task")?.ok_or("briefing")?;
    let read = began.elapsed();
    assert_eq!(page.members.len(), 64);
    assert_eq!(page.snapshot.as_ref(), &snapshot);
    assert_eq!(count(&store, "SELECT count(*) FROM monitor_findings")?, 64);
    store.audit_briefings()?;
    eprintln!(
        "guarded exact 64-member publication: admission={}us max tick={}us final read={}us; default 100ms cooperative bounds, no host-scale qualification",
        admission.as_micros(),
        maximum_tick.as_micros(),
        read.as_micros()
    );
    drop(store);
    assert_eq!(
        Store::open(&path)?.task_evidence_briefing("task")?,
        Some(page)
    );
    Ok(())
}
