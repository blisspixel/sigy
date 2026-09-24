//! Local recognition contracts. Admission, cancellation and publication are
//! crate-private and reachable only through the service supervisor, which must prove
//! that a native process tree is drained before it can construct a completion.
//! External callers can inspect history and describe profiles.
//!
//! ```compile_fail
//! use sigy_service::{recognition::LocalAsrRequest, storage::Store};
//! fn admit(store: &mut Store, request: &LocalAsrRequest) {
//!     let _ = store.admit_local_asr(request, 0);
//! }
//! ```
//!
//! ```compile_fail
//! use sigy_service::recognition::{LocalAsrOutcome, LocalAsrRequest, ReapedLocalAsr};
//! fn forge(request: LocalAsrRequest) -> ReapedLocalAsr {
//!     ReapedLocalAsr { request, generation: 1, outcome: LocalAsrOutcome::Cancelled }
//! }
//! ```
//!
//! ```compile_fail
//! use sigy_service::recognition::ReapedLocalAsr;
//! let _ = serde_json::from_str::<ReapedLocalAsr>("{}");
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result, storage::validate_key};

pub const MAX_ASR_CUES: usize = 256;
pub const MAX_ASR_TEXT_BYTES: usize = 65_536;
pub const TRANSCRIPT_PAGE_BYTES: usize = 65_536;
pub const TRANSCRIPT_PAGE_ITEMS: usize = 16;

/// The only native adapter. Its argument template is part of the profile identity.
pub const WHISPER_CPP_CLI: &str = "whisper-cpp-cli-v1";
pub(crate) const WHISPER_TEMPLATE: &str = "language=auto;translate=off;vad=on;gpu=off;processors=1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAsrRequest {
    pub id: String,
    pub analysis_id: String,
    pub analysis_revision: i64,
    pub profile: String,
    pub profile_sha256: String,
    /// Zero means there is no preceding transcript revision.
    pub parent_revision: i64,
}

impl LocalAsrRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_key(&self.id, "analysis job ID")?;
        validate_key(&self.analysis_id, "analysis input ID")?;
        validate_key(&self.profile, "recognition profile")?;
        if !(1..=64).contains(&self.analysis_revision)
            || !(0..64).contains(&self.parent_revision)
            || matches!(
                self.profile.as_str(),
                "local-unmeasured" | "retained-sha256-v1"
            )
            || !is_sha256(&self.profile_sha256)
        {
            return Err(Error::InvalidInput("recognition request"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAsrJob {
    pub request: LocalAsrRequest,
    pub generation: u32,
    pub recording_id: String,
    pub state: String,
    pub expected_bytes: u64,
    pub manifest_sha256: String,
    pub reason: Option<String>,
    pub amount_usd: String,
    pub created_ms: i64,
    pub finished_ms: Option<i64>,
}

/// An immutable catalog descriptor, never a source URL or filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalAsrInput {
    pub recording_id: String,
    pub media_sha256: String,
    pub timeline_sha256: String,
    pub object_key: String,
    pub byte_length: u64,
    pub interval_ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub source_sha256: String,
}

/// Returned only by a fresh committed admission. Replays cannot obtain a work token.
#[derive(Debug)]
pub struct LocalAsrWork {
    pub(crate) job: LocalAsrJob,
    pub(crate) input: LocalAsrInput,
}

impl LocalAsrWork {
    #[must_use]
    pub fn job(&self) -> &LocalAsrJob {
        &self.job
    }

    #[must_use]
    pub fn input(&self) -> &LocalAsrInput {
        &self.input
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionCue {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    /// Original UTF-8 bytes, with no normalization or translation.
    pub script: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionCoverage {
    pub interval_ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub source_sha256: String,
    pub decoded_sha256: String,
    pub sample_rate: u32,
    pub sample_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecognitionOutput {
    pub profile_sha256: String,
    pub manifest_sha256: String,
    pub coverage: RecognitionCoverage,
    /// An empty list means evaluated coverage with no recognized text, not a gap.
    pub cues: Vec<RecognitionCue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalAsrOutcome {
    Succeeded(RecognitionOutput),
    Failed(LocalAsrFailure),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAsrFailure {
    InputUnavailable,
    DecoderFailed,
    RecognizerFailed,
    Deadline,
    WorkerPanicked,
    /// A profile file is missing, changed, or no longer matches its recorded hash.
    ProfileUnavailable,
    /// The host cannot enforce the profile's process, memory and CPU limits.
    LimitsUnavailable,
    /// The recognizer's output failed bounded validation.
    InvalidOutput,
}

impl LocalAsrFailure {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::InputUnavailable => "input-unavailable",
            Self::DecoderFailed => "decoder-failed",
            Self::RecognizerFailed => "recognizer-failed",
            Self::Deadline => "deadline",
            Self::WorkerPanicked => "worker-panicked",
            Self::ProfileUnavailable => "profile-unavailable",
            Self::LimitsUnavailable => "limits-unavailable",
            Self::InvalidOutput => "invalid-worker-output",
        }
    }
}

/// A sealed service capability for a completed, drained native process tree.
/// Only an executor envelope, which carries containment evidence that only the
/// execution module can construct, can produce it.
/// Library-lock ownership, model output, caller assertions and process exit alone
/// cannot construct this value.
#[derive(Debug)]
pub(crate) struct ReapedLocalAsr {
    request: LocalAsrRequest,
    generation: u32,
    outcome: LocalAsrOutcome,
    /// The recognizer's own block language code, kept outside the replayed output.
    language: Option<String>,
}

impl ReapedLocalAsr {
    /// Accept an executor's envelope for exactly this job generation and spec. Only the
    /// execution module can construct an envelope, and only with containment evidence.
    /// # Errors
    /// Refuses an envelope for another task, generation or spec, or another task kind.
    pub(crate) fn from_envelope(
        job: &LocalAsrJob,
        envelope: crate::execution::ResultEnvelope,
        spec_sha256: Option<&str>,
    ) -> Result<Self> {
        use crate::execution::{RecognitionResult, TaskResult};
        let result = envelope.accept(&job.request.id, job.generation, spec_sha256)?;
        let TaskResult::Recognition(result) = result else {
            return Err(Error::StorageIntegrity);
        };
        let (outcome, language) = match result {
            RecognitionResult::Succeeded {
                coverage,
                cues,
                language,
            } => (
                LocalAsrOutcome::Succeeded(RecognitionOutput {
                    profile_sha256: job.request.profile_sha256.clone(),
                    manifest_sha256: job.manifest_sha256.clone(),
                    coverage,
                    cues,
                }),
                language,
            ),
            RecognitionResult::Failed(failure) => (LocalAsrOutcome::Failed(failure), None),
            RecognitionResult::Cancelled => (LocalAsrOutcome::Cancelled, None),
        };
        Ok(Self {
            request: job.request.clone(),
            generation: job.generation,
            outcome,
            language,
        })
    }

    pub(crate) fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub(crate) fn matches(&self, work: &LocalAsrWork) -> bool {
        self.request == work.job.request && self.generation == work.job.generation
    }

    pub(crate) fn outcome(&self) -> &LocalAsrOutcome {
        &self.outcome
    }

    #[cfg(test)]
    pub(crate) fn synthetic_fixture(work: &LocalAsrWork, outcome: LocalAsrOutcome) -> Self {
        Self {
            request: work.job.request.clone(),
            generation: work.job.generation,
            outcome,
            language: None,
        }
    }
}

/// One immutable local recognizer configuration. Paths locate files; hashes identify them.
/// The profile hash covers the engine, argument template, file hashes and limits, not paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionProfile {
    pub id: String,
    pub engine: String,
    pub runtime_dir: String,
    pub executable: String,
    pub runtime_sha256: String,
    pub runtime_files: u32,
    pub runtime_bytes: u64,
    pub model_path: String,
    pub model_sha256: String,
    pub model_bytes: u64,
    pub vad_path: String,
    pub vad_sha256: String,
    pub vad_bytes: u64,
    pub threads: u32,
    pub memory_bytes: u64,
    pub deadline_ms: u64,
    pub profile_sha256: String,
}

pub const MAX_PROFILE_PATH_BYTES: usize = 1024;
pub const MAX_MODEL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_VAD_BYTES: u64 = 64 * 1024 * 1024;
pub const MIN_WORKER_MEMORY: u64 = 256 * 1024 * 1024;
pub const MAX_WORKER_MEMORY: u64 = 64 * 1024 * 1024 * 1024;
pub const MAX_WORKER_THREADS: u32 = 64;
pub const MAX_WORKER_DEADLINE_MS: u64 = 3_600_000;

impl RecognitionProfile {
    /// The identity recorded on every job and transcript produced with this profile.
    /// # Errors
    /// Fails only if serialization fails.
    pub fn identity(&self) -> Result<String> {
        Ok(sha256_hex(&serde_json::to_vec(&(
            "sigy-recognition-profile-v1",
            &self.engine,
            WHISPER_TEMPLATE,
            &self.executable,
            &self.runtime_sha256,
            self.runtime_files,
            self.runtime_bytes,
            &self.model_sha256,
            self.model_bytes,
            &self.vad_sha256,
            self.vad_bytes,
            self.threads,
            self.memory_bytes,
            self.deadline_ms,
        ))?))
    }

    /// Validate bounds and the derived identity. Files are checked by the worker.
    /// # Errors
    /// Refuses invalid names, paths, hashes, limits or a mismatched identity.
    pub fn validate(&self) -> Result<()> {
        validate_key(&self.id, "recognition profile")?;
        let invalid = Error::InvalidInput("recognition profile");
        if matches!(self.id.as_str(), "local-unmeasured" | "retained-sha256-v1")
            || self.engine != WHISPER_CPP_CLI
            || !absolute_path(&self.runtime_dir)
            || !absolute_path(&self.model_path)
            || !absolute_path(&self.vad_path)
            || !file_name(&self.executable)
            || !is_sha256(&self.runtime_sha256)
            || !is_sha256(&self.model_sha256)
            || !is_sha256(&self.vad_sha256)
            || !(1..=256).contains(&self.runtime_files)
            || !(1..=1024 * 1024 * 1024).contains(&self.runtime_bytes)
            || !(1..=MAX_MODEL_BYTES).contains(&self.model_bytes)
            || !(1..=MAX_VAD_BYTES).contains(&self.vad_bytes)
            || !(1..=MAX_WORKER_THREADS).contains(&self.threads)
            || !(MIN_WORKER_MEMORY..=MAX_WORKER_MEMORY).contains(&self.memory_bytes)
            || !(1_000..=MAX_WORKER_DEADLINE_MS).contains(&self.deadline_ms)
        {
            return Err(invalid);
        }
        if self.identity()? != self.profile_sha256 {
            return Err(invalid);
        }
        Ok(())
    }
}

fn absolute_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROFILE_PATH_BYTES
        && !value.chars().any(char::is_control)
        && std::path::Path::new(value).is_absolute()
}

fn file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSummary {
    pub id: String,
    pub revision: i64,
    pub analysis_revision: i64,
    pub recording_id: String,
    pub media_sha256: String,
    pub kind: String,
    pub outcome: String,
    pub parent_revision: Option<i64>,
    pub profile: String,
    pub profile_sha256: Option<String>,
    pub job_id: Option<String>,
    pub job_generation: Option<u32>,
    pub cue_count: u32,
    pub text_bytes: u64,
    pub created_ms: i64,
    pub amount_usd: String,
    pub wording: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptRevisionPage {
    pub revisions: Vec<TranscriptSummary>,
    pub next_after_revision: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptCuePage {
    pub transcript: TranscriptSummary,
    pub coverage: Option<RecognitionCoverage>,
    pub cues: Vec<RecognitionCue>,
    /// Pass this ordinal with the same exact transcript ID and revision.
    pub next_after_ordinal: Option<u32>,
}

pub(crate) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    crate::storage::dvr::hex(&Sha256::digest(bytes))
}
