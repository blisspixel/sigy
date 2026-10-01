//! Populated execution fixtures use valid recognized and translated catalog history.

use super::*;
use crate::{
    monitor::{ActionOrigin, FindingCite, FindingOriginal, Proposal},
    task::{
        TaskSpec,
        run::{TaskRunSpec, TaskRunState},
    },
};

fn fixture(path: &Path, with_translation: bool) -> TestResultFor<(Store, i64)> {
    let mut store = transcribed(path)?;
    store.register_source(
        "quiet:v1",
        &HttpSource::new(
            "Quiet",
            "https://example.com/quiet",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    let mut policy = spec();
    policy.sources.push("quiet:v1".into());
    policy.candidate_sources.clear();
    policy.schedules.clear();
    store.create_monitor("fair", &policy, 25)?;
    if with_translation {
        translate(&mut store, "mt-1", translated(0, "A world fair"), 30)?;
    }
    let starts: i64 = store.connection.query_row(
        "SELECT starts_ms FROM capture_jobs WHERE id = 'one'",
        [],
        |row| row.get(0),
    )?;
    let now = crate::storage::now_ms()?;
    store.create_task(
        "task",
        &TaskSpec {
            goal: "Observe the fair across both sources".into(),
            monitor_id: "fair".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: starts,
            to_ms: starts + 1,
        },
        now,
    )?;
    let checkpoint = store.checkpoint_task("task", "observe", 0, now + 1)?;
    assert_eq!(checkpoint.citations.len(), 1);
    assert_eq!(checkpoint.coverage.sources.len(), 2);
    assert_eq!(checkpoint.coverage.sources[1].captures, 0);
    Ok((store, now))
}

type TestResultFor<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn run_spec() -> TaskRunSpec {
    TaskRunSpec {
        checkpoint_ordinal: 1,
        maximum_findings: 64,
    }
}

fn unrelated(store: &mut Store, now: i64) -> Result<()> {
    store.publish_finding(
        "fair",
        "unrelated",
        &FindingCite {
            transcript_id: "pin".into(),
            transcript_revision: 1,
            translation_revision: 1,
            cue_ordinal: 0,
            original: FindingOriginal::Retained,
        },
        now,
    )?;
    Ok(())
}

fn work_counts(store: &Store) -> Result<(i64, i64, i64, i64)> {
    Ok(store.connection.query_row(
        "SELECT (SELECT count(*) FROM capture_jobs), (SELECT count(*) FROM analysis_jobs), (SELECT count(*) FROM provider_attempts), (SELECT count(*) FROM requests)",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?)
}

#[test]
fn resumed_execution_matches_uninterrupted_effects_and_excludes_unowned_findings() -> TestResult {
    let directory = tempfile::tempdir()?;
    let base = directory.path().join("base.sqlite");
    let (mut store, now) = fixture(&base, true)?;
    unrelated(&mut store, now + 2)?;
    store.start_task_run("task", "execute", &run_spec(), 0, now + 3)?;
    let counts = work_counts(&store)?;
    let coverage = store.task_checkpoint("task", 1)?.coverage;
    drop(store);
    let direct = directory.path().join("direct.sqlite");
    let resumed = directory.path().join("resumed.sqlite");
    std::fs::copy(&base, &direct)?;
    std::fs::copy(&base, &resumed)?;
    let mut direct_store = Store::open(&direct)?;
    let direct_step = direct_store.advance_task_run("task", 1, now + 4)?;
    assert_eq!(direct_step.published_findings, 1);
    let expected = direct_store.advance_task_run("task", direct_step.generation, now + 5)?;
    assert_eq!(
        expected.state,
        TaskRunState::Partial,
        "quiet source has no coverage"
    );
    let mut resumed_store = Store::open(&resumed)?;
    let first = resumed_store.advance_task_run("task", 1, now + 4)?;
    drop(resumed_store);
    let mut resumed_store = Store::open(&resumed)?;
    assert_eq!(resumed_store.task_run("task")?.as_ref(), Some(&first));
    let actual = resumed_store.advance_task_run("task", first.generation, now + 5)?;
    assert_eq!(actual, expected);
    let briefing_id = actual.briefing_id.as_deref().ok_or("briefing missing")?;
    let page = resumed_store.briefing("fair", briefing_id)?;
    assert_eq!(page.members.len(), 1);
    assert_ne!(page.members[0].finding_id, "unrelated");
    assert_eq!(page.coverage, coverage);
    assert_eq!(work_counts(&resumed_store)?, counts);
    assert_eq!(
        resumed_store.start_task_run("task", "execute", &run_spec(), 0, now + 6)?,
        actual
    );
    drop(resumed_store);
    assert_eq!(Store::open(&resumed)?.task_run("task")?, Some(actual));
    Ok(())
}

#[test]
fn cancellation_keeps_committed_finding_and_stops_only_future_publication() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite");
    let (mut store, now) = fixture(&path, true)?;
    let counts = work_counts(&store)?;
    store.start_task_run("task", "execute", &run_spec(), 0, now + 2)?;
    let first = store.advance_task_run("task", 1, now + 3)?;
    assert_eq!(first.published_findings, 1);
    let cancelled = store.cancel_task_run("task", "stop", first.generation, now + 4)?;
    assert_eq!(cancelled.state, TaskRunState::Cancelled);
    assert!(cancelled.briefing_id.is_none());
    assert_eq!(work_counts(&store)?, counts);
    assert!(!store.monitor("fair")?.paused);
    assert_eq!(
        store.advance_task_run("task", cancelled.generation, now + 5)?,
        cancelled
    );
    assert!(
        store
            .finding(
                "fair",
                first.steps[0].finding_id.as_deref().ok_or("finding")?
            )
            .is_ok()
    );
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(
        store.cancel_task_run("task", "stop", first.generation, now + 6)?,
        cancelled
    );
    Ok(())
}

#[test]
fn policy_drift_revokes_pending_effects_without_replacing_history() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite");
    let (mut store, now) = fixture(&path, true)?;
    store.start_task_run("task", "execute", &run_spec(), 0, now + 2)?;
    let first = store.advance_task_run("task", 1, now + 3)?;
    store.propose_monitor_action(
        "fair",
        "pause",
        ActionOrigin::User,
        &Proposal::Pause,
        now + 4,
    )?;
    let revoked = store.advance_task_run("task", first.generation, now + 5)?;
    assert_eq!(revoked.state, TaskRunState::Revoked);
    assert_eq!(revoked.published_findings, 1);
    assert!(revoked.briefing_id.is_none());
    assert_eq!(
        store.start_task_run("task", "execute", &run_spec(), 0, now + 6)?,
        revoked
    );
    drop(store);
    assert_eq!(Store::open(&path)?.task_run("task")?, Some(revoked));
    Ok(())
}

#[test]
fn untranslated_citation_is_explicit_partial_without_a_new_job() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite");
    let (mut store, now) = fixture(&path, false)?;
    let counts = work_counts(&store)?;
    store.start_task_run("task", "execute", &run_spec(), 0, now + 2)?;
    let skipped = store.advance_task_run("task", 1, now + 3)?;
    assert_eq!(
        (skipped.published_findings, skipped.skipped_findings),
        (0, 1)
    );
    assert_eq!(
        skipped.steps[0].reason.as_deref(),
        Some("task-translation-unavailable")
    );
    let final_view = store.advance_task_run("task", skipped.generation, now + 4)?;
    assert_eq!(final_view.state, TaskRunState::Partial);
    assert!(
        store
            .briefing("fair", final_view.briefing_id.as_deref().ok_or("briefing")?)?
            .members
            .is_empty()
    );
    assert_eq!(work_counts(&store)?, counts);
    drop(store);
    assert_eq!(Store::open(&path)?.task_run("task")?, Some(final_view));
    Ok(())
}

#[test]
fn a_manual_finding_collision_is_not_claimed_or_included_in_the_task_briefing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite");
    let (mut store, now) = fixture(&path, true)?;
    store.start_task_run("task", "execute", &run_spec(), 0, now + 2)?;
    let effect: String = store.connection.query_row(
        "SELECT effect_id FROM task_run_intents WHERE task_id = 'task' AND ordinal = 1",
        [],
        |row| row.get(0),
    )?;
    let manual = store.publish_finding(
        "fair",
        &effect,
        &FindingCite {
            transcript_id: "pin".into(),
            transcript_revision: 1,
            translation_revision: 1,
            cue_ordinal: 0,
            original: FindingOriginal::Retained,
        },
        now + 3,
    )?;
    let skipped = store.advance_task_run("task", 1, now + 4)?;
    assert_eq!(skipped.published_findings, 0);
    assert_eq!(skipped.skipped_findings, 1);
    assert_eq!(skipped.steps[0].reason.as_deref(), Some("effect-conflict"));
    let done = store.advance_task_run("task", skipped.generation, now + 5)?;
    assert_eq!(done.state, TaskRunState::Partial);
    let briefing = store.briefing("fair", done.briefing_id.as_deref().ok_or("briefing")?)?;
    assert!(briefing.members.is_empty());
    assert_eq!(store.finding("fair", &effect)?, manual);
    drop(store);
    assert_eq!(Store::open(&path)?.task_run("task")?, Some(done));
    Ok(())
}

#[test]
fn fully_observed_publication_completes_and_keeps_its_original_history_after_retention()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite");
    let (mut store, now) = fixture(&path, true)?;
    let mut policy = spec();
    policy.candidate_sources.clear();
    policy.schedules.clear();
    store.create_monitor("complete", &policy, now + 2)?;
    let mut scope = store.task("task")?.spec;
    scope.monitor_id = "complete".into();
    store.create_task("complete-task", &scope, now + 3)?;
    let checkpoint = store.checkpoint_task("complete-task", "observe", 0, now + 4)?;
    assert_eq!(checkpoint.coverage.sources.len(), 1);
    assert_eq!(checkpoint.coverage.sources[0].translated_cues, 1);
    let counts = work_counts(&store)?;
    store.start_task_run("complete-task", "execute", &run_spec(), 0, now + 5)?;
    let first = store.advance_task_run("complete-task", 1, now + 6)?;
    assert_eq!((first.published_findings, first.skipped_findings), (1, 0));
    let done = store.advance_task_run("complete-task", first.generation, now + 7)?;
    assert_eq!(done.state, TaskRunState::Completed);
    let briefing_id = done.briefing_id.as_deref().ok_or("briefing")?;
    let page = store.briefing("complete", briefing_id)?;
    assert_eq!(page.members.len(), 1);
    assert_eq!(page.coverage, checkpoint.coverage);
    assert_eq!(work_counts(&store)?, counts);
    store.correct_transcript("pin", 1, 0, "Una feria local", now + 8)?;
    store.begin_delete("one", false)?;
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(reopened.task_run("complete-task")?, Some(done.clone()));
    let historical = reopened.briefing("complete", briefing_id)?;
    assert_eq!(historical.coverage, page.coverage);
    assert_eq!(
        historical.members[0].original_script,
        page.members[0].original_script
    );
    assert_eq!(historical.members[0].stale_transcript, Some(true));
    assert_eq!(reopened.recording("one")?.storage_state, "deleting");
    Ok(())
}
