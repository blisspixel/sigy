//! A task publication pass is bounded and does not dispatch processing or capture.

use super::*;
use crate::{
    monitor::{MonitorSpec, MonitorTerm},
    task::{
        TaskSpec,
        run::{TaskRunSpec, TaskRunState},
    },
};

fn admit_tasks(actor: &mut Actor, count: u32) -> Result<()> {
    let now = crate::storage::now_ms()?;
    let store = actor.library.store_mut();
    store.create_monitor(
        "task-monitor",
        &MonitorSpec {
            name: "Task fixture".into(),
            goal: "Observe water".into(),
            terms: vec![MonitorTerm {
                language: "und".into(),
                text: "water".into(),
            }],
            sources: vec!["radio:v1".into()],
            candidate_sources: vec![],
            schedules: vec![],
            daily_audio_seconds: 60,
            total_audio_seconds: 60,
            recognition_profile: None,
            translation_profile: None,
            capture: None,
        },
        now,
    )?;
    for number in 0..count {
        let id = format!("task-{number}");
        store.create_task(
            &id,
            &TaskSpec {
                goal: "Observe water".into(),
                monitor_id: "task-monitor".into(),
                monitor_version: 1,
                monitor_actions: 0,
                from_ms: 0,
                to_ms: 1,
            },
            now,
        )?;
        store.checkpoint_task(&id, "checkpoint", 0, now)?;
        store.start_task_run(
            &id,
            "execute",
            &TaskRunSpec {
                checkpoint_ordinal: 1,
                maximum_findings: 1,
            },
            0,
            now,
        )?;
    }
    Ok(())
}

#[tokio::test]
async fn publication_tick_is_bounded_rotates_and_preserves_unowned_work() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (sender, _) = mpsc::channel(8);
    let mut actor = actor(directory.path(), &sender)?;
    admit_tasks(&mut actor, 6)?;
    let jobs_before = count(&actor, "SELECT count(*) FROM analysis_jobs")?;
    let captures_before = count(&actor, "SELECT count(*) FROM capture_jobs")?;
    actor.reconcile_tasks()?;
    assert_eq!(count(&actor, "SELECT count(*) FROM task_run_events")?, 4);
    assert_eq!(actor.task_cursor.as_deref(), Some("task-3"));
    for number in 0..6 {
        let run = actor
            .library
            .store()
            .task_run(&format!("task-{number}"))?
            .ok_or("missing run")?;
        assert_eq!(
            run.state,
            if number < 4 {
                TaskRunState::Partial
            } else {
                TaskRunState::Running
            }
        );
    }
    actor.reconcile_tasks()?;
    assert_eq!(count(&actor, "SELECT count(*) FROM task_run_events")?, 6);
    assert_eq!(actor.task_cursor.as_deref(), Some("task-5"));
    actor.reconcile_tasks()?;
    assert!(actor.task_cursor.is_none());
    assert_eq!(count(&actor, "SELECT count(*) FROM task_run_events")?, 6);
    assert_eq!(
        count(&actor, "SELECT count(*) FROM analysis_jobs")?,
        jobs_before
    );
    assert_eq!(
        count(&actor, "SELECT count(*) FROM capture_jobs")?,
        captures_before
    );
    assert!(!actor.library.store().monitor("task-monitor")?.paused);
    Ok(())
}

#[tokio::test]
async fn cancellation_before_tick_produces_no_artifact_and_dispatch_does_not_stop_service()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let (sender, _) = mpsc::channel(8);
    let mut actor = actor(directory.path(), &sender)?;
    admit_tasks(&mut actor, 1)?;
    actor
        .library
        .store_mut()
        .cancel_task_run("task-0", "stop", 1, crate::storage::now_ms()?)?;
    let (stopped, shutdown) = deliver(&mut actor, Message::Schedules);
    assert!(!stopped);
    assert!(!shutdown);
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_findings")?, 0);
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_briefings")?, 0);
    assert_eq!(count(&actor, "SELECT count(*) FROM task_run_events")?, 1);
    Ok(())
}

#[tokio::test]
async fn regressed_clock_holds_task_publication_without_stopping_service() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (sender, _) = mpsc::channel(8);
    let mut actor = actor(directory.path(), &sender)?;
    admit_tasks(&mut actor, 0)?;
    let now = crate::storage::now_ms()?;
    actor.library.store_mut().create_task(
        "future",
        &TaskSpec {
            goal: "Observe water".into(),
            monitor_id: "task-monitor".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: 0,
            to_ms: 1,
        },
        now,
    )?;
    actor
        .library
        .store_mut()
        .checkpoint_task("future", "checkpoint", 0, now)?;
    let accepted = actor.library.store_mut().start_task_run(
        "future",
        "execute",
        &TaskRunSpec {
            checkpoint_ordinal: 1,
            maximum_findings: 1,
        },
        0,
        now + 60_000,
    )?;
    let (stopped, shutdown) = deliver(&mut actor, Message::Schedules);
    assert!(!stopped);
    assert!(!shutdown);
    assert_eq!(actor.library.store().task_run("future")?, Some(accepted));
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_briefings")?, 0);
    Ok(())
}
