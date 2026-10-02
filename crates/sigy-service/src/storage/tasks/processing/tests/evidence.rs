//! Collection reconciles through exact task lineage to literal evidence, and an interrupted
//! run reaches the same admitted jobs, receipts, interests, charges and citations.

use super::*;
use crate::task::{
    evidence::{TaskEvidenceView, TaskOutcome},
    processing::{ProcessingStep, plan},
    run::{TaskRunSpec, TaskRunState},
};

const SCRIPTS: [&str; 2] = ["Informe sobre el agua del río", "Sin coincidencias aquí"];
const ENGLISH: [&str; 2] = ["Report about the river water", "No matches here"];

/// One storage-level stand-in for a service pass: admit every planned step.
fn admit(store: &mut Store, step: &ProcessingStep, now: i64) -> Result<()> {
    match step {
        ProcessingStep::Recognize {
            ordinal,
            recording_id,
            audio_us,
        } => {
            let request = recognition(store, recording_id, now)?;
            store.enqueue_task_recognition(
                &scope(*ordinal, recording_id, *audio_us),
                &request,
                now,
            )?;
        }
        ProcessingStep::Translate {
            ordinal,
            recording_id,
            transcript_revision,
            ..
        } => {
            let request = translation(store, recording_id, *transcript_revision)?;
            store.enqueue_task_translation(&scope(*ordinal, recording_id, 0), &request, now)?;
        }
        ProcessingStep::SkipRecognition {
            ordinal,
            recording_id,
            reason,
        } => {
            store.record_task_processing_skip(
                &scope(*ordinal, recording_id, 0),
                "recognition",
                reason,
                now,
            )?;
        }
        ProcessingStep::SkipTranslation {
            ordinal,
            recording_id,
            reason,
        } => {
            store.record_task_processing_skip(
                &scope(*ordinal, recording_id, 0),
                "translation",
                reason,
                now,
            )?;
        }
    }
    Ok(())
}

/// Finish the oldest queued job of either kind with synthetic, deterministic output.
fn work(store: &mut Store, recordings: &[String], now: i64) -> Result<bool> {
    for (ordinal, recording) in recordings.iter().enumerate() {
        let profile = recognition_profile()?.profile_sha256;
        let job = pipeline::recognition_job_id(recording, &profile);
        if store
            .local_asr_job(&job)
            .is_ok_and(|job| job.state == "queued")
        {
            hear(store, &job, SCRIPTS[ordinal], now)?;
            return Ok(true);
        }
        let mt = translation(store, recording, 1)?.id;
        if store
            .translation_job(&mt)
            .is_ok_and(|job| job.state == "queued")
        {
            translate(store, &mt, ENGLISH[ordinal], now)?;
            return Ok(true);
        }
    }
    Ok(false)
}

const FAULTS: [&str; 3] = [
    "CREATE TRIGGER fixture BEFORE INSERT ON task_processing_steps BEGIN SELECT RAISE(ABORT, 'receipt fault'); END",
    "CREATE TRIGGER fixture BEFORE INSERT ON job_interests WHEN NEW.authority = 'task' BEGIN SELECT RAISE(ABORT, 'interest fault'); END",
    "CREATE TABLE IF NOT EXISTS fixture_parent(id INTEGER PRIMARY KEY); CREATE TABLE IF NOT EXISTS fixture_child(id INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fixture AFTER INSERT ON task_processing_steps BEGIN INSERT INTO fixture_child VALUES(1); END",
];

/// Drive collection through evidence. When interrupted, every task effect first fails at
/// one boundary and every effect is followed by a catalog close and reopen.
fn drive(path: &std::path::Path, interrupted: bool) -> Result<(Vec<String>, TaskEvidenceView)> {
    let mut store = setup(path, false)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let mut clock = START + 100_000;
    let mut boundary = 0;
    loop {
        let facts = store
            .task_processing_facts("task", clock)?
            .ok_or(Error::NotFound)?;
        let steps = plan(&facts);
        for step in &steps {
            if interrupted {
                store
                    .connection
                    .execute_batch(FAULTS[boundary % FAULTS.len()])?;
                boundary += 1;
                if admit(&mut store, step, clock).is_ok() {
                    return Err(Error::StorageIntegrity);
                }
                store.connection.execute_batch("DROP TRIGGER fixture")?;
                drop(store);
                store = Store::open(path)?;
            }
            admit(&mut store, step, clock)?;
            clock += 1000;
        }
        let worked = work(&mut store, &recordings, clock)?;
        clock += 1000;
        if interrupted {
            drop(store);
            store = Store::open(path)?;
        }
        if steps.is_empty() && !worked {
            break;
        }
    }
    let evidence = store.task_evidence("task")?.ok_or(Error::NotFound)?;
    Ok((effects(&store)?, evidence))
}

#[test]
fn interrupted_and_uninterrupted_runs_reach_identical_effects_charges_and_citations() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let (reference, expected) = drive(&directory.path().join("reference"), false)?;
    let (resumed, evidence) = drive(&directory.path().join("resumed"), true)?;
    assert_eq!(resumed, reference);
    assert_eq!(evidence, expected);
    assert_eq!(evidence.outcome, TaskOutcome::Cited);
    assert!(evidence.reasons.is_empty());
    assert_eq!(evidence.citations.len(), 1);
    assert_eq!(evidence.citations[0].translation_revision, Some(1));
    assert_eq!(
        (
            evidence.planned_us,
            evidence.recorded_us,
            evidence.uncovered_us
        ),
        (60_000_000, 60_000_000, 0)
    );
    assert_eq!(
        (evidence.recognized_us, evidence.unprocessed_us),
        (60_000_000, 0)
    );
    assert_eq!(
        reference
            .iter()
            .filter(|row| row.contains("|task|task|"))
            .count(),
        4
    );
    Ok(())
}

#[test]
fn cited_evidence_follows_exact_lineage_and_publishes_through_the_existing_path() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    drive(&path, false)?;
    let mut store = Store::open(&path)?;
    let evidence = store.task_evidence("task")?.ok_or("evidence")?;
    let cited = evidence.citations[0].clone();
    store.correct_transcript(
        &cited.transcript_id,
        1,
        0,
        "Texto corregido sin término",
        START + 200_000,
    )?;
    assert_eq!(store.task_evidence("task")?, Some(evidence.clone()));
    store.checkpoint_task("task", "observe", 0, START + 900_000)?;
    let checkpoint = store.task_checkpoint("task", 1)?;
    assert!(
        !checkpoint.citations.contains(&cited),
        "monitor checkpoints read the newest revision"
    );
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    drive(&path, false)?;
    let mut store = Store::open(&path)?;
    let evidence = store.task_evidence("task")?.ok_or("evidence")?;
    let checkpoint = store.checkpoint_task("task", "observe", 0, START + 900_000)?;
    assert!(
        evidence
            .citations
            .iter()
            .all(|citation| checkpoint.citations.contains(citation))
    );
    store.start_task_run(
        "task",
        "publish",
        &TaskRunSpec {
            checkpoint_ordinal: 1,
            maximum_findings: 4,
        },
        0,
        START + 900_001,
    )?;
    let mut now = START + 900_002;
    while store.task_run("task")?.ok_or("run")?.state == TaskRunState::Running {
        let generation = store.task_run("task")?.ok_or("run")?.generation;
        store.advance_task_run("task", generation, now)?;
        now += 1;
    }
    let run = store.task_run("task")?.ok_or("run")?;
    assert_eq!(run.published_findings, 1);
    assert!(run.briefing_id.is_some());
    Ok(())
}

#[test]
fn missing_coverage_and_unsupported_text_remain_explicitly_partial() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    assert!(matches!(
        store.task_evidence("task")?,
        Some(view) if view.outcome == TaskOutcome::Pending && view.entries.iter().all(|entry| entry.pending)
    ));
    store.start_task_processing("task", "grant", &spec(60, false), 0, START - 500)?;
    let mut launches = store.reconcile_schedules_at(START, true)?.launches;
    let launch = launches.pop().ok_or("launch")?;
    store.publish_recording(&launch.job.version, &publication(0))?;
    let recording = launch.job.version.id().to_owned();
    store.reconcile_schedules_at(START + 200_000, true)?;
    let view = store.task_evidence("task")?.ok_or("evidence")?;
    assert_eq!(view.outcome, TaskOutcome::Pending);
    assert_eq!(
        view.entries[1].reasons,
        ["capture-missed", "uncovered-time"]
    );
    let now = START + 200_001;
    let request = recognition(&mut store, &recording, now)?;
    store.enqueue_task_recognition(&scope(0, &recording, AUDIO[0]), &request, now)?;
    hear(&mut store, &request.id, "Informe sobre el agua", now + 1)?;
    let facts = store
        .task_processing_facts("task", now + 2)?
        .ok_or("facts")?;
    for step in plan(&facts) {
        admit(&mut store, &step, now + 2)?;
    }
    let view = store.task_evidence("task")?.ok_or("evidence")?;
    assert_eq!(view.outcome, TaskOutcome::Partial);
    assert_eq!(
        view.citations.len(),
        1,
        "an untranslated passage still cites its original"
    );
    assert_eq!(view.citations[0].translation_revision, None);
    assert_eq!(
        view.reasons,
        ["translation-skipped", "capture-missed", "uncovered-time"]
    );
    assert_eq!((view.uncovered_us, view.unprocessed_us), (30_000_000, 0));
    Ok(())
}

#[test]
fn holds_failed_recognition_and_no_text_never_report_success() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let recordings = collect(&mut store)?;
    let view = store.task_evidence("task")?.ok_or("evidence")?;
    assert_eq!(view.outcome, TaskOutcome::Partial);
    assert!(
        view.entries
            .iter()
            .all(|entry| entry.reasons == ["processing-not-granted", "unprocessed-time"])
    );
    assert_eq!(view.unprocessed_us, 60_000_000);
    store.start_task_processing("task", "grant", &spec(60, true), 0, START + 90_000)?;
    let now = START + 90_001;
    let request = recognition(&mut store, &recordings[0], now)?;
    store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
    hear(&mut store, &request.id, "", now + 1)?;
    let second = recognition(&mut store, &recordings[1], now + 2)?;
    store.enqueue_task_recognition(&scope(1, &recordings[1], AUDIO[1]), &second, now + 2)?;
    store.cancel_local_asr(&second.id, 1)?;
    assert_eq!(
        store.task_evidence("task")?.ok_or("evidence")?.outcome,
        TaskOutcome::Partial
    );
    let view = store.task_evidence("task")?.ok_or("evidence")?;
    assert_eq!(view.entries[0].reasons, ["no-recognized-text"]);
    assert_eq!(
        view.entries[0].transcript_outcome.as_deref(),
        Some("no_text")
    );
    assert_eq!(
        view.entries[1].reasons,
        ["recognition-cancelled", "unprocessed-time"]
    );
    assert!(view.citations.is_empty());
    let directory = tempfile::tempdir()?;
    let mut held = setup(&directory.path().join("catalog"), false)?;
    let recordings = collect(&mut held)?;
    held.start_task_processing("task", "grant", &spec(60, true), 0, START + 90_000)?;
    held.cancel_task_processing("task", "stop", 1, START + 90_001)?;
    let view = held.task_evidence("task")?.ok_or("evidence")?;
    assert_eq!(view.outcome, TaskOutcome::Partial);
    assert_eq!(view.reasons, ["processing-cancelled", "unprocessed-time"]);
    assert_eq!(view.entries.len(), recordings.len());
    Ok(())
}
