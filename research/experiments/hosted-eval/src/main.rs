//! Bounded hosted evaluation of FLEURS calibration items through `OpenRouter`.
//!
//! Every paid request is planned into a frozen manifest first, reserved in an
//! append-only ledger before it is sent, and settled or retained as liability
//! after. See README.md for the evidence boundary.

mod bleu;
mod client;
mod collect;
mod files;
mod ledger;
mod money;
mod outcome;
mod plan;
mod pricing;
/// A byte-identical copy of the frozen local judge parser; a test pins it.
mod response;
mod run;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

use scorer_probe::{
    judge_records::CalibrationSet,
    selection::{Selection, sha256},
    translation_records::{Candidates, InputOrigin, References},
};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const USAGE: &str = "usage:
  hosted-eval init LEDGER
  hosted-eval allocate LEDGER BATCH MICRO_USD NOTE
  hosted-eval status LEDGER
  hosted-eval key LEDGER
  hosted-eval reconcile LEDGER
  hosted-eval plan-probe ROUTE CATALOG ENDPOINTS SELECTION AUDIO_DIR OUT judge|translate|recognize
  hosted-eval plan-controls ROUTE CATALOG ENDPOINTS SELECTION CONTROLS CONTROLS_SHA256 OUT
  hosted-eval plan-outputs ROUTE CATALOG ENDPOINTS SELECTION REFERENCES REFERENCES_SHA256 OUT LABEL=PATH=SHA256...
  hosted-eval plan-translate ROUTE CATALOG ENDPOINTS SELECTION REFERENCES REFERENCES_SHA256 OUT [RECOGNIZED RECOGNIZED_SHA256]
  hosted-eval plan-recognize ROUTE CATALOG ENDPOINTS SELECTION AUDIO_DIR OUT CONFIG...
  hosted-eval run LEDGER BATCH PLAN MANIFEST_SHA256
  hosted-eval collect-controls PLAN MANIFEST_SHA256 SELECTION CONTROLS CONTROLS_SHA256
  hosted-eval collect-outputs PLAN MANIFEST_SHA256
  hosted-eval collect-translations PLAN MANIFEST_SHA256 SELECTION REFERENCES REFERENCES_SHA256 [RECOGNIZED RECOGNIZED_SHA256]
  hosted-eval score-translations SELECTION REFERENCES REFERENCES_SHA256 CANDIDATES CANDIDATES_SHA256 OUT
  hosted-eval recognition-references SELECTION REFERENCES REFERENCES_SHA256 OUT
  hosted-eval collect-recognition PLAN MANIFEST_SHA256 SELECTION RECOGNITION_REFERENCES SHA256
  hosted-eval baseline-recognition RESULTS RESULTS_SHA256 PROFILE SELECTION RECOGNITION_REFERENCES SHA256 OUT_DIR
Only `key`, `reconcile` and `run` contact the network; only `run` makes paid requests.
The key is read from OPENROUTER_API_KEY in this process environment and never printed.";

const MAX_INPUT: u64 = 8 * 1024 * 1024;
const BATCH_DEADLINE: Duration = Duration::from_mins(90);

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    match dispatch(&arguments).await {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("hosted-eval: {}", outcome::label(&error.to_string(), 512));
            ExitCode::FAILURE
        }
    }
}

fn text(argument: &OsString) -> Result<&str> {
    argument
        .to_str()
        .ok_or_else(|| "arguments must be Unicode".into())
}

fn selection(path: &OsString) -> Result<Selection> {
    Ok(Selection::from_frozen_bytes(&files::read(
        Path::new(path),
        MAX_INPUT,
    )?)?)
}

fn pinned_json<T: serde::de::DeserializeOwned>(path: &OsString, digest: &OsString) -> Result<T> {
    Ok(serde_json::from_slice(&files::pinned(
        Path::new(path),
        text(digest)?,
        MAX_INPUT,
    )?)?)
}

struct Priced {
    route: plan::Route,
    route_sha256: String,
    catalog: Vec<u8>,
    endpoints: Vec<u8>,
}

fn priced(route: &OsString, catalog: &OsString, endpoints: &OsString) -> Result<Priced> {
    let route_bytes = files::read(Path::new(route), 64 * 1024)?;
    Ok(Priced {
        route: serde_json::from_slice(&route_bytes)?,
        route_sha256: sha256(&route_bytes),
        catalog: files::read(Path::new(catalog), 16 * 1024 * 1024)?,
        endpoints: files::read(Path::new(endpoints), MAX_INPUT)?,
    })
}

fn write_plan(
    priced: &Priced,
    task: plan::Task,
    mut inputs: BTreeMap<String, String>,
    instruction: &str,
    drafts: Vec<plan::Draft>,
    out: &OsString,
) -> Result<Value> {
    inputs.insert("route".into(), priced.route_sha256.clone());
    let (manifest, digest) = plan::write(
        &plan::Inputs {
            task,
            route: &priced.route,
            catalog: &priced.catalog,
            endpoints: &priced.endpoints,
            digests: inputs,
            instruction,
        },
        drafts,
        Path::new(out),
    )?;
    Ok(json!({
        "manifest_sha256": digest,
        "requests": manifest.requests.len(),
        "sum_reservation_usd": money::usd(manifest.sum_reservation_micro),
        "max_reservation_usd": money::usd(manifest.max_reservation_micro),
        "sum_hard_bound_usd": money::usd(manifest.sum_hard_bound_micro),
        "max_price_per_million": [manifest.max_price_prompt_per_million, manifest.max_price_completion_per_million],
        "matched_endpoints": manifest.matched_endpoints,
    }))
}

fn status(ledger: &ledger::Ledger) -> Value {
    let batches: BTreeMap<_, _> = ledger
        .state
        .allocations
        .keys()
        .map(|batch| {
            let totals = ledger.state.totals(Some(batch));
            (
                batch.clone(),
                json!({
                    "allocated_usd": money::usd(totals.allocated),
                    "settled_usd": money::usd(totals.settled),
                    "outstanding_usd": money::usd(totals.outstanding),
                    "uncertain_requests": totals.uncertain_requests,
                }),
            )
        })
        .collect();
    let totals = ledger.state.totals(None);
    json!({
        "software_cap_usd": money::usd(ledger.state.software_cap),
        "ceiling_usd": money::usd(ledger.state.ceiling),
        "settled_usd": money::usd(totals.settled),
        "outstanding_usd": money::usd(totals.outstanding),
        "uncertain_requests": totals.uncertain_requests,
        "breaches": ledger.state.breaches.len(),
        "key_checks": ledger.state.key_checks,
        "batches": batches,
    })
}

/// Labelled candidate envelopes and their pinned digests.
type Runs = (Vec<(String, Candidates)>, BTreeMap<String, String>);

fn labelled_runs(arguments: &[OsString]) -> Result<Runs> {
    let mut runs = Vec::new();
    let mut inputs = BTreeMap::new();
    for argument in arguments {
        let mut parts = text(argument)?.splitn(3, '=');
        let (Some(label), Some(path), Some(digest)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err("output runs are LABEL=PATH=SHA256".into());
        };
        let bytes = files::pinned(Path::new(path), digest, MAX_INPUT)?;
        runs.push((label.to_owned(), serde_json::from_slice(&bytes)?));
        inputs.insert(format!("candidates:{label}"), digest.to_owned());
    }
    if runs.is_empty() {
        return Err("at least one output run is required".into());
    }
    Ok((runs, inputs))
}

async fn dispatch(arguments: &[OsString]) -> Result<Value> {
    let Some((command, rest)) = arguments.split_first() else {
        return Err(USAGE.into());
    };
    let command = text(command)?;
    if command.starts_with("plan-") {
        return planning(command, rest);
    }
    if matches!(
        command,
        "collect-controls"
            | "collect-outputs"
            | "collect-translations"
            | "score-translations"
            | "recognition-references"
            | "collect-recognition"
            | "baseline-recognition"
    ) {
        return collection(command, rest);
    }
    ledger_command(command, rest).await
}

async fn ledger_command(command: &str, rest: &[OsString]) -> Result<Value> {
    match (command, rest) {
        ("init", [path]) => {
            let ledger = ledger::Ledger::create(
                Path::new(path),
                money::SOFTWARE_CAP_MICRO,
                money::CEILING_MICRO,
                "2026-10-02 hosted evaluation: USD 20 recorded allocation; this tool caps itself at USD 18 including uncertain liabilities",
            )?;
            Ok(status(&ledger))
        }
        ("allocate", [path, batch, micro, note]) => {
            let mut ledger = ledger::Ledger::open(Path::new(path))?;
            ledger.append(ledger::Event::Allocate {
                batch: text(batch)?.into(),
                micro: text(micro)?.parse()?,
                note: outcome::label(text(note)?, 300),
            })?;
            Ok(status(&ledger))
        }
        ("status", [path]) => Ok(status(&ledger::Ledger::open(Path::new(path))?)),
        ("key", [path]) => {
            let mut ledger = ledger::Ledger::open(Path::new(path))?;
            let transport = client::OpenRouter::production(
                client::Secret::from_environment()?,
                Duration::from_secs(30),
            )?;
            Ok(serde_json::to_value(
                run::key_status(&transport, &mut ledger).await?,
            )?)
        }
        ("reconcile", [path]) => {
            let mut ledger = ledger::Ledger::open(Path::new(path))?;
            let transport = client::OpenRouter::production(
                client::Secret::from_environment()?,
                Duration::from_secs(30),
            )?;
            let settled = run::reconcile(&transport, &mut ledger).await?;
            Ok(json!({"settled": settled, "status": status(&ledger)}))
        }
        ("run", [path, batch, directory, digest]) => {
            let directory = PathBuf::from(directory);
            let (manifest, _) = plan::Manifest::load(&directory, text(digest)?)?;
            let secret = client::Secret::from_environment()?;
            let mut ledger = ledger::Ledger::open(Path::new(path))?;
            let transport = client::OpenRouter::production(secret, run::REQUEST_TIMEOUT)?;
            let summary = run::run(
                &transport,
                &directory,
                &manifest,
                text(digest)?,
                &mut ledger,
                &run::Options {
                    batch: text(batch)?.into(),
                    deadline: Instant::now() + BATCH_DEADLINE,
                    max_consecutive_failures: 3,
                },
            )
            .await?;
            Ok(json!({"summary": summary, "status": status(&ledger)}))
        }
        _ => Err(USAGE.into()),
    }
}

fn planning(command: &str, rest: &[OsString]) -> Result<Value> {
    match (command, rest) {
        ("plan-probe", [route, catalog, endpoints, manifest, audio, out, mode]) => {
            let priced = priced(route, catalog, endpoints)?;
            let (task, instruction, draft) =
                plan::probe(text(mode)?, &selection(manifest)?, Path::new(audio))?;
            write_plan(
                &priced,
                task,
                BTreeMap::new(),
                instruction,
                vec![draft],
                out,
            )
        }
        ("plan-controls", [route, catalog, endpoints, manifest, controls, digest, out]) => {
            let priced = priced(route, catalog, endpoints)?;
            let controls_set: CalibrationSet = pinned_json(controls, digest)?;
            let drafts = plan::judge_controls(&selection(manifest)?, &controls_set, &priced.route)?;
            let inputs = BTreeMap::from([("controls".into(), text(digest)?.into())]);
            write_plan(
                &priced,
                plan::Task::JudgeControls,
                inputs,
                plan::RUBRIC,
                drafts,
                out,
            )
        }
        (
            "plan-outputs",
            [
                route,
                catalog,
                endpoints,
                manifest,
                references,
                digest,
                out,
                runs @ ..,
            ],
        ) => {
            let priced = priced(route, catalog, endpoints)?;
            let references_set: References = pinned_json(references, digest)?;
            let (runs, mut inputs) = labelled_runs(runs)?;
            let drafts =
                plan::judge_outputs(&selection(manifest)?, &references_set, &runs, &priced.route)?;
            inputs.insert("references".into(), text(digest)?.into());
            write_plan(
                &priced,
                plan::Task::JudgeOutputs,
                inputs,
                plan::RUBRIC,
                drafts,
                out,
            )
        }
        _ => planning_inputs(command, rest),
    }
}

fn planning_inputs(command: &str, rest: &[OsString]) -> Result<Value> {
    match (command, rest) {
        (
            "plan-translate",
            [
                route,
                catalog,
                endpoints,
                manifest,
                references,
                digest,
                out,
                recognized @ ..,
            ],
        ) => {
            let priced = priced(route, catalog, endpoints)?;
            let references_set: References = pinned_json(references, digest)?;
            let mut inputs = BTreeMap::from([("references".into(), text(digest)?.into())]);
            let recognized: Option<Candidates> = match recognized {
                [] => None,
                [path, recognized_digest] => {
                    inputs.insert("recognized".into(), text(recognized_digest)?.into());
                    Some(pinned_json(path, recognized_digest)?)
                }
                _ => return Err(USAGE.into()),
            };
            let drafts =
                plan::translate(&selection(manifest)?, &references_set, recognized.as_ref())?;
            write_plan(
                &priced,
                plan::Task::Translate,
                inputs,
                plan::TRANSLATION_INSTRUCTION,
                drafts,
                out,
            )
        }
        (
            "plan-recognize",
            [
                route,
                catalog,
                endpoints,
                manifest,
                audio,
                out,
                configs @ ..,
            ],
        ) => {
            let priced = priced(route, catalog, endpoints)?;
            let configs: Vec<String> = configs
                .iter()
                .map(|config| text(config).map(str::to_owned))
                .collect::<Result<_>>()?;
            let drafts = plan::recognize(&selection(manifest)?, Path::new(audio), &configs)?;
            let inputs = drafts
                .iter()
                .filter_map(|draft| {
                    draft
                        .item
                        .audio_sha256
                        .clone()
                        .map(|digest| (format!("audio:{}", draft.item.clip_id), digest))
                })
                .collect();
            write_plan(
                &priced,
                plan::Task::Recognize,
                inputs,
                plan::TRANSCRIPTION_INSTRUCTION,
                drafts,
                out,
            )
        }
        _ => Err(USAGE.into()),
    }
}

fn collection(command: &str, rest: &[OsString]) -> Result<Value> {
    match (command, rest) {
        ("collect-controls", [directory, digest, manifest, controls, controls_digest]) => {
            let controls_set: CalibrationSet = pinned_json(controls, controls_digest)?;
            collect::controls(
                Path::new(directory),
                text(digest)?,
                &selection(manifest)?,
                &controls_set,
                text(controls_digest)?,
            )
        }
        ("collect-outputs", [directory, digest]) => {
            collect::outputs(Path::new(directory), text(digest)?)
        }
        (
            "collect-translations",
            [
                directory,
                digest,
                manifest,
                references,
                references_digest,
                recognized @ ..,
            ],
        ) => {
            let references_set: References = pinned_json(references, references_digest)?;
            let origin = match recognized {
                [] => InputOrigin::ReferenceText {},
                [path, recognized_digest] => {
                    pinned_json::<Candidates>(path, recognized_digest)?.input_origin
                }
                _ => return Err(USAGE.into()),
            };
            collect::translations(
                Path::new(directory),
                text(digest)?,
                &selection(manifest)?,
                &references_set,
                text(references_digest)?,
                &origin,
            )
        }
        (
            "score-translations",
            [
                manifest,
                references,
                references_digest,
                candidates,
                candidates_digest,
                out,
            ],
        ) => {
            let references_set: References = pinned_json(references, references_digest)?;
            let bytes = files::pinned(Path::new(candidates), text(candidates_digest)?, MAX_INPUT)?;
            collect::score_translation_envelope(
                &selection(manifest)?,
                &references_set,
                text(references_digest)?,
                &bytes,
                Path::new(out),
            )
        }
        _ => recognition(command, rest),
    }
}

fn recognition(command: &str, rest: &[OsString]) -> Result<Value> {
    match (command, rest) {
        ("recognition-references", [manifest, references, digest, out]) => {
            let references_set: References = pinned_json(references, digest)?;
            let envelope = collect::recognition_references(&selection(manifest)?, &references_set)?;
            let bytes = serde_json::to_vec_pretty(&envelope)?;
            files::write_new(Path::new(out), &bytes)?;
            Ok(json!({"sha256": sha256(&bytes)}))
        }
        ("collect-recognition", [directory, digest, manifest, references, references_digest]) => {
            let bytes = files::pinned(Path::new(references), text(references_digest)?, MAX_INPUT)?;
            collect::recognition(
                Path::new(directory),
                text(digest)?,
                &selection(manifest)?,
                &bytes,
                text(references_digest)?,
            )
        }
        (
            "baseline-recognition",
            [
                results,
                results_digest,
                profile,
                manifest,
                references,
                references_digest,
                out,
            ],
        ) => {
            let results = files::pinned(Path::new(results), text(results_digest)?, MAX_INPUT)?;
            let bytes = files::pinned(Path::new(references), text(references_digest)?, MAX_INPUT)?;
            std::fs::create_dir(Path::new(out))?;
            collect::baseline(
                &results,
                text(profile)?,
                &selection(manifest)?,
                &bytes,
                text(references_digest)?,
                Path::new(out),
            )
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::tests::scratch;

    fn os(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn judge_parser_is_the_frozen_local_parser_byte_for_byte() {
        let frozen = include_bytes!("../../local-mt/judge-runner/src/response.rs");
        let copy = include_bytes!("response.rs");
        assert_eq!(sha256(copy), sha256(frozen));
    }

    #[tokio::test]
    async fn offline_commands_manage_the_ledger_and_refuse_bad_usage() -> Result<()> {
        let directory = scratch("cli")?;
        let path = directory.join("ledger.jsonl");
        let path_text = path.to_str().ok_or("path")?;
        let created = dispatch(&os(&["init", path_text])).await?;
        assert_eq!(created["software_cap_usd"], "18.000000");
        let allocated = dispatch(&os(&[
            "allocate",
            path_text,
            "judge",
            "6000000",
            "judge calibration",
        ]))
        .await?;
        assert_eq!(allocated["batches"]["judge"]["allocated_usd"], "6.000000");
        assert!(
            dispatch(&os(&["allocate", path_text, "big", "13000000", "too much"]))
                .await
                .is_err()
        );
        assert_eq!(
            dispatch(&os(&["status", path_text])).await?["settled_usd"],
            "0.000000"
        );
        assert!(dispatch(&os(&[])).await.is_err());
        assert!(dispatch(&os(&["unknown"])).await.is_err());
        assert!(
            dispatch(&os(&["plan-outputs", "r", "c", "e", "m", "x", "y", "o"]))
                .await
                .is_err()
        );
        assert!(labelled_runs(&os(&["bad"])).is_err());
        assert!(labelled_runs(&[]).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn offline_planning_and_outputs_collection_round_trip() -> Result<()> {
        let directory = scratch("cli-plan")?;
        let route = directory.join("route.json");
        std::fs::write(&route, serde_json::to_vec(&plan::tests::route())?)?;
        let catalog = directory.join("catalog.json");
        std::fs::write(&catalog, plan::tests::catalog())?;
        let endpoints = directory.join("endpoints.json");
        std::fs::write(&endpoints, plan::tests::endpoints())?;
        let priced = priced(
            &route.clone().into_os_string(),
            &catalog.into_os_string(),
            &endpoints.into_os_string(),
        )?;
        let out: OsString = directory.join("plan").into_os_string();
        let value = write_plan(
            &priced,
            plan::Task::JudgeOutputs,
            BTreeMap::new(),
            plan::RUBRIC,
            vec![plan::tests::draft("a", true)],
            &out,
        )?;
        let digest = value["manifest_sha256"]
            .as_str()
            .ok_or("digest")?
            .to_owned();
        let report = dispatch(&os(&[
            "collect-outputs",
            out.to_str().ok_or("path")?,
            &digest,
        ]))
        .await?;
        assert_eq!(report["by_run"]["fixture"]["total"]["abstained"], 1);
        assert_eq!(
            report["abstention_reasons"]["unattempted-or-batch-stopped"],
            1
        );
        Ok(())
    }
}
