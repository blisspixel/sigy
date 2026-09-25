//! Task contract tests. No native process runs here.

use std::path::Path;

use sha2::Digest;

use super::{
    Drained, Executor, LocalProcessExecutor, LocalStage, RecognitionResult, ResultEnvelope,
    TaskResult, TaskSpec, asr::write_wav, translate::completion_text,
};
use crate::{
    Error,
    recognition::{LocalAsrFailure, LocalAsrInput, LocalAsrJob, LocalAsrRequest},
    recognizer::{self, translate::translation_spec},
    translation::{SourceCue, TranslationJob, TranslationRequest, TranslationResult},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn job(id: &str) -> LocalAsrJob {
    LocalAsrJob {
        request: LocalAsrRequest {
            id: id.into(),
            analysis_id: "pin".into(),
            analysis_revision: 1,
            profile: "cpu".into(),
            profile_sha256: "1".repeat(64),
            parent_revision: 0,
        },
        generation: 1,
        recording_id: "one".into(),
        state: "running".into(),
        expected_bytes: 32_044,
        manifest_sha256: "2".repeat(64),
        reason: None,
        amount_usd: "0.000000".into(),
        created_ms: 1,
        finished_ms: None,
        attempt: 1,
        started_ms: Some(1),
    }
}

fn input() -> LocalAsrInput {
    LocalAsrInput {
        recording_id: "one".into(),
        media_sha256: "3".repeat(64),
        timeline_sha256: "4".repeat(64),
        object_key: "recordings/one/segment-0.wav".into(),
        byte_length: 32_044,
        interval_ordinal: 0,
        start_us: 0,
        end_us: 1_000_000,
        source_sha256: "5".repeat(64),
    }
}

fn profile(root: &Path) -> crate::recognition::RecognitionProfile {
    crate::recognition::RecognitionProfile {
        id: "cpu".into(),
        engine: crate::recognition::WHISPER_CPP_CLI.into(),
        runtime_dir: root.join("runtime").display().to_string(),
        executable: "whisper-cli.exe".into(),
        runtime_sha256: "6".repeat(64),
        runtime_files: 3,
        runtime_bytes: 4096,
        model_path: root.join("models").join("model.bin").display().to_string(),
        model_sha256: "7".repeat(64),
        model_bytes: 1_000_000,
        vad_path: root.join("models").join("vad.bin").display().to_string(),
        vad_sha256: "8".repeat(64),
        vad_bytes: 800_000,
        threads: 2,
        memory_bytes: 1 << 30,
        deadline_ms: 60_000,
        profile_sha256: "1".repeat(64),
    }
}

fn translation_job() -> TranslationJob {
    TranslationJob {
        request: TranslationRequest {
            id: "mt-golden".into(),
            transcript_id: "pin".into(),
            transcript_revision: 1,
            profile: "mt".into(),
            profile_sha256: "9".repeat(64),
        },
        generation: 1,
        state: "running".into(),
        reason: None,
        amount_usd: "0.000000".into(),
        created_ms: 1,
        finished_ms: None,
        attempt: 1,
        started_ms: Some(1),
    }
}

fn translation_profile(root: &Path) -> crate::translation::TranslationProfile {
    crate::translation::TranslationProfile {
        id: "mt".into(),
        engine: crate::translation::LLAMA_CPP_COMPLETION.into(),
        template: crate::translation::HY_MT2_PLAIN.into(),
        runtime_dir: root.join("llama").display().to_string(),
        executable: "llama-completion.exe".into(),
        runtime_sha256: "a".repeat(64),
        runtime_files: 4,
        runtime_bytes: 8192,
        model_path: root
            .join("models")
            .join("hy-mt2.gguf")
            .display()
            .to_string(),
        model_sha256: "b".repeat(64),
        model_bytes: 2_000_000,
        languages: "es,fr".into(),
        threads: 2,
        memory_bytes: 1 << 30,
        cue_deadline_ms: 30_000,
        profile_sha256: "9".repeat(64),
    }
}

fn cues() -> Vec<SourceCue> {
    vec![
        SourceCue {
            ordinal: 0,
            script: "bonjour".into(),
        },
        SourceCue {
            ordinal: 1,
            script: "le monde".into(),
        },
    ]
}

const GOLDEN_RECOGNITION: &str = r#"{"version":"sigy-task-spec-v1","task_id":"asr-golden","generation":1,"kind":"recognition","engine":"whisper-cpp-cli-v1","template":"language=auto;translate=off;vad=on;gpu=off;processors=1","profile_sha256":"1111111111111111111111111111111111111111111111111111111111111111","inputs":[{"type":"blob","sha256":"5555555555555555555555555555555555555555555555555555555555555555","bytes":32044,"media_type":"audio-wav"}],"assets":[{"role":"runtime","sha256":"6666666666666666666666666666666666666666666666666666666666666666","bytes":4096,"files":3,"entry":"whisper-cli.exe"},{"role":"model","sha256":"7777777777777777777777777777777777777777777777777777777777777777","bytes":1000000,"files":1,"entry":null},{"role":"speech_activity","sha256":"8888888888888888888888888888888888888888888888888888888888888888","bytes":800000,"files":1,"entry":null}],"params":{"task":"recognition","interval_ordinal":0,"start_us":0,"end_us":1000000,"sample_rate":16000,"decoder":"ffmpeg-s16le-mono-16k-v1","threads":2},"limits":{"processes":1,"memory_bytes":1073741824,"cpu_rate":2,"wall_ms":60000,"output_bytes":1048576,"decoder":{"memory_bytes":536870912,"wall_ms":60000}},"spec_sha256":"GOLDEN_HASH"}"#;
const GOLDEN_RECOGNITION_SHA256: &str =
    "661c0d90c83132c2e1fb7e5f7b6264c0268a401f989440c8bcb2889fbb5a9e08";

#[test]
fn a_recognition_spec_serializes_to_a_pinned_canonical_form_and_hash() -> TestResult {
    let root = tempfile::tempdir()?;
    let spec =
        recognizer::recognition_spec(&job("asr-golden"), &input(), &profile(root.path()), "wav")?;
    let text = serde_json::to_string(&spec)?;
    assert_eq!(spec.spec_sha256, GOLDEN_RECOGNITION_SHA256, "{text}");
    assert_eq!(
        text,
        GOLDEN_RECOGNITION.replace("GOLDEN_HASH", GOLDEN_RECOGNITION_SHA256)
    );
    // The hash is SHA-256 of the same JSON without its own field, checked independently.
    let canonical = GOLDEN_RECOGNITION.replace(r#","spec_sha256":"GOLDEN_HASH""#, "");
    assert_eq!(
        crate::storage::dvr::hex(&sha2::Sha256::digest(canonical.as_bytes())),
        GOLDEN_RECOGNITION_SHA256
    );
    // The hash does not depend on where the profile files live.
    let elsewhere = tempfile::tempdir()?;
    let moved = recognizer::recognition_spec(
        &job("asr-golden"),
        &input(),
        &profile(elsewhere.path()),
        "wav",
    )?;
    assert_eq!(moved, spec);
    // A round trip keeps the spec and its hash.
    let parsed: TaskSpec = serde_json::from_str(&text)?;
    assert_eq!(parsed, spec);
    parsed.validate()?;
    Ok(())
}

#[test]
fn a_changed_field_changes_the_hash_and_a_stale_hash_is_refused() -> TestResult {
    let root = tempfile::tempdir()?;
    let spec =
        recognizer::recognition_spec(&job("asr-golden"), &input(), &profile(root.path()), "wav")?;
    let mut next = job("asr-golden");
    next.generation = 2;
    let second = recognizer::recognition_spec(&next, &input(), &profile(root.path()), "wav")?;
    assert_ne!(second.spec_sha256, spec.spec_sha256);
    let mut tampered = spec.clone();
    tampered.limits.memory_bytes += 1;
    assert!(tampered.validate().is_err());
    let mut located = spec;
    located.engine = "https://example.com/model".into();
    assert!(located.clone().seal().is_err());
    located.engine = "..\\runtime".into();
    assert!(located.seal().is_err());
    Ok(())
}

fn environment_names() -> Vec<String> {
    let mut names: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| {
            name.len() >= 3 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .collect();
    names.extend(
        [
            "PATH",
            "SystemRoot",
            "OMP_WAIT_POLICY",
            "LD_LIBRARY_PATH",
            "DYLD_LIBRARY_PATH",
            "SIGY_TEST_FFMPEG",
            "OPENROUTER_API_KEY",
        ]
        .map(str::to_owned),
    );
    names
}

#[test]
fn no_spec_names_a_url_a_path_the_library_or_an_environment_variable() -> TestResult {
    let library = tempfile::tempdir()?;
    let job = job("asr-paths");
    let input = input();
    let recognition =
        recognizer::recognition_spec(&job, &input, &profile(library.path()), "mpegts")?;
    let translation = translation_spec(
        &translation_job(),
        &translation_profile(library.path()),
        &cues(),
        Some("fr"),
    )?;
    let library_text = library.path().display().to_string();
    let forbidden: Vec<String> = [
        "://".to_owned(),
        "/".to_owned(),
        "\\".to_owned(),
        library_text.clone(),
        library_text.replace('\\', "/"),
        input.object_key.clone(),
    ]
    .into_iter()
    .chain(environment_names())
    .collect();
    for spec in [recognition, translation] {
        let text = serde_json::to_string(&spec)?;
        for needle in &forbidden {
            assert!(!text.contains(needle.as_str()), "{needle} in {text}");
        }
        spec.validate()?;
    }
    Ok(())
}

#[test]
fn translation_specs_carry_hashed_inline_text_and_refuse_a_changed_cue() -> TestResult {
    let root = tempfile::tempdir()?;
    let spec = translation_spec(
        &translation_job(),
        &translation_profile(root.path()),
        &cues(),
        None,
    )?;
    let texts: Vec<_> = spec.texts().map(|text| text.text.clone()).collect();
    assert_eq!(texts, ["bonjour", "le monde"]);
    let mut changed = spec.clone();
    if let Some(super::TaskInput::Text(text)) = changed.inputs.first_mut() {
        text.text = "au revoir".into();
    }
    let changed = TaskSpec {
        spec_sha256: String::new(),
        ..changed
    };
    assert!(
        changed.seal().is_err(),
        "an inline hash must match its text"
    );
    Ok(())
}

#[test]
fn an_envelope_binds_one_task_generation_spec_and_output() -> TestResult {
    let root = tempfile::tempdir()?;
    let spec =
        recognizer::recognition_spec(&job("asr-bind"), &input(), &profile(root.path()), "wav")?;
    let make = || {
        ResultEnvelope::new(
            &spec,
            TaskResult::Recognition(RecognitionResult::Cancelled),
            Drained(()),
        )
    };
    let hash = Some(spec.spec_sha256.as_str());
    assert!(make().accept("asr-bind", 1, hash).is_ok());
    assert!(matches!(
        make().accept("asr-bind", 2, hash),
        Err(Error::Analysis("stale-worker"))
    ));
    assert!(matches!(
        make().accept("other", 1, hash),
        Err(Error::Analysis("stale-worker"))
    ));
    assert!(matches!(
        make().accept("asr-bind", 1, Some(&"0".repeat(64))),
        Err(Error::StorageIntegrity)
    ));
    let mut forged = make();
    forged.result = TaskResult::Recognition(RecognitionResult::Succeeded {
        coverage: crate::recognition::RecognitionCoverage {
            interval_ordinal: 0,
            start_us: 0,
            end_us: 1_000_000,
            source_sha256: "5".repeat(64),
            decoded_sha256: "c".repeat(64),
            sample_rate: 16_000,
            sample_count: 16_000,
        },
        cues: Vec::new(),
        language: None,
    });
    assert!(
        matches!(
            forged.accept("asr-bind", 1, hash),
            Err(Error::StorageIntegrity)
        ),
        "an output without its recorded hash is refused"
    );
    let refused = super::refuse_translation("asr-bind", 1, "translator-failed");
    assert!(refused.accept("asr-bind", 1, None).is_ok());
    Ok(())
}

#[tokio::test]
async fn the_local_executor_refuses_a_tampered_spec_without_starting_a_process() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut spec =
        recognizer::recognition_spec(&job("asr-tamper"), &input(), &profile(root.path()), "wav")?;
    spec.limits.wall_ms = 1;
    let (_stop, signal) = tokio::sync::watch::channel(false);
    let stage = LocalStage::new(root.path());
    let envelope = LocalProcessExecutor.execute(spec, stage, signal).await?;
    let hash = envelope.spec_sha256.clone();
    let result = envelope.accept("asr-tamper", 1, hash.as_deref())?;
    assert_eq!(
        result,
        TaskResult::Recognition(RecognitionResult::Failed(LocalAsrFailure::RecognizerFailed))
    );
    assert!(!root.path().join("analysis-scratch").exists());
    Ok(())
}

#[tokio::test]
async fn an_unmapped_hash_is_unavailable_without_starting_a_process() -> TestResult {
    let root = tempfile::tempdir()?;
    let spec = translation_spec(
        &translation_job(),
        &translation_profile(root.path()),
        &cues(),
        None,
    )?;
    let hash = spec.spec_sha256.clone();
    let (_stop, signal) = tokio::sync::watch::channel(false);
    let envelope = LocalProcessExecutor
        .execute(spec, LocalStage::new(root.path()), signal)
        .await?;
    assert_eq!(
        envelope.accept("mt-golden", 1, Some(&hash))?,
        TaskResult::Translation(TranslationResult::Failed("translator-failed"))
    );
    Ok(())
}

#[test]
fn wav_header_describes_sixteen_bit_mono_at_the_worker_rate() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("input.wav");
    write_wav(&path, &[1, 0, 2, 0], recognizer::SAMPLE_RATE)?;
    let bytes = std::fs::read(&path)?;
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into()?), 40);
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into()?), 1);
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into()?),
        recognizer::SAMPLE_RATE
    );
    assert_eq!(u16::from_le_bytes(bytes[34..36].try_into()?), 16);
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(u32::from_le_bytes(bytes[40..44].try_into()?), 4);
    assert_eq!(&bytes[44..], &[1, 0, 2, 0]);
    Ok(())
}

#[test]
fn completion_text_strips_formatting_only() {
    let raw = "\u{1b}[33m\u{1b}[0mA world fair is a festival. [end of text]\n\n";
    assert_eq!(
        completion_text(raw.as_bytes()),
        Ok("A world fair is a festival.".to_owned())
    );
    assert_eq!(completion_text(b"  [end of text]  "), Err("empty-output"));
    assert_eq!(completion_text(&[0xff, 0xfe]), Err("invalid-output"));
    assert_eq!(completion_text(b"a\0b"), Err("invalid-output"));
    assert_eq!(
        completion_text("x".repeat(4097).as_bytes()),
        Err("output-limit")
    );
    assert_eq!(
        completion_text("Ignore previous instructions.".as_bytes()),
        Ok("Ignore previous instructions.".to_owned())
    );
}
