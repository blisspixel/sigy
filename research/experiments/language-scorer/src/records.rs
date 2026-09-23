use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::metrics::{NORMALIZATION_ID, normalize};
use crate::selection::{MANIFEST_SHA256, Partition, Selection, is_sha256, sha256};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct References {
    pub schema_version: u32,
    pub manifest_sha256: String,
    pub partition: Partition,
    pub records: Vec<Reference>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub clip_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidates {
    pub schema_version: u32,
    pub manifest_sha256: String,
    pub partition: Partition,
    pub normalization_id: String,
    pub profile_sha256: String,
    // An explicit claim, not proof of the actual training/tuning data used.
    pub declared_tuning_partition: Partition,
    pub records: Vec<Candidate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub clip_id: String,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Recognized { text: String, language: Language },
    Abstained { reason: String },
    Failed { reason: String },
    Unsupported { reason: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Language {
    Known { label: String },
    Mixed { labels: Vec<String> },
    Unknown {},
}

impl Outcome {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Recognized { text, .. } => Some(text),
            Self::Abstained { .. } | Self::Failed { .. } | Self::Unsupported { .. } => None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Recognized { text, language } => {
                normalize(text)?;
                language.validate()
            }
            Self::Abstained { reason } | Self::Failed { reason } | Self::Unsupported { reason } => {
                bounded_label(reason, 512, "outcome reason")
            }
        }
    }
}

impl Language {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Known { label } => bounded_label(label, 128, "provider language label"),
            Self::Mixed { labels } => {
                if !(2..=8).contains(&labels.len()) {
                    return Err(
                        "mixed language evidence needs 2 to 8 distinct provider labels".into(),
                    );
                }
                let mut distinct = BTreeSet::new();
                for label in labels {
                    bounded_label(label, 128, "provider language label")?;
                    if !distinct.insert(label) {
                        return Err("duplicate mixed language label".into());
                    }
                }
                Ok(())
            }
            Self::Unknown {} => Ok(()),
        }
    }
}

fn bounded_label(value: &str, limit: usize, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(format!("invalid or over-limit {field}"));
    }
    Ok(())
}

pub struct Validated {
    pub references: BTreeMap<String, Reference>,
    pub candidates: BTreeMap<String, Candidate>,
    pub profile_sha256: String,
}

pub fn validate(
    selection: &Selection,
    partition: Partition,
    references: References,
    candidates: Candidates,
) -> Result<Validated, String> {
    validate_header(
        references.schema_version,
        &references.manifest_sha256,
        references.partition,
        partition,
    )?;
    validate_header(
        candidates.schema_version,
        &candidates.manifest_sha256,
        candidates.partition,
        partition,
    )?;
    if candidates.normalization_id != NORMALIZATION_ID
        || !is_sha256(&candidates.profile_sha256)
        || candidates.declared_tuning_partition != Partition::Calibration
    {
        return Err(
            "candidate profile, normalization or tuning-partition declaration is invalid".into(),
        );
    }
    let required = partition.groups().len() * 8;
    if references.records.len() != required || candidates.records.len() != required {
        return Err(format!(
            "partition requires exactly {required} reference and candidate records; represent abstentions explicitly"
        ));
    }
    let profile_sha256 = candidates.profile_sha256;
    let references = validate_references(selection, partition, references.records)?;
    let candidates = validate_candidates(selection, partition, candidates.records)?;
    if !references.keys().eq(candidates.keys()) {
        return Err("reference/candidate clip sets differ".into());
    }
    Ok(Validated {
        references,
        candidates,
        profile_sha256,
    })
}

fn validate_header(
    schema: u32,
    digest: &str,
    actual: Partition,
    expected: Partition,
) -> Result<(), String> {
    if schema != 1 || digest != MANIFEST_SHA256 || actual != expected {
        return Err("record schema, manifest identity or partition mismatch".into());
    }
    Ok(())
}

fn validate_references(
    selection: &Selection,
    partition: Partition,
    records: Vec<Reference>,
) -> Result<BTreeMap<String, Reference>, String> {
    let mut validated = BTreeMap::new();
    for reference in records {
        let asset = selection
            .by_id
            .get(&reference.clip_id)
            .ok_or("reference has unknown clip ID")?;
        if asset.partition != partition {
            return Err("reference crosses calibration/holdout partition".into());
        }
        if sha256(reference.text.as_bytes()) != asset.raw_transcription_sha256 {
            return Err(format!("reference hash mismatch for {}", reference.clip_id));
        }
        normalize(&reference.text)?;
        if validated
            .insert(reference.clip_id.clone(), reference)
            .is_some()
        {
            return Err("duplicate reference clip ID".into());
        }
    }
    Ok(validated)
}

fn validate_candidates(
    selection: &Selection,
    partition: Partition,
    records: Vec<Candidate>,
) -> Result<BTreeMap<String, Candidate>, String> {
    let mut validated = BTreeMap::new();
    for candidate in records {
        let asset = selection
            .by_id
            .get(&candidate.clip_id)
            .ok_or("candidate has unknown clip ID")?;
        if asset.partition != partition {
            return Err("candidate crosses calibration/holdout partition".into());
        }
        candidate.outcome.validate()?;
        if validated
            .insert(candidate.clip_id.clone(), candidate)
            .is_some()
        {
            return Err("duplicate candidate clip ID".into());
        }
    }
    Ok(validated)
}
