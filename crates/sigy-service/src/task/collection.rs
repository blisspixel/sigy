//! Explicit finite collection authority. Collection is not a semantic task result.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const COLLECTION_TEMPLATE: &str = "bounded-collection-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCaptureSpec {
    pub source_revision: String,
    pub start_ms: i64,
    pub duration_seconds: u32,
    pub maximum_bytes: u64,
}

impl TaskCaptureSpec {
    /// # Errors
    /// Refuses unbounded capture, invalid source identities or nonintegral UTC seconds.
    pub fn validate(&self) -> Result<()> {
        crate::storage::validate_key(&self.source_revision, "task capture source")?;
        if self.start_ms < 0
            || self.start_ms % 1000 != 0
            || !(1..=900).contains(&self.duration_seconds)
            || !(1..=268_435_456).contains(&self.maximum_bytes)
        {
            return Err(Error::InvalidInput("task capture bounds"));
        }
        jiff::Timestamp::from_millisecond(self.end_ms()?)
            .map_err(|_| Error::InvalidInput("task capture clock"))?;
        Ok(())
    }

    /// # Errors
    /// Refuses a UTC window that overflows the stored clock.
    pub fn end_ms(&self) -> Result<i64> {
        self.start_ms
            .checked_add(i64::from(self.duration_seconds) * 1000)
            .ok_or(Error::InvalidInput("task capture clock"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCollectionSpec {
    pub captures: Vec<TaskCaptureSpec>,
}

impl TaskCollectionSpec {
    /// # Errors
    /// Refuses empty or oversized plans and repeated sources.
    pub fn validate(&self) -> Result<()> {
        if !(1..=2).contains(&self.captures.len()) {
            return Err(Error::InvalidInput("task collection capture count"));
        }
        for capture in &self.captures {
            capture.validate()?;
        }
        if self.captures.len() == 2
            && self.captures[0].source_revision == self.captures[1].source_revision
        {
            return Err(Error::InvalidInput("task collection duplicate source"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCollectionCapture {
    pub rule_id: String,
    pub occurrence_id: Option<String>,
    /// Schedule state, not recording or semantic task success.
    pub state: String,
    pub recording_id: Option<String>,
    pub recording_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCollectionView {
    pub id: String,
    pub request_id: String,
    pub template: String,
    pub paid_allowance_usd: String,
    pub spec: TaskCollectionSpec,
    pub grant_sha256: String,
    pub scope_sha256: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub generation: u32,
    pub cancelled: bool,
    pub scope_current: bool,
    pub hold_reason: Option<String>,
    pub captures: Vec<TaskCollectionCapture>,
}
