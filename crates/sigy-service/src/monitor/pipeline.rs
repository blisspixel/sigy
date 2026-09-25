//! The first stage controller: which automatic steps a monitor wants next.
//!
//! This is a pure function over facts read from the catalog. It is level-triggered: it asks
//! which derivations the monitor's policy wants and are missing, so a crash, a restart or a
//! repeated pass cannot duplicate work. Job and pin IDs are derived from content, so two
//! monitors that want the same derivation share it. The caller performs the I/O through the
//! same paths a user command uses and records each step.

use serde::{Deserialize, Serialize};

use crate::recognition::sha256_hex;

/// Most steps one monitor may take in one pass.
pub const STEPS_PER_PASS: usize = 4;

/// Milliseconds in one UTC day, the unit of the daily cap.
pub const DAY_MS: i64 = 86_400_000;

/// A published recording from a followed source with no recognition step yet, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub recording_id: String,
    pub audio_us: u64,
}

/// A queued recognition step whose job has ended, with no translation step yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recognized {
    pub recording_id: String,
    pub analysis_id: String,
    /// The job's terminal state, and the transcript revision it published, if any.
    pub job_state: String,
    pub transcript: Option<(i64, String)>,
}

/// What the catalog says about one monitor right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub monitor_id: String,
    pub version: u32,
    pub paused: bool,
    pub daily_cap_us: u64,
    pub total_cap_us: u64,
    pub used_today_us: u64,
    pub used_total_us: u64,
    pub recognition_profile: Option<String>,
    pub translation_profile: Option<String>,
    pub candidates: Vec<Candidate>,
    pub recognized: Vec<Recognized>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Pin the recording and queue recognition, charging its audio to today's cap.
    Recognize {
        recording_id: String,
        audio_us: u64,
        profile: String,
    },
    /// A recording that can never fit the caps, recorded once so it is not retried.
    SkipRecognition {
        recording_id: String,
        reason: &'static str,
    },
    Translate {
        recording_id: String,
        analysis_id: String,
        transcript_revision: i64,
        profile: String,
    },
    SkipTranslation {
        recording_id: String,
        reason: String,
    },
}

/// Why no more recognition was queued in this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    None,
    Paused,
    NoRecognitionProfile,
    DailyCap,
    TotalCap,
}

/// Plan the next steps. Recognition is strictly oldest first: when the oldest waiting
/// recording does not fit today's remaining cap, nothing newer jumps ahead of it.
#[must_use]
pub fn plan(facts: &Facts) -> (Vec<Step>, Hold) {
    let mut steps = Vec::new();
    if facts.paused {
        return (steps, Hold::Paused);
    }
    for item in &facts.recognized {
        if steps.len() == STEPS_PER_PASS {
            break;
        }
        steps.push(translation_step(facts, item));
    }
    let Some(profile) = &facts.recognition_profile else {
        return (steps, Hold::NoRecognitionProfile);
    };
    let mut today = facts.used_today_us;
    let mut total = facts.used_total_us;
    for candidate in &facts.candidates {
        if steps.len() == STEPS_PER_PASS {
            return (steps, Hold::None);
        }
        if candidate.audio_us > facts.daily_cap_us {
            steps.push(Step::SkipRecognition {
                recording_id: candidate.recording_id.clone(),
                reason: "longer-than-daily-cap",
            });
            continue;
        }
        if total.saturating_add(candidate.audio_us) > facts.total_cap_us {
            return (steps, Hold::TotalCap);
        }
        if today.saturating_add(candidate.audio_us) > facts.daily_cap_us {
            return (steps, Hold::DailyCap);
        }
        today += candidate.audio_us;
        total += candidate.audio_us;
        steps.push(Step::Recognize {
            recording_id: candidate.recording_id.clone(),
            audio_us: candidate.audio_us,
            profile: profile.clone(),
        });
    }
    (steps, Hold::None)
}

fn translation_step(facts: &Facts, item: &Recognized) -> Step {
    let skip = |reason: String| Step::SkipTranslation {
        recording_id: item.recording_id.clone(),
        reason,
    };
    match (&item.transcript, facts.translation_profile.as_ref()) {
        _ if item.job_state != "succeeded" => skip(format!("recognition-{}", item.job_state)),
        (Some((_, outcome)), _) if outcome == "no_text" => skip("no-text".into()),
        (None, _) => skip("no-transcript".into()),
        (Some(_), None) => skip("no-translation-profile".into()),
        (Some((revision, _)), Some(profile)) => Step::Translate {
            recording_id: item.recording_id.clone(),
            analysis_id: item.analysis_id.clone(),
            transcript_revision: *revision,
            profile: profile.clone(),
        },
    }
}

fn short(label: &str, parts: &[&str]) -> String {
    let joined = parts.join("\n");
    let digest = sha256_hex(format!("sigy-monitor-{label}-v1\n{joined}").as_bytes());
    format!("mon-{label}-{}", &digest[..24])
}

/// One pin per recording, shared by every monitor that follows its source.
#[must_use]
pub fn pin_id(recording_id: &str) -> String {
    short("pin", &[recording_id])
}

/// One recognition job per recording and profile, shared across monitors.
#[must_use]
pub fn recognition_job_id(recording_id: &str, profile_sha256: &str) -> String {
    short("asr", &[recording_id, profile_sha256])
}

/// One translation job per transcript revision and profile, shared across monitors.
#[must_use]
pub fn translation_job_id(
    analysis_id: &str,
    transcript_revision: i64,
    profile_sha256: &str,
) -> String {
    short(
        "mt",
        &[
            analysis_id,
            &transcript_revision.to_string(),
            profile_sha256,
        ],
    )
}

#[cfg(test)]
mod tests;
