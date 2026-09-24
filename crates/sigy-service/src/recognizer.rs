//! Native local recognition supervisor.
//!
//! One job decodes one retained interval to 16 kHz mono PCM with the configured
//! `FFmpeg`, then runs one pinned recognizer process. The service describes the job as a
//! content-addressed task spec and runs it through the local process executor; see
//! [`crate::execution`] for containment and the drain proof. The spec carries no
//! source URL, catalog handle, library path, provider secret or network configuration.
//! This is not an operating-system network sandbox; see the recognition decision record
//! for that limitation.

use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use tokio::sync::watch;

use crate::{
    Error, Result,
    execution::{
        self, AssetRef, AssetRole, BlobRef, DecoderLimits, Executor, LocalProcessExecutor,
        LocalStage, RecognitionParams, TaskInput, TaskKind, TaskLimits, TaskParams, TaskSpec,
        hash_file, runtime_manifest,
    },
    recognition::{
        LocalAsrFailure, LocalAsrInput, LocalAsrJob, MAX_MODEL_BYTES, MAX_VAD_BYTES,
        ReapedLocalAsr, RecognitionProfile, WHISPER_CPP_CLI, WHISPER_TEMPLATE,
    },
};

pub const SAMPLE_RATE: u32 = 16_000;
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
const DECODER_MEMORY: u64 = 512 * 1024 * 1024;
const DECODER_DEADLINE_MS: u64 = 60_000;

/// Everything one recognition job may read. Paths come from the catalog, never a source.
#[derive(Debug)]
pub(crate) struct RecognitionTask {
    pub directory: PathBuf,
    pub decoder: String,
    pub format: String,
    pub profile: RecognitionProfile,
    pub job: LocalAsrJob,
    pub input: LocalAsrInput,
}

/// Hash a local runtime directory, model and speech-activity model into a profile.
/// Reads only the named local files. Runs on the calling thread.
/// # Errors
/// Refuses missing files, links, oversized inputs or invalid limits.
#[allow(clippy::too_many_arguments)]
pub fn describe_profile(
    id: &str,
    runtime_dir: &Path,
    executable: &str,
    model: &Path,
    vad: &Path,
    threads: u32,
    memory_bytes: u64,
    deadline_ms: u64,
) -> Result<RecognitionProfile> {
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or(Error::InvalidInput("profile path is not valid Unicode"))
    };
    let never = AtomicBool::new(false);
    let runtime = runtime_manifest(runtime_dir, executable, &never)?;
    let (model_sha256, model_bytes) = hash_file(model, MAX_MODEL_BYTES, &never)?;
    let (vad_sha256, vad_bytes) = hash_file(vad, MAX_VAD_BYTES, &never)?;
    let mut profile = RecognitionProfile {
        id: id.to_owned(),
        engine: WHISPER_CPP_CLI.to_owned(),
        runtime_dir: text(runtime_dir)?,
        executable: executable.to_owned(),
        runtime_sha256: runtime.sha256,
        runtime_files: runtime.files,
        runtime_bytes: runtime.bytes,
        model_path: text(model)?,
        model_sha256,
        model_bytes,
        vad_path: text(vad)?,
        vad_sha256,
        vad_bytes,
        threads,
        memory_bytes,
        deadline_ms,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    profile.validate()?;
    Ok(profile)
}

/// A completion for a job whose worker never started a native process.
pub(crate) fn not_started(job: &LocalAsrJob) -> Result<ReapedLocalAsr> {
    refused(job, LocalAsrFailure::RecognizerFailed)
}

fn refused(job: &LocalAsrJob, failure: LocalAsrFailure) -> Result<ReapedLocalAsr> {
    let envelope = execution::refuse_recognition(&job.request.id, job.generation, failure);
    ReapedLocalAsr::from_envelope(job, envelope, None)
}

/// Remove scratch directories left by an earlier service process.
pub(crate) fn clear_scratch(directory: &Path) -> Result<()> {
    execution::clear_scratch(directory)
}

fn asset(role: AssetRole, sha256: &str, bytes: u64, files: u32, entry: Option<&str>) -> AssetRef {
    AssetRef {
        role,
        sha256: sha256.to_owned(),
        bytes,
        files,
        entry: entry.map(str::to_owned),
    }
}

/// The content-addressed description of one recognition job. It names the retained
/// interval and the pinned profile files by hash only.
/// # Errors
/// Refuses a profile or input whose values cannot form a valid spec.
pub(crate) fn recognition_spec(
    job: &LocalAsrJob,
    input: &LocalAsrInput,
    profile: &RecognitionProfile,
    format: &str,
) -> Result<TaskSpec> {
    TaskSpec {
        version: execution::SPEC_VERSION.to_owned(),
        task_id: job.request.id.clone(),
        generation: job.generation,
        kind: TaskKind::Recognition,
        engine: profile.engine.clone(),
        template: WHISPER_TEMPLATE.to_owned(),
        profile_sha256: profile.profile_sha256.clone(),
        inputs: vec![TaskInput::Blob(BlobRef {
            sha256: input.source_sha256.clone(),
            bytes: input.byte_length,
            media_type: format!("audio-{format}"),
        })],
        assets: vec![
            asset(
                AssetRole::Runtime,
                &profile.runtime_sha256,
                profile.runtime_bytes,
                profile.runtime_files,
                Some(&profile.executable),
            ),
            asset(
                AssetRole::Model,
                &profile.model_sha256,
                profile.model_bytes,
                1,
                None,
            ),
            asset(
                AssetRole::SpeechActivity,
                &profile.vad_sha256,
                profile.vad_bytes,
                1,
                None,
            ),
        ],
        params: TaskParams::Recognition(RecognitionParams {
            interval_ordinal: input.interval_ordinal,
            start_us: input.start_us,
            end_us: input.end_us,
            sample_rate: SAMPLE_RATE,
            decoder: execution::DECODER_TEMPLATE.to_owned(),
            threads: profile.threads,
        }),
        limits: TaskLimits {
            processes: 1,
            memory_bytes: profile.memory_bytes,
            cpu_rate: profile.threads,
            wall_ms: profile.deadline_ms,
            output_bytes: MAX_OUTPUT_BYTES,
            decoder: Some(DecoderLimits {
                memory_bytes: DECODER_MEMORY,
                wall_ms: DECODER_DEADLINE_MS,
            }),
        },
        spec_sha256: String::new(),
    }
    .seal()
}

/// Map the spec's hashes to the retained interval, the decoder and the profile files.
/// # Errors
/// Refuses an object key that does not name a file inside the library.
pub(crate) fn recognition_stage(task: &RecognitionTask) -> Result<LocalStage> {
    let input = crate::recordings::media_path(&task.directory, &task.input.object_key)?;
    let profile = &task.profile;
    Ok(LocalStage::new(&task.directory)
        .decoder(&task.decoder)
        .blob(&task.input.source_sha256, input)
        .asset(
            AssetRole::Runtime,
            &profile.runtime_sha256,
            PathBuf::from(&profile.runtime_dir),
        )
        .asset(
            AssetRole::Model,
            &profile.model_sha256,
            PathBuf::from(&profile.model_path),
        )
        .asset(
            AssetRole::SpeechActivity,
            &profile.vad_sha256,
            PathBuf::from(&profile.vad_path),
        ))
}

/// Run one job. Returns a completion only when every started process has drained.
/// An error means cleanup could not be proven; the caller must keep the read lease.
pub(crate) async fn run(
    task: RecognitionTask,
    signal: watch::Receiver<bool>,
) -> Result<ReapedLocalAsr> {
    let stage = recognition_stage(&task)?;
    let job = &task.job;
    if task.profile.identity().ok().as_deref() != Some(task.profile.profile_sha256.as_str()) {
        return refused(job, LocalAsrFailure::ProfileUnavailable);
    }
    let Ok(spec) = recognition_spec(job, &task.input, &task.profile, &task.format) else {
        return not_started(job);
    };
    let spec_sha256 = spec.spec_sha256.clone();
    let envelope = LocalProcessExecutor.execute(spec, stage, signal).await?;
    ReapedLocalAsr::from_envelope(job, envelope, Some(&spec_sha256))
}

/// The recognizer's block language label for one published text transcript.
/// It covers the whole interval at block resolution. It is recognizer evidence, not a
/// measured language capability, so the route stays unevaluated.
pub(crate) fn language_evidence(
    job: &LocalAsrJob,
    input: &LocalAsrInput,
    code: &str,
) -> crate::languages::LanguageEvidence {
    use crate::languages::{
        LanguageEvidence, LanguageLabel, LanguageMethod, LanguageRoute, LanguageSpan,
        TranscriptReference,
    };
    let request = &job.request;
    LanguageEvidence {
        id: request.id.clone(),
        revision: 1,
        analysis_id: request.analysis_id.clone(),
        analysis_revision: request.analysis_revision,
        transcript: Some(TranscriptReference {
            id: request.analysis_id.clone(),
            revision: request.parent_revision + 1,
        }),
        method: LanguageMethod {
            origin: "recognizer".into(),
            profile: request.profile.clone(),
            profile_sha256: request.profile_sha256.clone(),
            resolution: "block".into(),
            alias_map: "whisper-cpp-codes-v1".into(),
        },
        outcome: "succeeded".into(),
        reason: None,
        spans: vec![LanguageSpan {
            ordinal: 0,
            interval_ordinal: input.interval_ordinal,
            start_us: input.start_us,
            end_us: input.end_us,
            cue_ordinal: None,
            observation: "identified".into(),
            languages: vec![LanguageLabel {
                tag: whisper::language_tag(code),
                provider_label: code.to_owned(),
            }],
            route: LanguageRoute {
                task: "transcription".into(),
                capability: "unevaluated".into(),
                profile: request.profile.clone(),
                profile_sha256: request.profile_sha256.clone(),
                basis: "declared".into(),
                basis_sha256: request.profile_sha256.clone(),
            },
        }],
    }
}

pub mod translate;
mod whisper;
pub(crate) use whisper::parse_whisper_json;

#[cfg(test)]
mod tests;
