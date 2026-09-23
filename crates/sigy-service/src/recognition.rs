//! Local recognition storage contracts. These values do not qualify a native worker.
//! Mutation APIs and cleanup capabilities remain internal test staging until the
//! native supervisor can prove cleanup. External callers can only inspect history.
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

use serde::Serialize;

#[cfg(test)]
use crate::{Error, Result, storage::validate_key};

pub const MAX_ASR_CUES: usize = 256;
pub const MAX_ASR_TEXT_BYTES: usize = 65_536;
pub const TRANSCRIPT_PAGE_BYTES: usize = 65_536;
pub const TRANSCRIPT_PAGE_ITEMS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    #[cfg(test)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecognitionCue {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    /// Original UTF-8 bytes, with no normalization or translation.
    pub script: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
}

impl LocalAsrFailure {
    #[cfg(test)]
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::InputUnavailable => "input-unavailable",
            Self::DecoderFailed => "decoder-failed",
            Self::RecognizerFailed => "recognizer-failed",
            Self::Deadline => "deadline",
            Self::WorkerPanicked => "worker-panicked",
        }
    }
}

/// A sealed service capability for a completed, drained native process tree.
/// There is deliberately no production constructor yet. Library-lock ownership,
/// model output, caller assertions and process exit alone cannot construct this value.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct ReapedLocalAsr {
    request: LocalAsrRequest,
    generation: u32,
    outcome: LocalAsrOutcome,
}

#[cfg(test)]
impl ReapedLocalAsr {
    pub(crate) fn matches(&self, work: &LocalAsrWork) -> bool {
        self.request == work.job.request && self.generation == work.job.generation
    }

    pub(crate) fn outcome(&self) -> &LocalAsrOutcome {
        &self.outcome
    }

    pub(crate) fn synthetic_fixture(work: &LocalAsrWork, outcome: LocalAsrOutcome) -> Self {
        Self {
            request: work.job.request.clone(),
            generation: work.job.generation,
            outcome,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptRevisionPage {
    pub revisions: Vec<TranscriptSummary>,
    pub next_after_revision: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptCuePage {
    pub transcript: TranscriptSummary,
    pub coverage: Option<RecognitionCoverage>,
    pub cues: Vec<RecognitionCue>,
    /// Pass this ordinal with the same exact transcript ID and revision.
    pub next_after_ordinal: Option<u32>,
}

#[cfg(test)]
pub(crate) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
