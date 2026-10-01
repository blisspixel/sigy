use crate::selection::{MANIFEST_SHA256, Partition, Selection, sha256, synthetic_selection};
use crate::translation_metric::SIGNATURE;
use crate::translation_records::{
    Candidate, Candidates, InputOrigin, Outcome, Reference, References, validate,
};

fn fixtures(partition: Partition) -> Result<(Selection, References, Candidates), String> {
    let selection = synthetic_selection()?;
    let references: Vec<_> = selection
        .by_id
        .iter()
        .filter(|(_, asset)| asset.partition == partition && asset.config != "en_us")
        .map(|(id, _)| Reference {
            clip_id: id.clone(),
            source_text: "synthetic reference".into(),
            english_text: "synthetic reference".into(),
        })
        .collect();
    let candidates = references
        .iter()
        .map(|reference| Candidate {
            clip_id: reference.clip_id.clone(),
            input_text: reference.source_text.clone(),
            input_sha256: sha256(reference.source_text.as_bytes()),
            outcome: Outcome::Translated {
                text: reference.english_text.clone(),
            },
        })
        .collect();
    Ok((
        selection,
        References {
            schema_version: 1,
            manifest_sha256: MANIFEST_SHA256.into(),
            partition,
            records: references,
        },
        Candidates {
            schema_version: 1,
            manifest_sha256: MANIFEST_SHA256.into(),
            partition,
            metric_signature: SIGNATURE.into(),
            profile_sha256: sha256(b"synthetic translation profile"),
            declared_tuning_partition: Partition::Calibration,
            input_origin: InputOrigin::ReferenceText {},
            records: candidates,
        },
    ))
}

#[test]
fn both_synthetic_partitions_score_with_independent_reference_counts() -> Result<(), String> {
    for partition in [Partition::Calibration, Partition::Holdout] {
        let (selection, references, candidates) = fixtures(partition)?;
        let report = crate::translation_scoring::score(
            &selection,
            partition,
            validate(&selection, partition, references, candidates)?,
            sha256(b"references"),
            sha256(b"candidates"),
        )?;
        assert_eq!(
            report.total.coverage.clips,
            u32::try_from(partition.groups().len() * 7).map_err(|error| error.to_string())?
        );
        assert!((report.total.all_clips_chrfpp - 100.0).abs() < 1e-12);
        assert_eq!(report.by_language.len(), 7);
        assert_eq!(report.distinct_english_references, 1);
    }
    Ok(())
}

#[test]
fn explicit_absence_stays_in_all_clip_scores_and_coverage() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, mut candidates) = fixtures(partition)?;
    candidates.records[0].outcome = Outcome::Abstained {
        reason: "uncertain".into(),
    };
    candidates.records[1].outcome = Outcome::Failed {
        reason: "deadline".into(),
    };
    candidates.records[2].outcome = Outcome::Unsupported {
        reason: "profile language".into(),
    };
    candidates.records[3].outcome = Outcome::Translated {
        text: String::new(),
    };
    let report = crate::translation_scoring::score(
        &selection,
        partition,
        validate(&selection, partition, references, candidates)?,
        sha256(b"references"),
        sha256(b"candidates"),
    )?;
    assert_eq!(report.total.coverage.clips, 28);
    assert_eq!(report.total.coverage.translated, 25);
    assert_eq!(report.total.coverage.translated_empty, 1);
    assert_eq!(report.total.coverage.abstained, 1);
    assert_eq!(report.total.coverage.failed, 1);
    assert_eq!(report.total.coverage.unsupported, 1);
    assert!(
        report.total.all_clips_chrfpp
            < report
                .total
                .translated_only_chrfpp
                .ok_or("missing conditional score")?
    );
    assert!(
        report
            .clips
            .iter()
            .filter(|clip| clip.absent_output_scored_as_empty)
            .all(|clip| clip.chrfpp.abs() < 1e-12)
    );
    Ok(())
}

#[test]
fn no_translations_have_no_conditional_score() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, mut candidates) = fixtures(partition)?;
    for candidate in &mut candidates.records {
        candidate.outcome = Outcome::Failed {
            reason: "bounded failure".into(),
        };
    }
    let report = crate::translation_scoring::score(
        &selection,
        partition,
        validate(&selection, partition, references, candidates)?,
        sha256(b"references"),
        sha256(b"candidates"),
    )?;
    assert!(report.total.all_clips_chrfpp.abs() < 1e-12);
    assert_eq!(report.total.translated_only_chrfpp, None);
    Ok(())
}

#[test]
fn altered_original_and_parallel_english_references_fail() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut original = references.clone();
    original.records[0].source_text.push('!');
    assert!(validate(&selection, partition, original, candidates.clone()).is_err());
    let mut english = references;
    english.records[0].english_text.push('!');
    assert!(validate(&selection, partition, english, candidates).is_err());
    Ok(())
}

#[test]
fn missing_duplicate_unknown_english_and_cross_partition_clips_fail() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut missing = candidates.clone();
    missing.records.pop();
    assert!(validate(&selection, partition, references.clone(), missing).is_err());
    for id in [
        "unknown".to_owned(),
        candidates.records[1].clip_id.clone(),
        selection
            .by_id
            .iter()
            .find(|(_, asset)| asset.partition == partition && asset.config == "en_us")
            .ok_or("missing synthetic English")?
            .0
            .clone(),
        selection
            .by_id
            .iter()
            .find(|(_, asset)| asset.partition == Partition::Holdout)
            .ok_or("missing synthetic holdout")?
            .0
            .clone(),
    ] {
        let mut changed = candidates.clone();
        changed.records[0].clip_id = id;
        assert!(validate(&selection, partition, references.clone(), changed).is_err());
    }
    let mut duplicate = references.clone();
    duplicate.records[0] = duplicate.records[1].clone();
    assert!(validate(&selection, partition, duplicate, candidates).is_err());
    Ok(())
}

#[test]
fn source_origin_input_hash_and_profile_are_checked() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut changed = candidates.clone();
    changed.records[0].input_text = "recognition differed".into();
    assert!(validate(&selection, partition, references.clone(), changed.clone()).is_err());
    changed.records[0].input_sha256 = sha256(changed.records[0].input_text.as_bytes());
    assert!(validate(&selection, partition, references.clone(), changed.clone()).is_err());
    changed.input_origin = InputOrigin::RecognizedText {
        recognition_profile_sha256: sha256(b"recognizer"),
        recognition_artifact_sha256: sha256(b"recognized artifact"),
    };
    assert!(validate(&selection, partition, references.clone(), changed.clone()).is_ok());
    changed.input_origin = InputOrigin::RecognizedText {
        recognition_profile_sha256: "invalid".into(),
        recognition_artifact_sha256: sha256(b"artifact"),
    };
    assert!(validate(&selection, partition, references.clone(), changed).is_err());
    let mut holdout_tuning = candidates.clone();
    holdout_tuning.declared_tuning_partition = Partition::Holdout;
    assert!(validate(&selection, partition, references.clone(), holdout_tuning).is_err());
    let mut wrong_metric = candidates;
    wrong_metric.metric_signature = "another metric".into();
    assert!(validate(&selection, partition, references, wrong_metric).is_err());
    Ok(())
}

#[test]
fn outcome_limits_and_untrusted_json_fields_fail() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut oversized = candidates.clone();
    oversized.records[0].outcome = Outcome::Translated {
        text: "x".repeat(1025),
    };
    assert!(validate(&selection, partition, references.clone(), oversized).is_err());
    let mut invalid_reason = candidates;
    invalid_reason.records[0].outcome = Outcome::Failed {
        reason: "\u{001b}untrusted".into(),
    };
    assert!(validate(&selection, partition, references, invalid_reason).is_err());
    for bytes in [
        br#"{"status":"failed","reason":"error","text":"invented"}"#.as_slice(),
        br#"{"status":"translated","text":"first","text":"duplicate"}"#.as_slice(),
        br#"{"status":"translated","text":"ok","authority":true}"#.as_slice(),
    ] {
        assert!(serde_json::from_slice::<Outcome>(bytes).is_err());
    }
    Ok(())
}
