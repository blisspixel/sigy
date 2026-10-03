//! Reachable owner withdrawal fixtures. Synthetic completion checks catalog invariants;
//! native termination is exercised separately through the contained executor.

use super::*;

fn admitted(path: &std::path::Path, direct: bool) -> Result<(Store, LocalAsrRequest, Vec<String>)> {
    let mut store = setup(path, false)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let request = recognition(&mut store, &recordings[0], START + 90_000)?;
    if direct {
        store.enqueue_local_asr(&request, START + 90_000)?;
    }
    store.enqueue_task_recognition(
        &scope(0, &recordings[0], AUDIO[0]),
        &request,
        START + 90_001,
    )?;
    Ok((store, request, recordings))
}

#[test]
fn recognition_drain_preserves_observed_regression_and_effective_completion() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, request, _) = admitted(&path, false)?;
    let work = store
        .claim_local_asr(&request.id, "owner", START + 90_002)?
        .ok_or("claim")?;
    let stop = START + 90_003;
    store.withdraw_task_processing("task", "withdraw", 1, 0, stop)?;
    let proof = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Cancelled);
    store.connection.execute_batch("CREATE TRIGGER clock_fixture BEFORE INSERT ON native_stop_completions BEGIN SELECT RAISE(ABORT, 'fixture'); END")?;
    assert!(store.finish_local_asr(&work, &proof, stop - 1).is_err());
    assert_eq!(store.local_asr_job(&request.id)?.state, "cancelling");
    assert!(store.native_completion_unproven()?);
    assert_eq!(
        count(&store, "SELECT count(*) FROM native_stop_completions")?,
        0
    );
    store
        .connection
        .execute_batch("DROP TRIGGER clock_fixture")?;
    store.finish_local_asr(&work, &proof, stop - 1)?;
    assert_eq!(store.local_asr_job(&request.id)?.finished_ms, Some(stop));
    let clocks: (i64, i64) = store.connection.query_row("SELECT observed_clock_ms, completed_ms FROM native_stop_completions WHERE family = 'recognition' AND job_id = ?1", [&request.id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    assert_eq!(clocks, (stop - 1, stop));
    assert!(!store.native_completion_unproven()?);
    drop(store);
    Store::open(&path)?;
    Ok(())
}

#[test]
fn translation_drain_preserves_observed_regression_and_effective_completion() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, request, recordings) = admitted(&path, false)?;
    hear(
        &mut store,
        &request.id,
        "Informe sobre el agua",
        START + 90_002,
    )?;
    let mt = translation(&store, &recordings[0], 1)?;
    store.enqueue_task_translation(&scope(0, &recordings[0], 0), &mt, START + 90_003)?;
    let work = store
        .claim_translation(&mt.id, "owner", START + 90_004)?
        .ok_or("claim")?;
    let stop = START + 90_005;
    store.withdraw_task_processing("task", "withdraw", 1, 0, stop)?;
    let proof = TranslationOutcome::synthetic_fixture(&work.job, TranslationResult::Cancelled);
    store.connection.execute_batch("CREATE TRIGGER clock_fixture BEFORE INSERT ON native_stop_completions BEGIN SELECT RAISE(ABORT, 'fixture'); END")?;
    assert!(store.finish_translation(&work, &proof, stop - 1).is_err());
    assert_eq!(store.translation_job(&mt.id)?.state, "cancelling");
    assert!(store.native_completion_unproven()?);
    assert_eq!(
        count(&store, "SELECT count(*) FROM native_stop_completions")?,
        0
    );
    store
        .connection
        .execute_batch("DROP TRIGGER clock_fixture")?;
    store.finish_translation(&work, &proof, stop - 1)?;
    assert_eq!(store.translation_job(&mt.id)?.finished_ms, Some(stop));
    let clocks: (i64, i64) = store.connection.query_row("SELECT observed_clock_ms, completed_ms FROM native_stop_completions WHERE family = 'translation' AND job_id = ?1", [&mt.id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    assert_eq!(clocks, (stop - 1, stop));
    assert!(!store.native_completion_unproven()?);
    drop(store);
    Store::open(&path)?;
    Ok(())
}

#[test]
fn queued_withdrawal_is_atomic_exact_and_never_refills() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, request, recordings) = admitted(&path, false)?;
    let view = store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_002)?;
    assert_eq!(view.receipt.interests.len(), 1);
    assert_eq!(view.receipt.interests[0].decision, "queued-cancelled");
    assert!(!view.completion_unproven);
    assert_eq!(store.local_asr_job(&request.id)?.state, "cancelled");
    assert_eq!(
        store
            .task_processing("task")?
            .ok_or("processing")?
            .charged_audio_us,
        AUDIO[0]
    );
    assert_eq!(
        store.withdraw_task_processing("task", "withdraw", 1, 0, -1)?,
        view
    );
    assert!(matches!(
        store.withdraw_task_processing("task", "changed", 1, 0, START + 90_003),
        Err(Error::IdempotencyConflict)
    ));
    let second = recognition(&mut store, &recordings[1], START + 90_003)?;
    assert!(
        store
            .enqueue_task_recognition(&scope(1, &recordings[1], AUDIO[1]), &second, START + 90_003)
            .is_err()
    );
    assert!(store.pending_task_processing_ids(None, 4)?.0.is_empty());
    assert!(matches!(
        store.cancel_task_processing("task", "later-legacy-stop", 1, START + 90_001),
        Err(Error::InvalidInput("task processing clock"))
    ));
    drop(store);
    assert_eq!(Store::open(&path)?.task_withdrawal("task")?, Some(view));
    Ok(())
}

#[test]
fn a_direct_first_interest_preserves_shared_work() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, request, _) = admitted(&directory.path().join("catalog"), true)?;
    let view = store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_002)?;
    assert_eq!(view.receipt.interests[0].decision, "shared-preserved");
    assert_eq!(store.local_asr_job(&request.id)?.state, "queued");
    assert_eq!(
        count(&store, "SELECT count(*) FROM native_stop_targets")?,
        0
    );
    Ok(())
}

#[test]
fn a_monitor_first_interest_preserves_shared_work() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), true)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let request = recognition(&mut store, &recordings[0], START + 90_000)?;
    let monitor = crate::storage::monitors::MonitorJobScope {
        monitor_id: "monitor",
        policy_version: 1,
        action_count: 0,
        recording_id: &recordings[0],
        audio_us: AUDIO[0],
    };
    store.enqueue_monitor_recognition(&monitor, &request, START + 90_000)?;
    store.enqueue_task_recognition(
        &scope(0, &recordings[0], AUDIO[0]),
        &request,
        START + 90_001,
    )?;
    let view = store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_002)?;
    assert_eq!(view.receipt.interests[0].decision, "shared-preserved");
    assert_eq!(store.local_asr_job(&request.id)?.state, "queued");
    Ok(())
}

#[test]
fn old_cancellation_replay_does_not_withdraw_but_new_request_can() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, request, _) = admitted(&directory.path().join("catalog"), false)?;
    store.cancel_task_processing("task", "legacy", 1, START + 90_002)?;
    store.cancel_task_processing("task", "legacy", 1, -1)?;
    assert!(store.task_withdrawal("task")?.is_none());
    assert_eq!(store.local_asr_job(&request.id)?.state, "queued");
    store.withdraw_task_processing("task", "withdraw", 2, 0, START + 90_003)?;
    store.cancel_task_processing("task", "legacy", 1, -1)?;
    assert_eq!(store.local_asr_job(&request.id)?.state, "cancelled");
    Ok(())
}

#[test]
fn a_receipt_fault_rolls_back_interest_and_cancellation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, request, _) = admitted(&directory.path().join("catalog"), false)?;
    store.connection.execute_batch("CREATE TRIGGER fixture BEFORE INSERT ON task_interest_withdrawals BEGIN SELECT RAISE(ABORT, 'fixture'); END;")?;
    assert!(
        store
            .withdraw_task_processing("task", "withdraw", 1, 0, START + 90_002)
            .is_err()
    );
    assert_eq!(store.local_asr_job(&request.id)?.state, "queued");
    assert_eq!(
        count(&store, "SELECT count(*) FROM job_interest_withdrawals")?,
        0
    );
    store.connection.execute_batch("DROP TRIGGER fixture")?;
    store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_002)?;
    Ok(())
}

#[test]
fn restart_without_drain_keeps_generation_lease_and_completion_unknown() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, request, recordings) = admitted(&path, false)?;
    store
        .claim_local_asr(&request.id, "old-owner", START + 90_002)?
        .ok_or("claim")?;
    let view = store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_003)?;
    assert!(view.completion_unproven);
    assert!(store.begin_delete(&recordings[0], false).is_err());
    drop(store);
    let mut reopened = Store::open(&path)?;
    reopened.recover_analysis_jobs()?;
    assert_eq!(reopened.local_asr_job(&request.id)?.state, "cancelling");
    assert_eq!(reopened.local_asr_job(&request.id)?.generation, 1);
    assert!(reopened.native_completion_unproven()?);
    assert!(reopened.begin_delete(&recordings[0], false).is_err());
    assert_eq!(reopened.task_withdrawal("task")?, Some(view));
    Ok(())
}

#[test]
fn drained_completion_wins_no_success_after_withdrawal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, request, _) = admitted(&path, false)?;
    let work = store
        .claim_local_asr(&request.id, "owner", START + 90_002)?
        .ok_or("claim")?;
    store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_003)?;
    let proof = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Cancelled);
    store.finish_local_asr(&work, &proof, START + 90_004)?;
    assert!(!store.native_completion_unproven()?);
    assert_eq!(store.local_asr_job(&request.id)?.state, "cancelled");
    assert_eq!(
        count(&store, "SELECT count(*) FROM native_stop_completions")?,
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM transcripts WHERE kind = 'recognition'"
        )?,
        0
    );
    drop(store);
    Store::open(&path)?;
    Ok(())
}

#[test]
fn translation_stop_keeps_original_and_rejects_late_success() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, request, recordings) = admitted(&path, false)?;
    hear(
        &mut store,
        &request.id,
        "Informe sobre el agua",
        START + 90_002,
    )?;
    let mt = translation(&store, &recordings[0], 1)?;
    store.enqueue_task_translation(&scope(0, &recordings[0], 0), &mt, START + 90_003)?;
    let work = store
        .claim_translation(&mt.id, "mt-owner", START + 90_004)?
        .ok_or("translation claim")?;
    let view = store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_005)?;
    assert_eq!(view.receipt.interests[0].decision, "terminal-preserved");
    assert_eq!(view.receipt.interests[1].decision, "running-cancelling");
    let output = TranslationOutcome::synthetic_fixture(
        &work.job,
        TranslationResult::Succeeded(vec![TranslatedCue {
            ordinal: 0,
            state: "translated".into(),
            english: Some("Report about water".into()),
            reason: None,
        }]),
    );
    store.finish_translation(&work, &output, START + 90_006)?;
    assert_eq!(store.translation_job(&mt.id)?.state, "cancelled");
    assert_eq!(count(&store, "SELECT count(*) FROM translations")?, 0);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM transcripts WHERE kind = 'recognition'"
        )?,
        1
    );
    assert!(!store.native_completion_unproven()?);
    drop(store);
    Store::open(&path)?;
    Ok(())
}

#[test]
fn succeeded_reuse_is_distinct_from_cancelled_job_attachment() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let request = recognition(&mut store, &recordings[0], START + 90_000)?;
    store.enqueue_local_asr(&request, START + 90_000)?;
    hear(
        &mut store,
        &request.id,
        "Informe sobre el agua",
        START + 90_001,
    )?;
    let attached = store.enqueue_task_recognition(
        &scope(0, &recordings[0], AUDIO[0]),
        &request,
        START + 90_002,
    )?;
    assert!(!attached.job_created && attached.step_created);
    let second = recognition(&mut store, &recordings[1], START + 90_003)?;
    store.enqueue_local_asr(&second, START + 90_003)?;
    store.cancel_local_asr(&second.id, 1)?;
    assert!(matches!(
        store.enqueue_task_recognition(
            &scope(1, &recordings[1], AUDIO[1]),
            &second,
            START + 90_004
        ),
        Err(Error::Analysis("job-not-attachable"))
    ));
    assert_eq!(
        store
            .task_processing("task")?
            .ok_or("processing")?
            .charged_audio_us,
        AUDIO[0]
    );
    Ok(())
}

#[test]
fn populated_legacy_job_gets_conservative_guard_after_migration() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    let recordings = collect(&mut store)?;
    let request = recognition(&mut store, &recordings[0], START + 90_000)?;
    store.enqueue_local_asr(&request, START + 90_000)?;
    super::super::remove_processing_schema(&store)?;
    store.connection.execute_batch("PRAGMA user_version = 43")?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    reopened.start_task_processing("task", "grant", &spec(60, true), 0, START + 90_001)?;
    reopened.enqueue_task_recognition(
        &scope(0, &recordings[0], AUDIO[0]),
        &request,
        START + 90_002,
    )?;
    let view = reopened.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_003)?;
    assert_eq!(view.receipt.interests[0].decision, "legacy-preserved");
    assert_eq!(reopened.local_asr_job(&request.id)?.state, "queued");
    Ok(())
}

#[test]
fn withdrawal_rows_reject_mutation_and_hostile_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, _, _) = admitted(&path, false)?;
    store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_002)?;
    assert!(
        store
            .connection
            .execute(
                "UPDATE job_interest_withdrawals SET interest_created_ms = 0",
                []
            )
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM task_interest_withdrawals", [])
            .is_err()
    );
    store.connection.execute_batch("DROP TRIGGER task_withdrawal_no_update; UPDATE task_interest_withdrawals SET request_id = 'different'")?;
    drop(store);
    assert!(Store::open(&path).is_err());
    Ok(())
}

#[test]
fn a_completion_receipt_fault_retains_liability_after_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, request, _) = admitted(&path, false)?;
    let work = store
        .claim_local_asr(&request.id, "owner", START + 90_002)?
        .ok_or("claim")?;
    store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_003)?;
    store.connection.execute_batch("CREATE TRIGGER fixture BEFORE INSERT ON native_stop_completions BEGIN SELECT RAISE(ABORT, 'fixture'); END")?;
    let proof = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Cancelled);
    assert!(
        store
            .finish_local_asr(&work, &proof, START + 90_004)
            .is_err()
    );
    assert_eq!(store.local_asr_job(&request.id)?.state, "cancelling");
    assert!(store.native_completion_unproven()?);
    store.connection.execute_batch("DROP TRIGGER fixture")?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    reopened.recover_analysis_jobs()?;
    assert!(reopened.native_completion_unproven()?);
    Ok(())
}

#[test]
fn a_new_monitor_cannot_attach_after_task_cancellation_commits() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), true)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let request = recognition(&mut store, &recordings[0], START + 90_000)?;
    store.enqueue_task_recognition(
        &scope(0, &recordings[0], AUDIO[0]),
        &request,
        START + 90_001,
    )?;
    store.withdraw_task_processing("task", "withdraw", 1, 0, START + 90_002)?;
    let monitor = crate::storage::monitors::MonitorJobScope {
        monitor_id: "monitor",
        policy_version: 1,
        action_count: 0,
        recording_id: &recordings[0],
        audio_us: AUDIO[0],
    };
    assert!(matches!(
        store.enqueue_monitor_recognition(&monitor, &request, START + 90_003),
        Err(Error::Analysis("job-not-attachable"))
    ));
    assert_eq!(
        store
            .monitor_processing("monitor", START + 90_003)?
            .used_total_us,
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM job_interests WHERE authority = 'monitor'"
        )?,
        0
    );
    Ok(())
}
