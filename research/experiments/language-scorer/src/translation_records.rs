use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::metrics::normalize;
use crate::selection::{MANIFEST_SHA256, Partition, Selection, is_sha256, sha256};
use crate::translation_metric::SIGNATURE;

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
    pub source_text: String,
    pub english_text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidates {
    pub schema_version: u32,
    pub manifest_sha256: String,
    pub partition: Partition,
    pub metric_signature: String,
    pub profile_sha256: String,
    pub declared_tuning_partition: Partition,
    pub input_origin: InputOrigin,
    pub records: Vec<Candidate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputOrigin {
    ReferenceText {},
    RecognizedText {
        recognition_profile_sha256: String,
        recognition_artifact_sha256: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub clip_id: String,
    pub input_text: String,
    pub input_sha256: String,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Translated { text: String },
    Abstained { reason: String },
    Failed { reason: String },
    Unsupported { reason: String },
}

impl Outcome {
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Translated { text } => Some(text),
            Self::Abstained { .. } | Self::Failed { .. } | Self::Unsupported { .. } => None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Translated { text } => {
                normalize(text)?;
            }
            Self::Abstained { reason } | Self::Failed { reason } | Self::Unsupported { reason } => {
                if reason.trim().is_empty()
                    || reason.len() > 512
                    || reason.chars().any(char::is_control)
                {
                    return Err("invalid translation outcome reason".into());
                }
            }
        }
        Ok(())
    }
}

pub struct Validated {
    pub references: BTreeMap<String, Reference>,
    pub candidates: BTreeMap<String, Candidate>,
    pub profile_sha256: String,
    pub input_origin: InputOrigin,
}

/// Validate or score bounded offline artifacts.
///
/// # Errors
/// Rejects invalid identities, references, partitions, text bounds or evidence.
pub fn validate(
    selection: &Selection,
    partition: Partition,
    references: References,
    candidates: Candidates,
) -> Result<Validated, String> {
    for (schema, digest, declared_partition) in [
        (
            references.schema_version,
            references.manifest_sha256.as_str(),
            references.partition,
        ),
        (
            candidates.schema_version,
            candidates.manifest_sha256.as_str(),
            candidates.partition,
        ),
    ] {
        if schema != 1 || digest != MANIFEST_SHA256 || declared_partition != partition {
            return Err("translation schema, manifest or partition mismatch".into());
        }
    }
    if candidates.metric_signature != SIGNATURE
        || !is_sha256(&candidates.profile_sha256)
        || candidates.declared_tuning_partition != Partition::Calibration
    {
        return Err("translation profile, metric or tuning declaration is invalid".into());
    }
    if let InputOrigin::RecognizedText {
        recognition_profile_sha256,
        recognition_artifact_sha256,
    } = &candidates.input_origin
        && (!is_sha256(recognition_profile_sha256) || !is_sha256(recognition_artifact_sha256))
    {
        return Err("recognition identities must be lowercase SHA-256 digests".into());
    }
    let required = partition.groups().len() * 7;
    if references.records.len() != required || candidates.records.len() != required {
        return Err(format!(
            "translation partition requires {required} reference and candidate records; represent absent output explicitly"
        ));
    }
    let references = validate_references(selection, partition, references.records)?;
    let records = validate_candidates(
        selection,
        partition,
        &references,
        &candidates.input_origin,
        candidates.records,
    )?;
    if !references.keys().eq(records.keys()) {
        return Err("translation reference and candidate clip sets differ".into());
    }
    Ok(Validated {
        references,
        candidates: records,
        profile_sha256: candidates.profile_sha256,
        input_origin: candidates.input_origin,
    })
}

fn validate_references(
    selection: &Selection,
    partition: Partition,
    records: Vec<Reference>,
) -> Result<BTreeMap<String, Reference>, String> {
    let mut validated = BTreeMap::new();
    for reference in records {
        validate_reference(selection, partition, &reference)?;
        if validated
            .insert(reference.clip_id.clone(), reference)
            .is_some()
        {
            return Err("duplicate translation reference clip".into());
        }
    }
    Ok(validated)
}

/// Validate or score bounded offline artifacts.
///
/// # Errors
/// Rejects invalid identities, references, partitions, text bounds or evidence.
pub fn validate_reference(
    selection: &Selection,
    partition: Partition,
    reference: &Reference,
) -> Result<(), String> {
    let asset = selection
        .by_id
        .get(&reference.clip_id)
        .ok_or("unknown translation reference clip")?;
    if asset.partition != partition || asset.config == "en_us" {
        return Err("translation reference crosses partition or uses English as source".into());
    }
    let english = selection
        .by_id
        .values()
        .find(|entry| {
            entry.config == "en_us"
                && entry.partition == partition
                && entry.sentence_group_id == asset.sentence_group_id
        })
        .ok_or("missing frozen parallel English reference")?;
    if sha256(reference.source_text.as_bytes()) != asset.raw_transcription_sha256
        || sha256(reference.english_text.as_bytes()) != english.raw_transcription_sha256
    {
        return Err("original or parallel English reference differs from frozen hash".into());
    }
    normalize(&reference.source_text)?;
    normalize(&reference.english_text)?;
    Ok(())
}

fn validate_candidates(
    selection: &Selection,
    partition: Partition,
    references: &BTreeMap<String, Reference>,
    origin: &InputOrigin,
    records: Vec<Candidate>,
) -> Result<BTreeMap<String, Candidate>, String> {
    let mut validated = BTreeMap::new();
    for candidate in records {
        let asset = selection
            .by_id
            .get(&candidate.clip_id)
            .ok_or("unknown translation candidate clip")?;
        if asset.partition != partition || asset.config == "en_us" {
            return Err("translation candidate crosses partition or uses English as source".into());
        }
        let reference = references
            .get(&candidate.clip_id)
            .ok_or("candidate has no paired translation reference")?;
        normalize(&candidate.input_text)?;
        if sha256(candidate.input_text.as_bytes()) != candidate.input_sha256 {
            return Err("translation input text differs from declared digest".into());
        }
        if matches!(origin, InputOrigin::ReferenceText {})
            && candidate.input_text != reference.source_text
        {
            return Err("reference-text translation did not use the published source text".into());
        }
        candidate.outcome.validate()?;
        if validated
            .insert(candidate.clip_id.clone(), candidate)
            .is_some()
        {
            return Err("duplicate translation candidate clip".into());
        }
    }
    Ok(validated)
}
