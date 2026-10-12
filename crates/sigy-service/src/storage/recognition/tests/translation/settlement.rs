//! Publication and replay faults use synthetic drained outcomes, never native execution.

use std::sync::atomic::Ordering;

use super::super::settlement::{clear_hooks, deny_reads_after_commit, deny_reads_after_update};
use super::*;
use crate::storage::analysis_jobs::TEST_OWNER;

#[test]
fn translation_claim_and_completion_need_no_read_after_commit() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = transcribed(&directory.path().join("catalog"))?;
    store.enqueue_translation(&request("mt")?, 30)?;
    let committed = deny_reads_after_commit(&store.connection)?;
    let work = store
        .claim_translation("mt", TEST_OWNER, 31)?
        .ok_or("claim")?;
    assert!(committed.load(Ordering::SeqCst));
    assert!(store.translation_job("mt").is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.translation_job("mt")?, work.job);
    let result = TranslationOutcome::synthetic_fixture(
        &work.job,
        TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
    );
    let committed = deny_reads_after_commit(&store.connection)?;
    let job = store.finish_translation(&work, &result, 32)?;
    assert!(committed.load(Ordering::SeqCst));
    assert!(store.translation_job("mt").is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.translation_job("mt")?, job);
    assert_eq!(store.translation_page("pin", 1, 1, None)?.pairs.len(), 1);
    Ok(())
}

#[test]
fn translation_response_read_failure_rolls_back_then_cancellation_wins() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = transcribed(&directory.path().join("catalog"))?;
    store.enqueue_translation(&request("mt")?, 30)?;
    deny_reads_after_update(&store.connection, "translation_jobs")?;
    assert!(store.claim_translation("mt", TEST_OWNER, 31).is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.translation_job("mt")?.state, "queued");
    let work = store
        .claim_translation("mt", TEST_OWNER, 31)?
        .ok_or("claim")?;
    let result = TranslationOutcome::synthetic_fixture(
        &work.job,
        TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
    );
    deny_reads_after_update(&store.connection, "translation_jobs")?;
    assert!(store.finish_translation(&work, &result, 32).is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.translation_job("mt")?, work.job);
    assert_eq!(store.latest_translation_revision("pin", 1)?, 0);
    store.cancel_translation("mt", work.job.generation)?;
    assert_eq!(
        store.finish_translation(&work, &result, 33)?.state,
        "cancelled"
    );
    assert_eq!(store.latest_translation_revision("pin", 1)?, 0);
    Ok(())
}

#[test]
fn translation_commit_refusal_returns_no_claim_or_partial_result() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = transcribed(&directory.path().join("catalog"))?;
    store.enqueue_translation(&request("mt")?, 30)?;
    store.connection.commit_hook(Some(|| true))?;
    assert!(store.claim_translation("mt", TEST_OWNER, 31).is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.translation_job("mt")?.state, "queued");
    let work = store
        .claim_translation("mt", TEST_OWNER, 31)?
        .ok_or("claim")?;
    let result = TranslationOutcome::synthetic_fixture(
        &work.job,
        TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
    );
    store.connection.commit_hook(Some(|| true))?;
    assert!(store.finish_translation(&work, &result, 32).is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.translation_job("mt")?, work.job);
    assert_eq!(store.latest_translation_revision("pin", 1)?, 0);
    let finished = store.finish_translation(&work, &result, 33)?;
    assert_eq!(store.finish_translation(&work, &result, 34)?, finished);
    assert_eq!(store.latest_translation_revision("pin", 1)?, 1);
    Ok(())
}

#[test]
fn successful_replay_binds_exact_job_and_every_cue_field_after_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = transcribed(&path)?;
    let (job, work) = store.admit_translation(&request("mt")?, 30)?;
    let work = work.ok_or("work")?;
    let text = "A world fair e\u{301} العربية\n";
    let result = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, text)]),
    );
    let finished = store.finish_translation(&work, &result, 31)?;
    let (next, next_work) = store.admit_translation(&request("next")?, 32)?;
    store.finish_translation(
        &next_work.ok_or("next work")?,
        &TranslationOutcome::synthetic_fixture(
            &next,
            TranslationResult::Succeeded(vec![translated(0, "Later")]),
        ),
        33,
    )?;
    store.begin_delete("one", false)?;
    store.finish_delete("one")?;
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.finish_translation(&work, &result, 99)?, finished);
    for changed in [
        TranslationResult::Succeeded(vec![translated(0, "A world fair é العربية\n")]),
        TranslationResult::Succeeded(vec![translated(0, "different")]),
        TranslationResult::Succeeded(vec![translated(1, text)]),
        TranslationResult::Succeeded(vec![translated(0, text), translated(1, "extra")]),
        TranslationResult::Succeeded(Vec::new()),
        TranslationResult::Succeeded(vec![TranslatedCue {
            ordinal: 0,
            state: "untranslated".into(),
            english: None,
            reason: Some("deadline".into()),
        }]),
        TranslationResult::Failed("deadline"),
        TranslationResult::Cancelled,
    ] {
        let conflict = TranslationOutcome::synthetic_fixture(&job, changed);
        assert!(matches!(
            store.finish_translation(&work, &conflict, 100),
            Err(Error::IdempotencyConflict)
        ));
    }
    assert_eq!(store.latest_translation_revision("pin", 1)?, 2);
    let counts: (u32, u32, u32, u32) = store.connection.query_row(
        "SELECT (SELECT count(*) FROM translations), (SELECT count(*) FROM translation_cues), (SELECT count(*) FROM requests), (SELECT count(*) FROM ledger_events)", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(counts, (2, 2, 0, 0));
    Ok(())
}

#[test]
fn successful_replay_preserves_untranslated_reason_and_detects_missing_membership() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = transcribed(&directory.path().join("catalog"))?;
    let (job, work) = store.admit_translation(&request("mt")?, 30)?;
    let work = work.ok_or("work")?;
    let cue = TranslatedCue {
        ordinal: 0,
        state: "untranslated".into(),
        english: None,
        reason: Some("unsupported-language".into()),
    };
    let result = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![cue.clone()]),
    );
    let finished = store.finish_translation(&work, &result, 31)?;
    assert_eq!(store.finish_translation(&work, &result, 32)?, finished);
    let changed = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![TranslatedCue {
            reason: Some("deadline".into()),
            ..cue
        }]),
    );
    assert!(matches!(
        store.finish_translation(&work, &changed, 33),
        Err(Error::IdempotencyConflict)
    ));
    // Deliberately corrupt only this disposable fixture, bypassing immutability.
    store
        .connection
        .execute_batch("DROP TRIGGER translation_cue_no_delete; DELETE FROM translation_cues;")?;
    assert!(matches!(
        store.finish_translation(&work, &result, 34),
        Err(Error::StorageIntegrity)
    ));
    Ok(())
}

#[test]
fn successful_replay_rejects_corrupted_artifact_identity() -> TestResult {
    for change in [
        "profile = 'wrong'",
        "profile_sha256 = printf('%064d', 0)",
        "job_generation = 2",
        "created_ms = 32",
        "cue_count = 2",
        "translated_count = 0",
    ] {
        let directory = tempfile::tempdir()?;
        let mut store = transcribed(&directory.path().join("catalog"))?;
        let (job, work) = store.admit_translation(&request("mt")?, 30)?;
        let work = work.ok_or("work")?;
        let result = TranslationOutcome::synthetic_fixture(
            &job,
            TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
        );
        store.finish_translation(&work, &result, 31)?;
        store.connection.execute_batch(&format!(
            "DROP TRIGGER translation_no_update; UPDATE translations SET {change};"
        ))?;
        assert!(
            matches!(
                store.finish_translation(&work, &result, 33),
                Err(Error::StorageIntegrity)
            ),
            "{change}"
        );
    }
    Ok(())
}

#[test]
fn successful_replay_checks_the_last_cue_at_the_full_worker_bound() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let asr = admit(&mut store, "asr", 0)?;
    let mut heard = output(&asr, "one");
    let first = heard.cues[0].clone();
    heard.cues = (0..256)
        .map(|ordinal| RecognitionCue {
            ordinal,
            start_us: first.start_us + u64::from(ordinal),
            end_us: first.start_us + u64::from(ordinal) + 1,
            script: format!("source {ordinal}"),
        })
        .collect();
    let complete = ReapedLocalAsr::synthetic_fixture(&asr, LocalAsrOutcome::Succeeded(heard));
    assert_eq!(
        store.finish_local_asr(&asr, &complete, 21)?.state,
        "succeeded"
    );
    store.add_translation_profile(&profile()?, 22)?;
    let (job, work) = store.admit_translation(&request("mt")?, 30)?;
    let work = work.ok_or("work")?;
    let mut cues: Vec<_> = (0..256)
        .map(|ordinal| translated(ordinal, &format!("result {ordinal}")))
        .collect();
    let result =
        TranslationOutcome::synthetic_fixture(&job, TranslationResult::Succeeded(cues.clone()));
    let finished = store.finish_translation(&work, &result, 31)?;
    assert_eq!(store.finish_translation(&work, &result, 32)?, finished);
    cues[255] = translated(255, "changed last cue");
    let changed = TranslationOutcome::synthetic_fixture(&job, TranslationResult::Succeeded(cues));
    assert!(matches!(
        store.finish_translation(&work, &changed, 33),
        Err(Error::IdempotencyConflict)
    ));
    let count: u32 =
        store
            .connection
            .query_row("SELECT count(*) FROM translation_cues", [], |row| {
                row.get(0)
            })?;
    assert_eq!(count, 256);
    assert_eq!(store.latest_translation_revision("pin", 1)?, 1);
    Ok(())
}

#[test]
fn successful_replay_detects_extra_untranslated_membership_with_lookahead() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = transcribed(&directory.path().join("catalog"))?;
    let (job, work) = store.admit_translation(&request("mt")?, 30)?;
    let work = work.ok_or("work")?;
    let result = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
    );
    store.finish_translation(&work, &result, 31)?;
    // This fixture deliberately bypasses the original-cue FK to inject an extra row.
    store.connection.execute_batch("PRAGMA foreign_keys = OFF; INSERT INTO translation_cues VALUES ('pin', 1, 1, 1, 'untranslated', NULL, 'fixture-extra'); PRAGMA foreign_keys = ON;")?;
    assert!(matches!(
        store.finish_translation(&work, &result, 32),
        Err(Error::StorageIntegrity)
    ));
    Ok(())
}
