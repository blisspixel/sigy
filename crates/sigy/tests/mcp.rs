use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn mcp_stdio_reads_one_library_and_rejects_another_directory() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("library");
    let init = Command::new(env!("CARGO_BIN_EXE_sigy"))
        .arg("--data-dir")
        .arg(&library)
        .arg("--json")
        .args(["library", "init"])
        .output()?;
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
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
    let mut lines = BufReader::new(stdout).lines();
    for line in [
        r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"library_status","arguments":{"data_dir":"elsewhere"}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"library_status","arguments":{}}}"#,
    ] {
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
    }
    stdin.flush()?;
    drop(stdin);
    let discover: serde_json::Value = serde_json::from_str(&lines.next().ok_or("discover")??)?;
    let rejected: serde_json::Value = serde_json::from_str(&lines.next().ok_or("rejected")??)?;
    let status: serde_json::Value = serde_json::from_str(&lines.next().ok_or("status")??)?;
    let _ = child.wait();
    if discover["result"]["supportedVersions"][0] != "2026-07-28" {
        return Err(discover.to_string().into());
    }
    let rejected_text = rejected["result"]["content"][0]["text"]
        .as_str()
        .ok_or("rejected text")?;
    if rejected["result"]["isError"] != true || !rejected_text.contains("unexpected") {
        return Err(rejected.to_string().into());
    }
    if status["result"]["isError"] != false
        || status["result"]["structuredContent"]["sqlite_version"].is_null()
    {
        return Err(status.to_string().into());
    }
    Ok(())
}
