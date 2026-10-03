//! Exact, immutable observations of a finite task's own collection and processing.
//!
//! The observation records current facts once. Later completion, correction or media
//! release cannot expand its membership. Neither the observation nor its digest grants
//! permission to dispatch work or establishes language or semantic quality.

use serde::{Deserialize, Serialize};

use super::{
    TaskSpec, collection::TaskCollectionView, evidence::TaskEvidenceView,
    processing::TaskProcessingView,
};

pub const SNAPSHOT_TEMPLATE: &str = "collected-evidence-snapshot-v1";
pub const SNAPSHOT_MODE: &str = "freeze-now-v1";
pub const MAX_SNAPSHOT_BYTES: usize = 65_536;

/// Exact request identity and native attempt observed in the snapshot transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskJobObservation {
    pub ordinal: u32,
    pub stage: String,
    pub job_id: String,
    pub observed_generation: u32,
    pub observed_state: String,
    pub input_id: String,
    pub input_revision: i64,
    pub profile: String,
    pub profile_sha256: String,
    /// The recognition input manifest, absent for a translation of stored text.
    pub manifest_sha256: Option<String>,
    pub expected_parent_revision: Option<i64>,
    /// Translation direction is explicit. Recognition has no target.
    pub target: Option<String>,
}

/// Frozen observed metadata of an exact recording, without paths or source URLs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskMediaObservation {
    pub ordinal: u32,
    pub recording_id: String,
    pub media_sha256: Option<String>,
    /// Bytes observed at freeze. Later segment release may reduce the live count.
    pub media_bytes: Option<u64>,
    pub decoded_us: Option<u64>,
}

/// A historical observation. Pending work stays observed pending after it completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEvidenceSnapshot {
    pub task_id: String,
    pub ordinal: u32,
    pub request_id: String,
    pub expected_snapshot: u32,
    pub observed_ms: i64,
    pub template: String,
    pub mode: String,
    pub scope: TaskSpec,
    pub scope_sha256: String,
    pub monitor_spec_sha256: String,
    pub collection: TaskCollectionView,
    pub processing: Option<TaskProcessingView>,
    pub jobs: Vec<TaskJobObservation>,
    pub media: Vec<TaskMediaObservation>,
    /// Legacy planned/recorded/recognized counters keep their original meanings.
    pub evidence: TaskEvidenceView,
}
