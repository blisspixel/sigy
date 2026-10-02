//! Offline collection of receipts into scorer envelopes and reports.
//!
//! Judge replies pass through the unchanged parser of the frozen local screen.
//! Every unattempted, ambiguous, unsent or unparseable reply remains an
//! explicit abstention or failure in the full denominator.

use std::{collections::BTreeMap, path::Path};

use scorer_probe::{
    judge_records::{CalibrationSet, Judgment, JudgmentOutcome, Judgments},
    metrics::NORMALIZATION_ID,
    selection::{MANIFEST_SHA256, Partition, Selection, sha256},
    translation_metric::SIGNATURE,
    translation_records::{Candidates, InputOrigin, References},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Result, bleu, files,
    plan::{Item, Manifest, RUBRIC, parser_control},
    response,
    run::{Receipt, receipts},
};

fn abstained(reason: &str) -> JudgmentOutcome {
    JudgmentOutcome::Abstained {
        reason: reason.into(),
    }
}

/// Map each item to its receipt, if its request was attempted.
fn by_item<'a>(
    manifest: &Manifest,
    receipts: &'a BTreeMap<u32, Receipt>,
) -> BTreeMap<String, &'a Receipt> {
    manifest
        .requests
        .iter()
        .filter_map(|planned| {
            receipts
                .get(&planned.index)
                .map(|receipt| (planned.item_id.clone(), receipt))
        })
        .collect()
}

#[must_use]
pub fn judge_outcome(item: &Item, receipt: Option<&Receipt>) -> JudgmentOutcome {
    let Some(receipt) = receipt else {
        return abstained("unattempted-or-batch-stopped");
    };
    match &receipt.settlement {
        Some(settlement) => match settlement.content.as_deref() {
            Some(content) => response::parse(content.as_bytes(), &parser_control(item))
                .unwrap_or_else(|_| abstained("invalid-json-status-reason-or-literal-evidence")),
            None => abstained("hosted-response-without-content"),
        },
        None if receipt.outcome == "not_sent" => abstained("hosted-request-not-sent"),
        None => abstained("hosted-ambiguous-outcome"),
    }
}

fn loaded(directory: &Path, digest: &str) -> Result<(Manifest, Vec<Item>, BTreeMap<u32, Receipt>)> {
    let (manifest, items) = Manifest::load(directory, digest)?;
    Ok((manifest, items, receipts(directory)?))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec_pretty(value)?;
    files::write_new(path, &bytes)?;
    Ok(sha256(&bytes))
}

/// Score a judge's replies to the 126 frozen controls with the frozen criteria.
pub fn controls(
    directory: &Path,
    digest: &str,
    selection: &Selection,
    controls: &CalibrationSet,
    controls_sha256: &str,
) -> Result<Value> {
    let (manifest, items, receipts) = loaded(directory, digest)?;
    let attempted = by_item(&manifest, &receipts);
    let items: BTreeMap<_, _> = items.iter().map(|item| (item.id.clone(), item)).collect();
    let mut records = Vec::new();
    for control in &controls.controls {
        let item = items
            .get(&control.control_id)
            .ok_or("control missing from plan")?;
        records.push(Judgment {
            control_id: control.control_id.clone(),
            outcome: judge_outcome(item, attempted.get(&control.control_id).copied()),
        });
    }
    let judgments = Judgments {
        schema_version: 1,
        control_artifact_sha256: controls_sha256.into(),
        rubric_sha256: sha256(RUBRIC.as_bytes()),
        judge_profile_sha256: digest.into(),
        judge_family: format!("{} via OpenRouter", manifest.route.model),
        records,
    };
    let judgments_sha256 = write_json(&directory.join("judgments.json"), &judgments)?;
    let report = scorer_probe::judge_scoring::score(
        selection,
        controls,
        &judgments,
        controls_sha256,
        &judgments_sha256,
    )?;
    let report_sha256 = write_json(&directory.join("control-score.json"), &report)?;
    let mut languages = BTreeMap::new();
    for (config, language) in &report.by_language {
        let counts = &language.counts;
        languages.insert(
            config.clone(),
            json!({
                "critical_detected": counts.true_positive,
                "expected_critical": counts.expected_critical,
                "false_positive": counts.false_positive,
                "expected_acceptable": counts.expected_acceptable,
                "critical_abstained": counts.abstained_critical,
                "acceptable_abstained": counts.abstained_acceptable,
                "false_negative": counts.false_negative,
                "passes_frozen_criteria": language.declared_control_criteria_pass,
            }),
        );
    }
    Ok(json!({
        "judge": manifest.route.model,
        "judgments_sha256": judgments_sha256,
        "report_sha256": report_sha256,
        "by_language": languages,
    }))
}

/// Count a judge's verdicts on real translations, by run and language.
pub fn outputs(directory: &Path, digest: &str) -> Result<Value> {
    let (manifest, items, receipts) = loaded(directory, digest)?;
    let attempted = by_item(&manifest, &receipts);
    let mut counts: BTreeMap<String, BTreeMap<String, [u32; 3]>> = BTreeMap::new();
    let mut critical = Vec::new();
    let mut abstentions: BTreeMap<String, u32> = BTreeMap::new();
    for item in &items {
        let outcome = judge_outcome(item, attempted.get(&item.id).copied());
        let slot = counts
            .entry(item.group.clone())
            .or_default()
            .entry(item.config.clone())
            .or_default();
        match outcome {
            JudgmentOutcome::Critical {
                source_quote,
                reference_quote,
                output_quote,
                reason,
            } => {
                slot[0] += 1;
                critical.push(json!({
                    "run": item.group, "config": item.config, "clip_id": item.clip_id,
                    "source_quote": source_quote.text, "reference_quote": reference_quote.text,
                    "output_quote": output_quote.map(|quote| quote.text), "reason": reason,
                    "output_text": item.output_text,
                }));
            }
            JudgmentOutcome::Acceptable {} => slot[1] += 1,
            JudgmentOutcome::Abstained { reason } => {
                slot[2] += 1;
                *abstentions.entry(reason).or_default() += 1;
            }
        }
    }
    let table: BTreeMap<_, _> = counts
        .into_iter()
        .map(|(run, languages)| {
            let total = languages.values().fold([0; 3], |sum, value| [sum[0] + value[0], sum[1] + value[1], sum[2] + value[2]]);
            let rows: BTreeMap<_, _> = languages
                .into_iter()
                .map(|(config, value)| (config, json!({"critical": value[0], "acceptable": value[1], "abstained": value[2]})))
                .collect();
            (run, json!({"by_language": rows, "total": {"critical": total[0], "acceptable": total[1], "abstained": total[2]}}))
        })
        .collect();
    let report = json!({
        "judge": manifest.route.model,
        "manifest_sha256": digest,
        "interpretation": "Model-reviewed critical-error screen of real translations against published FLEURS sources and English references. One judge's verdicts are not ground truth; abstentions stay in the denominator. Not a qualification.",
        "by_run": table,
        "abstention_reasons": abstentions,
        "critical_items": critical,
    });
    let report_sha256 = write_json(&directory.join("output-judgments.json"), &report)?;
    Ok(
        json!({"report_sha256": report_sha256, "by_run": report["by_run"], "abstention_reasons": report["abstention_reasons"]}),
    )
}

fn text_outcome(receipt: Option<&Receipt>) -> std::result::Result<String, &'static str> {
    let receipt = receipt.ok_or("not attempted in this batch")?;
    let settlement = match (&receipt.settlement, receipt.outcome.as_str()) {
        (Some(settlement), _) => settlement,
        (None, "not_sent") => return Err("hosted request not sent"),
        (None, _) => return Err("hosted outcome ambiguous"),
    };
    if settlement.finish_reason.as_deref() == Some("length") {
        return Err("hosted output reached its token limit");
    }
    Ok(settlement
        .content
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_owned())
}

/// BLEU per language and overall over translated outputs only.
fn bleu_table(report: &scorer_probe::translation_scoring::Report) -> BTreeMap<String, f64> {
    let mut pairs: BTreeMap<String, Vec<(&str, &str)>> = BTreeMap::new();
    for clip in &report.clips {
        if let Some(text) = clip.outcome.text() {
            for key in [clip.config.clone(), "all".into()] {
                pairs
                    .entry(key)
                    .or_default()
                    .push((text, clip.raw_english_reference.as_str()));
            }
        }
    }
    pairs
        .into_iter()
        .map(|(key, pairs)| (key, bleu::corpus(&pairs)))
        .collect()
}

fn translation_summary(
    report: &scorer_probe::translation_scoring::Report,
    candidate_sha256: &str,
    report_sha256: &str,
) -> Value {
    let chrf: BTreeMap<_, _> = report
        .by_language
        .iter()
        .map(|(config, aggregate)| (config.clone(), json!({"translated": aggregate.coverage.translated, "clips": aggregate.coverage.clips, "translated_only_chrfpp": aggregate.translated_only_chrfpp, "all_clips_chrfpp": aggregate.all_clips_chrfpp})))
        .collect();
    json!({
        "candidates_sha256": candidate_sha256,
        "report_sha256": report_sha256,
        "chrfpp": chrf,
        "total": {"translated": report.total.coverage.translated, "clips": report.total.coverage.clips, "translated_only_chrfpp": report.total.translated_only_chrfpp, "all_clips_chrfpp": report.total.all_clips_chrfpp},
        "bleu_translated_only": bleu_table(report),
    })
}

/// Score an existing translation envelope with chrF++ and BLEU.
pub fn score_translation_envelope(
    selection: &Selection,
    references: &References,
    references_sha256: &str,
    candidate_bytes: &[u8],
    report_path: &Path,
) -> Result<Value> {
    let candidates: Candidates = serde_json::from_slice(candidate_bytes)?;
    let candidate_sha256 = sha256(candidate_bytes);
    let validated = scorer_probe::translation_records::validate(
        selection,
        Partition::Calibration,
        references.clone(),
        candidates,
    )?;
    let report = scorer_probe::translation_scoring::score(
        selection,
        Partition::Calibration,
        validated,
        references_sha256.into(),
        candidate_sha256.clone(),
    )?;
    let report_sha256 = write_json(report_path, &report)?;
    Ok(translation_summary(
        &report,
        &candidate_sha256,
        &report_sha256,
    ))
}

/// Build a translation envelope from hosted receipts and score it.
pub fn translations(
    directory: &Path,
    digest: &str,
    selection: &Selection,
    references: &References,
    references_sha256: &str,
    origin: &InputOrigin,
) -> Result<Value> {
    let (manifest, items, receipts) = loaded(directory, digest)?;
    let attempted = by_item(&manifest, &receipts);
    let mut records = Vec::new();
    for item in &items {
        let outcome = if item.requested {
            match text_outcome(attempted.get(&item.id).copied()) {
                Ok(text) if !text.is_empty() => json!({"status": "translated", "text": text}),
                Ok(_) => json!({"status": "failed", "reason": "hosted output was empty"}),
                Err(reason) => json!({"status": "failed", "reason": reason}),
            }
        } else {
            json!({"status": "abstained", "reason": "prior calibration had no recognized source text"})
        };
        records.push(json!({"clip_id": item.clip_id, "input_text": item.source_text, "input_sha256": sha256(item.source_text.as_bytes()), "outcome": outcome}));
    }
    let envelope = json!({
        "schema_version": 1, "manifest_sha256": MANIFEST_SHA256, "partition": "calibration",
        "metric_signature": SIGNATURE, "profile_sha256": digest, "declared_tuning_partition": "calibration",
        "input_origin": origin, "records": records,
    });
    let bytes = serde_json::to_vec_pretty(&envelope)?;
    files::write_new(&directory.join("candidates.json"), &bytes)?;
    score_translation_envelope(
        selection,
        references,
        references_sha256,
        &bytes,
        &directory.join("translation-score.json"),
    )
}

/// Recognition references for all 32 calibration clips, from the paired
/// translation references. English clips take their parallel English text.
pub fn recognition_references(selection: &Selection, references: &References) -> Result<Value> {
    let mut by_group = BTreeMap::new();
    let mut records = Vec::new();
    for reference in &references.records {
        scorer_probe::translation_records::validate_reference(
            selection,
            Partition::Calibration,
            reference,
        )?;
        let asset = selection
            .asset(&reference.clip_id)
            .ok_or("unknown reference clip")?;
        by_group.insert(asset.sentence_group_id, reference.english_text.clone());
        records.push(json!({"clip_id": reference.clip_id, "text": reference.source_text}));
    }
    for clip in selection.inventory(Partition::Calibration).clips {
        if clip.config == "en_us" {
            let asset = selection
                .asset(clip.clip_id)
                .ok_or("unknown English clip")?;
            let text = by_group
                .get(&asset.sentence_group_id)
                .ok_or("missing parallel English reference")?;
            records.push(json!({"clip_id": clip.clip_id, "text": text}));
        }
    }
    if records.len() != 32 {
        return Err("recognition references must cover all 32 calibration clips".into());
    }
    Ok(
        json!({"schema_version": 1, "manifest_sha256": MANIFEST_SHA256, "partition": "calibration", "records": records}),
    )
}

fn score_recognition(
    selection: &Selection,
    references_bytes: &[u8],
    references_sha256: &str,
    envelope: &Value,
    directory: &Path,
    name: &str,
) -> Result<Value> {
    let bytes = serde_json::to_vec_pretty(envelope)?;
    files::write_new(&directory.join(format!("{name}-candidates.json")), &bytes)?;
    let candidate_sha256 = sha256(&bytes);
    let validated = scorer_probe::records::validate(
        selection,
        Partition::Calibration,
        serde_json::from_slice(references_bytes)?,
        serde_json::from_slice(&bytes)?,
    )?;
    let report = scorer_probe::scoring::score(
        selection,
        Partition::Calibration,
        validated,
        references_sha256.into(),
        candidate_sha256.clone(),
    )?;
    let report_sha256 = write_json(&directory.join(format!("{name}-score.json")), &report)?;
    let rate = |metric: &scorer_probe::metrics::Metric| json!({"errors": metric.errors, "reference_units": metric.reference_units});
    let languages: BTreeMap<_, _> = report
        .by_language
        .iter()
        .map(|(config, language)| {
            let scores = &language.scores;
            (
                config.clone(),
                json!({
                    "recognized": scores.counts.recognized, "abstained": scores.counts.abstained,
                    "failed": scores.counts.failed, "unsupported": scores.counts.unsupported,
                    "recognized_only_cer": rate(&scores.recognized_only.cer),
                    "recognized_only_wer": rate(&scores.recognized_only.whitespace_wer),
                }),
            )
        })
        .collect();
    Ok(
        json!({"candidates_sha256": candidate_sha256, "report_sha256": report_sha256, "by_language": languages}),
    )
}

/// Score hosted transcripts. Clips outside the batch are explicit `unsupported`.
pub fn recognition(
    directory: &Path,
    digest: &str,
    selection: &Selection,
    references_bytes: &[u8],
    references_sha256: &str,
) -> Result<Value> {
    let (manifest, items, receipts) = loaded(directory, digest)?;
    let attempted = by_item(&manifest, &receipts);
    let planned: BTreeMap<_, _> = items
        .iter()
        .map(|item| (item.clip_id.clone(), item))
        .collect();
    let mut records = Vec::new();
    for clip in selection.inventory(Partition::Calibration).clips {
        let outcome = match planned.get(clip.clip_id) {
            None => json!({"status": "unsupported", "reason": "not requested in this batch"}),
            Some(item) => match text_outcome(attempted.get(&item.id).copied()) {
                Ok(text) if text.is_empty() => {
                    json!({"status": "abstained", "reason": "no transcript text returned"})
                }
                Ok(text) => {
                    json!({"status": "recognized", "text": text, "language": {"state": "unknown"}})
                }
                Err(reason) => json!({"status": "failed", "reason": reason}),
            },
        };
        records.push(json!({"clip_id": clip.clip_id, "outcome": outcome}));
    }
    let envelope = json!({
        "schema_version": 1, "manifest_sha256": MANIFEST_SHA256, "partition": "calibration",
        "normalization_id": NORMALIZATION_ID, "profile_sha256": digest, "declared_tuning_partition": "calibration",
        "records": records,
    });
    score_recognition(
        selection,
        references_bytes,
        references_sha256,
        &envelope,
        directory,
        "recognition",
    )
}

/// Score the recorded local recognizer outputs of the 32-clip calibration.
pub fn baseline(
    results: &[u8],
    profile: &str,
    selection: &Selection,
    references_bytes: &[u8],
    references_sha256: &str,
    directory: &Path,
) -> Result<Value> {
    let document: Value = serde_json::from_slice(results)?;
    let rows = document
        .get(profile)
        .and_then(Value::as_array)
        .ok_or("profile absent from results")?;
    let mut by_asset = BTreeMap::new();
    for row in rows {
        let stem = row
            .get("stem")
            .and_then(Value::as_str)
            .ok_or("row without stem")?;
        let Some((config, id)) = stem.split_once('-') else {
            continue;
        };
        if config == "control" {
            continue;
        }
        by_asset.insert(format!("{config}/train/{id}.wav"), row);
    }
    let mut records = Vec::new();
    for clip in selection.inventory(Partition::Calibration).clips {
        let row = by_asset
            .get(clip.asset_id)
            .ok_or("recorded results omit a calibration clip")?;
        let text = row.get("text").and_then(Value::as_str).unwrap_or("").trim();
        let outcome = if text.is_empty() {
            json!({"status": "abstained", "reason": "no transcript text returned"})
        } else {
            let label = row
                .get("lang")
                .and_then(Value::as_str)
                .unwrap_or("unlabeled");
            json!({"status": "recognized", "text": text, "language": {"state": "known", "label": label}})
        };
        records.push(json!({"clip_id": clip.clip_id, "outcome": outcome}));
    }
    let envelope = json!({
        "schema_version": 1, "manifest_sha256": MANIFEST_SHA256, "partition": "calibration",
        "normalization_id": NORMALIZATION_ID, "profile_sha256": sha256(format!("recorded-local-baseline-v1:{profile}:{}", sha256(results)).as_bytes()),
        "declared_tuning_partition": "calibration", "records": records,
    });
    score_recognition(
        selection,
        references_bytes,
        references_sha256,
        &envelope,
        directory,
        profile,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{outcome::Settlement, plan::tests as fixtures};

    fn receipt(outcome: &str, content: Option<&str>, finish: Option<&str>) -> Receipt {
        Receipt {
            index: 0,
            item_id: "x".into(),
            request_id: "r".into(),
            outcome: outcome.into(),
            settlement: content.map(|content| Settlement {
                micro: 1,
                cost_text: "0.000001".into(),
                generation_id: "gen".into(),
                provider: "Fixture".into(),
                served_model: "vendor/model".into(),
                usage: crate::outcome::Usage::default(),
                content: Some(content.into()),
                finish_reason: finish.map(str::to_owned),
                routing: None,
            }),
            reason: None,
            http_status: None,
            error_message: None,
            elapsed_ms: None,
            breach: None,
            response_sha256: None,
        }
    }

    #[test]
    fn judge_replies_use_the_frozen_parser_and_failures_abstain() {
        let item = fixtures::draft("x", true).item;
        let critical = r#"{"status":"critical","source_quote":"2020","reference_quote":"2020","output_quote":"2030","reason":"Date changed."}"#;
        assert!(matches!(
            judge_outcome(
                &item,
                Some(&receipt("settled", Some(critical), Some("stop")))
            ),
            JudgmentOutcome::Critical { .. }
        ));
        let fenced = format!("```json\n{critical}\n```");
        let invented = r#"{"status":"critical","source_quote":"1999","reference_quote":"2020","output_quote":"","reason":"Invented."}"#;
        for (receipt, reason) in [
            (
                receipt("settled", Some(&fenced), None),
                "invalid-json-status-reason-or-literal-evidence",
            ),
            (
                receipt("settled", Some(invented), None),
                "invalid-json-status-reason-or-literal-evidence",
            ),
            (receipt("not_sent", None, None), "hosted-request-not-sent"),
            (receipt("uncertain", None, None), "hosted-ambiguous-outcome"),
        ] {
            assert_eq!(
                serde_json::to_string(&judge_outcome(&item, Some(&receipt))).unwrap_or_default(),
                serde_json::to_string(&abstained(reason)).unwrap_or_default(),
                "{reason}"
            );
        }
        let mut empty = receipt("settled", Some("x"), None);
        if let Some(settlement) = empty.settlement.as_mut() {
            settlement.content = None;
        }
        assert!(
            matches!(judge_outcome(&item, Some(&empty)), JudgmentOutcome::Abstained { reason } if reason == "hosted-response-without-content")
        );
        assert!(
            matches!(judge_outcome(&item, None), JudgmentOutcome::Abstained { reason } if reason == "unattempted-or-batch-stopped")
        );
    }

    #[test]
    fn text_outcomes_keep_failures_explicit() {
        assert_eq!(
            text_outcome(Some(&receipt("settled", Some("  hi \n"), Some("stop")))),
            Ok("hi".into())
        );
        assert_eq!(
            text_outcome(Some(&receipt("settled", Some("partial"), Some("length")))),
            Err("hosted output reached its token limit")
        );
        assert_eq!(
            text_outcome(Some(&receipt("not_sent", None, None))),
            Err("hosted request not sent")
        );
        assert_eq!(
            text_outcome(Some(&receipt("uncertain", None, None))),
            Err("hosted outcome ambiguous")
        );
        assert_eq!(text_outcome(None), Err("not attempted in this batch"));
    }
}
