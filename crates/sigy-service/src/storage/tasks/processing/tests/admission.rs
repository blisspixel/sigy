//! Atomic admission: job, receipt, interest and charge commit or roll back together.

use super::*;

fn granted(path: &std::path::Path, seconds: u32) -> Result<(Store, Vec<String>)> {
    let mut store = setup(path, false)?;
    store.start_task_processing("task", "grant", &spec(seconds, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    Ok((store, recordings))
}

#[test]
fn recognition_commits_job_receipt_interest_and_charge_once() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, recordings) = granted(&path, 60)?;
    assert_eq!(
        store.pending_task_processing_ids(None, 4)?.0,
        vec!["task".to_owned()]
    );
    let now = START + 90_000;
    let request = recognition(&mut store, &recordings[0], now)?;
    let first =
        store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
    assert_eq!(
        first,
        TaskJobAdmission {
            job_created: true,
            step_created: true
        }
    );
    let job = store.local_asr_job(&request.id)?;
    assert_eq!((job.state.as_str(), job.started_ms), ("queued", None));
    assert_eq!(count(&store, "SELECT count(*) FROM job_attempts")?, 0);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM job_interests WHERE authority = 'task' AND owner_id = 'task' AND family = 'recognition'"
        )?,
        1
    );
    let replay =
        store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, -5)?;
    assert!(!replay.job_created && !replay.step_created);
    assert!(matches!(
        store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0] + 1), &request, now),
        Err(Error::IdempotencyConflict)
    ));
    let second = recognition(&mut store, &recordings[1], now + 1)?;
    store.enqueue_task_recognition(&scope(1, &recordings[1], AUDIO[1]), &second, now + 1)?;
    let view = store.task_processing("task")?.ok_or("processing")?;
    assert_eq!(view.charged_audio_us, AUDIO[0] + AUDIO[1]);
    assert_eq!(view.steps.len(), 2);
    let sharing = view.steps[0].sharing.ok_or("sharing")?;
    assert_eq!(
        (sharing.direct, sharing.monitors, sharing.tasks),
        (false, 0, 1)
    );
    assert_eq!(view.steps[0].job_state.as_deref(), Some("queued"));
    assert_eq!(store.pending_task_processing_ids(None, 4)?.0.len(), 1);
    let before = effects(&store)?;
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(effects(&reopened)?, before);
    assert_eq!(reopened.task_processing("task")?, Some(view));
    Ok(())
}

#[test]
fn receipt_interest_and_commit_faults_leave_no_job_receipt_or_charge() -> TestResult {
    let reference = tempfile::tempdir()?;
    let (mut clean, recordings) = granted(&reference.path().join("catalog"), 60)?;
    let now = START + 90_000;
    let request = recognition(&mut clean, &recordings[0], now)?;
    clean.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
    let expected = effects(&clean)?;
    for fault in [
        "CREATE TRIGGER fixture BEFORE INSERT ON task_processing_steps BEGIN SELECT RAISE(ABORT, 'receipt fault'); END",
        "CREATE TRIGGER fixture BEFORE INSERT ON job_interests WHEN NEW.authority = 'task' BEGIN SELECT RAISE(ABORT, 'interest fault'); END",
        "CREATE TABLE fixture_parent(id INTEGER PRIMARY KEY); CREATE TABLE fixture_child(id INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fixture AFTER INSERT ON job_interests BEGIN INSERT INTO fixture_child VALUES(1); END",
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let (mut store, recordings) = granted(&path, 60)?;
        let request = recognition(&mut store, &recordings[0], now)?;
        store.connection.execute_batch(fault)?;
        assert!(
            store
                .enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)
                .is_err(),
            "{fault}"
        );
        for table in ["analysis_jobs", "task_processing_steps", "job_interests"] {
            assert_eq!(count(&store, &format!("SELECT count(*) FROM {table}"))?, 0);
        }
        assert_eq!(
            store
                .task_processing("task")?
                .ok_or("processing")?
                .charged_audio_us,
            0
        );
        store.connection.execute_batch("DROP TRIGGER fixture")?;
        drop(store);
        let mut reopened = Store::open(&path)?;
        reopened.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
        assert_eq!(effects(&reopened)?, expected);
    }
    Ok(())
}

#[test]
fn a_monitor_job_is_shared_once_and_each_authority_charges_its_own_cap() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, true)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let now = START + 90_000;
    let request = recognition(&mut store, &recordings[0], now)?;
    let monitor = crate::storage::monitors::MonitorJobScope {
        monitor_id: "monitor",
        policy_version: 1,
        action_count: 0,
        recording_id: &recordings[0],
        audio_us: AUDIO[0],
    };
    assert!(
        store
            .enqueue_monitor_recognition(&monitor, &request, now)?
            .job_created
    );
    let task =
        store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now + 1)?;
    assert_eq!(
        task,
        TaskJobAdmission {
            job_created: false,
            step_created: true
        }
    );
    assert_eq!(count(&store, "SELECT count(*) FROM analysis_jobs")?, 1);
    assert_eq!(
        store.monitor_processing("monitor", now)?.used_total_us,
        AUDIO[0]
    );
    let view = store.task_processing("task")?.ok_or("processing")?;
    assert_eq!(view.charged_audio_us, AUDIO[0]);
    let sharing = view.steps[0].sharing.ok_or("sharing")?;
    assert_eq!(
        (sharing.direct, sharing.monitors, sharing.tasks),
        (false, 1, 1)
    );
    let direct = LocalAsrRequest {
        id: "direct-asr".into(),
        ..request.clone()
    };
    assert!(store.enqueue_local_asr(&direct, now + 2)?.1);
    assert!(!store.enqueue_local_asr(&direct, now + 3)?.1);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM job_interests WHERE authority = 'direct' AND owner_id = '' AND job_id = 'direct-asr' AND created_ms = 1790078490002"
        )?,
        1
    );
    drop(store);
    Store::open(&path)?;
    Ok(())
}

#[test]
fn cancellation_fences_task_admissions_and_keeps_admitted_and_independent_work() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, recordings) = granted(&path, 60)?;
    let now = START + 90_000;
    let request = recognition(&mut store, &recordings[0], now)?;
    store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
    assert!(matches!(
        store.cancel_task_processing("task", "stop", 2, now + 1),
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        store.cancel_task_processing("task", "stop", 1, now - 1),
        Err(Error::InvalidInput("task processing clock"))
    ));
    let cancelled = store.cancel_task_processing("task", "stop", 1, now + 1)?;
    assert_eq!((cancelled.generation, cancelled.cancelled), (2, true));
    assert_eq!(cancelled.hold_reason.as_deref(), Some("cancelled"));
    assert_eq!(
        store.cancel_task_processing("task", "stop", 1, 0)?,
        cancelled
    );
    assert!(matches!(
        store.cancel_task_processing("task", "other", 2, now + 2),
        Err(Error::IdempotencyConflict)
    ));
    let second = recognition(&mut store, &recordings[1], now + 2)?;
    assert!(matches!(
        store.enqueue_task_recognition(&scope(1, &recordings[1], AUDIO[1]), &second, now + 2),
        Err(Error::Analysis("task-processing-cancelled"))
    ));
    assert!(matches!(
        store.record_task_processing_skip(
            &scope(1, &recordings[1], 0),
            "recognition",
            "fixture",
            now + 2
        ),
        Err(Error::Analysis("task-processing-cancelled"))
    ));
    assert_eq!(store.local_asr_job(&request.id)?.state, "queued");
    assert!(
        store
            .claim_local_asr(&request.id, "fixture", now + 3)?
            .is_some()
    );
    assert!(store.enqueue_local_asr(&second, now + 3)?.1);
    assert!(store.pending_task_processing_ids(None, 4)?.0.is_empty());
    let facts = store
        .task_processing_facts("task", now + 4)?
        .ok_or("facts")?;
    assert_eq!(facts.hold, Some("cancelled"));
    assert!(crate::task::processing::plan(&facts).is_empty());
    store
        .connection
        .execute_batch("DROP TRIGGER task_processing_step_binding")?;
    assert!(
        store
            .connection
            .execute(
                "INSERT INTO task_processing_steps(task_id, ordinal, stage, recording_id, decision, reason, audio_us, created_ms) VALUES ('task', 1, 'recognition', ?1, 'skipped', 'late', 0, ?2)",
                rusqlite::params![recordings[1], now + 5],
            )
            .is_ok()
    );
    drop(store);
    assert!(matches!(Store::open(&path), Err(Error::StorageIntegrity)));
    Ok(())
}

#[test]
fn drift_clock_profile_audio_and_allowance_refuse_without_partial_rows() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, recordings) = granted(&directory.path().join("catalog"), 40)?;
    let now = START + 90_000;
    let first = recognition(&mut store, &recordings[0], now)?;
    let second = recognition(&mut store, &recordings[1], now)?;
    assert!(matches!(
        store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &first, START - 600),
        Err(Error::Analysis("task-processing-clock"))
    ));
    assert!(matches!(
        store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0] - 1), &first, now),
        Err(Error::InvalidInput("task recording audio"))
    ));
    assert!(matches!(
        store.enqueue_task_recognition(&scope(1, &recordings[0], AUDIO[0]), &first, now),
        Err(Error::InvalidInput("task recording is not collected"))
    ));
    let mut other = first.clone();
    other.id = "other-profile".into();
    other.profile_sha256 = "f".repeat(64);
    assert!(matches!(
        store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &other, now),
        Err(Error::Analysis("task-profile-changed"))
    ));
    assert_eq!(count(&store, "SELECT count(*) FROM analysis_jobs")?, 0);
    store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &first, now)?;
    assert!(matches!(
        store.enqueue_task_recognition(&scope(1, &recordings[1], AUDIO[1]), &second, now),
        Err(Error::Analysis("task-audio-allowance"))
    ));
    let facts = store.task_processing_facts("task", now)?.ok_or("facts")?;
    assert!(matches!(
        &crate::task::processing::plan(&facts)[..],
        [crate::task::processing::ProcessingStep::SkipRecognition { reason, .. }] if reason == "task-audio-allowance"
    ));
    assert!(store.record_task_processing_skip(
        &scope(1, &recordings[1], 0),
        "recognition",
        "task-audio-allowance",
        now
    )?);
    assert!(!store.record_task_processing_skip(
        &scope(1, &recordings[1], 0),
        "recognition",
        "task-audio-allowance",
        now + 1
    )?);
    assert!(matches!(
        store.record_task_processing_skip(
            &scope(1, &recordings[1], 0),
            "recognition",
            "other",
            now + 1
        ),
        Err(Error::IdempotencyConflict)
    ));
    store.propose_monitor_action(
        "monitor",
        "pause",
        crate::monitor::ActionOrigin::User,
        &crate::monitor::Proposal::Pause,
        now + 2,
    )?;
    let facts = store
        .task_processing_facts("task", now + 3)?
        .ok_or("facts")?;
    assert_eq!(facts.hold, Some("scope-changed"));
    assert!(matches!(
        store.record_task_processing_skip(
            &scope(0, &recordings[0], 0),
            "translation",
            "fixture",
            now + 3
        ),
        Err(Error::Analysis("task-scope-changed"))
    ));
    let view = store.task_processing("task")?.ok_or("processing")?;
    assert_eq!(view.hold_reason.as_deref(), Some("scope-changed"));
    assert_eq!(view.charged_audio_us, AUDIO[0]);
    assert_eq!(count(&store, "SELECT count(*) FROM analysis_jobs")?, 1);
    Ok(())
}
