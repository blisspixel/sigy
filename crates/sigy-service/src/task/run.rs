//! Explicit zero-cost delegation for one frozen literal-evidence briefing.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, task::MAX_CHECKPOINTS};

pub const RUN_TEMPLATE: &str = "literal-briefing-v1";
pub const MAX_RUN_FINDINGS: u32 = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRunSpec {
    pub checkpoint_ordinal: u32,
    pub maximum_findings: u32,
}

impl TaskRunSpec {
    /// # Errors
    /// Refuses an absent checkpoint or a finding ceiling outside the finite template.
    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_CHECKPOINTS).contains(&self.checkpoint_ordinal)
            || !(1..=MAX_RUN_FINDINGS).contains(&self.maximum_findings)
        {
            return Err(Error::InvalidInput("task run scope"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunState {
    Running,
    Completed,
    Partial,
    Cancelled,
    Revoked,
}

/// A stored catalog effect or terminal lifecycle receipt, in committed order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRunStep {
    pub ordinal: u32,
    pub kind: String,
    pub effect_id: String,
    pub citation_ordinal: Option<u32>,
    pub finding_id: Option<String>,
    pub reason: Option<String>,
    pub recorded_ms: i64,
}

/// Completed means the finite template ended. Semantic goal success stays unmeasured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRunView {
    pub task_id: String,
    pub request_id: String,
    pub spec: TaskRunSpec,
    pub generation: u32,
    pub state: TaskRunState,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub planned_findings: u32,
    pub published_findings: u32,
    pub skipped_findings: u32,
    pub briefing_id: Option<String>,
    pub steps: Vec<TaskRunStep>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegation_has_finite_citations_and_no_embedded_authority() -> Result<()> {
        let spec = TaskRunSpec {
            checkpoint_ordinal: 1,
            maximum_findings: 64,
        };
        spec.validate()?;
        for invalid in [
            TaskRunSpec {
                checkpoint_ordinal: 0,
                ..spec.clone()
            },
            TaskRunSpec {
                checkpoint_ordinal: 129,
                ..spec.clone()
            },
            TaskRunSpec {
                maximum_findings: 0,
                ..spec.clone()
            },
            TaskRunSpec {
                maximum_findings: 65,
                ..spec.clone()
            },
        ] {
            assert!(invalid.validate().is_err());
        }
        let mut value = serde_json::to_value(&spec)?;
        value["shell"] = serde_json::json!("arbitrary");
        assert!(serde_json::from_value::<TaskRunSpec>(value).is_err());
        Ok(())
    }
}
