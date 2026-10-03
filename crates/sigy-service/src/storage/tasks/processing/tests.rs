//! Catalog fixtures for task processing. Synthetic recognizer and translator outcomes
//! supply storage facts only; they are not native containment or language evidence.

use super::*;
use crate::{
    monitor::{MonitorCaptureBounds, MonitorSpec, MonitorTerm, pipeline},
    recognition::{
        LocalAsrOutcome, LocalAsrRequest, ReapedLocalAsr, RecognitionCoverage, RecognitionCue,
        RecognitionOutput, RecognitionProfile,
    },
    sources::{HttpHop, HttpSource, NetworkScope},
    storage::dvr::Publication,
    task::{
        TaskSpec,
        collection::{TaskCaptureSpec, TaskCollectionSpec},
    },
    translation::{
        TranslatedCue, TranslationOutcome, TranslationProfile, TranslationRequest,
        TranslationResult,
    },
};

mod admission;
mod bounded_evidence;
mod evidence;
mod lineage;
mod recovery;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const START: i64 = 1_790_078_400_000;
/// Decoded audio of the two collected recordings: each covers its full planned interval.
const AUDIO: [u64; 2] = [30_000_000, 30_000_000];

fn recognition_profile() -> Result<RecognitionProfile> {
    let root = std::env::temp_dir();
    let mut profile = RecognitionProfile {
        id: "asr".into(),
        engine: crate::recognition::WHISPER_CPP_CLI.into(),
        runtime_dir: root.join("sigy-absent-runtime").display().to_string(),
        executable: "whisper-cli.exe".into(),
        runtime_sha256: "1".repeat(64),
        runtime_files: 3,
        runtime_bytes: 300,
        model_path: root.join("sigy-absent-model.bin").display().to_string(),
        model_sha256: "2".repeat(64),
        model_bytes: 1000,
        vad_path: root.join("sigy-absent-vad.bin").display().to_string(),
        vad_sha256: "3".repeat(64),
        vad_bytes: 10,
        threads: 2,
        memory_bytes: 1 << 30,
        deadline_ms: 60_000,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    Ok(profile)
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
        model_path: root.join("sigy-absent-mt.gguf").display().to_string(),
        model_sha256: "5".repeat(64),
        model_bytes: 1000,
        languages: "ar,es,fr".into(),
        threads: 1,
        memory_bytes: 1 << 30,
        cue_deadline_ms: 60_000,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    Ok(profile)
}

fn policy(monitor_processing: bool) -> MonitorSpec {
    MonitorSpec {
        name: "Water".into(),
        goal: "Follow literal water reports".into(),
        terms: vec![
            MonitorTerm {
                language: "es".into(),
                text: "agua".into(),
            },
            MonitorTerm {
                language: "en".into(),
                text: "water".into(),
            },
        ],
        sources: vec!["a:v1".into(), "b:v1".into()],
        candidate_sources: vec![],
        schedules: vec![],
        daily_audio_seconds: 600,
        total_audio_seconds: 600,
        recognition_profile: monitor_processing.then(|| "asr".into()),
        translation_profile: monitor_processing.then(|| "mt".into()),
        capture: Some(MonitorCaptureBounds {
            daily_seconds: 120,
            total_seconds: 120,
            total_bytes: 4096,
        }),
    }
}

fn collection() -> TaskCollectionSpec {
    TaskCollectionSpec {
        captures: vec![
            TaskCaptureSpec {
                source_revision: "a:v1".into(),
                start_ms: START,
                duration_seconds: 30,
                maximum_bytes: 1024,
            },
            TaskCaptureSpec {
                source_revision: "b:v1".into(),
                start_ms: START + 60_000,
                duration_seconds: 30,
                maximum_bytes: 1024,
            },
        ],
    }
}

/// A store with two sources, local profiles, a monitor, a task and its collection grant.
fn setup(path: &std::path::Path, monitor_processing: bool) -> Result<Store> {
    let mut store = Store::open(path)?;
    populate(&mut store, monitor_processing)?;
    Ok(store)
}

fn populate(store: &mut Store, monitor_processing: bool) -> Result<()> {
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        std::env::current_exe()?
            .to_str()
            .ok_or(Error::StorageIntegrity)?,
    )?;
    for source in ["a:v1", "b:v1", "c:v1"] {
        store.register_source(
            source,
            &HttpSource::new(
                "Public",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
    }
    store.add_recognition_profile(&recognition_profile()?, START - 5000)?;
    store.add_translation_profile(&translation_profile()?, START - 5000)?;
    store.create_monitor("monitor", &policy(monitor_processing), START - 3000)?;
    store.create_task(
        "task",
        &TaskSpec {
            goal: "Follow water reports".into(),
            monitor_id: "monitor".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: START,
            to_ms: START + 900_000,
        },
        START - 2000,
    )?;
    store.start_task_collection("task", "collect", &collection(), 0, START - 1000)?;
    Ok(())
}

/// Bytes standing in for one retained file. They are checksum fixtures, not speech.
fn media(ordinal: usize) -> Vec<u8> {
    format!("task processing fixture {ordinal}").into_bytes()
}

fn publication(ordinal: usize) -> Publication {
    use sha2::{Digest, Sha256};
    let bytes = media(ordinal);
    let decoded = AUDIO[ordinal];
    Publication {
        bytes: bytes.len() as u64,
        sha256: crate::storage::dvr::hex(&Sha256::digest(&bytes)),
        format: "wav",
        decoded_microseconds: decoded,
        end_reason: "end_of_body",
        http_route: vec![HttpHop {
            origin: "https://example.com".into(),
            peer: ([8, 8, 8, 8], 443).into(),
            status: 200,
        }],
        observations: Vec::new(),
        segments_sealed: false,
        gap: None,
    }
}

/// Admit and publish both collected recordings; returns their exact identities.
fn collect(store: &mut Store) -> Result<Vec<String>> {
    let mut recordings = Vec::new();
    for (ordinal, at) in [START, START + 60_000].into_iter().enumerate() {
        let mut launches = store.reconcile_schedules_at(at, true)?.launches;
        if launches.len() != 1 {
            return Err(Error::StorageIntegrity);
        }
        let launch = launches.remove(0);
        store.publish_recording(&launch.job.version, &publication(ordinal))?;
        recordings.push(launch.job.version.id().to_owned());
    }
    Ok(recordings)
}

fn spec(seconds: u32, translation: bool) -> TaskProcessingSpec {
    TaskProcessingSpec {
        recognition_profile: "asr".into(),
        translation_profile: translation.then(|| "mt".into()),
        maximum_audio_seconds: seconds,
    }
}

/// Publish the shared pin and build the derived canonical recognition request.
fn recognition(store: &mut Store, recording: &str, now: i64) -> Result<LocalAsrRequest> {
    let pin = pipeline::pin_id(recording);
    let (_, record) = store.admit_analysis(&pin, recording, false, now)?;
    let record = if record.state == "published" {
        record
    } else {
        store.publish_analysis(&pin, record.revision)?
    };
    let profile = store.recognition_profile("asr")?;
    Ok(LocalAsrRequest {
        id: pipeline::recognition_job_id(recording, &profile.profile_sha256),
        analysis_id: pin,
        analysis_revision: record.revision,
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
        parent_revision: 0,
    })
}

fn translation(store: &Store, recording: &str, revision: i64) -> Result<TranslationRequest> {
    let pin = pipeline::pin_id(recording);
    let profile = store.translation_profile("mt")?;
    Ok(TranslationRequest {
        id: pipeline::translation_job_id(&pin, revision, &profile.profile_sha256),
        transcript_id: pin,
        transcript_revision: revision,
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
    })
}

fn scope(ordinal: u32, recording: &str, audio_us: u64) -> TaskJobScope<'_> {
    TaskJobScope {
        task_id: "task",
        ordinal,
        recording_id: recording,
        audio_us,
    }
}

/// Run the queued recognition job through a synthetic drained proof.
fn hear(store: &mut Store, job: &str, script: &str, now: i64) -> Result<()> {
    let work = store
        .claim_local_asr(job, "fixture", now)?
        .ok_or(Error::StorageIntegrity)?;
    let coverages = work
        .input
        .chunks
        .iter()
        .map(|chunk| RecognitionCoverage {
            ordinal: chunk.ordinal,
            interval_ordinal: chunk.interval_ordinal,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            source_sha256: chunk.source_sha256.clone(),
            decoded_sha256: "c".repeat(64),
            sample_rate: 16_000,
            sample_count: (chunk.end_us - chunk.start_us).saturating_mul(16_000) / 1_000_000,
        })
        .collect();
    let chunk = work.input.chunks.first().ok_or(Error::StorageIntegrity)?;
    let cues = if script.is_empty() {
        Vec::new()
    } else {
        vec![RecognitionCue {
            ordinal: 0,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            script: script.into(),
        }]
    };
    let output = RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages,
        cues,
    };
    let proof = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(output));
    store.finish_local_asr(&work, &proof, now)?;
    Ok(())
}

fn translate(store: &mut Store, job: &str, english: &str, now: i64) -> Result<()> {
    let work = store
        .claim_translation(job, "fixture", now)?
        .ok_or(Error::StorageIntegrity)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &work.job,
        TranslationResult::Succeeded(vec![TranslatedCue {
            ordinal: 0,
            state: "translated".into(),
            english: Some(english.into()),
            reason: None,
        }]),
    );
    store.finish_translation(&work, &outcome, now)?;
    Ok(())
}

fn count(store: &Store, sql: &str) -> Result<u32> {
    Ok(store.connection.query_row(sql, [], |row| row.get(0))?)
}

/// Every effect a task admission can write, in a comparable order.
fn effects(store: &Store) -> Result<Vec<String>> {
    let mut rows = Vec::new();
    for sql in [
        "SELECT id || '|' || state || '|' || created_ms FROM analysis_jobs WHERE kind = 'local_asr' ORDER BY id",
        "SELECT id || '|' || state || '|' || created_ms FROM translation_jobs ORDER BY id",
        "SELECT task_id || '|' || ordinal || '|' || stage || '|' || decision || '|' || coalesce(reason, '') || '|' || coalesce(job_id, '') || '|' || audio_us || '|' || created_ms FROM task_processing_steps ORDER BY task_id, ordinal, stage",
        "SELECT family || '|' || job_id || '|' || authority || '|' || owner_id || '|' || origin || '|' || created_ms FROM job_interests ORDER BY family, job_id, authority, owner_id",
        "SELECT monitor_id || '|' || recording_id || '|' || stage || '|' || decision || '|' || audio_us FROM monitor_steps ORDER BY monitor_id, recording_id, stage",
    ] {
        let mut statement = store.connection.prepare(sql)?;
        let values = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.extend(values);
        rows.push("--".into());
    }
    Ok(rows)
}

#[test]
fn grant_requires_collection_profiles_and_current_scope_and_replays_exactly() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    for invalid in [spec(0, true), spec(1_801, true)] {
        assert!(matches!(
            store.start_task_processing("task", "grant", &invalid, 0, START - 500),
            Err(Error::InvalidInput(_))
        ));
    }
    let mut missing = spec(60, true);
    missing.recognition_profile = "absent".into();
    assert!(matches!(
        store.start_task_processing("task", "grant", &missing, 0, START - 500),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.start_task_processing("task", "grant", &spec(60, true), 0, START - 1500),
        Err(Error::InvalidInput("task processing clock"))
    ));
    let view = store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    assert_eq!(
        (view.generation, view.cancelled, view.charged_audio_us),
        (1, false, 0)
    );
    assert_eq!(view.paid_allowance_usd, "0.000000");
    assert_eq!(
        view.recognition_profile_sha256,
        recognition_profile()?.profile_sha256
    );
    assert!(view.steps.is_empty() && view.hold_reason.is_none());
    assert_eq!(
        store.start_task_processing("task", "grant", &spec(60, true), 0, 0)?,
        view
    );
    for (request, changed, generation) in [
        ("other", spec(60, true), 0),
        ("grant", spec(61, true), 0),
        ("grant", spec(60, false), 0),
        ("grant", spec(60, true), 1),
    ] {
        assert!(matches!(
            store.start_task_processing("task", request, &changed, generation, START),
            Err(Error::IdempotencyConflict)
        ));
    }
    for table in [
        "analysis_jobs",
        "translation_jobs",
        "task_processing_steps",
        "job_interests",
        "capture_jobs",
    ] {
        assert_eq!(count(&store, &format!("SELECT count(*) FROM {table}"))?, 0);
    }
    assert_eq!(
        store.pending_task_processing_ids(None, 4)?.0,
        Vec::<String>::new()
    );
    drop(store);
    assert_eq!(Store::open(&path)?.task_processing("task")?, Some(view));
    Ok(())
}

#[test]
fn grant_without_collection_or_after_drift_is_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    store.create_task(
        "bare",
        &TaskSpec {
            goal: "No collection".into(),
            monitor_id: "monitor".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: START,
            to_ms: START + 1000,
        },
        START - 900,
    )?;
    assert!(matches!(
        store.start_task_processing("bare", "grant", &spec(60, true), 0, START - 500),
        Err(Error::InvalidInput("task processing requires a collection"))
    ));
    store.propose_monitor_action(
        "monitor",
        "pause",
        crate::monitor::ActionOrigin::User,
        &crate::monitor::Proposal::Pause,
        START - 800,
    )?;
    assert!(matches!(
        store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500),
        Err(Error::InvalidInput("task monitor scope changed"))
    ));
    assert!(store.task_processing("task")?.is_none());
    Ok(())
}
