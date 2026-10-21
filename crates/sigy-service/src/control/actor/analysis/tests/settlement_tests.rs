//! Completed-slot settlement. Outcomes are synthetic. No native recognizer is started.

use std::{future::pending, time::Instant};

use tokio::sync::{mpsc, watch};

use super::super::settlement::{SETTLEMENT_DEADLINE, SettlementOverride, Step};
use super::super::{Actor, Message, RecognitionWorker};
use super::actor;
use crate::{
    Error, Result, execution,
    recognition::{
        LocalAsrOutcome, LocalAsrRequest, ReapedLocalAsr, RecognitionCoverage, RecognitionCue,
        RecognitionOutput,
    },
    recognizer,
    storage::{job_pool::JobKind, now_ms},
    translation::{TranslationOutcome, TranslationProfile, TranslationRequest},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn request() -> LocalAsrRequest {
    LocalAsrRequest {
        id: "asr".into(),
        analysis_id: "pin".into(),
        analysis_revision: 1,
        profile: "synthetic-slot-fixture-v1".into(),
        profile_sha256: "b".repeat(64),
        parent_revision: 0,
    }
}

fn enqueue(actor: &mut Actor) -> Result<()> {
    actor
        .library
        .store_mut()
        .enqueue_local_asr(&request(), now_ms()?)?;
    Ok(())
}

fn claimed(actor: &mut Actor) -> Result<crate::recognition::LocalAsrWork> {
    enqueue(actor)?;
    actor
        .library
        .store_mut()
        .claim_local_asr("asr", "local-test", now_ms()?)?
        .ok_or(Error::StorageIntegrity)
}

fn attempts(actor: &Actor) -> Result<Vec<(u32, u32)>> {
    actor.library.store().ended_attempts("analysis", "asr")
}

fn translation_attempts(actor: &Actor) -> Result<Vec<(u32, u32)>> {
    actor.library.store().ended_attempts("translation", "mt")
}

/// Publish one recognized cue so a translation can be claimed. The profile is absent,
/// so a later launch refuses through the slot instead of starting a translator.
fn recognized(actor: &mut Actor) -> Result<()> {
    let work = claimed(actor)?;
    let chunk = work
        .input
        .chunks
        .first()
        .ok_or(Error::StorageIntegrity)?
        .clone();
    let duration = chunk.end_us.saturating_sub(chunk.start_us);
    let output = RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages: vec![RecognitionCoverage {
            ordinal: 0,
            interval_ordinal: chunk.interval_ordinal,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            source_sha256: chunk.source_sha256.clone(),
            decoded_sha256: "c".repeat(64),
            sample_rate: 16_000,
            sample_count: duration.saturating_mul(16_000) / 1_000_000,
        }],
        cues: vec![RecognitionCue {
            ordinal: 0,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            script: "eau".into(),
        }],
    };
    let outcome = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(output));
    let job = actor
        .library
        .store_mut()
        .finish_local_asr(&work, &outcome, now_ms()?)?;
    if job.state != "succeeded" {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

fn translator(actor: &mut Actor) -> Result<TranslationProfile> {
    let root = std::env::temp_dir();
    let mut profile = TranslationProfile {
        id: "slot-mt".into(),
        engine: crate::translation::LLAMA_CPP_COMPLETION.into(),
        template: crate::translation::HY_MT2_PLAIN.into(),
        runtime_dir: root
            .join("sigy-absent-slot-translator")
            .display()
            .to_string(),
        executable: "llama-completion.exe".into(),
        runtime_sha256: "4".repeat(64),
        runtime_files: 3,
        runtime_bytes: 300,
        model_path: root
            .join("sigy-absent-slot-translation.gguf")
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
    actor
        .library
        .store_mut()
        .add_translation_profile(&profile, now_ms()?)?;
    Ok(profile)
}

fn translation_request(profile: &TranslationProfile) -> TranslationRequest {
    TranslationRequest {
        id: "mt".into(),
        transcript_id: "pin".into(),
        transcript_revision: 1,
        profile: profile.id.clone(),
        profile_sha256: profile.profile_sha256.clone(),
    }
}

fn claimed_translation(actor: &mut Actor) -> Result<crate::translation::TranslationWork> {
    recognized(actor)?;
    let profile = translator(actor)?;
    actor
        .library
        .store_mut()
        .enqueue_translation(&translation_request(&profile), now_ms()?)?;
    actor
        .library
        .store_mut()
        .claim_translation("mt", "local-test", now_ms()?)?
        .ok_or(Error::StorageIntegrity)
}

fn refused_translation(work: &crate::translation::TranslationWork) -> Result<TranslationOutcome> {
    let envelope = execution::refuse_translation(
        &work.job.request.id,
        work.job.generation,
        "worker-start-failed",
    );
    TranslationOutcome::from_envelope(&work.job, envelope, None)
}

#[tokio::test]
async fn a_committed_launch_refusal_does_not_stop_the_service() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    enqueue(&mut actor)?;
    assert!(!actor.launch_recognition("asr")?);
    let job = actor.library.store().local_asr_job("asr")?;
    assert_eq!(job.state, "failed");
    assert_eq!(job.reason.as_deref(), Some("recognizer-failed"));
    assert_eq!(job.attempt, 1);
    assert!(attempts(&actor)?.is_empty());
    assert!(actor.pool.recognition_slots.is_empty());
    assert!(actor.pool.recognition.is_empty());
    assert!(actor.idle());
    assert_eq!(actor.settlement_attempts, 0);
    Ok(())
}

#[tokio::test]
async fn a_busy_refusal_stays_in_the_slot_until_a_later_finish_commits() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let generation = work.job.generation;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Busy);
    assert_eq!(actor.park_recognition(work, reaped)?, Step::Pending);
    assert_eq!(actor.pool.recognition_slots.len(), 1);
    assert!(actor.pool.recognition.is_empty());
    assert_eq!(actor.pool.occupied(JobKind::Recognition), 1);
    assert_eq!(actor.pool.recognition_tries("asr"), Some(1));
    assert_eq!(actor.settlement_attempts, 1);
    assert!(!actor.idle());
    let running = actor.library.store().local_asr_job("asr")?;
    assert_eq!(running.state, "running");
    assert_eq!((running.generation, running.attempt), (generation, 1));
    assert!(attempts(&actor)?.is_empty());
    actor.pool.stop();
    assert!(!actor.pool.accepting);
    assert_eq!(actor.pool.recognition_slots.len(), 1);
    actor.settlement_override = None;
    assert_eq!(actor.drive_settlement()?, ());
    let job = actor.library.store().local_asr_job("asr")?;
    assert_eq!(job.state, "failed");
    assert_eq!(job.reason.as_deref(), Some("recognizer-failed"));
    assert_eq!(job.attempt, 1);
    assert!(attempts(&actor)?.is_empty());
    assert!(actor.pool.recognition_slots.is_empty());
    assert_eq!(actor.settlement_attempts, 1);
    Ok(())
}

#[tokio::test]
async fn three_busy_attempts_drop_the_slot_and_leave_the_running_row() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Busy);
    assert_eq!(actor.park_recognition(work, reaped)?, Step::Pending);
    assert_eq!(actor.drive_settlement()?, ());
    assert_eq!(actor.pool.recognition_tries("asr"), Some(2));
    assert_eq!(actor.drive_settlement()?, ());
    assert!(actor.pool.recognition_slots.is_empty());
    assert_eq!(actor.settlement_attempts, 3);
    let job = actor.library.store().local_asr_job("asr")?;
    assert_eq!(job.state, "running");
    assert_eq!(job.attempt, 1);
    assert!(attempts(&actor)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_deadline_drops_the_slot_without_another_finish() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Busy);
    assert_eq!(actor.park_recognition(work, reaped)?, Step::Pending);
    actor.settlement_clock.advance = SETTLEMENT_DEADLINE + std::time::Duration::from_secs(1);
    assert_eq!(actor.drive_settlement()?, ());
    assert!(actor.pool.recognition_slots.is_empty());
    assert_eq!(actor.settlement_attempts, 1);
    assert_eq!(actor.library.store().local_asr_job("asr")?.state, "running");
    Ok(())
}

#[tokio::test]
async fn uncertain_io_does_not_finish_again_while_the_row_is_running() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Io);
    assert_eq!(actor.park_recognition(work, reaped)?, Step::Pending);
    assert_eq!(actor.drive_settlement()?, ());
    assert_eq!(actor.pool.recognition_tries("asr"), Some(1));
    assert_eq!(actor.settlement_attempts, 1);
    assert_eq!(actor.library.store().local_asr_job("asr")?.state, "running");
    assert!(attempts(&actor)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_constraint_fault_drops_the_slot_and_stops() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Constraint);
    assert!(matches!(
        actor.park_recognition(work, reaped),
        Err(Error::Database(_))
    ));
    assert!(actor.pool.recognition_slots.is_empty());
    assert_eq!(actor.settlement_attempts, 1);
    assert_eq!(actor.library.store().local_asr_job("asr")?.state, "running");
    Ok(())
}

#[tokio::test]
async fn a_changed_claim_refuses_without_a_second_publication() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Busy);
    assert_eq!(actor.park_recognition(work, reaped)?, Step::Pending);
    assert!(actor.pool.set_recognition_attempt("asr", 9));
    actor.settlement_override = None;
    assert!(matches!(
        actor.drive_settlement(),
        Err(Error::Analysis("stale-worker"))
    ));
    assert!(actor.pool.recognition_slots.is_empty());
    let job = actor.library.store().local_asr_job("asr")?;
    assert_eq!(job.state, "running");
    assert_eq!(job.attempt, 1);
    assert!(attempts(&actor)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn cancellation_wins_on_the_retry_that_commits() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let generation = work.job.generation;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Locked);
    assert_eq!(actor.park_recognition(work, reaped)?, Step::Pending);
    actor
        .library
        .store_mut()
        .cancel_local_asr("asr", generation)?;
    actor.settlement_override = None;
    assert_eq!(actor.drive_settlement()?, ());
    let job = actor.library.store().local_asr_job("asr")?;
    assert_eq!(job.state, "cancelled");
    assert_eq!(job.attempt, 1);
    assert!(attempts(&actor)?.is_empty());
    assert!(actor.pool.recognition_slots.is_empty());
    Ok(())
}

#[tokio::test]
async fn an_unproven_drain_does_not_open_a_slot() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let generation = work.job.generation;
    let id = work.job.request.id.clone();
    let worker_id = id.clone();
    let worker = actor.spawn_worker(
        |_signal| async { pending::<Result<crate::recognition::ReapedLocalAsr>>().await },
        move |result| Message::RecognitionFinished {
            id: worker_id,
            generation,
            result,
        },
    )?;
    actor.pool.recognition.insert(
        id.clone(),
        RecognitionWorker {
            generation,
            worker,
            work,
        },
    );
    assert!(
        actor
            .finish_recognition(&id, generation, Err(Error::Analysis("cancelled")))
            .is_err()
    );
    assert!(actor.pool.recognition.is_empty());
    assert!(actor.pool.recognition_slots.is_empty());
    assert_eq!(actor.settlement_attempts, 0);
    assert_eq!(actor.library.store().local_asr_job("asr")?.state, "running");
    Ok(())
}

#[tokio::test]
async fn shutdown_with_an_expired_slot_finishes_the_catalog_thread() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed(&mut actor)?;
    let reaped = recognizer::not_started(&work.job)?;
    actor.settlement_override = Some(SettlementOverride::Busy);
    assert_eq!(actor.park_recognition(work, reaped)?, Step::Pending);
    actor.settlement_clock.advance = SETTLEMENT_DEADLINE + std::time::Duration::from_secs(1);
    drop(sender);
    let (stopping, _) = watch::channel(false);
    let mut stopped = false;
    let mut shutdown = true;
    let keep = super::super::super::poll_catalog(
        &mut actor,
        &mut receiver,
        &stopping,
        &mut stopped,
        &mut shutdown,
        Instant::now(),
    );
    assert!(!keep);
    assert!(actor.idle());
    assert!(actor.pool.recognition_slots.is_empty());
    assert_eq!(actor.settlement_attempts, 1);
    assert_eq!(actor.library.store().local_asr_job("asr")?.state, "running");
    assert!(attempts(&actor)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_committed_translation_launch_refusal_does_not_stop_the_service() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    recognized(&mut actor)?;
    let profile = translator(&mut actor)?;
    actor
        .library
        .store_mut()
        .enqueue_translation(&translation_request(&profile), now_ms()?)?;
    // The profile row cannot be removed. Dropping the catalog sender makes worker
    // spawn fail after the claim, which is the launch refusal that must settle.
    drop(sender);
    assert!(!actor.launch_translation("mt")?);
    let job = actor.library.store().translation_job("mt")?;
    assert_eq!(job.state, "failed");
    assert_eq!(job.reason.as_deref(), Some("worker-start-failed"));
    assert_eq!(job.attempt, 1);
    assert!(translation_attempts(&actor)?.is_empty());
    assert!(actor.pool.translation_slots.is_empty());
    assert!(actor.pool.translation.is_empty());
    assert!(actor.idle());
    assert_eq!(actor.settlement_attempts, 0);
    Ok(())
}

#[tokio::test]
async fn a_busy_translation_stays_in_the_slot_until_a_later_finish_commits() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(4);
    let mut actor = actor(root.path(), &sender)?;
    let work = claimed_translation(&mut actor)?;
    let outcome = refused_translation(&work)?;
    actor.settlement_override = Some(SettlementOverride::Busy);
    assert_eq!(actor.park_translation(work, outcome)?, Step::Pending);
    assert_eq!(actor.pool.translation_slots.len(), 1);
    assert!(actor.pool.translation.is_empty());
    assert_eq!(actor.pool.occupied(JobKind::Translation), 1);
    assert_eq!(actor.pool.translation_tries("mt"), Some(1));
    assert_eq!(
        actor.library.store().translation_job("mt")?.state,
        "running"
    );
    actor.settlement_override = None;
    assert_eq!(actor.drive_settlement()?, ());
    let job = actor.library.store().translation_job("mt")?;
    assert_eq!(job.state, "failed");
    assert_eq!(job.reason.as_deref(), Some("worker-start-failed"));
    assert_eq!(job.attempt, 1);
    assert!(translation_attempts(&actor)?.is_empty());
    assert!(actor.pool.translation_slots.is_empty());
    assert_eq!(actor.settlement_attempts, 1);
    Ok(())
}
