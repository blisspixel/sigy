//! Exact frozen task evidence, through legal collection and publication APIs.

use super::*;
use crate::task::evidence::TaskOutcome;

mod adversarial;
mod bounded;
mod recovery;
mod runtime;

fn clock(store: &Store) -> Result<i64> {
    let latest: i64 = store.connection.query_row(
        "SELECT coalesce(max(moment),0) FROM (SELECT updated_ms AS moment FROM capture_jobs UNION ALL SELECT created_ms FROM analysis_jobs UNION ALL SELECT started_ms FROM analysis_jobs UNION ALL SELECT finished_ms FROM analysis_jobs UNION ALL SELECT created_ms FROM translation_jobs UNION ALL SELECT started_ms FROM translation_jobs UNION ALL SELECT finished_ms FROM translation_jobs)",
        [], |row| row.get(0),
    )?;
    Ok(Store::clock_ms()?.max(latest).max(START))
}

fn recognized(path: &std::path::Path) -> Result<(Store, Vec<String>)> {
    let mut store = setup(path, false)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let start = clock(&store)?;
    for (ordinal, recording) in recordings.iter().enumerate() {
        let request = recognition(&mut store, recording, start + 90_000)?;
        store.enqueue_task_recognition(
            &scope(
                u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?,
                recording,
                AUDIO[ordinal],
            ),
            &request,
            start + 90_000,
        )?;
        hear(
            &mut store,
            &request.id,
            if ordinal == 0 {
                "Informe sobre el agua"
            } else {
                "Sin coincidencias"
            },
            start + 91_000,
        )?;
    }
    Ok((store, recordings))
}

#[test]
fn freeze_replay_preserves_exact_pending_revision_after_completion_and_correction() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, recordings) = recognized(&path)?;
    let start = clock(&store)?;
    let request = translation(&store, &recordings[0], 1)?;
    store.enqueue_task_translation(&scope(0, &recordings[0], 0), &request, start + 92_000)?;
    let snapshot = store.freeze_task_evidence("task", "freeze", 0, start + 93_000)?;
    assert_eq!(snapshot.evidence.outcome, TaskOutcome::Pending);
    assert_eq!(snapshot.evidence.citations.len(), 1);
    assert_eq!(snapshot.evidence.citations[0].transcript_revision, 1);
    assert_eq!(snapshot.evidence.citations[0].translation_revision, None);
    assert_eq!(snapshot.jobs.len(), 3);
    translate(
        &mut store,
        &request.id,
        "Report about water",
        start + 94_000,
    )?;
    store.correct_transcript(
        &request.transcript_id,
        1,
        0,
        "Sin el término anterior",
        start + 95_000,
    )?;
    assert_eq!(
        store.task_evidence("task")?.ok_or("evidence")?.citations[0].translation_revision,
        Some(1)
    );
    assert_eq!(
        store.freeze_task_evidence("task", "freeze", 0, 0)?,
        snapshot
    );
    assert_eq!(store.task_evidence_snapshot("task", 1)?, snapshot);
    drop(store);
    assert_eq!(
        Store::open(&path)?.task_evidence_snapshot("task", 1)?,
        snapshot
    );
    Ok(())
}

#[test]
fn freeze_fault_rolls_back_every_row_and_exact_replay_cannot_change_ordinal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, _) = recognized(&path)?;
    let start = clock(&store)?;
    store.connection.execute_batch("CREATE TRIGGER fixture BEFORE INSERT ON task_evidence_snapshots BEGIN SELECT RAISE(ABORT, 'snapshot fault'); END")?;
    assert!(
        store
            .freeze_task_evidence("task", "freeze", 0, start + 93_000)
            .is_err()
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM task_evidence_snapshots")?,
        0
    );
    store.connection.execute_batch("DROP TRIGGER fixture")?;
    let snapshot = store.freeze_task_evidence("task", "freeze", 0, start + 93_000)?;
    assert!(matches!(
        store.freeze_task_evidence("task", "freeze", 1, start + 94_000),
        Err(Error::IdempotencyConflict)
    ));
    assert_eq!(
        count(&store, "SELECT count(*) FROM task_evidence_snapshots")?,
        1
    );
    drop(store);
    assert_eq!(
        Store::open(&path)?.task_evidence_snapshot("task", 1)?,
        snapshot
    );
    Ok(())
}

#[test]
fn exact_and_legacy_observations_share_capacity_without_changing_legacy_ordinals() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let start = clock(&store)?;
    for ordinal in 0..127 {
        store.checkpoint_task(
            "task",
            &format!("old-{ordinal}"),
            ordinal,
            start + 100_000 + i64::from(ordinal),
        )?;
    }
    let old = store.task_checkpoint("task", 127)?;
    let snapshot = store.freeze_task_evidence("task", "last", 0, start + 101_000)?;
    assert_eq!(snapshot.ordinal, 1);
    assert_eq!(store.task("task")?.checkpoint, 127);
    assert!(
        store
            .checkpoint_task("task", "overflow", 127, start + 102_000)
            .is_err()
    );
    assert!(
        store
            .freeze_task_evidence("task", "overflow", 1, start + 102_000)
            .is_err()
    );
    assert_eq!(store.freeze_task_evidence("task", "last", 0, 0)?, snapshot);
    assert_eq!(store.checkpoint_task("task", "old-126", 126, 0)?, old);
    Ok(())
}

#[test]
fn fresh_freeze_refuses_backwards_clock_and_history_survives_scope_drift() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let start = clock(&store)?;
    let snapshot = store.freeze_task_evidence("task", "first", 0, start + 100_000)?;
    assert!(
        store
            .freeze_task_evidence("task", "second", 1, start + 99_999)
            .is_err()
    );
    let mut changed = policy(false);
    changed.name = "Changed accepted policy".into();
    store.revise_monitor("monitor", 1, &changed, start + 101_000)?;
    assert!(
        store
            .freeze_task_evidence("task", "second", 1, start + 102_000)
            .is_err()
    );
    assert_eq!(store.freeze_task_evidence("task", "first", 0, 0)?, snapshot);
    Ok(())
}
