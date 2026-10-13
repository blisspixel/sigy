//! Exercise the schemas advertised by the actual stdio server against CLI results.
mod common;
#[path = "mcp_output_schemas/fixture.rs"]
mod fixture;
#[path = "mcp_output_schemas/rpc.rs"]
mod rpc;

use std::{path::Path, process::Command};

use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Case {
    tool: &'static str,
    arguments: Value,
    cli: Vec<&'static str>,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            tool: "doctor",
            arguments: json!({}),
            cli: vec!["doctor"],
        },
        Case {
            tool: "analysis_search",
            arguments: json!({"term":"feria"}),
            cli: vec!["analysis", "search", "--term", "feria"],
        },
        Case {
            tool: "analysis_transcript",
            arguments: json!({"id":"pin"}),
            cli: vec!["analysis", "transcript", "pin"],
        },
        Case {
            tool: "analysis_translation",
            arguments: json!({"id":"pin"}),
            cli: vec!["analysis", "translation", "pin"],
        },
        Case {
            tool: "monitor_matches",
            arguments: json!({"id":"fair","from_ms":0,"to_ms":2000}),
            cli: vec![
                "monitor",
                "matches",
                "fair",
                "--from-ms",
                "0",
                "--to-ms",
                "2000",
            ],
        },
        Case {
            tool: "monitor_coverage",
            arguments: json!({"id":"fair","from_ms":0,"to_ms":2000}),
            cli: vec![
                "monitor",
                "coverage",
                "fair",
                "--from-ms",
                "0",
                "--to-ms",
                "2000",
            ],
        },
    ]
}

fn invoke(library: &Path, arguments: &[&str]) -> TestResult<(bool, Value)> {
    let output = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(library)
            .arg("--json")
            .args(arguments),
    )?;
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let value = serde_json::from_slice(bytes)
        .map_err(|error| format!("{error}: {}", String::from_utf8_lossy(bytes)))?;
    Ok((output.status.success(), value))
}

fn validator(list: &Value, tool: &str) -> TestResult<jsonschema::Validator> {
    let schema = &list["tools"]
        .as_array()
        .ok_or("tool list")?
        .iter()
        .find(|entry| entry["name"] == tool)
        .ok_or("declared tool")?["outputSchema"];
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_local_refs(schema);
    Ok(jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(schema)?)
}

fn assert_local_refs(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "$ref" {
                    assert!(
                        child
                            .as_str()
                            .is_some_and(|reference| reference.starts_with('#'))
                    );
                }
                assert_local_refs(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                assert_local_refs(child);
            }
        }
        _ => {}
    }
}

fn assert_valid(validator: &jsonschema::Validator, value: &Value, tool: &str) {
    let errors = validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{tool}: {errors:?}\n{value}");
}

#[test]
fn populated_responses_validate_and_equal_cli_json() -> TestResult {
    let directory = tempfile::tempdir()?;
    assert!(invoke(directory.path(), &["library", "init"])?.0);
    fixture::populate(directory.path())?;
    let list = rpc::schemas(directory.path())?;
    let cases = cases();
    let requests = cases
        .iter()
        .map(|case| rpc::call(case.tool, case.arguments.clone()))
        .collect::<Vec<_>>();
    let responses = rpc::exchange(directory.path(), &requests)?;
    for (case, response) in cases.iter().zip(responses) {
        let result = &response["result"];
        let value = &result["structuredContent"];
        let validator = validator(&list, case.tool)?;
        assert_valid(&validator, value, case.tool);
        let (success, cli) = invoke(directory.path(), &case.cli)?;
        assert_eq!(result["isError"], !success, "{}", case.tool);
        assert_eq!(value, &cli, "{} CLI parity", case.tool);
        if case.tool == "doctor" {
            assert_doctor(value, success)?;
        } else {
            assert!(success, "{}: {value}", case.tool);
        }
        assert_evidence(case.tool, value)?;
        assert_negative(case.tool, value, &validator)?;
    }
    Ok(())
}

fn assert_doctor(value: &Value, success: bool) -> TestResult {
    if cfg!(windows) {
        assert!(success, "Windows containment supports doctor: {value}");
    }
    if !success {
        let checks = value["doctor"]["checks"]
            .as_array()
            .ok_or("doctor checks")?;
        assert!(
            checks
                .iter()
                .any(|check| check["name"] == "decoder_containment" && check["state"] == "blocked"),
            "{value}"
        );
        assert!(
            checks
                .iter()
                .filter(|check| check["state"] == "blocked")
                .all(|check| check["name"] == "decoder_containment")
        );
    }
    Ok(())
}

fn assert_evidence(tool: &str, value: &Value) -> TestResult {
    let recognition = &value["recognition"]["page"];
    match tool {
        "analysis_search" => assert_eq!(recognition["hits"].as_array().ok_or("hits")?.len(), 2),
        "analysis_transcript" => {
            assert_eq!(recognition["cues"][0]["script"], "Una feria mundial");
            assert_eq!(
                recognition["coverages"].as_array().ok_or("coverage")?.len(),
                1
            );
        }
        "analysis_translation" => {
            assert_eq!(recognition["pairs"][0]["english"], "A world fair");
            assert_eq!(recognition["pairs"][1]["reason"], "unsupported-language");
        }
        "monitor_matches" => {
            let matches = value["monitor"]["matches"]["matches"]
                .as_array()
                .ok_or("matches")?;
            assert_eq!(matches.len(), 3);
            for passage in matches {
                assert_eq!(passage["recording_id"], "one");
                assert_eq!(passage["transcript_revision"], 1);
                assert_eq!(passage["translation_revision"], 1);
            }
        }
        "monitor_coverage" => {
            let source = &value["monitor"]["coverage"]["sources"][0];
            assert_eq!(source["translated_cues"], 1);
            assert_eq!(
                source["untranslated_reasons"],
                json!([["unsupported-language", 1]])
            );
        }
        _ => {}
    }
    Ok(())
}

fn assert_negative(tool: &str, value: &Value, validator: &jsonschema::Validator) -> TestResult {
    let pointer = match tool {
        "doctor" => "/doctor/checks/0/state",
        "analysis_search" => "/recognition/page/hits/0/transcript_revision",
        "analysis_transcript" => "/recognition/page/cues/0/start_us",
        "analysis_translation" => "/recognition/page/pairs/0/english",
        "monitor_matches" => "/monitor/matches/matches/0/transcript_revision",
        "monitor_coverage" => "/monitor/coverage/sources/0/untranslated_reasons/0/1",
        _ => return Err("unknown tool".into()),
    };
    let mut mutated = value.clone();
    *mutated
        .pointer_mut(pointer)
        .ok_or_else(|| format!("missing {pointer}"))? = json!(false);
    assert!(
        !validator.is_valid(&mutated),
        "{tool} rejects malformed evidence"
    );
    let mut mutated = value.clone();
    mutated
        .as_object_mut()
        .ok_or("snapshot object")?
        .remove("budgets");
    assert!(
        !validator.is_valid(&mutated),
        "{tool} requires common snapshot fields"
    );
    let mut mutated = value.clone();
    mutated["undeclared"] = json!(true);
    assert!(
        !validator.is_valid(&mutated),
        "{tool} rejects unknown properties"
    );
    Ok(())
}

#[test]
fn catalog_failures_and_argument_errors_validate() -> TestResult {
    let directory = tempfile::tempdir()?;
    let list = rpc::schemas(directory.path())?;
    std::fs::write(directory.path().join("catalog.sqlite3"), b"truncated")?;
    for case in cases() {
        let validator = validator(&list, case.tool)?;
        let responses = rpc::exchange(
            directory.path(),
            &[
                rpc::call(case.tool, case.arguments),
                rpc::call(case.tool, json!({"unexpected":true})),
            ],
        )?;
        for response in &responses {
            assert_eq!(response["result"]["isError"], true);
            assert_valid(
                &validator,
                &response["result"]["structuredContent"],
                case.tool,
            );
            assert!(response["result"]["structuredContent"]["error"].is_string());
        }
        let (success, cli) = invoke(directory.path(), &case.cli)?;
        assert!(!success);
        assert_eq!(responses[0]["result"]["structuredContent"], cli);
        assert!(!validator.is_valid(&json!({"error":7})));
    }
    Ok(())
}

#[test]
fn rate_limit_errors_validate_without_command_dispatch() -> TestResult {
    let directory = tempfile::tempdir()?;
    let list = rpc::schemas(directory.path())?;
    let validator = validator(&list, "analysis_search")?;
    let responses = rpc::exchange(
        directory.path(),
        &vec![rpc::call("analysis_search", json!({})); 61],
    )?;
    for response in &responses {
        assert_eq!(response["result"]["isError"], true);
        assert_valid(
            &validator,
            &response["result"]["structuredContent"],
            "analysis_search",
        );
    }
    assert!(
        responses[60]["result"]["structuredContent"]["error"]
            .as_str()
            .ok_or("error")?
            .contains("rate limit")
    );
    assert!(!directory.path().join("catalog.sqlite3").exists());
    Ok(())
}

#[test]
fn empty_and_cursor_variants_validate_against_discovery() -> TestResult {
    let directory = tempfile::tempdir()?;
    assert!(invoke(directory.path(), &["library", "init"])?.0);
    fixture::populate(directory.path())?;
    let list = rpc::schemas(directory.path())?;
    for (tool, command) in [
        ("analysis_transcript", "transcript"),
        ("analysis_translation", "translation"),
    ] {
        let validator = validator(&list, tool)?;
        let responses = rpc::exchange(
            directory.path(),
            &[
                rpc::call(tool, json!({"id":"absent"})),
                rpc::call(tool, json!({"id":"pin","after":0})),
            ],
        )?;
        for response in &responses {
            assert_eq!(response["result"]["isError"], false);
            assert_valid(&validator, &response["result"]["structuredContent"], tool);
        }
        let empty = &responses[0]["result"]["structuredContent"];
        assert_eq!(empty["recognition"], json!({"kind":"empty","id":"absent"}));
        assert_eq!(
            *empty,
            invoke(directory.path(), &["analysis", command, "absent"])?.1
        );
        let remainder = &responses[1]["result"]["structuredContent"];
        assert_eq!(
            *remainder,
            invoke(
                directory.path(),
                &["analysis", command, "pin", "--after", "0"]
            )?
            .1
        );
        let collection = if tool == "analysis_transcript" {
            "cues"
        } else {
            "pairs"
        };
        let page = &remainder["recognition"]["page"];
        assert_eq!(page[collection].as_array().ok_or("remainder")?.len(), 1);
        assert_eq!(page[collection][0]["ordinal"], 1);
        assert!(page["next_after_ordinal"].is_null());
    }
    assert_search_cursor(directory.path(), &list)?;
    Ok(())
}

fn assert_search_cursor(library: &Path, list: &Value) -> TestResult {
    let validator = validator(list, "analysis_search")?;
    let first = rpc::exchange(
        library,
        &[rpc::call(
            "analysis_search",
            json!({"term":"feria","limit":1}),
        )],
    )?;
    let value = &first[0]["result"]["structuredContent"];
    assert_eq!(first[0]["result"]["isError"], false);
    assert_valid(&validator, value, "analysis_search");
    assert_eq!(
        *value,
        invoke(
            library,
            &["analysis", "search", "--term", "feria", "--limit", "1"]
        )?
        .1
    );
    let page = &value["recognition"]["page"];
    assert_eq!(page["hits"].as_array().ok_or("first page")?.len(), 1);
    let next = page["next"].as_str().ok_or("next cursor")?;
    let second = rpc::exchange(
        library,
        &[rpc::call(
            "analysis_search",
            json!({"term":"feria","limit":1,"after":next}),
        )],
    )?;
    let value = &second[0]["result"]["structuredContent"];
    assert_eq!(second[0]["result"]["isError"], false);
    assert_valid(&validator, value, "analysis_search");
    assert_eq!(
        *value,
        invoke(
            library,
            &[
                "analysis", "search", "--term", "feria", "--limit", "1", "--after", next
            ]
        )?
        .1
    );
    assert_eq!(value["recognition"]["page"]["hits"][0]["cue_ordinal"], 1);
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a systemd user manager with cpu/memory/pids delegation"]
fn doctor_ready_response_validates_with_delegated_containment() -> TestResult {
    let directory = tempfile::tempdir()?;
    assert!(invoke(directory.path(), &["library", "init"])?.0);
    let _cleanup = ServiceCleanup(directory.path().to_owned());
    assert!(invoke(directory.path(), &["service", "start"])?.0);
    let list = rpc::schemas(directory.path())?;
    let validator = validator(&list, "doctor")?;
    let responses = rpc::exchange(directory.path(), &[rpc::call("doctor", json!({}))])?;
    let result = &responses[0]["result"];
    assert_eq!(result["isError"], false, "{result}");
    assert_valid(&validator, &result["structuredContent"], "doctor");
    let (success, mut cli) = invoke(directory.path(), &["doctor"])?;
    assert!(success, "{cli}");
    assert_valid(&validator, &cli, "doctor CLI");
    let mcp_service = &result["structuredContent"]["service"];
    assert_eq!(mcp_service["process_id"], cli["service"]["process_id"]);
    let earlier = mcp_service["uptime_seconds"]
        .as_u64()
        .ok_or("MCP service uptime")?;
    let later = cli["service"]["uptime_seconds"]
        .as_u64()
        .ok_or("CLI service uptime")?;
    assert!(later >= earlier, "the same service's uptime is monotonic");
    // The actor stamps elapsed uptime on each read. Keep every other field exact.
    cli["service"]["uptime_seconds"] = json!(earlier);
    assert_eq!(result["structuredContent"], cli);
    assert!(invoke(directory.path(), &["service", "stop"])?.0);
    wait_for_service_close(directory.path())?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn wait_for_service_close(directory: &Path) -> TestResult {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while sigy_service::library::Library::open(directory, false).is_err() {
        if std::time::Instant::now() >= deadline {
            return Err("service shutdown deadline".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct ServiceCleanup(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl Drop for ServiceCleanup {
    fn drop(&mut self) {
        let _ = invoke(&self.0, &["service", "stop"]);
        let _ = wait_for_service_close(&self.0);
    }
}
