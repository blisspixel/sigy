use std::collections::BTreeMap;

use serde::Serialize;

use crate::judge_records::{
    CATEGORIES, CalibrationSet, Category, Criteria, JudgmentOutcome, Judgments,
};
use crate::selection::{CONFIGS, MANIFEST_SHA256, Selection};

#[derive(Default, Serialize)]
pub struct Counts {
    pub controls: u32,
    pub expected_critical: u32,
    pub expected_acceptable: u32,
    pub true_positive: u32,
    pub false_negative: u32,
    pub false_positive: u32,
    pub true_negative: u32,
    pub abstained_critical: u32,
    pub abstained_acceptable: u32,
}

impl Counts {
    fn add(&mut self, critical: bool, outcome: &JudgmentOutcome) {
        self.controls += 1;
        self.expected_critical += u32::from(critical);
        self.expected_acceptable += u32::from(!critical);
        match (critical, outcome) {
            (true, JudgmentOutcome::Critical { .. }) => self.true_positive += 1,
            (false, JudgmentOutcome::Critical { .. }) => self.false_positive += 1,
            (true, JudgmentOutcome::Acceptable {}) => self.false_negative += 1,
            (false, JudgmentOutcome::Acceptable {}) => self.true_negative += 1,
            (true, JudgmentOutcome::Abstained { .. }) => self.abstained_critical += 1,
            (false, JudgmentOutcome::Abstained { .. }) => self.abstained_acceptable += 1,
        }
    }
}

#[derive(Default, Serialize)]
pub struct LanguageReport {
    pub counts: Counts,
    pub by_category: BTreeMap<Category, Counts>,
    pub checks: BTreeMap<&'static str, bool>,
    pub declared_control_criteria_pass: bool,
}

impl LanguageReport {
    fn finish(&mut self, criteria: &Criteria) {
        let category_coverage = CATEGORIES.iter().all(|category| {
            self.by_category
                .get(category)
                .is_some_and(|counts| counts.controls >= criteria.minimum_per_category)
        });
        let sensitivity = criteria
            .minimum_sensitivity
            .at_least(self.counts.true_positive, self.counts.expected_critical);
        let false_positive_rate = criteria
            .maximum_false_positive_rate
            .at_most(self.counts.false_positive, self.counts.expected_acceptable);
        let abstention_rate = criteria.maximum_abstention_rate.at_most(
            self.counts.abstained_critical + self.counts.abstained_acceptable,
            self.counts.controls,
        );
        self.checks = BTreeMap::from([
            ("category_coverage", category_coverage),
            ("sensitivity", sensitivity),
            ("false_positive_rate", false_positive_rate),
            ("abstention_rate", abstention_rate),
        ]);
        self.declared_control_criteria_pass = self.checks.values().all(|passes| *passes);
    }
}

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub manifest_sha256: &'static str,
    pub control_artifact_sha256: String,
    pub judgment_artifact_sha256: String,
    pub scorer_source_bundle_sha256: String,
    pub rubric_sha256: String,
    pub declared_judge_profile_sha256: String,
    pub declared_judge_family: String,
    pub criteria: Criteria,
    pub interpretation: &'static str,
    pub by_language: BTreeMap<String, LanguageReport>,
}

/// Validate or score bounded offline artifacts.
///
/// # Errors
/// Rejects invalid identities, references, partitions, text bounds or evidence.
pub fn score(
    selection: &Selection,
    controls: &CalibrationSet,
    judgments: &Judgments,
    control_digest: &str,
    judgment_digest: &str,
) -> Result<Report, String> {
    crate::judge_records::validate(selection, controls)?;
    crate::judge_records::validate_judgments(controls, control_digest, judgments)?;
    let mut by_language: BTreeMap<_, _> = CONFIGS
        .iter()
        .filter(|config| **config != "en_us")
        .map(|config| ((*config).to_owned(), LanguageReport::default()))
        .collect();
    for control in &controls.controls {
        let asset = selection
            .by_id
            .get(&control.clip_id)
            .ok_or("missing judge calibration asset")?;
        let judgment = judgments
            .records
            .iter()
            .find(|judgment| judgment.control_id == control.control_id)
            .ok_or("missing validated judgment")?;
        let language = by_language
            .get_mut(&asset.config)
            .ok_or("missing judge language")?;
        language
            .counts
            .add(control.category.expected_critical(), &judgment.outcome);
        language
            .by_category
            .entry(control.category)
            .or_default()
            .add(control.category.expected_critical(), &judgment.outcome);
    }
    for language in by_language.values_mut() {
        language.finish(&controls.criteria);
    }
    Ok(Report {
        schema_version: 1,
        manifest_sha256: MANIFEST_SHA256,
        control_artifact_sha256: control_digest.into(),
        judgment_artifact_sha256: judgment_digest.into(),
        scorer_source_bundle_sha256: crate::scoring::source_bundle_digest(),
        rubric_sha256: controls.rubric_sha256.clone(),
        declared_judge_profile_sha256: judgments.judge_profile_sha256.clone(),
        declared_judge_family: judgments.judge_family.clone(),
        criteria: controls.criteria.clone(),
        interpretation: "Offline scoring of declared calibration controls only; no judge is executed and no language is qualified. Control labels and critical categories are experiment assertions: matching a deliberate string mutation does not prove its semantic severity. Quoted UTF-8 ranges are checked for literal membership, not valid reasoning. Sensitivity includes critical abstentions in its denominator. False-positive rate includes all acceptable controls; abstentions are separately limited across every control. Every non-English screening language is reported; absent languages fail coverage. Criteria are pinned with the control artifact, but this cannot prove they were frozen before results were seen. Blinded inputs hide labels and categories, not reference text or prior model exposure. Passing controls cannot establish natural broadcast or held-out translation quality. Two independent families, order variation, per-language sensitivity, honest provenance, and separate holdout remain external gates.",
        by_language,
    })
}

#[derive(Serialize)]
pub struct BlindedInput<'a> {
    pub control_id: &'a str,
    pub source_text: &'a str,
    pub english_reference: &'a str,
    pub output_text: &'a str,
    pub untrusted_context: &'a str,
}

#[derive(Serialize)]
pub struct BlindedInputs<'a> {
    pub schema_version: u32,
    pub control_artifact_sha256: &'a str,
    pub rubric_sha256: &'a str,
    pub records: Vec<BlindedInput<'a>>,
}

/// Validate or score bounded offline artifacts.
///
/// # Errors
/// Rejects invalid identities, references, partitions, text bounds or evidence.
pub fn inputs<'a>(
    selection: &Selection,
    controls: &'a CalibrationSet,
    digest: &'a str,
) -> Result<BlindedInputs<'a>, String> {
    crate::judge_records::validate(selection, controls)?;
    Ok(BlindedInputs {
        schema_version: 1,
        control_artifact_sha256: digest,
        rubric_sha256: &controls.rubric_sha256,
        records: controls
            .controls
            .iter()
            .map(|control| BlindedInput {
                control_id: &control.control_id,
                source_text: &control.source_text,
                english_reference: &control.english_reference,
                output_text: &control.output_text,
                untrusted_context: &control.untrusted_context,
            })
            .collect(),
    })
}
