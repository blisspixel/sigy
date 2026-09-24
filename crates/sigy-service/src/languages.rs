//! Bounded language evidence. Provider labels and hints do not establish permissions or quality.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    domain::language::{
        DetectionOutcome, EvidenceOrigin, LanguageTask, MediaRange, Observation, Resolution,
        RouteCapability,
    },
    storage::validate_key,
};

pub const MAX_EVIDENCE_BYTES: usize = 65_536;
pub const LANGUAGE_PAGE_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptReference {
    pub id: String,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageMethod {
    pub origin: String,
    pub profile: String,
    pub profile_sha256: String,
    pub resolution: String,
    pub alias_map: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageLabel {
    /// Well-formed, case-normalized BCP 47 syntax, not a registry or capability claim.
    pub tag: String,
    /// The bounded original provider value, preserved independently of its mapped tag.
    pub provider_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageRoute {
    pub task: String,
    pub capability: String,
    pub profile: String,
    pub profile_sha256: String,
    /// Declared model metadata or a measured evaluation artifact, not a universal quality claim.
    pub basis: String,
    pub basis_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageSpan {
    pub ordinal: u32,
    pub interval_ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub cue_ordinal: Option<u32>,
    pub observation: String,
    pub languages: Vec<LanguageLabel>,
    pub route: LanguageRoute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageEvidence {
    pub id: String,
    pub revision: u32,
    pub analysis_id: String,
    pub analysis_revision: i64,
    pub transcript: Option<TranscriptReference>,
    pub method: LanguageMethod,
    pub outcome: String,
    pub reason: Option<String>,
    pub spans: Vec<LanguageSpan>,
}

impl LanguageEvidence {
    /// Validate semantic dimensions and normalize tag case before publication.
    /// # Errors
    /// Rejects malformed, contradictory or oversized evidence. Storage validates lineage and timing.
    pub(crate) fn validate(mut self) -> Result<Self> {
        validate_key(&self.id, "language evidence ID")?;
        validate_key(&self.analysis_id, "analysis input ID")?;
        if !(1..=64).contains(&self.revision) || !(1..=64).contains(&self.analysis_revision) {
            return Err(Error::InvalidInput("language evidence revision"));
        }
        if let Some(transcript) = &self.transcript {
            validate_key(&transcript.id, "transcript ID")?;
            if !(1..=64).contains(&transcript.revision) {
                return Err(Error::InvalidInput("transcript revision"));
            }
        }
        self.validate_method()?;
        if let Some(reason) = &self.reason {
            validate_key(reason, "language outcome reason")?;
        }
        self.outcome
            .parse::<DetectionOutcome>()
            .and_then(|outcome| outcome.validate_spans(self.spans.len(), self.reason.is_some()))
            .map_err(|error| Error::InvalidInput(error.0))?;
        for (ordinal, span) in self.spans.iter_mut().enumerate() {
            if usize::try_from(span.ordinal).ok() != Some(ordinal) {
                return Err(Error::InvalidInput("language span order"));
            }
            span.validate()?;
            validate_span_origin(&self.method, span)?;
        }
        if serde_json::to_vec(&self)?.len() > MAX_EVIDENCE_BYTES {
            return Err(Error::InvalidInput("language evidence byte limit"));
        }
        Ok(self)
    }

    fn validate_method(&self) -> Result<()> {
        validate_key(&self.method.profile, "language profile ID")?;
        validate_key(&self.method.alias_map, "language alias mapping ID")?;
        validate_hash(&self.method.profile_sha256)?;
        let origin = self
            .method
            .origin
            .parse::<EvidenceOrigin>()
            .map_err(|error| Error::InvalidInput(error.0))?;
        let resolution = self
            .method
            .resolution
            .parse::<Resolution>()
            .map_err(|error| Error::InvalidInput(error.0))?;
        if origin == EvidenceOrigin::Text && self.transcript.is_none() {
            return Err(Error::InvalidInput(
                "text evidence requires a transcript revision",
            ));
        }
        if resolution == Resolution::Word
            && (origin != EvidenceOrigin::Recognizer || self.transcript.is_none())
        {
            return Err(Error::InvalidInput(
                "word evidence requires a timed recognizer transcript",
            ));
        }
        Ok(())
    }
}

impl LanguageSpan {
    fn validate(&mut self) -> Result<()> {
        MediaRange::new(self.start_us, self.end_us)
            .map_err(|error| Error::InvalidInput(error.0))?;
        self.observation
            .parse::<Observation>()
            .and_then(|observation| observation.validate_labels(self.languages.len()))
            .map_err(|error| Error::InvalidInput(error.0))?;
        self.route
            .task
            .parse::<LanguageTask>()
            .map_err(|error| Error::InvalidInput(error.0))?;
        self.route
            .capability
            .parse::<RouteCapability>()
            .map_err(|error| Error::InvalidInput(error.0))?;
        validate_key(&self.route.profile, "language route profile ID")?;
        validate_hash(&self.route.profile_sha256)?;
        validate_hash(&self.route.basis_sha256)?;
        if !matches!(self.route.basis.as_str(), "declared" | "measured") {
            return Err(Error::InvalidInput("language route capability basis"));
        }
        let mut labels = HashSet::new();
        for label in &mut self.languages {
            label.tag = normalize_tag(&label.tag)?;
            if label.provider_label.is_empty() || label.provider_label.len() > 128 {
                return Err(Error::InvalidInput("provider language label size"));
            }
            if !labels.insert(label.tag.clone()) {
                return Err(Error::InvalidInput("duplicate language label"));
            }
        }
        Ok(())
    }
}

fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Error::InvalidInput("language profile or evidence checksum"));
    }
    Ok(())
}

fn validate_span_origin(method: &LanguageMethod, span: &LanguageSpan) -> Result<()> {
    if method.origin == EvidenceOrigin::Acoustic.as_str() {
        for label in &span.languages {
            let tag = oxilangtag::LanguageTag::parse(label.tag.as_str())
                .map_err(|_| Error::InvalidInput("language tag syntax"))?;
            if tag.script().is_some() {
                return Err(Error::InvalidInput(
                    "acoustic evidence cannot establish written script",
                ));
            }
        }
    }
    if span.route.task == LanguageTask::Detection.as_str()
        && span.route.capability == RouteCapability::Unsupported.as_str()
        && span.route.profile == method.profile
        && span.route.profile_sha256 == method.profile_sha256
    {
        return Err(Error::InvalidInput(
            "unsupported detector cannot produce observations",
        ));
    }
    Ok(())
}

pub(crate) fn normalize_tag(value: &str) -> Result<String> {
    if value.is_empty() || value.len() > 128 {
        return Err(Error::InvalidInput("language tag size"));
    }
    let tag = oxilangtag::LanguageTag::parse_and_normalize(value)
        .map_err(|_| Error::InvalidInput("language tag syntax"))?;
    let mut variants = HashSet::new();
    let mut extensions = HashSet::new();
    if !tag
        .variant_subtags()
        .all(|variant| variants.insert(variant))
        || !tag
            .extension_subtags()
            .all(|(singleton, _)| extensions.insert(singleton))
    {
        return Err(Error::InvalidInput("duplicate language subtag"));
    }
    if matches!(tag.primary_language(), "und" | "mul" | "zxx") {
        return Err(Error::InvalidInput(
            "special language tags require an observation state",
        ));
    }
    Ok(tag.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_preserve_script_and_region_without_claiming_alias_canonicalization() -> Result<()> {
        assert_eq!(normalize_tag("FR-ca")?, "fr-CA");
        assert_eq!(normalize_tag("tlh-latn")?, "tlh-Latn");
        assert_eq!(normalize_tag("nv")?, "nv");
        assert_eq!(normalize_tag("i-klingon")?, "i-klingon");
        assert_eq!(normalize_tag("iw")?, "iw");
        for invalid in [
            "",
            "fr_CA",
            "fr--CA",
            "fr-123456789",
            "fr\n",
            "é",
            "und",
            "mul",
            "zxx",
            "x--",
            "de-1901-1901",
            "en-a-aaa-a-bbb",
        ] {
            assert!(normalize_tag(invalid).is_err(), "{invalid:?}");
        }
        assert!(normalize_tag(&"a".repeat(129)).is_err());
        Ok(())
    }
}
