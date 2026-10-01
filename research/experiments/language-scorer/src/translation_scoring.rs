use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::selection::{DATASET_REVISION, MANIFEST_SHA256, Partition, Selection, sha256};
use crate::translation_metric::{SIGNATURE, Statistics, statistics};
use crate::translation_records::{InputOrigin, Outcome, Validated};

#[derive(Default, Serialize)]
pub struct Coverage {
    pub clips: u32,
    pub translated: u32,
    pub translated_empty: u32,
    pub abstained: u32,
    pub failed: u32,
    pub unsupported: u32,
}

#[derive(Default, Serialize)]
pub struct Aggregate {
    pub coverage: Coverage,
    pub all_clips_missing_output_as_empty: Statistics,
    pub translated_only: Statistics,
    pub all_clips_chrfpp: f64,
    pub translated_only_chrfpp: Option<f64>,
}

impl Aggregate {
    fn add(&mut self, outcome: &Outcome, statistics: &Statistics) {
        self.coverage.clips += 1;
        self.all_clips_missing_output_as_empty.add(statistics);
        match outcome {
            Outcome::Translated { text } => {
                self.coverage.translated += 1;
                self.coverage.translated_empty += u32::from(text.is_empty());
                self.translated_only.add(statistics);
            }
            Outcome::Abstained { .. } => self.coverage.abstained += 1,
            Outcome::Failed { .. } => self.coverage.failed += 1,
            Outcome::Unsupported { .. } => self.coverage.unsupported += 1,
        }
        self.all_clips_chrfpp = self.all_clips_missing_output_as_empty.score();
        self.translated_only_chrfpp =
            (self.coverage.translated > 0).then(|| self.translated_only.score());
    }
}

#[derive(Serialize)]
pub struct TranslationClip {
    pub clip_id: String,
    pub config: String,
    pub sentence_group_id: u32,
    pub raw_source_reference: String,
    pub raw_english_reference: String,
    pub raw_input: String,
    pub input_sha256: String,
    pub outcome: Outcome,
    pub absent_output_scored_as_empty: bool,
    pub statistics: Statistics,
    pub chrfpp: f64,
}

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub metric_signature: &'static str,
    pub manifest_sha256: &'static str,
    pub dataset_revision: &'static str,
    pub partition: Partition,
    pub scorer_source_bundle_sha256: String,
    pub reference_artifact_sha256: String,
    pub candidate_artifact_sha256: String,
    pub declared_profile_sha256: String,
    pub declared_tuning_partition: Partition,
    pub input_origin: InputOrigin,
    pub distinct_english_references: usize,
    pub interpretation: &'static str,
    pub by_language: BTreeMap<String, Aggregate>,
    pub total: Aggregate,
    pub clips: Vec<TranslationClip>,
}

/// Validate or score bounded offline artifacts.
///
/// # Errors
/// Rejects invalid identities, references, partitions, text bounds or evidence.
pub fn score(
    selection: &Selection,
    partition: Partition,
    validated: Validated,
    reference_digest: String,
    candidate_digest: String,
) -> Result<Report, String> {
    let mut total = Aggregate::default();
    let mut by_language: BTreeMap<String, Aggregate> = BTreeMap::new();
    let mut distinct = BTreeSet::new();
    let mut clips = Vec::new();
    for (id, reference) in validated.references {
        let asset = selection
            .by_id
            .get(&id)
            .ok_or("missing translation scoring asset")?;
        let candidate = validated
            .candidates
            .get(&id)
            .ok_or("missing translation scoring candidate")?;
        let statistics = statistics(
            candidate.outcome.text().unwrap_or(""),
            &reference.english_text,
        );
        total.add(&candidate.outcome, &statistics);
        by_language
            .entry(asset.config.clone())
            .or_default()
            .add(&candidate.outcome, &statistics);
        distinct.insert(sha256(reference.english_text.as_bytes()));
        clips.push(TranslationClip {
            clip_id: id,
            config: asset.config.clone(),
            sentence_group_id: asset.sentence_group_id,
            raw_source_reference: reference.source_text,
            raw_english_reference: reference.english_text,
            raw_input: candidate.input_text.clone(),
            input_sha256: candidate.input_sha256.clone(),
            outcome: candidate.outcome.clone(),
            absent_output_scored_as_empty: candidate.outcome.text().is_none(),
            chrfpp: statistics.score(),
            statistics,
        });
    }
    Ok(Report {
        schema_version: 1,
        metric_signature: SIGNATURE,
        manifest_sha256: MANIFEST_SHA256,
        dataset_revision: DATASET_REVISION,
        partition,
        scorer_source_bundle_sha256: crate::scoring::source_bundle_digest(),
        reference_artifact_sha256: reference_digest,
        candidate_artifact_sha256: candidate_digest,
        declared_profile_sha256: validated.profile_sha256,
        declared_tuning_partition: Partition::Calibration,
        input_origin: validated.input_origin,
        distinct_english_references: distinct.len(),
        interpretation: "Reference-scored screening only, not semantic error detection, language qualification or model ranking. Parallel references repeat across languages and overall pooling does not create independent sentences. Counts are exact; chrF++ scores are derived floating-point values. Raw case and Unicode forms are retained. Missing outputs are empty in all-clips statistics; translated-only scores must be read with coverage. Published source and parallel English strings match frozen manifest hashes. Recognizer profile/artifact digests are declarations, not proof of worker isolation or model provenance. Holdout tuning declarations are refused; operator isolation remains external. Reports contain evaluator-only reference text. Each text is bounded to 8192 UTF-8 bytes and 1024 raw/normalized scalars by admission; 28 calibration or 70 holdout pairs; no inference or network client.",
        by_language,
        total,
        clips,
    })
}
