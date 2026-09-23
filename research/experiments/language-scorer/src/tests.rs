use crate::metrics::{MAX_SCALARS, NORMALIZATION_ID};
use crate::records::{Candidate, Candidates, Language, Outcome, Reference, References, validate};
use crate::scoring::score;
use crate::selection::{MANIFEST_SHA256, Partition, Selection, sha256, synthetic_selection};

fn fixtures(partition: Partition) -> Result<(Selection, References, Candidates), String> {
    let selection = synthetic_selection()?;
    let mut references = Vec::new();
    let mut candidates = Vec::new();
    for (id, asset) in &selection.by_id {
        if asset.partition != partition {
            continue;
        }
        references.push(Reference {
            clip_id: id.clone(),
            text: "synthetic reference".into(),
        });
        candidates.push(Candidate {
            clip_id: id.clone(),
            outcome: Outcome::Recognized {
                text: "synthetic reference".into(),
                language: Language::Unknown {},
            },
        });
    }
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
            normalization_id: NORMALIZATION_ID.into(),
            profile_sha256: sha256(b"synthetic profile"),
            declared_tuning_partition: Partition::Calibration,
            records: candidates,
        },
    ))
}

#[test]
fn full_synthetic_partition_scores_reproducibly_without_changing_raw_text() -> Result<(), String> {
    for partition in [Partition::Calibration, Partition::Holdout] {
        let (selection, references, candidates) = fixtures(partition)?;
        let first = score(
            &selection,
            partition,
            validate(
                &selection,
                partition,
                references.clone(),
                candidates.clone(),
            )?,
            sha256(b"references"),
            sha256(b"candidates"),
        )?;
        let second = score(
            &selection,
            partition,
            validate(&selection, partition, references, candidates)?,
            sha256(b"references"),
            sha256(b"candidates"),
        )?;
        assert_eq!(
            serde_json::to_string(&first).map_err(|error| error.to_string())?,
            serde_json::to_string(&second).map_err(|error| error.to_string())?
        );
        assert_eq!(first.total.counts.clips, partition.groups().len() * 8);
        assert_eq!(first.total.all_clips_missing_output_as_empty.cer.errors, 0);
        assert_eq!(first.clips[0].raw_reference, "synthetic reference");
    }
    Ok(())
}

#[test]
fn missing_duplicate_and_unknown_candidates_fail() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut missing = candidates.clone();
    missing.records.pop();
    assert!(validate(&selection, partition, references.clone(), missing).is_err());
    let mut duplicate = candidates.clone();
    duplicate.records[1] = duplicate.records[0].clone();
    assert!(validate(&selection, partition, references.clone(), duplicate).is_err());
    let mut unknown = candidates;
    unknown.records[0].clip_id = "unknown".into();
    assert!(validate(&selection, partition, references, unknown).is_err());
    Ok(())
}

#[test]
fn missing_duplicate_and_changed_references_fail() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut missing = references.clone();
    missing.records.pop();
    assert!(validate(&selection, partition, missing, candidates.clone()).is_err());
    let mut duplicate = references.clone();
    duplicate.records[1] = duplicate.records[0].clone();
    assert!(validate(&selection, partition, duplicate, candidates.clone()).is_err());
    let mut changed = references;
    changed.records[0].text.push('!');
    assert!(validate(&selection, partition, changed, candidates).is_err());
    Ok(())
}

#[test]
fn envelope_and_record_partition_leakage_is_refused() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut wrong_header = candidates.clone();
    wrong_header.partition = Partition::Holdout;
    assert!(validate(&selection, partition, references.clone(), wrong_header).is_err());
    let holdout_id = selection
        .by_id
        .iter()
        .find(|(_, asset)| asset.partition == Partition::Holdout)
        .map(|(id, _)| id.clone())
        .ok_or("test holdout missing")?;
    let mut leaking_candidate = candidates.clone();
    leaking_candidate.records[0].clip_id.clone_from(&holdout_id);
    assert!(validate(&selection, partition, references.clone(), leaking_candidate).is_err());
    let mut leaking_reference = references.clone();
    leaking_reference.records[0].clip_id = holdout_id;
    assert!(validate(&selection, partition, leaking_reference, candidates.clone()).is_err());
    let mut holdout_tuning = candidates;
    holdout_tuning.declared_tuning_partition = Partition::Holdout;
    assert!(validate(&selection, partition, references, holdout_tuning).is_err());
    Ok(())
}

#[test]
fn revision_and_policy_mismatches_fail() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, candidates) = fixtures(partition)?;
    let mut wrong_manifest = references.clone();
    wrong_manifest.manifest_sha256 = sha256(b"other selection");
    assert!(validate(&selection, partition, wrong_manifest, candidates.clone()).is_err());
    let mut wrong_policy = candidates.clone();
    wrong_policy.normalization_id = "lowercase-with-stripped-accents".into();
    assert!(validate(&selection, partition, references.clone(), wrong_policy).is_err());
    let mut wrong_profile = candidates;
    wrong_profile.profile_sha256 = "mutable-model-latest".into();
    assert!(validate(&selection, partition, references, wrong_profile).is_err());
    Ok(())
}

#[test]
fn mixed_unknown_and_missing_outputs_do_not_disappear_from_denominators() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, mut candidates) = fixtures(partition)?;
    candidates.records[0].outcome = Outcome::Recognized {
        text: "synthetic reference".into(),
        language: Language::Mixed {
            labels: vec!["ar".into(), "fr".into()],
        },
    };
    candidates.records[1].outcome = Outcome::Recognized {
        text: "synthetic reference".into(),
        language: Language::Known {
            label: "provider alias".into(),
        },
    };
    candidates.records[2].outcome = Outcome::Abstained {
        reason: "below configured threshold".into(),
    };
    candidates.records[3].outcome = Outcome::Failed {
        reason: "worker failed".into(),
    };
    candidates.records[4].outcome = Outcome::Unsupported {
        reason: "route cannot perform task".into(),
    };
    let report = score(
        &selection,
        partition,
        validate(&selection, partition, references, candidates)?,
        sha256(b"references"),
        sha256(b"candidates"),
    )?;
    assert_eq!(report.total.counts.clips, 32);
    assert_eq!(report.total.counts.recognized, 29);
    assert_eq!(report.total.counts.abstained, 1);
    assert_eq!(report.total.counts.failed, 1);
    assert_eq!(report.total.counts.unsupported, 1);
    assert_eq!(report.total.counts.language_mixed, 1);
    assert_eq!(report.total.counts.language_known, 1);
    assert_eq!(report.total.counts.language_unknown, 27);
    assert_eq!(
        report
            .total
            .all_clips_missing_output_as_empty
            .whitespace_wer
            .reference_units,
        64
    );
    assert_eq!(
        report
            .total
            .all_clips_missing_output_as_empty
            .whitespace_wer
            .edits
            .deletions,
        6
    );
    assert_eq!(
        report.total.recognized_only.whitespace_wer.reference_units,
        58
    );
    Ok(())
}

#[test]
fn raw_combining_forms_are_preserved_while_scoring_equally() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (mut selection, mut references, mut candidates) = fixtures(partition)?;
    let reference = &mut references.records[0];
    reference.text = "cafe\u{301}".into();
    selection
        .by_id
        .get_mut(&reference.clip_id)
        .ok_or("test asset missing")?
        .raw_transcription_sha256 = sha256(reference.text.as_bytes());
    candidates.records[0].outcome = Outcome::Recognized {
        text: "café".into(),
        language: Language::Unknown {},
    };
    let report = score(
        &selection,
        partition,
        validate(&selection, partition, references, candidates)?,
        sha256(b"references"),
        sha256(b"candidates"),
    )?;
    assert_eq!(report.clips[0].raw_reference, "cafe\u{301}");
    assert_eq!(report.clips[0].normalized_reference, "café");
    assert_eq!(report.clips[0].metrics.cer.errors, 0);
    Ok(())
}

#[test]
fn empty_reference_insertions_and_empty_recognition_are_reported() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (mut selection, mut references, mut candidates) = fixtures(partition)?;
    references.records[0].text.clear();
    selection
        .by_id
        .get_mut(&references.records[0].clip_id)
        .ok_or("test asset missing")?
        .raw_transcription_sha256 = sha256(b"");
    candidates.records[1].outcome = Outcome::Recognized {
        text: String::new(),
        language: Language::Unknown {},
    };
    let report = score(
        &selection,
        partition,
        validate(&selection, partition, references, candidates)?,
        sha256(b"references"),
        sha256(b"candidates"),
    )?;
    assert_eq!(report.total.counts.empty_reference, 1);
    assert_eq!(report.total.counts.empty_reference_with_insertions, 1);
    assert_eq!(report.total.counts.recognized_empty, 1);
    assert_eq!(report.clips[0].metrics.cer.rate, None);
    assert_eq!(report.clips[0].metrics.whitespace_wer.edits.insertions, 2);
    assert_eq!(report.clips[1].metrics.whitespace_wer.edits.deletions, 2);
    Ok(())
}

#[test]
fn invalid_mixed_labels_and_illegal_outcome_fields_fail() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (selection, references, mut candidates) = fixtures(partition)?;
    candidates.records[0].outcome = Outcome::Recognized {
        text: "synthetic reference".into(),
        language: Language::Mixed {
            labels: vec!["fr".into(), "fr".into()],
        },
    };
    assert!(validate(&selection, partition, references, candidates).is_err());
    assert!(
        serde_json::from_str::<Outcome>(
            r#"{"status":"abstained","reason":"uncertain","text":"hidden answer"}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<Outcome>(r#"{"status":"recognized","text":"hello","text":"replacement","language":{"state":"unknown"}}"#).is_err());
    Ok(())
}

#[test]
fn run_cell_budget_refuses_large_but_individually_valid_texts() -> Result<(), String> {
    let partition = Partition::Calibration;
    let (mut selection, mut references, mut candidates) = fixtures(partition)?;
    let text = "a".repeat(MAX_SCALARS);
    for reference in &mut references.records {
        reference.text.clone_from(&text);
        selection
            .by_id
            .get_mut(&reference.clip_id)
            .ok_or("test asset missing")?
            .raw_transcription_sha256 = sha256(text.as_bytes());
    }
    for candidate in &mut candidates.records {
        candidate.outcome = Outcome::Recognized {
            text: text.clone(),
            language: Language::Unknown {},
        };
    }
    let validated = validate(&selection, partition, references, candidates)?;
    assert!(
        score(
            &selection,
            partition,
            validated,
            sha256(b"references"),
            sha256(b"candidates")
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn input_reads_stop_at_the_cap_and_immutable_digests_fail_closed() -> Result<(), String> {
    let exact_limit =
        usize::try_from(crate::MAX_ARTIFACT_BYTES).map_err(|error| error.to_string())?;
    assert_eq!(
        crate::read_limited(std::io::Cursor::new(vec![b'x'; exact_limit]))?.len(),
        exact_limit
    );
    let mut oversized = std::io::Cursor::new(vec![b'x'; exact_limit + 9]);
    assert!(crate::read_limited(&mut oversized).is_err());
    assert_eq!(oversized.position(), crate::MAX_ARTIFACT_BYTES + 1);
    let original = b"immutable reference artifact";
    let digest = sha256(original);
    assert!(crate::verify_digest(original, &digest).is_ok());
    assert!(crate::verify_digest(b"changed reference artifact", &digest).is_err());
    assert!(crate::verify_digest(original, &digest.to_uppercase()).is_err());
    Ok(())
}

#[test]
fn unknown_language_outcomes_refuse_hidden_fields() -> Result<(), String> {
    let unknown: Language = serde_json::from_str(r#"{"state":"unknown"}"#)
        .map_err(|_| "valid unknown fixture rejected")?;
    assert!(matches!(unknown, Language::Unknown {}));
    for input in [
        r#"{"state":"unknown","label":"fr"}"#,
        r#"{"state":"unknown","labels":["fr","ar"]}"#,
        r#"{"state":"unknown","text":"hidden reference"}"#,
        r#"{"state":"unknown","confidence":1}"#,
    ] {
        assert!(serde_json::from_str::<Language>(input).is_err());
    }
    let nested =
        r#"{"status":"recognized","text":"synthetic","language":{"state":"unknown","label":"fr"}}"#;
    assert!(serde_json::from_str::<Outcome>(nested).is_err());
    Ok(())
}

#[test]
fn parse_errors_are_static_and_diagnostics_are_bounded_ascii() -> Result<(), String> {
    let forged_field = br#"{"\u001b[31mFORGED\nSECRET":true}"#;
    assert_eq!(
        crate::parse_references(forged_field).err().as_deref(),
        Some("invalid reference JSON")
    );
    assert_eq!(
        crate::parse_candidates(forged_field).err().as_deref(),
        Some("invalid candidate JSON")
    );
    let forged_state = br#"{"schema_version":1,"manifest_sha256":"invalid","partition":"\u001b[31mFORGED\nSECRET"}"#;
    let error = crate::parse_references(forged_state)
        .err()
        .ok_or("forged partition accepted")?;
    assert_eq!(error, "invalid reference JSON");
    let diagnostic =
        crate::diagnostic(&format!("\u{1b}[31mFORGED\n\r\u{202e}{}", "é".repeat(1024)));
    assert_eq!(diagnostic.len(), crate::MAX_DIAGNOSTIC_BYTES);
    assert!(
        diagnostic
            .chars()
            .all(|character| character.is_ascii_graphic() || character == ' ')
    );
    assert!(diagnostic.starts_with(r"\u{1b}[31mFORGED\n\r\u{202e}"));
    Ok(())
}
