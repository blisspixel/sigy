//! Collection-to-evidence reconciliation for one task, read from exact owned identities.
//!
//! Each collected entry follows its own schedule occurrence, recording, task recognition
//! job, the transcript revision that job published, and the task translation of that
//! revision. Uncovered time (planned but not recorded) and unprocessed time (recorded but
//! not recognized) are reported separately. Citations are literal term matches against
//! the frozen monitor version; they are not semantic support, and quality stays unmeasured.

use serde::{Deserialize, Serialize};

use crate::task::TaskCitation;

pub const EVIDENCE_TEMPLATE: &str = "collected-literal-evidence-v1";

/// Operational outcome of the finite workflow. None of these measures language quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    /// Collection or processing can still advance under current authority.
    Pending,
    /// Every planned entry was recorded, recognized and translated, with literal citations.
    Cited,
    /// Complete coverage and processing, and no literal term matched.
    NoLiteralMatch,
    /// Settled or held with missing coverage, unprocessed audio or unsupported text.
    Partial,
}

/// One task processing stage: the immutable receipt and the live canonical job state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEvidenceStage {
    pub decision: String,
    pub reason: Option<String>,
    pub job_id: Option<String>,
    pub job_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEvidenceEntry {
    pub ordinal: u32,
    pub source_revision: String,
    pub planned_start_ms: i64,
    pub planned_us: u64,
    /// Schedule occurrence state: waiting, admitted or missed.
    pub schedule_state: String,
    pub recording_id: Option<String>,
    pub capture_state: Option<String>,
    pub storage_state: Option<String>,
    /// Decoded audio of the exact collected recording.
    pub recorded_us: u64,
    /// Planned time with no recorded audio.
    pub uncovered_us: u64,
    /// Recorded time the task recognition job published coverage for.
    pub recognized_us: u64,
    /// Recorded time without task recognition coverage.
    pub unprocessed_us: u64,
    pub recognition: Option<TaskEvidenceStage>,
    pub transcript_revision: Option<i64>,
    pub transcript_outcome: Option<String>,
    pub translation: Option<TaskEvidenceStage>,
    pub translation_revision: Option<i64>,
    pub cues: u32,
    pub translated_cues: u32,
    /// Why this entry is incomplete, in stage order.
    pub reasons: Vec<String>,
    /// Whether this entry can still advance under current authority.
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEvidenceView {
    pub id: String,
    pub template: String,
    pub monitor_id: String,
    /// The frozen monitor version whose literal terms were matched.
    pub monitor_version: u32,
    pub outcome: TaskOutcome,
    pub reasons: Vec<String>,
    pub planned_us: u64,
    pub recorded_us: u64,
    pub uncovered_us: u64,
    pub recognized_us: u64,
    pub unprocessed_us: u64,
    pub entries: Vec<TaskEvidenceEntry>,
    pub citations: Vec<TaskCitation>,
    pub more: bool,
    pub paid_allowance_usd: String,
}
