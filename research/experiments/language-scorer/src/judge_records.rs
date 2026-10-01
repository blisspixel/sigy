use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::metrics::normalize;
use crate::selection::{MANIFEST_SHA256, Partition, Selection, is_sha256};
use crate::translation_records::Reference;

pub const MAX_CONTROLS: usize = 448;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationSet {
    pub schema_version: u32,
    pub manifest_sha256: String,
    pub partition: Partition,
    pub rubric_sha256: String,
    pub criteria: Criteria,
    pub controls: Vec<CalibrationControl>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Criteria {
    pub minimum_per_category: u32,
    pub minimum_sensitivity: Fraction,
    pub maximum_false_positive_rate: Fraction,
    pub maximum_abstention_rate: Fraction,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fraction {
    pub numerator: u32,
    pub denominator: u32,
}

impl Fraction {
    fn validate(self) -> Result<(), String> {
        if self.denominator == 0 || self.denominator > 1000 || self.numerator > self.denominator {
            return Err("judge criteria must use fractions between zero and one with denominators at most 1000".into());
        }
        Ok(())
    }

    #[must_use]
    pub fn at_least(self, numerator: u32, denominator: u32) -> bool {
        denominator > 0
            && u64::from(numerator) * u64::from(self.denominator)
                >= u64::from(self.numerator) * u64::from(denominator)
    }

    #[must_use]
    pub fn at_most(self, numerator: u32, denominator: u32) -> bool {
        denominator > 0
            && u64::from(numerator) * u64::from(self.denominator)
                <= u64::from(self.numerator) * u64::from(denominator)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Unchanged,
    MeaningPreservingFormatting,
    MeaningPreservingParaphrase,
    Entity,
    Quantity,
    Negation,
    Omission,
    FluentUnrelated,
    InstructionInjection,
}

pub const CATEGORIES: [Category; 9] = [
    Category::Unchanged,
    Category::MeaningPreservingFormatting,
    Category::MeaningPreservingParaphrase,
    Category::Entity,
    Category::Quantity,
    Category::Negation,
    Category::Omission,
    Category::FluentUnrelated,
    Category::InstructionInjection,
];

impl Category {
    #[must_use]
    pub const fn expected_critical(self) -> bool {
        matches!(
            self,
            Self::Entity | Self::Quantity | Self::Negation | Self::Omission | Self::FluentUnrelated
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationControl {
    pub control_id: String,
    pub clip_id: String,
    pub category: Category,
    pub source_text: String,
    pub english_reference: String,
    pub output_text: String,
    pub untrusted_context: String,
    pub label_basis: LabelBasis,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LabelBasis {
    Unchanged {},
    FormattingOnly {},
    DeliberateMutation {
        reference_range: Quote,
        replacement_text: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Quote {
    pub start_utf8_byte: u32,
    pub end_utf8_byte: u32,
    pub text: String,
}

impl Quote {
    /// Validate or score bounded offline artifacts.
    ///
    /// # Errors
    /// Rejects invalid identities, references, partitions, text bounds or evidence.
    pub fn validate(&self, text: &str) -> Result<(), String> {
        let start = usize::try_from(self.start_utf8_byte).map_err(|_| "quote start overflow")?;
        let end = usize::try_from(self.end_utf8_byte).map_err(|_| "quote end overflow")?;
        if start >= end || text.get(start..end) != Some(self.text.as_str()) {
            return Err("judge quote does not match a nonempty UTF-8 byte range".into());
        }
        Ok(())
    }
}

/// Validate or score bounded offline artifacts.
///
/// # Errors
/// Rejects invalid identities, references, partitions, text bounds or evidence.
pub fn validate(selection: &Selection, controls: &CalibrationSet) -> Result<(), String> {
    if controls.schema_version != 1
        || controls.manifest_sha256 != MANIFEST_SHA256
        || controls.partition != Partition::Calibration
        || !is_sha256(&controls.rubric_sha256)
    {
        return Err(
            "judge calibration requires the frozen calibration partition and a rubric digest"
                .into(),
        );
    }
    if controls.controls.is_empty()
        || controls.controls.len() > MAX_CONTROLS
        || !(1..=64).contains(&controls.criteria.minimum_per_category)
    {
        return Err("judge control count or minimum category coverage is invalid".into());
    }
    controls.criteria.minimum_sensitivity.validate()?;
    controls.criteria.maximum_false_positive_rate.validate()?;
    controls.criteria.maximum_abstention_rate.validate()?;
    let mut ids = BTreeSet::new();
    for control in &controls.controls {
        if !is_sha256(&control.control_id) || !ids.insert(&control.control_id) {
            return Err("judge controls need unique opaque SHA-256 IDs".into());
        }
        validate_control(selection, control)?;
    }
    Ok(())
}

fn validate_control(selection: &Selection, control: &CalibrationControl) -> Result<(), String> {
    // Reuse the paired reference hash contract. A complete duplicated temporary
    // partition is unnecessary: validate the one original/English pair directly.
    let reference = Reference {
        clip_id: control.clip_id.clone(),
        source_text: control.source_text.clone(),
        english_text: control.english_reference.clone(),
    };
    crate::translation_records::validate_reference(selection, Partition::Calibration, &reference)?;
    normalize(&control.output_text)?;
    normalize(&control.untrusted_context)?;
    let valid_basis = match (&control.category, &control.label_basis) {
        (Category::Unchanged | Category::InstructionInjection, LabelBasis::Unchanged {}) => {
            control.output_text == control.english_reference
        }
        (Category::MeaningPreservingFormatting, LabelBasis::FormattingOnly {}) => {
            control.output_text != control.english_reference
                && normalize(&control.output_text)? == normalize(&control.english_reference)?
        }
        (
            category,
            LabelBasis::DeliberateMutation {
                reference_range,
                replacement_text,
            },
        ) if category.expected_critical() || *category == Category::MeaningPreservingParaphrase => {
            reference_range.validate(&control.english_reference)?;
            normalize(replacement_text)?;
            let start = usize::try_from(reference_range.start_utf8_byte)
                .map_err(|_| "mutation start overflow")?;
            let end = usize::try_from(reference_range.end_utf8_byte)
                .map_err(|_| "mutation end overflow")?;
            let mut mutated = control.english_reference.clone();
            mutated.replace_range(start..end, replacement_text);
            mutated == control.output_text && mutated != control.english_reference
        }
        _ => false,
    };
    if !valid_basis
        || (control.category == Category::InstructionInjection
            && control.untrusted_context.is_empty())
    {
        return Err(
            "judge control output does not match its declared mutation or acceptable-control basis"
                .into(),
        );
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Judgments {
    pub schema_version: u32,
    pub control_artifact_sha256: String,
    pub rubric_sha256: String,
    pub judge_profile_sha256: String,
    pub judge_family: String,
    pub records: Vec<Judgment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Judgment {
    pub control_id: String,
    pub outcome: JudgmentOutcome,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum JudgmentOutcome {
    Critical {
        source_quote: Quote,
        reference_quote: Quote,
        output_quote: Option<Quote>,
        reason: String,
    },
    Acceptable {},
    Abstained {
        reason: String,
    },
}

/// Validate or score bounded offline artifacts.
///
/// # Errors
/// Rejects invalid identities, references, partitions, text bounds or evidence.
pub fn validate_judgments(
    controls: &CalibrationSet,
    digest: &str,
    judgments: &Judgments,
) -> Result<(), String> {
    if judgments.schema_version != 1
        || judgments.control_artifact_sha256 != digest
        || judgments.rubric_sha256 != controls.rubric_sha256
        || !is_sha256(&judgments.judge_profile_sha256)
        || judgments.judge_family.trim().is_empty()
        || judgments.judge_family.len() > 128
        || judgments.judge_family.chars().any(char::is_control)
        || judgments.records.len() != controls.controls.len()
    {
        return Err(
            "judge result identities, family or complete control coverage are invalid".into(),
        );
    }
    let mut seen = BTreeSet::new();
    for judgment in &judgments.records {
        if !seen.insert(&judgment.control_id) {
            return Err("duplicate judge result control ID".into());
        }
        let control = controls
            .controls
            .iter()
            .find(|control| control.control_id == judgment.control_id)
            .ok_or("unknown judge result control ID")?;
        match &judgment.outcome {
            JudgmentOutcome::Critical {
                source_quote,
                reference_quote,
                output_quote,
                reason,
            } => {
                source_quote.validate(&control.source_text)?;
                reference_quote.validate(&control.english_reference)?;
                if let Some(quote) = output_quote {
                    quote.validate(&control.output_text)?;
                }
                validate_reason(reason)?;
            }
            JudgmentOutcome::Abstained { reason } => validate_reason(reason)?,
            JudgmentOutcome::Acceptable {} => {}
        }
    }
    Ok(())
}

fn validate_reason(reason: &str) -> Result<(), String> {
    if reason.trim().is_empty() || reason.len() > 512 || reason.chars().any(char::is_control) {
        return Err("judge reason is empty, over limit or contains controls".into());
    }
    Ok(())
}
