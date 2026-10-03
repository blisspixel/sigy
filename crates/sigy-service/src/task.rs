//! Durable user task scope and observed progress. These records grant no authority.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, monitor::MonitorCoverage};

pub mod collection;
pub mod evidence;
pub mod processing;
pub mod run;
pub mod snapshot;
pub mod withdrawal;

pub const MAX_TASKS: u32 = 256;
pub const MAX_CHECKPOINTS: u32 = 128;
pub const MAX_CHECKPOINT_BYTES: usize = 65_536;
pub const TASK_TEMPLATE: &str = "monitor-observation-v1";

/// A finite task bound to the monitor policy and complete stored actions the user inspected.
/// Paid allowance is fixed at zero; this observation contract creates no worker jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub goal: String,
    pub monitor_id: String,
    pub monitor_version: u32,
    pub monitor_actions: u32,
    pub from_ms: i64,
    pub to_ms: i64,
}

impl TaskSpec {
    /// # Errors
    /// Rejects invalid keys, empty/control-bearing goals, versions and unbounded windows.
    pub fn validate(&self) -> Result<()> {
        crate::storage::validate_key(&self.monitor_id, "task monitor")?;
        if self.goal.trim().is_empty()
            || self.goal.len() > 2_048
            || self.goal.chars().any(char::is_control)
            || self.monitor_version == 0
        {
            return Err(Error::InvalidInput("task scope"));
        }
        crate::monitor::check_window(self.from_ms, self.to_ms)
    }
}

/// Reference to observed text and media time, not a finding or a retention hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCitation {
    pub source: String,
    pub recording_id: String,
    pub transcript_id: String,
    pub transcript_revision: i64,
    pub translation_revision: Option<i64>,
    pub cue_ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
}

/// Frozen bounded observations. Nothing here establishes semantic task success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCheckpoint {
    pub task_id: String,
    pub ordinal: u32,
    pub request_id: String,
    pub observed_ms: i64,
    pub coverage: MonitorCoverage,
    pub citations: Vec<TaskCitation>,
    pub transcripts_scanned: u32,
    pub more: bool,
    pub window_elapsed: bool,
    pub monitor_paused: bool,
}

/// Scope remains readable after policy changes; only fresh observation is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskView {
    pub id: String,
    pub template: String,
    pub paid_allowance_usd: String,
    pub spec: TaskSpec,
    pub scope_sha256: String,
    pub monitor_spec_sha256: String,
    pub created_ms: i64,
    pub checkpoint: u32,
    pub snapshots: u32,
    pub scope_current: bool,
    pub latest_checkpoint: Option<Box<TaskCheckpoint>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TaskSpec {
        TaskSpec {
            goal: "تابع المياه / suivre les eaux".into(),
            monitor_id: "water".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: 0,
            to_ms: 86_400_000,
        }
    }

    #[test]
    fn scope_is_bounded_without_changing_original_script() -> Result<()> {
        let accepted = spec();
        accepted.validate()?;
        let json = serde_json::to_string(&accepted)?;
        assert_eq!(serde_json::from_str::<TaskSpec>(&json)?, accepted);
        for goal in [
            String::new(),
            " \t".into(),
            "a\u{1b}b".into(),
            "a".repeat(2_049),
        ] {
            let invalid = TaskSpec { goal, ..spec() };
            assert!(invalid.validate().is_err());
        }
        Ok(())
    }

    #[test]
    fn scope_refuses_bad_identity_version_and_clock() {
        for invalid in [
            TaskSpec {
                monitor_id: "../other".into(),
                ..spec()
            },
            TaskSpec {
                monitor_version: 0,
                ..spec()
            },
            TaskSpec {
                from_ms: -1,
                ..spec()
            },
            TaskSpec { to_ms: 0, ..spec() },
            TaskSpec {
                to_ms: crate::monitor::MAX_WINDOW_MS + 1,
                ..spec()
            },
        ] {
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn scope_refuses_embedded_authority_and_unknown_fields() -> Result<()> {
        let mut value = serde_json::to_value(spec())?;
        if let serde_json::Value::Object(ref mut fields) = value {
            fields.insert("paid_allowance".into(), serde_json::json!("20.000000"));
        }
        assert!(serde_json::from_value::<TaskSpec>(value).is_err());
        Ok(())
    }
}
