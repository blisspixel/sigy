//! Native local recognition supervisor.
//!
//! One job walks each retained segment. A window decodes at most 30 seconds of that
//! segment to 16 kHz mono PCM with the configured `FFmpeg`, then runs one pinned
//! recognizer process. When a phrase reaches the window edge and audio remains, that
//! phrase is not stored and the next window starts where the phrase starts. A window
//! with no internal phrase boundary is stored through its end, so a word inside that
//! one phrase can still be clipped. The service describes each window as a
//! content-addressed task spec and runs it through the local process executor; see
//! [`crate::execution`] for containment and the drain proof. The spec carries no
//! source URL, catalog handle, library path, provider secret or network configuration.
//! This is not an operating-system network sandbox; see the recognition decision
//! record for that limitation. A failure or cancellation publishes nothing. Restart
//! redoes every window.

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
        AsrSegment, CHUNK_US, ChunkLanguage, LocalAsrFailure, LocalAsrInput, LocalAsrJob,
        MAX_MODEL_BYTES, MAX_VAD_BYTES, PlannedChunk, ReapedLocalAsr, RecognitionCue,
        RecognitionProfile, WHISPER_CPP_CLI, WHISPER_TEMPLATE, coverages_fit_one_page,
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

/// The content-addressed description of one chunk. It names the retained segment and
/// the pinned profile files by hash only.
/// # Errors
/// Refuses a chunk that is outside its segment or a value that cannot form a valid spec.
pub(crate) fn recognition_spec(
    job: &LocalAsrJob,
    segment: &AsrSegment,
    chunk: &PlannedChunk,
    profile: &RecognitionProfile,
) -> Result<TaskSpec> {
    let (file_offset_us, slice_us) = chunk_trim(segment, chunk)?;
    TaskSpec {
        version: execution::SPEC_VERSION.to_owned(),
        task_id: job.request.id.clone(),
        generation: job.generation,
        kind: TaskKind::Recognition,
        engine: profile.engine.clone(),
        template: WHISPER_TEMPLATE.to_owned(),
        profile_sha256: profile.profile_sha256.clone(),
        inputs: vec![TaskInput::Blob(BlobRef {
            sha256: segment.source_sha256.clone(),
            bytes: segment.byte_length,
            media_type: format!("audio-{}", segment.format),
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
            interval_ordinal: chunk.interval_ordinal,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            sample_rate: SAMPLE_RATE,
            decoder: execution::DECODER_TEMPLATE.to_owned(),
            threads: profile.threads,
            file_offset_us: (file_offset_us > 0).then_some(file_offset_us),
            slice_us,
            chunk_ordinal: (chunk.ordinal > 0).then_some(chunk.ordinal),
            segment_end_us: (chunk.end_us < segment.end_us).then_some(segment.end_us),
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

fn chunk_trim(segment: &AsrSegment, chunk: &PlannedChunk) -> Result<(u64, Option<u64>)> {
    let inside = chunk.interval_ordinal == segment.ordinal
        && chunk.source_sha256 == segment.source_sha256
        && chunk.start_us >= segment.start_us
        && chunk.end_us <= segment.end_us
        && chunk.end_us > chunk.start_us;
    if !inside {
        return Err(Error::InvalidInput("task spec"));
    }
    let offset = chunk.start_us - segment.start_us;
    let whole = chunk.start_us == segment.start_us && chunk.end_us == segment.end_us;
    Ok((offset, (!whole).then_some(chunk.end_us - chunk.start_us)))
}

/// Map the spec's hashes to the retained segments, the decoder and the profile files.
/// # Errors
/// Refuses an object key that does not name a file inside the library.
pub(crate) fn recognition_stage(task: &RecognitionTask) -> Result<LocalStage> {
    let mut stage = LocalStage::new(&task.directory).decoder(&task.decoder);
    for segment in &task.input.segments {
        let path = crate::recordings::media_path(&task.directory, &segment.object_key)?;
        stage = stage.blob(&segment.source_sha256, path);
    }
    let profile = &task.profile;
    Ok(stage
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
    if task.input.chunks.is_empty() || task.input.segments.is_empty() {
        return refused(job, LocalAsrFailure::InputUnavailable);
    }
    let mut gathered = Gathered::default();
    for segment in &task.input.segments {
        if let Some(reaped) =
            advance_segment(&task, &stage, segment, &signal, &mut gathered).await?
        {
            return Ok(reaped);
        }
    }
    finish_chunks(job, gathered)
}

async fn advance_segment(
    task: &RecognitionTask,
    stage: &LocalStage,
    segment: &AsrSegment,
    signal: &watch::Receiver<bool>,
    gathered: &mut Gathered,
) -> Result<Option<ReapedLocalAsr>> {
    let mut cursor = segment.start_us;
    while cursor < segment.end_us {
        if *signal.borrow() {
            return Ok(Some(cancelled(&task.job)?));
        }
        if !coverages_fit_one_page(gathered.coverages.len().saturating_add(1)) {
            return Ok(Some(refused(&task.job, LocalAsrFailure::InvalidOutput)?));
        }
        let heard_end = cursor.saturating_add(CHUNK_US).min(segment.end_us);
        let ordinal =
            u32::try_from(gathered.coverages.len()).map_err(|_| Error::StorageIntegrity)?;
        let window = PlannedChunk {
            ordinal,
            interval_ordinal: segment.ordinal,
            start_us: cursor,
            end_us: heard_end,
            source_sha256: segment.source_sha256.clone(),
        };
        match run_chunk(task, stage, &window, signal).await? {
            ChunkStep::Stop(reaped) => return Ok(Some(reaped)),
            ChunkStep::Piece(piece) => {
                if !window_accepted(&piece, &window) {
                    return Ok(Some(refused(&task.job, LocalAsrFailure::InvalidOutput)?));
                }
                cursor = piece.coverage.end_us;
                gathered.push(piece)?;
            }
        }
    }
    Ok(None)
}

fn window_accepted(piece: &ChunkPiece, window: &PlannedChunk) -> bool {
    let coverage = &piece.coverage;
    let inside = coverage.ordinal == window.ordinal
        && coverage.interval_ordinal == window.interval_ordinal
        && coverage.source_sha256 == window.source_sha256
        && coverage.start_us == window.start_us
        && coverage.end_us > window.start_us
        && coverage.end_us <= window.end_us;
    let cues_inside = piece.cues.iter().all(|cue| {
        cue.start_us >= coverage.start_us
            && cue.end_us <= coverage.end_us
            && cue.end_us > cue.start_us
    });
    let language_inside = piece.language.as_ref().is_none_or(|language| {
        language.ordinal == coverage.ordinal
            && language.interval_ordinal == coverage.interval_ordinal
            && language.start_us == coverage.start_us
            && language.end_us == coverage.end_us
            && !language.code.is_empty()
    });
    inside && cues_inside && language_inside
}

enum ChunkStep {
    Stop(ReapedLocalAsr),
    Piece(ChunkPiece),
}

struct ChunkPiece {
    coverage: crate::recognition::RecognitionCoverage,
    cues: Vec<RecognitionCue>,
    language: Option<ChunkLanguage>,
    spec_sha256: String,
}

#[derive(Default)]
struct Gathered {
    coverages: Vec<crate::recognition::RecognitionCoverage>,
    cues: Vec<RecognitionCue>,
    languages: Vec<ChunkLanguage>,
    hashes: Vec<String>,
}

impl Gathered {
    fn push(&mut self, piece: ChunkPiece) -> Result<()> {
        self.coverages.push(piece.coverage);
        let base = u32::try_from(self.cues.len()).map_err(|_| Error::StorageIntegrity)?;
        for (offset, mut cue) in piece.cues.into_iter().enumerate() {
            let index = u32::try_from(offset).map_err(|_| Error::StorageIntegrity)?;
            cue.ordinal = base.checked_add(index).ok_or(Error::StorageIntegrity)?;
            self.cues.push(cue);
        }
        if let Some(language) = piece.language {
            self.languages.push(language);
        }
        self.hashes.push(piece.spec_sha256);
        Ok(())
    }
}

async fn run_chunk(
    task: &RecognitionTask,
    stage: &LocalStage,
    chunk: &PlannedChunk,
    signal: &watch::Receiver<bool>,
) -> Result<ChunkStep> {
    let job = &task.job;
    let Some(segment) = task.input.segment_for(chunk.interval_ordinal) else {
        return Ok(ChunkStep::Stop(refused(
            job,
            LocalAsrFailure::InputUnavailable,
        )?));
    };
    let Ok(spec) = recognition_spec(job, segment, chunk, &task.profile) else {
        return Ok(ChunkStep::Stop(not_started(job)?));
    };
    let spec_sha256 = spec.spec_sha256.clone();
    let envelope = LocalProcessExecutor
        .execute(spec, stage.clone(), signal.clone())
        .await?;
    absorb_chunk(job, chunk, envelope, spec_sha256)
}

fn absorb_chunk(
    job: &LocalAsrJob,
    chunk: &PlannedChunk,
    envelope: execution::ResultEnvelope,
    spec_sha256: String,
) -> Result<ChunkStep> {
    let result = envelope.accept(&job.request.id, job.generation, Some(&spec_sha256))?;
    let execution::TaskResult::Recognition(result) = result else {
        return Err(Error::StorageIntegrity);
    };
    match result {
        execution::RecognitionResult::Succeeded {
            coverages,
            cues,
            languages,
        } => one_piece(job, chunk, coverages, cues, languages, spec_sha256),
        execution::RecognitionResult::Failed(failure) => {
            Ok(ChunkStep::Stop(refused(job, failure)?))
        }
        execution::RecognitionResult::Cancelled => Ok(ChunkStep::Stop(cancelled(job)?)),
    }
}

fn one_piece(
    job: &LocalAsrJob,
    chunk: &PlannedChunk,
    coverages: Vec<crate::recognition::RecognitionCoverage>,
    cues: Vec<RecognitionCue>,
    languages: Vec<ChunkLanguage>,
    spec_sha256: String,
) -> Result<ChunkStep> {
    if coverages.len() != 1 || languages.len() > 1 {
        return Ok(ChunkStep::Stop(refused(
            job,
            LocalAsrFailure::InvalidOutput,
        )?));
    }
    let mut coverage = coverages
        .into_iter()
        .next()
        .ok_or(Error::StorageIntegrity)?;
    coverage.ordinal = chunk.ordinal;
    let language = (!cues.is_empty())
        .then(|| languages.into_iter().next())
        .flatten()
        .map(|item| ChunkLanguage {
            ordinal: chunk.ordinal,
            interval_ordinal: coverage.interval_ordinal,
            start_us: coverage.start_us,
            end_us: coverage.end_us,
            code: item.code,
        });
    Ok(ChunkStep::Piece(ChunkPiece {
        coverage,
        cues,
        language,
        spec_sha256,
    }))
}

fn finish_chunks(job: &LocalAsrJob, gathered: Gathered) -> Result<ReapedLocalAsr> {
    let digest = crate::recognition::sha256_hex(&serde_json::to_vec(&(
        "sigy-local-asr-chunks-v1",
        &gathered.hashes,
    ))?);
    let envelope = execution::succeed_recognition(
        &job.request.id,
        job.generation,
        &digest,
        gathered.coverages,
        gathered.cues,
        gathered.languages,
    );
    ReapedLocalAsr::from_envelope(job, envelope, Some(&digest))
}

fn cancelled(job: &LocalAsrJob) -> Result<ReapedLocalAsr> {
    let envelope = execution::cancel_recognition(&job.request.id, job.generation);
    ReapedLocalAsr::from_envelope(job, envelope, None)
}

/// Block language labels for the coverages that produced cues.
/// Span ordinals count emitted spans, not silent coverages. The route stays unevaluated.
pub(crate) fn language_evidence(
    job: &LocalAsrJob,
    languages: &[ChunkLanguage],
) -> crate::languages::LanguageEvidence {
    use crate::languages::{LanguageEvidence, LanguageMethod, TranscriptReference};
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
        spans: language_spans(job, languages),
    }
}

fn language_spans(
    job: &LocalAsrJob,
    languages: &[ChunkLanguage],
) -> Vec<crate::languages::LanguageSpan> {
    use crate::languages::{LanguageLabel, LanguageRoute, LanguageSpan};
    let mut spans = Vec::new();
    for language in languages {
        if language.code.is_empty() || language.end_us <= language.start_us {
            continue;
        }
        let Ok(ordinal) = u32::try_from(spans.len()) else {
            break;
        };
        let request = &job.request;
        spans.push(LanguageSpan {
            ordinal,
            interval_ordinal: language.interval_ordinal,
            start_us: language.start_us,
            end_us: language.end_us,
            cue_ordinal: None,
            observation: "identified".into(),
            languages: vec![LanguageLabel {
                tag: whisper::language_tag(&language.code),
                provider_label: language.code.clone(),
            }],
            route: LanguageRoute {
                task: "transcription".into(),
                capability: "unevaluated".into(),
                profile: request.profile.clone(),
                profile_sha256: request.profile_sha256.clone(),
                basis: "declared".into(),
                basis_sha256: request.profile_sha256.clone(),
            },
        });
    }
    spans
}

pub mod translate;
mod whisper;
pub(crate) use whisper::parse_whisper_json;

#[cfg(test)]
mod tests;
