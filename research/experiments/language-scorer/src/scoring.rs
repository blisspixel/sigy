use std::collections::BTreeMap;

use serde::Serialize;

use crate::metrics::{
    MAX_ALIGNMENT_CELLS, Metrics, NORMALIZATION_ID, Rule, alignment_cells, normalize, rule,
};
use crate::records::{Language, Outcome, Validated};
use crate::selection::{CONFIGS, DATASET_REVISION, MANIFEST_SHA256, Partition, Selection, sha256};

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub scope: &'static str,
    pub scorer_version: &'static str,
    pub scorer_source_bundle_sha256: String,
    pub manifest_sha256: &'static str,
    pub dataset_revision: &'static str,
    pub partition: Partition,
    pub reference_artifact_sha256: String,
    pub candidate_artifact_sha256: String,
    pub declared_profile_sha256: String,
    pub declared_tuning_partition: Partition,
    pub normalization_id: &'static str,
    pub interpretation: &'static str,
    pub limits: &'static str,
    pub total: Aggregate,
    pub by_language: BTreeMap<String, LanguageReport>,
    pub clips: Vec<ClipReport>,
}

#[derive(Default, Serialize)]
pub struct Counts {
    pub clips: usize,
    pub recognized: usize,
    pub recognized_empty: usize,
    pub abstained: usize,
    pub failed: usize,
    pub unsupported: usize,
    pub language_known: usize,
    pub language_mixed: usize,
    pub language_unknown: usize,
    pub empty_reference: usize,
    pub empty_reference_with_insertions: usize,
    pub exact_normalized_matches_all_clips: usize,
    pub exact_normalized_matches_recognized: usize,
}

#[derive(Default, Serialize)]
pub struct Aggregate {
    pub counts: Counts,
    pub all_clips_missing_output_as_empty: Metrics,
    pub recognized_only: Metrics,
}

#[derive(Serialize)]
pub struct LanguageReport {
    pub normalization: Rule,
    pub scores: Aggregate,
}

#[derive(Serialize)]
pub struct ClipReport {
    pub clip_id: String,
    pub config: String,
    pub raw_reference: String,
    pub outcome: Outcome,
    pub normalized_reference: String,
    pub normalized_candidate: String,
    pub absent_output_scored_as_empty: bool,
    pub metrics: Metrics,
}

impl Aggregate {
    fn add(&mut self, clip: &ClipReport) {
        self.counts.clips += 1;
        self.all_clips_missing_output_as_empty.add(&clip.metrics);
        self.counts.exact_normalized_matches_all_clips += usize::from(clip.metrics.cer.errors == 0);
        if clip.normalized_reference.is_empty() {
            self.counts.empty_reference += 1;
            self.counts.empty_reference_with_insertions += usize::from(clip.metrics.cer.errors > 0);
        }
        match &clip.outcome {
            Outcome::Recognized { language, .. } => {
                self.counts.recognized += 1;
                self.counts.recognized_empty += usize::from(clip.normalized_candidate.is_empty());
                self.counts.exact_normalized_matches_recognized +=
                    usize::from(clip.metrics.cer.errors == 0);
                self.recognized_only.add(&clip.metrics);
                match language {
                    Language::Known { .. } => self.counts.language_known += 1,
                    Language::Mixed { .. } => self.counts.language_mixed += 1,
                    Language::Unknown {} => self.counts.language_unknown += 1,
                }
            }
            Outcome::Abstained { .. } => self.counts.abstained += 1,
            Outcome::Failed { .. } => self.counts.failed += 1,
            Outcome::Unsupported { .. } => self.counts.unsupported += 1,
        }
    }
}

pub fn score(
    selection: &Selection,
    partition: Partition,
    validated: Validated,
    reference_digest: String,
    candidate_digest: String,
) -> Result<Report, String> {
    let mut clips = prepare_clips(selection, &validated)?;
    let mut total = Aggregate::default();
    let mut by_language = BTreeMap::new();
    for config in CONFIGS {
        by_language.insert(
            config.to_owned(),
            LanguageReport {
                normalization: rule(config)?,
                scores: Aggregate::default(),
            },
        );
    }
    for clip in &mut clips {
        clip.metrics = Metrics::measured(&clip.normalized_reference, &clip.normalized_candidate);
        total.add(clip);
        by_language
            .get_mut(&clip.config)
            .ok_or("missing language aggregation rule")?
            .scores
            .add(clip);
    }
    Ok(Report {
        schema_version: 1,
        scope: "offline scorer prototype; 4 calibration and 10 holdout clips per language are smoke screening, not qualification",
        scorer_version: env!("CARGO_PKG_VERSION"),
        scorer_source_bundle_sha256: source_bundle_digest(),
        manifest_sha256: MANIFEST_SHA256,
        dataset_revision: DATASET_REVISION,
        partition,
        reference_artifact_sha256: reference_digest,
        candidate_artifact_sha256: candidate_digest,
        declared_profile_sha256: validated.profile_sha256,
        declared_tuning_partition: Partition::Calibration,
        normalization_id: NORMALIZATION_ID,
        interpretation: "Rates are exact unreduced integer fractions (S+D+I)/reference units, can exceed 1, and are null for zero reference units even with insertions. Counts use per-clip alignment then micro sums. All-clips scores include absent outputs as empty strings; recognized-only scores are conditional and must be read with coverage/outcome counts. Mixed and unknown language labels do not exclude recognized text. Language labels are opaque provider assertions, not validated tags or measured language accuracy. Overall micro scores are descriptive, not a multilingual pass/fail criterion. Raw and normalized text in this report are evaluator-only. Partition checks do not prove worker isolation, prior model exposure, honest tuning declarations or profile contents.",
        limits: "2 MiB per input artifact; 8192 UTF-8 bytes, 1024 raw/normalized Unicode scalars and 512 normalized tokens per text; 32000000 alignment cells per run; 2 dynamic-programming rows; no network client, audio/model processing, process launch or file output. Mapped filesystems, reparse points and UNC input paths can cause operating-system network access; this is not network isolation.",
        total,
        by_language,
        clips,
    })
}

fn source_bundle_digest() -> String {
    let files: [(&str, &[u8]); 10] = [
        ("Cargo.toml", include_bytes!("../Cargo.toml")),
        ("Cargo.lock", include_bytes!("../Cargo.lock")),
        (
            "rust-toolchain.toml",
            include_bytes!("../rust-toolchain.toml"),
        ),
        ("src/main.rs", include_bytes!("main.rs")),
        ("src/metrics.rs", include_bytes!("metrics.rs")),
        ("src/records.rs", include_bytes!("records.rs")),
        ("src/scoring.rs", include_bytes!("scoring.rs")),
        ("src/selection.rs", include_bytes!("selection.rs")),
        ("src/tests.rs", include_bytes!("tests.rs")),
        (
            "../language-corpus/fleurs-screening-manifest.json",
            include_bytes!("../../language-corpus/fleurs-screening-manifest.json"),
        ),
    ];
    let mut inventory = String::new();
    for (path, bytes) in files {
        inventory.push_str(path);
        inventory.push(':');
        inventory.push_str(&sha256(bytes));
        inventory.push('\n');
    }
    sha256(inventory.as_bytes())
}

fn prepare_clips(selection: &Selection, validated: &Validated) -> Result<Vec<ClipReport>, String> {
    let mut clips = Vec::new();
    let mut cells = 0_usize;
    for (id, reference) in &validated.references {
        let candidate = validated
            .candidates
            .get(id)
            .ok_or("missing validated candidate")?;
        let asset = selection.by_id.get(id).ok_or("missing validated asset")?;
        let normalized_reference = normalize(&reference.text)?;
        let normalized_candidate = normalize(candidate.outcome.text().unwrap_or(""))?;
        cells = cells
            .checked_add(alignment_cells(
                &normalized_reference,
                &normalized_candidate,
            ))
            .ok_or("alignment work budget overflow")?;
        if cells > MAX_ALIGNMENT_CELLS {
            return Err("run exceeds 32000000 alignment-cell budget; no partial report".into());
        }
        clips.push(ClipReport {
            clip_id: id.clone(),
            config: asset.config.clone(),
            raw_reference: reference.text.clone(),
            outcome: candidate.outcome.clone(),
            normalized_reference,
            normalized_candidate,
            absent_output_scored_as_empty: candidate.outcome.text().is_none(),
            metrics: Metrics::default(),
        });
    }
    Ok(clips)
}
