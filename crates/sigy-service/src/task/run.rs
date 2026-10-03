//! Explicit zero-cost delegation for one frozen literal-evidence briefing.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, task::MAX_CHECKPOINTS};

pub const RUN_TEMPLATE: &str = "literal-briefing-v1";
pub const EXACT_RUN_TEMPLATE: &str = "collected-literal-briefing-v1";
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

/// A publication selection bound to one exact frozen task observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSnapshotRunSpec {
    pub snapshot_ordinal: u32,
    pub maximum_findings: u32,
}

impl TaskSnapshotRunSpec {
    /// # Errors
    /// Refuses an absent snapshot or a ceiling outside the same finite publication plan.
    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_CHECKPOINTS).contains(&self.snapshot_ordinal)
            || !(1..=MAX_RUN_FINDINGS).contains(&self.maximum_findings)
        {
            return Err(Error::InvalidInput("task exact publication scope"));
        }
        Ok(())
    }
}

/// Disjoint wire shapes preserve the legacy checkpoint payload and distinguish exact work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TaskRunSelection {
    Checkpoint(TaskRunSpec),
    Snapshot(TaskSnapshotRunSpec),
}

impl TaskRunSelection {
    #[must_use]
    pub fn maximum_findings(&self) -> u32 {
        match self {
            Self::Checkpoint(spec) => spec.maximum_findings,
            Self::Snapshot(spec) => spec.maximum_findings,
        }
    }

    #[must_use]
    pub fn ordinal(&self) -> u32 {
        match self {
            Self::Checkpoint(spec) => spec.checkpoint_ordinal,
            Self::Snapshot(spec) => spec.snapshot_ordinal,
        }
    }

    #[must_use]
    pub fn origin(&self) -> &'static str {
        match self {
            Self::Checkpoint(_) => "checkpoint",
            Self::Snapshot(_) => "snapshot",
        }
    }

    #[must_use]
    pub fn template(&self) -> &'static str {
        match self {
            Self::Checkpoint(_) => RUN_TEMPLATE,
            Self::Snapshot(_) => EXACT_RUN_TEMPLATE,
        }
    }

    /// # Errors
    /// Rejects an invalid typed selection without interpreting text as authority.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Checkpoint(spec) => spec.validate(),
            Self::Snapshot(spec) => spec.validate(),
        }
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
    pub spec: TaskRunSelection,
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

/// A canonical finding reference. Text remains available through finding inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEvidenceBriefingMember {
    pub finding_id: String,
    pub group_ordinal: u32,
}

/// Exact frozen task coverage, distinct from monitor-wide coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEvidenceBriefing {
    pub task_id: String,
    pub id: String,
    pub monitor_id: String,
    pub generation: u32,
    pub created_ms: i64,
    pub snapshot: Box<super::snapshot::TaskEvidenceSnapshot>,
    pub members: Vec<TaskEvidenceBriefingMember>,
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
