//! Job scheduling must follow atomic monitor admission, even when its receipt aborts.
//! Synthetic recognized text supplies storage facts, not language-quality evidence.

use super::{TestResult, actor, completion, count, deliver, monitor_spec, recognition_profile};
use crate::{
    Error, Result,
    control::actor::{Actor, Message, analysis::TranscribeRequest},
    monitor::pipeline::{self, Facts, Step},
    recognition::{
        LocalAsrOutcome, LocalAsrRequest, ReapedLocalAsr, RecognitionCoverage, RecognitionCue,
        RecognitionOutput,
    },
    storage::{monitors::MonitorJobScope, now_ms},
    translation::TranslationProfile,
};
use tokio::sync::mpsc;

fn monitor(actor: &mut Actor, translation: bool) -> Result<()> {
    actor
        .library
        .store_mut()
        .add_recognition_profile(&recognition_profile()?, 1)?;
    let mut spec = monitor_spec(Some("asr"));
    if translation {
        actor
            .library
            .store_mut()
            .add_translation_profile(&translation_profile()?, 1)?;
        spec.translation_profile = Some("mt".into());
    }
    actor
        .library
        .store_mut()
        .create_monitor("automatic", &spec, 1)?;
    Ok(())
}

fn translation_profile() -> Result<TranslationProfile> {
    let root = std::env::temp_dir();
    let mut profile = TranslationProfile {
        id: "mt".into(),
        engine: crate::translation::LLAMA_CPP_COMPLETION.into(),
        template: crate::translation::HY_MT2_PLAIN.into(),
        runtime_dir: root.join("sigy-absent-translator").display().to_string(),
        executable: "llama-completion.exe".into(),
        runtime_sha256: "4".repeat(64),
        runtime_files: 3,
        runtime_bytes: 300,
        model_path: root
            .join("sigy-absent-translation.gguf")
            .display()
            .to_string(),
        model_sha256: "5".repeat(64),
        model_bytes: 1000,
        languages: "fr".into(),
        threads: 1,
        memory_bytes: 1 << 30,
        cue_deadline_ms: 60_000,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    Ok(profile)
}

fn fault(root: &std::path::Path, stage: &str) -> Result<rusqlite::Connection> {
    let connection = rusqlite::Connection::open(root.join("catalog.sqlite3"))?;
    let sql = match stage {
        "recognition" => {
            "CREATE TRIGGER monitor_receipt_fault BEFORE INSERT ON monitor_steps WHEN NEW.stage = 'recognition' AND NEW.decision = 'queued' BEGIN SELECT RAISE(ABORT, 'injected monitor recognition receipt'); END"
        }
        _ => {
            "CREATE TRIGGER monitor_receipt_fault BEFORE INSERT ON monitor_steps WHEN NEW.stage = 'translation' AND NEW.decision = 'queued' BEGIN SELECT RAISE(ABORT, 'injected monitor translation receipt'); END"
        }
    };
    connection.execute_batch(sql)?;
    Ok(connection)
}

fn budgets(actor: &Actor) -> std::result::Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(serde_json::to_value(
        crate::control::snapshot(actor.library.store())?.budgets,
    )?)
}

#[tokio::test]
async fn an_aborted_recognition_receipt_cannot_spawn_then_commit_launches_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    monitor(&mut actor, false)?;
    let before = budgets(&actor)?;
    let fault = fault(root.path(), "recognition")?;
    assert!(matches!(
        actor.reconcile_monitors(),
        Err(Error::Database(_))
    ));
    assert!(actor.pool.recognition.is_empty());
    assert!(actor.idle());
    assert!(receiver.try_recv().is_err());
    assert_eq!(count(&actor, "SELECT count(*) FROM analysis_jobs")?, 0);
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_steps")?, 0);
    assert_eq!(
        actor
            .library
            .store()
            .monitor("automatic")?
            .processing
            .used_total_us,
        0
    );
    assert_eq!(budgets(&actor)?, before);
    fault.execute_batch("DROP TRIGGER monitor_receipt_fault")?;
    actor.next_monitor_pass_ms = 0;
    actor.reconcile_monitors()?;
    assert_eq!(actor.pool.recognition.len(), 1);
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM analysis_jobs WHERE state = 'running' AND lease_owner IS NOT NULL"
        )?,
        1
    );
    assert_eq!(
        actor
            .library
            .store()
            .monitor("automatic")?
            .processing
            .used_total_us,
        1_000_000
    );
    actor.next_monitor_pass_ms = 0;
    actor.reconcile_monitors()?;
    assert_eq!(count(&actor, "SELECT count(*) FROM analysis_jobs")?, 1);
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_steps")?, 1);
    let message = completion(&mut receiver).await?;
    assert!(matches!(&message, Message::RecognitionFinished { .. }));
    assert_eq!(deliver(&mut actor, message), (false, false));
    assert!(actor.idle());
    Ok(())
}

fn transcribed(actor: &mut Actor) -> Result<()> {
    monitor(actor, true)?;
    let store = actor.library.store_mut();
    let pin = pipeline::pin_id("one");
    let (_, input) = store.admit_analysis(&pin, "one", false, 1)?;
    store.publish_analysis(&pin, input.revision)?;
    let profile = store.recognition_profile("asr")?;
    let request = LocalAsrRequest {
        id: pipeline::recognition_job_id("one", &profile.profile_sha256),
        analysis_id: pin,
        analysis_revision: input.revision,
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
        parent_revision: 0,
    };
    let now = now_ms()?.saturating_sub(10);
    store.enqueue_monitor_recognition(
        &MonitorJobScope {
            monitor_id: "automatic",
            policy_version: 1,
            action_count: 0,
            recording_id: "one",
            audio_us: 1_000_000,
        },
        &request,
        now,
    )?;
    let work = store
        .claim_local_asr(&request.id, "synthetic-actor-fixture", now + 1)?
        .ok_or(Error::StorageIntegrity)?;
    let chunk = work.input.chunks.first().ok_or(Error::StorageIntegrity)?;
    let output = RecognitionOutput {
        profile_sha256: request.profile_sha256,
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages: vec![RecognitionCoverage {
            ordinal: 0,
            interval_ordinal: chunk.interval_ordinal,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            source_sha256: chunk.source_sha256.clone(),
            decoded_sha256: "c".repeat(64),
            sample_rate: 16_000,
            sample_count: (chunk.end_us - chunk.start_us) * 16_000 / 1_000_000,
        }],
        cues: vec![RecognitionCue {
            ordinal: 0,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            script: "eau".into(),
        }],
    };
    let outcome = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(output));
    store.finish_local_asr(&work, &outcome, now + 2)?;
    Ok(())
}

#[tokio::test]
async fn an_aborted_translation_receipt_cannot_spawn_then_commit_launches_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    transcribed(&mut actor)?;
    let before = budgets(&actor)?;
    let fault = fault(root.path(), "translation")?;
    assert!(matches!(
        actor.reconcile_monitors(),
        Err(Error::Database(_))
    ));
    assert!(actor.pool.translation.is_empty());
    assert!(actor.idle());
    assert!(receiver.try_recv().is_err());
    assert_eq!(count(&actor, "SELECT count(*) FROM translation_jobs")?, 0);
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM monitor_steps WHERE stage = 'translation'"
        )?,
        0
    );
    assert_eq!(
        actor
            .library
            .store()
            .monitor("automatic")?
            .processing
            .used_total_us,
        1_000_000
    );
    assert_eq!(budgets(&actor)?, before);
    fault.execute_batch("DROP TRIGGER monitor_receipt_fault")?;
    actor.next_monitor_pass_ms = 0;
    actor.reconcile_monitors()?;
    assert_eq!(actor.pool.translation.len(), 1);
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM translation_jobs WHERE state = 'running' AND lease_owner IS NOT NULL"
        )?,
        1
    );
    actor.next_monitor_pass_ms = 0;
    actor.reconcile_monitors()?;
    assert_eq!(count(&actor, "SELECT count(*) FROM translation_jobs")?, 1);
    let message = completion(&mut receiver).await?;
    assert!(matches!(&message, Message::TranslationFinished { .. }));
    assert_eq!(deliver(&mut actor, message), (false, false));
    assert!(actor.idle());
    Ok(())
}

fn recognition_step() -> Step {
    Step::Recognize {
        recording_id: "one".into(),
        audio_us: 1_000_000,
        profile: "asr".into(),
    }
}

fn facts(actor: &Actor) -> Result<Facts> {
    actor.library.store().monitor_facts("automatic", now_ms()?)
}

#[tokio::test]
async fn stale_actions_and_backward_clock_hold_without_fossilizing_a_candidate() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    monitor(&mut actor, false)?;
    let stale = facts(&actor)?;
    actor.library.store_mut().propose_monitor_action(
        "automatic",
        "pause",
        crate::monitor::ActionOrigin::User,
        &crate::monitor::Proposal::Pause,
        now_ms()?,
    )?;
    assert!(!actor.take_step(&stale, &recognition_step(), now_ms()?)?);
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_steps")?, 0);
    actor.library.store_mut().propose_monitor_action(
        "automatic",
        "resume",
        crate::monitor::ActionOrigin::User,
        &crate::monitor::Proposal::Resume,
        now_ms()?,
    )?;
    let current = facts(&actor)?;
    assert!(!actor.take_step(&current, &recognition_step(), 0)?);
    assert_eq!(count(&actor, "SELECT count(*) FROM analysis_jobs")?, 0);
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_steps")?, 0);
    assert!(actor.idle());
    assert!(actor.take_step(&current, &recognition_step(), now_ms()?)?);
    assert_eq!(actor.pool.recognition.len(), 1);
    assert_eq!(
        actor
            .library
            .store()
            .monitor("automatic")?
            .processing
            .used_total_us,
        1_000_000
    );
    let message = completion(&mut receiver).await?;
    assert_eq!(deliver(&mut actor, message), (false, false));
    Ok(())
}

#[tokio::test]
async fn direct_recognition_and_translation_replays_never_dispatch_a_waiting_job() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    transcribed(&mut actor)?;
    let pin = pipeline::pin_id("one");
    actor.pool.accepting = false;
    actor.start_translation("direct-mt", &pin, Some(1), "mt")?;
    let profile = recognition_profile()?;
    let request = || TranscribeRequest {
        id: "direct-asr".into(),
        input: "pin".into(),
        revision: 1,
        profile: profile.id.clone(),
        parent_revision: Some(0),
    };
    actor.start_recognition(request())?;
    actor.pool.accepting = true;
    actor.start_translation("direct-mt", &pin, None, "mt")?;
    actor.start_recognition(request())?;
    assert!(actor.idle());
    assert_eq!(
        actor.library.store().translation_job("direct-mt")?.state,
        "queued"
    );
    assert_eq!(
        actor.library.store().local_asr_job("direct-asr")?.state,
        "queued"
    );
    Ok(())
}

#[tokio::test]
async fn a_recorded_monitor_step_replays_after_pause_and_clock_drift_without_dispatch() -> TestResult
{
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    monitor(&mut actor, false)?;
    let accepted = facts(&actor)?;
    actor.pool.accepting = false;
    assert!(actor.take_step(&accepted, &recognition_step(), now_ms()?)?);
    let job_id = pipeline::recognition_job_id("one", &recognition_profile()?.profile_sha256);
    let job = actor.library.store().local_asr_job(&job_id)?;
    assert_eq!(job.state, "queued");
    actor.library.store_mut().propose_monitor_action(
        "automatic",
        "pause",
        crate::monitor::ActionOrigin::User,
        &crate::monitor::Proposal::Pause,
        now_ms()?,
    )?;
    actor.pool.accepting = true;
    assert!(actor.take_step(&accepted, &recognition_step(), 0)?);
    assert!(actor.idle());
    assert_eq!(actor.library.store().local_asr_job(&job_id)?, job);
    assert_eq!(count(&actor, "SELECT count(*) FROM monitor_steps")?, 1);
    assert_eq!(
        actor
            .library
            .store()
            .monitor("automatic")?
            .processing
            .used_total_us,
        1_000_000
    );
    Ok(())
}
