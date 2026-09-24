//! Topic monitors: user-authored versions and an append-only action log.
//!
//! Only the user creates a version. Anything else, including a rule, a schedule or model
//! output, can only propose an action. A proposal is checked against the current version
//! and is applied only when that version already permits it; otherwise it is refused and
//! kept with its reason. No action can raise a cap, add a source outside the version, or
//! enable paid processing. See `docs/design/topic-monitoring.md`.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, storage::validate_key};

pub const MAX_TERMS: usize = 64;
pub const MAX_SOURCES: usize = 32;
pub const MAX_GOAL_CHARS: usize = 2_000;
pub const MAX_TERM_CHARS: usize = 200;
pub const MAX_DAILY_AUDIO_SECONDS: u32 = 86_400;
pub const MAX_TOTAL_AUDIO_SECONDS: u64 = 366 * 86_400;

/// One literal term in one language, as the user wrote it, in any script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorTerm {
    /// A primary language subtag such as `ar`, or `und` for any language.
    pub language: String,
    pub text: String,
}

/// The user's bounds for one monitor version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorSpec {
    pub name: String,
    pub goal: String,
    pub terms: Vec<MonitorTerm>,
    /// Source revisions the monitor follows now.
    pub sources: Vec<String>,
    /// Source revisions the user approved in advance for the monitor to add later.
    #[serde(default)]
    pub candidate_sources: Vec<String>,
    #[serde(default)]
    pub schedules: Vec<String>,
    pub daily_audio_seconds: u32,
    pub total_audio_seconds: u64,
    #[serde(default)]
    pub recognition_profile: Option<String>,
    #[serde(default)]
    pub translation_profile: Option<String>,
}

fn text_ok(value: &str, limit: usize) -> bool {
    let count = value.chars().count();
    count > 0 && count <= limit && !value.chars().any(char::is_control) && value.trim() == value
}

fn language_ok(value: &str) -> bool {
    (2..=3).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_lowercase())
}

fn unique(values: &[String]) -> bool {
    let mut sorted: Vec<&String> = values.iter().collect();
    sorted.sort();
    sorted.windows(2).all(|pair| pair[0] != pair[1])
}

impl MonitorSpec {
    /// Check bounds that do not need the catalog. References are checked by storage.
    /// # Errors
    /// Refuses empty or oversized text, invalid languages, duplicate or invalid references,
    /// and caps outside their bounds.
    pub fn validate(&self) -> Result<()> {
        let invalid = Error::InvalidInput("monitor specification");
        if !text_ok(&self.name, 120)
            || !text_ok(&self.goal, MAX_GOAL_CHARS)
            || self.terms.is_empty()
            || self.terms.len() > MAX_TERMS
            || self.sources.is_empty()
            || self.sources.len() > MAX_SOURCES
            || self.candidate_sources.len() > MAX_SOURCES
            || self.schedules.len() > MAX_SOURCES
            || !(1..=MAX_DAILY_AUDIO_SECONDS).contains(&self.daily_audio_seconds)
            || !(1..=MAX_TOTAL_AUDIO_SECONDS).contains(&self.total_audio_seconds)
            || u64::from(self.daily_audio_seconds) > self.total_audio_seconds
            || !unique(&self.sources)
            || !unique(&self.candidate_sources)
            || !unique(&self.schedules)
            || self
                .candidate_sources
                .iter()
                .any(|candidate| self.sources.contains(candidate))
        {
            return Err(invalid);
        }
        for term in &self.terms {
            if !(language_ok(&term.language)) || !text_ok(&term.text, MAX_TERM_CHARS) {
                return Err(Error::InvalidInput("monitor term"));
            }
        }
        for reference in self
            .sources
            .iter()
            .chain(&self.candidate_sources)
            .chain(&self.schedules)
        {
            validate_key(reference, "monitor reference")?;
        }
        for profile in self
            .recognition_profile
            .iter()
            .chain(&self.translation_profile)
        {
            validate_key(profile, "monitor profile")?;
        }
        Ok(())
    }

    /// The version identity: a hash of the canonical specification.
    /// # Errors
    /// Fails only if serialization fails.
    pub fn digest(&self) -> Result<String> {
        Ok(crate::recognition::sha256_hex(&serde_json::to_vec(&(
            "sigy-monitor-spec-v1",
            self,
        ))?))
    }
}

/// Who asked for an action. Only `user` may pause or resume without a policy check; no
/// origin can change a version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOrigin {
    User,
    Schedule,
    Rule,
    Model,
}

impl ActionOrigin {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Schedule => "schedule",
            Self::Rule => "rule",
            Self::Model => "model",
        }
    }
}

/// A proposed change. Changing caps, terms or profiles, or enabling paid processing, is not a
/// proposal kind: those need a new user version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Proposal {
    Pause,
    Resume,
    AddSource {
        source: String,
    },
    RemoveSource {
        source: String,
    },
    /// Anything else a proposer asked for, kept verbatim (bounded) so the refusal is visible.
    Other {
        request: String,
    },
}

impl Proposal {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::AddSource { .. } => "add_source",
            Self::RemoveSource { .. } => "remove_source",
            Self::Other { .. } => "other",
        }
    }
}

/// The policy decision for one proposal against one version. Pure and deterministic.
#[must_use]
pub fn decide(
    spec: &MonitorSpec,
    active_sources: &[String],
    proposal: &Proposal,
) -> (bool, &'static str) {
    match proposal {
        Proposal::Pause | Proposal::Resume => (true, "within-policy"),
        Proposal::AddSource { source } if active_sources.contains(source) => {
            (false, "already-followed")
        }
        Proposal::AddSource { source }
            if spec.sources.contains(source) || spec.candidate_sources.contains(source) =>
        {
            if active_sources.len() >= MAX_SOURCES {
                (false, "source-limit")
            } else {
                (true, "approved-candidate")
            }
        }
        Proposal::AddSource { .. } => (false, "outside-approved-sources"),
        Proposal::RemoveSource { source } if active_sources.contains(source) => {
            if active_sources.len() == 1 {
                (false, "last-source")
            } else {
                (true, "within-policy")
            }
        }
        Proposal::RemoveSource { .. } => (false, "not-followed"),
        Proposal::Other { .. } => (false, "requires-user-version"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorVersion {
    pub monitor_id: String,
    pub version: u32,
    pub spec: MonitorSpec,
    pub spec_sha256: String,
    pub created_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorAction {
    pub monitor_id: String,
    pub ordinal: u32,
    /// The version this proposal was checked against.
    pub policy_version: u32,
    pub origin: ActionOrigin,
    pub proposal: Proposal,
    /// `applied` or `refused`.
    pub decision: String,
    pub reason: String,
    /// Always zero: no monitor action reserves or spends money in this release.
    pub amount_usd: String,
    pub created_ms: i64,
}

/// Current state derived from the latest version and applied actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorView {
    pub id: String,
    pub version: MonitorVersion,
    pub paused: bool,
    pub active_sources: Vec<String>,
    pub actions: u32,
}

#[cfg(test)]
mod tests;
