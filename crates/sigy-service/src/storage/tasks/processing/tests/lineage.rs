//! Translation follows only the transcript this task's own recognition published.

use super::*;

fn heard(path: &std::path::Path, script: &str) -> Result<(Store, Vec<String>, LocalAsrRequest)> {
    let mut store = setup(path, false)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let now = START + 90_000;
    let request = recognition(&mut store, &recordings[0], now)?;
    store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
    hear(&mut store, &request.id, script, now + 1)?;
    Ok((store, recordings, request))
}

#[test]
fn translation_binds_this_tasks_recognized_revision_and_profile() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, recordings, recognized) = heard(&path, "Informe sobre el agua")?;
    let now = START + 95_000;
    let facts = store.task_processing_facts("task", now)?.ok_or("facts")?;
    let steps = crate::task::processing::plan(&facts);
    assert!(matches!(
        &steps[0],
        crate::task::processing::ProcessingStep::Translate {
            ordinal: 0,
            transcript_revision: 1,
            ..
        }
    ));
    store.correct_transcript(
        &recognized.analysis_id,
        1,
        0,
        "Informe corregido sobre el agua",
        now,
    )?;
    let corrected = translation(&store, &recordings[0], 2)?;
    assert!(matches!(
        store.enqueue_task_translation(&scope(0, &recordings[0], 0), &corrected, now + 1),
        Err(Error::Analysis("task-translation-unsupported"))
    ));
    assert!(matches!(
        store.enqueue_task_translation(&scope(0, &recordings[0], 1), &corrected, now + 1),
        Err(Error::InvalidInput("translation audio charge"))
    ));
    let request = translation(&store, &recordings[0], 1)?;
    let admitted =
        store.enqueue_task_translation(&scope(0, &recordings[0], 0), &request, now + 1)?;
    assert!(admitted.job_created && admitted.step_created);
    let replay = store.enqueue_task_translation(&scope(0, &recordings[0], 0), &request, 0)?;
    assert!(!replay.job_created && !replay.step_created);
    translate(&mut store, &request.id, "Report about water", now + 2)?;
    let view = store.task_processing("task")?.ok_or("processing")?;
    assert_eq!(view.steps.len(), 2);
    assert_eq!(view.steps[1].stage, "translation");
    assert_eq!(view.steps[1].input_revision, Some(1));
    assert_eq!(view.steps[1].job_state.as_deref(), Some("succeeded"));
    assert_eq!(view.charged_audio_us, AUDIO[0]);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM job_interests WHERE authority = 'task'"
        )?,
        2
    );
    assert!(
        crate::task::processing::plan(
            &store
                .task_processing_facts("task", now + 3)?
                .ok_or("facts")?
        )
        .iter()
        .all(|step| matches!(
            step,
            crate::task::processing::ProcessingStep::Recognize { ordinal: 1, .. }
        ))
    );
    drop(store);
    assert_eq!(Store::open(&path)?.task_processing("task")?, Some(view));
    Ok(())
}

#[test]
fn ungranted_translation_and_out_of_order_refusals_are_rejected() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    store.start_task_processing("task", "grant", &spec(60, false), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let now = START + 90_000;
    assert!(matches!(
        store.record_task_processing_skip(
            &scope(1, &recordings[1], 0),
            "translation",
            "no-text",
            now
        ),
        Err(Error::InvalidInput("task translation before recognition"))
    ));
    for (stage, reason) in [
        ("summary", "fixture"),
        ("recognition", "bad reason"),
        ("recognition", &"x".repeat(65)),
    ] {
        assert!(
            store
                .record_task_processing_skip(&scope(1, &recordings[1], 0), stage, reason, now)
                .is_err()
        );
    }
    let request = recognition(&mut store, &recordings[0], now)?;
    store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
    hear(&mut store, &request.id, "", now + 1)?;
    let facts = store
        .task_processing_facts("task", now + 2)?
        .ok_or("facts")?;
    assert!(matches!(
        &crate::task::processing::plan(&facts)[..],
        [crate::task::processing::ProcessingStep::SkipTranslation { reason, .. }, ..] if reason == "no-text"
    ));
    let mt = translation(&store, &recordings[0], 1)?;
    assert!(matches!(
        store.enqueue_task_translation(&scope(0, &recordings[0], 0), &mt, now + 2),
        Err(Error::Analysis("task-profile-changed"))
    ));
    assert!(store.record_task_processing_skip(
        &scope(0, &recordings[0], 0),
        "translation",
        "no-text",
        now + 2
    )?);
    assert_eq!(
        store.pending_task_processing_ids(None, 4)?.0,
        vec!["task".to_owned()]
    );
    assert!(store.record_task_processing_skip(
        &scope(1, &recordings[1], 0),
        "recognition",
        "recording-failed",
        now + 3
    )?);
    assert!(store.pending_task_processing_ids(None, 4)?.0.is_empty());
    assert_eq!(count(&store, "SELECT count(*) FROM translation_jobs")?, 0);
    Ok(())
}
