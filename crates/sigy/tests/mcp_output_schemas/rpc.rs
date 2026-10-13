use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn call(name: &str, arguments: Value) -> Value {
    let mut request = json!({"method":"tools/call","params":{"name":name}});
    request["params"]["arguments"] = arguments;
    request
}

pub fn exchange(library: &Path, requests: &[Value]) -> Result<Vec<Value>> {
    let mut input = tempfile::tempfile()?;
    for (id, request) in requests.iter().enumerate() {
        let mut request = request.clone();
        request["jsonrpc"] = json!("2.0");
        request["id"] = json!(id);
        serde_json::to_writer(&mut input, &request)?;
        writeln!(input)?;
    }
    input.rewind()?;
    let mut output = tempfile::tempfile()?;
    let mut errors = tempfile::tempfile()?;
    let mut child = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(library)
            .arg("mcp")
            .stdin(Stdio::from(input))
            .stdout(output.try_clone()?)
            .stderr(errors.try_clone()?)
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            output.metadata()?.len() < 4 * 1024 * 1024,
            "bounded MCP output"
        );
        assert!(
            errors.metadata()?.len() < 256 * 1024,
            "bounded MCP diagnostics"
        );
        if let Some(status) = child.0.try_wait()? {
            let diagnostic = read_bounded(&mut errors, 256 * 1024)?;
            assert!(status.success(), "MCP exited: {diagnostic}");
            break;
        }
        assert!(Instant::now() < deadline, "MCP deadline exceeded");
        thread::sleep(Duration::from_millis(10));
    }
    let contents = read_bounded(&mut output, 4 * 1024 * 1024)?;
    let responses = contents
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<Vec<Value>, _>>()?;
    assert_eq!(responses.len(), requests.len());
    for (id, response) in responses.iter().enumerate() {
        assert_eq!(response["id"], json!(id));
        assert!(response["error"].is_null(), "{response}");
    }
    Ok(responses)
}

fn read_bounded(file: &mut File, limit: u64) -> Result<String> {
    file.rewind()?;
    let mut contents = String::new();
    file.take(limit + 1).read_to_string(&mut contents)?;
    assert!(
        u64::try_from(contents.len())? <= limit,
        "bounded final MCP output"
    );
    Ok(contents)
}

pub fn schemas(library: &Path) -> Result<Value> {
    let responses = exchange(
        library,
        &[
            json!({"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
            json!({"method":"tools/list"}),
        ],
    )?;
    assert_eq!(responses[0]["result"]["supportedVersions"][0], "2026-07-28");
    let tools = responses[1]["result"]["tools"]
        .as_array()
        .ok_or("tools list")?;
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool.get("outputSchema").is_some())
            .count(),
        6
    );
    assert!(
        serde_json::to_vec(&responses[1])?.len() < 262_144,
        "bounded discovery response"
    );
    Ok(responses[1]["result"].clone())
}
