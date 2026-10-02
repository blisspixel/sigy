//! `analysis search` from the command line and through `sigy mcp`, on a real catalog.
//! Fixtures with stored transcripts live in the service crate; these check the boundary.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Output, Stdio},
};
mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn invoke(library: &Path, json: bool, args: &[&str]) -> std::io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sigy"));
    command.arg("--data-dir").arg(library);
    if json {
        command.arg("--json");
    }
    common::output(command.args(args))
}

fn catalog_bytes(library: &Path) -> std::io::Result<Vec<(String, u64)>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(library)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            files.push((
                entry.file_name().to_string_lossy().into_owned(),
                entry.metadata()?.len(),
            ));
        }
    }
    files.sort();
    Ok(files)
}

#[test]
fn an_empty_library_search_is_exact_json_plain_text_and_writes_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("library");
    assert!(
        invoke(&library, true, &["library", "init"])?
            .status
            .success()
    );
    let before = catalog_bytes(&library)?;
    let output = invoke(
        &library,
        true,
        &[
            "analysis",
            "search",
            "--term",
            "-سد",
            "--in",
            "original",
            "--history",
            "--language",
            "FR-ca",
            "--limit",
            "3",
        ],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let page = &value["recognition"]["page"];
    assert_eq!(value["recognition"]["kind"], "search");
    assert_eq!(page["hits"], serde_json::json!([]));
    assert_eq!(page["stopped"], serde_json::Value::Null);
    assert_eq!(page["next"], serde_json::Value::Null);
    assert_eq!(page["rows_scanned"], 0);
    assert_eq!(page["query"]["term"], "-سد");
    assert_eq!(page["query"]["fields"], "original");
    assert_eq!(page["query"]["revisions"], "all");
    assert_eq!(page["query"]["language"], "fr-CA");
    assert_eq!(page["query"]["limit"], 3);
    assert_eq!(page["query"]["scan_rows"], 20_000);
    assert_eq!(page["query"]["deadline_ms"], 1_000);
    let human = invoke(&library, false, &["analysis", "search", "--term", "presa"])?;
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout)?;
    assert!(text.contains("Archive search for \"presa\""), "{text}");
    assert!(text.contains("No hits."), "{text}");
    assert!(
        text.contains("The scan reached the end of the catalog for this query."),
        "{text}"
    );
    for refused in [
        vec!["analysis", "search", "--term", "presa", "--limit", "0"],
        vec!["analysis", "search", "--term", "presa", "--scan-rows", "1"],
        vec![
            "analysis",
            "search",
            "--term",
            "presa",
            "--deadline-ms",
            "5000",
        ],
        vec!["analysis", "search", "--term", " presa"],
        vec![
            "analysis",
            "search",
            "--term",
            "presa",
            "--after",
            "pin/0/0/0",
        ],
        vec![
            "analysis",
            "search",
            "--term",
            "presa",
            "--language",
            "fr_CA",
        ],
        vec!["analysis", "search", "--term", "presa", "--from-ms", "10"],
        vec!["analysis", "search", "--term", "presa", "--in", "titles"],
    ] {
        let output = invoke(&library, true, &refused)?;
        assert!(!output.status.success(), "{refused:?}");
        assert!(output.stdout.is_empty(), "{refused:?}");
    }
    assert_eq!(catalog_bytes(&library)?, before);
    Ok(())
}

#[test]
fn the_agent_tool_searches_only_the_startup_library() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("library");
    let elsewhere = directory.path().join("elsewhere");
    assert!(
        invoke(&library, true, &["library", "init"])?
            .status
            .success()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_sigy"))
        .arg("--data-dir")
        .arg(&library)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let stdout = child.stdout.take().ok_or("stdout")?;
    let term = format!("--data-dir={}", elsewhere.display());
    for (id, arguments) in [
        (1, serde_json::json!({"term": term})),
        (
            2,
            serde_json::json!({"term": "presa", "in": "english", "limit": 2, "after": "pin/1/0/3"}),
        ),
        (
            3,
            serde_json::json!({"term": "presa", "data_dir": "elsewhere"}),
        ),
    ] {
        serde_json::to_writer(
            &mut stdin,
            &serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": "analysis_search", "arguments": arguments}}),
        )?;
        stdin.write_all(b"\n")?;
    }
    stdin.flush()?;
    drop(stdin);
    let replies = BufReader::new(stdout)
        .lines()
        .collect::<Result<Vec<_>, _>>()?;
    assert!(child.wait()?.success());
    assert_eq!(replies.len(), 3);
    let first: serde_json::Value = serde_json::from_str(&replies[0])?;
    assert_eq!(first["result"]["isError"], false, "{first}");
    let page = &first["result"]["structuredContent"]["recognition"]["page"];
    assert_eq!(page["query"]["term"], term);
    assert_eq!(page["hits"], serde_json::json!([]));
    let second: serde_json::Value = serde_json::from_str(&replies[1])?;
    let page = &second["result"]["structuredContent"]["recognition"]["page"];
    assert_eq!(page["query"]["fields"], "english");
    assert_eq!(page["query"]["limit"], 2);
    assert_eq!(page["query"]["after"], "pin/1/0/3");
    let third: serde_json::Value = serde_json::from_str(&replies[2])?;
    assert_eq!(third["result"]["isError"], true);
    assert!(!elsewhere.exists());
    Ok(())
}
