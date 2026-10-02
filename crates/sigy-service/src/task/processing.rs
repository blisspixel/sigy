//! Explicit finite processing of a task's exact collected recordings.
//!
//! A grant names local profiles and one lifetime audio allowance. The pure planner below
//! decides which canonical recognition or translation step a task wants next from durable
//! facts; the service admits each through the canonical job pool with its receipt, interest
//! and charge in one transaction. Processing success is not semantic or language quality.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Authorities holding one canonical job. Counts only; no owner text is copied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSharing {
    pub direct: bool,
    pub monitors: u32,
    pub tasks: u32,
}

pub const PROCESSING_TEMPLATE: &str = "collected-processing-v1";
/// Two collected recordings of at most 900 planned seconds each.
pub const MAX_PROCESSING_AUDIO_SECONDS: u32 = 1_800;
/// Most steps one task may take in one service pass.
pub const STEPS_PER_PASS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProcessingSpec {
    pub recognition_profile: String,
    pub translation_profile: Option<String>,
    /// Lifetime recognition audio across every collected recording. Never refilled.
    pub maximum_audio_seconds: u32,
}

impl TaskProcessingSpec {
    /// # Errors
    /// Refuses malformed profile identities or an allowance outside the finite template.
    pub fn validate(&self) -> Result<()> {
        crate::storage::validate_key(&self.recognition_profile, "task recognition profile")?;
        if let Some(profile) = &self.translation_profile {
            crate::storage::validate_key(profile, "task translation profile")?;
        }
        if !(1..=MAX_PROCESSING_AUDIO_SECONDS).contains(&self.maximum_audio_seconds) {
            return Err(Error::InvalidInput("task processing allowance"));
        }
        Ok(())
    }

    #[must_use]
    pub fn maximum_audio_us(&self) -> u64 {
        u64::from(self.maximum_audio_seconds) * 1_000_000
    }
}

/// One immutable task receipt. The current job state is a separate live read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProcessingStep {
    pub ordinal: u32,
    pub stage: String,
    pub recording_id: String,
    pub decision: String,
    pub reason: Option<String>,
    pub input_id: Option<String>,
    pub input_revision: Option<i64>,
    pub job_id: Option<String>,
    /// Decoded audio charged to this task's lifetime allowance.
    pub audio_us: u64,
    pub created_ms: i64,
    /// Live canonical job state, not part of the immutable receipt.
    pub job_state: Option<String>,
    /// Every authority now holding the same canonical job, including this task.
    pub sharing: Option<JobSharing>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProcessingView {
    pub id: String,
    pub request_id: String,
    pub template: String,
    pub paid_allowance_usd: String,
    pub spec: TaskProcessingSpec,
    pub recognition_profile_sha256: String,
    pub translation_profile_sha256: Option<String>,
    pub grant_sha256: String,
    pub collection_sha256: String,
    pub scope_sha256: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub generation: u32,
    pub cancelled: bool,
    pub scope_current: bool,
    pub hold_reason: Option<String>,
    pub charged_audio_us: u64,
    pub steps: Vec<TaskProcessingStep>,
}

/// The task's recognition receipt for one collected recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecognitionFacts {
    pub queued: bool,
    pub analysis_id: Option<String>,
    pub job_state: Option<String>,
    /// Revision and outcome published by this exact job, if any.
    pub transcript: Option<(i64, String)>,
}

/// One admitted collection entry and its exact recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureFacts {
    pub ordinal: u32,
    pub recording_id: String,
    pub capture_state: String,
    pub retained: bool,
    pub decoded_us: Option<u64>,
    pub recognition: Option<RecognitionFacts>,
    pub translation_recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessingFacts {
    pub task_id: String,
    /// Why no fresh step may be admitted now: cancelled, scope-changed or clock.
    pub hold: Option<&'static str>,
    pub remaining_audio_us: u64,
    pub recognition_profile: String,
    pub translation_profile: Option<String>,
    pub captures: Vec<CaptureFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessingStep {
    Recognize {
        ordinal: u32,
        recording_id: String,
        audio_us: u64,
    },
    SkipRecognition {
        ordinal: u32,
        recording_id: String,
        reason: String,
    },
    Translate {
        ordinal: u32,
        recording_id: String,
        analysis_id: String,
        transcript_revision: i64,
    },
    SkipTranslation {
        ordinal: u32,
        recording_id: String,
        reason: String,
    },
}

const TERMINAL_JOBS: [&str; 4] = ["succeeded", "failed", "cancelled", "interrupted"];
const TERMINAL_CAPTURES: [&str; 3] = ["interrupted", "cancelled", "failed"];

/// Plan the next steps in collection order. Level-triggered: a repeated pass or restart
/// rereads durable receipts, so it cannot duplicate an admission.
#[must_use]
pub fn plan(facts: &ProcessingFacts) -> Vec<ProcessingStep> {
    let mut steps = Vec::new();
    if facts.hold.is_some() {
        return steps;
    }
    let mut remaining = facts.remaining_audio_us;
    for capture in &facts.captures {
        if steps.len() == STEPS_PER_PASS {
            break;
        }
        match &capture.recognition {
            None => {
                if let Some(step) = recognition_step(capture, &mut remaining) {
                    steps.push(step);
                }
            }
            Some(recognition) if !capture.translation_recorded && recognition.queued => {
                if let Some(step) = translation_step(facts, capture, recognition) {
                    steps.push(step);
                }
            }
            Some(_) => {}
        }
    }
    steps
}

fn recognition_step(capture: &CaptureFacts, remaining: &mut u64) -> Option<ProcessingStep> {
    let skip = |reason: String| {
        Some(ProcessingStep::SkipRecognition {
            ordinal: capture.ordinal,
            recording_id: capture.recording_id.clone(),
            reason,
        })
    };
    if TERMINAL_CAPTURES.contains(&capture.capture_state.as_str()) {
        return skip(format!("recording-{}", capture.capture_state));
    }
    if capture.capture_state != "completed" {
        return None;
    }
    if !capture.retained {
        return skip("recording-not-retained".into());
    }
    let audio_us = match capture.decoded_us {
        Some(value) if value > 0 => value,
        _ => return skip("no-decoded-audio".into()),
    };
    if audio_us > *remaining {
        // The lifetime allowance never refills, so this recording can never fit.
        return skip("task-audio-allowance".into());
    }
    *remaining -= audio_us;
    Some(ProcessingStep::Recognize {
        ordinal: capture.ordinal,
        recording_id: capture.recording_id.clone(),
        audio_us,
    })
}

fn translation_step(
    facts: &ProcessingFacts,
    capture: &CaptureFacts,
    recognition: &RecognitionFacts,
) -> Option<ProcessingStep> {
    let state = recognition.job_state.as_deref()?;
    if !TERMINAL_JOBS.contains(&state) {
        return None;
    }
    let skip = |reason: String| {
        Some(ProcessingStep::SkipTranslation {
            ordinal: capture.ordinal,
            recording_id: capture.recording_id.clone(),
            reason,
        })
    };
    if state != "succeeded" {
        return skip(format!("recognition-{state}"));
    }
    match (&recognition.transcript, &facts.translation_profile) {
        (None, _) => skip("no-transcript".into()),
        (Some((_, outcome)), _) if outcome != "text" => skip("no-text".into()),
        (Some(_), None) => skip("no-translation-profile".into()),
        (Some((revision, _)), Some(_)) => Some(ProcessingStep::Translate {
            ordinal: capture.ordinal,
            recording_id: capture.recording_id.clone(),
            analysis_id: recognition.analysis_id.clone()?,
            transcript_revision: *revision,
        }),
    }
}

#[cfg(test)]
mod tests;
