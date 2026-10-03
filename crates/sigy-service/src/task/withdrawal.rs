//! Explicit task-owned interest withdrawal, distinct from legacy admission cancellation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawnInterest {
    pub family: String,
    pub job_id: String,
    pub interest_created_ms: i64,
    pub job_generation: u32,
    pub decision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskWithdrawal {
    pub task_id: String,
    pub request_id: String,
    pub semantics: String,
    pub grant_sha256: String,
    pub expected_processing_generation: u32,
    pub withdrawal_generation: u32,
    pub step_mask: u32,
    pub created_ms: i64,
    pub interests: Vec<WithdrawnInterest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskWithdrawalView {
    pub receipt: TaskWithdrawal,
    pub completion_unproven: bool,
}
