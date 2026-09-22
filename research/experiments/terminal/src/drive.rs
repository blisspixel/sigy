//! Drives the probe through ConPTY and Windows Terminal.

use std::env;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};

use crate::textutil::has_color_sgr;

type MeasureResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub fn run(args: impl Iterator<Item = String>) -> MeasureResult<()> {
    let mut out = PathBuf::from("research/experiments/terminal/results/2026-09-21.json");
    let mut only = String::from("all");
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = PathBuf::from(args.next().ok_or("missing --out value")?);
            }
            "--only" => {
                only = args.next().ok_or("missing --only value")?;
            }
            other => return Err(format!("unknown measure argument {other}").into()),
        }
    }
    if !matches!(only.as_str(), "all" | "conpty" | "hosted") {
        return Err(format!("unknown --only {only}").into());
    }
    let exe = env::current_exe()?;
    let temp = env::temp_dir().join(format!("sigy-terminal-measure-{}", std::process::id()));
    fs::create_dir_all(&temp)?;
    let mut runs = Vec::new();
    if matches!(only.as_str(), "all" | "conpty") {
        for backend in ["crossterm", "termina"] {
            for repeat in 1..=3 {
                runs.push(conpty_workload(&exe, &temp, backend, repeat)?);
            }
            runs.push(conpty_simple(&exe, &temp, backend, "color", false, true)?);
            runs.push(conpty_simple(
                &exe, &temp, backend, "contrast", true, false,
            )?);
            runs.push(conpty_simple(&exe, &temp, backend, "modes", true, true)?);
            runs.push(conpty_panic(&exe, &temp, backend)?);
        }
    }
    if matches!(only.as_str(), "all" | "hosted") {
        for backend in ["crossterm", "termina"] {
            for (cols, rows, label) in [
                (80u16, 24u16, "80x24"),
                (120, 36, "120x36"),
                (200, 50, "large"),
            ] {
                runs.push(hosted_window(&exe, &temp, backend, cols, rows, label)?);
            }
        }
    }
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let document = json!({
        "date": "2026-09-21",
        "host": "Windows x86_64",
        "windows_terminal": "1.24.11911.0",
        "driver_wt_session": env::var_os("WT_SESSION").is_some(),
        "rustc": rustc_version(),
        "ratatui": "0.30.2",
        "crossterm": "0.29.0",
        "termina_linked_by_ratatui": "0.3",
        "runs": runs,
    });
    fs::write(&out, serde_json::to_string_pretty(&document)?)?;
    let failing = runs
        .iter()
        .filter(|run| run.get("pass").and_then(Value::as_bool) == Some(false))
        .count();
    println!(
        "wrote {} runs to {}; {failing} runs failed a candidate check",
        runs.len(),
        out.display()
    );
    let _ = fs::remove_dir_all(&temp);
    Ok(())
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .unwrap_or("unknown")
        .to_string()
}

fn conpty_workload(exe: &Path, temp: &Path, backend: &str, repeat: u32) -> MeasureResult<Value> {
    let report = temp.join(format!("workload-{backend}-{repeat}.jsonl"));
    let mut failures = Vec::new();
    let captured = match scripted(
        exe,
        &report,
        backend,
        "workload",
        true,
        true,
        Script::Workload,
    ) {
        Ok(value) => value,
        Err(error) => {
            let mut failures = vec![error.to_string()];
            let lines = read_lines(&report);
            expect_idle(&lines, &mut failures);
            expect_text(&lines, &mut failures);
            expect_modes(&lines, &mut failures);
            return Ok(json!({
                "backend": backend,
                "host": "conpty",
                "phase": "workload",
                "repeat": repeat,
                "pass": false,
                "failures": failures,
                "lines": lines,
            }));
        }
    };
    let lines = read_lines(&report);
    if let Some(error) = &captured.step_error {
        failures.push(error.clone());
    }
    expect_idle(&lines, &mut failures);
    expect_text(&lines, &mut failures);
    expect_modes(&lines, &mut failures);
    expect_done(&lines, &captured.status_ok, &mut failures);
    if has_color_sgr(&captured.output) {
        failures.push("NO_COLOR run still emitted a color SGR sequence".into());
    }
    let latencies = key_latencies(&lines);
    if latencies.len() < 6 {
        failures.push(format!("expected 6 search keys, saw {}", latencies.len()));
    }
    let paste = find_tag(&lines, "paste");
    match paste {
        Some(value) => {
            let query = value.get("query").and_then(Value::as_str).unwrap_or("");
            if !query.contains('q') || !query.contains('東') {
                failures.push(format!("paste query was {query}"));
            }
            if value.get("quit").and_then(Value::as_bool) != Some(false) {
                failures.push("paste was treated as quit".into());
            }
        }
        None => failures.push("paste event was not reported".into()),
    }
    expect_resize(&lines, 120, 36, &mut failures);
    expect_resize(&lines, 200, 50, &mut failures);
    let ready = find_tag(&lines, "ready");
    let ready_size = ready.map(|value| {
        (
            value.get("cols").and_then(Value::as_u64).unwrap_or(0),
            value.get("rows").and_then(Value::as_u64).unwrap_or(0),
        )
    });
    if ready_size != Some((80, 24)) {
        failures.push(format!("initial size was {ready_size:?}, expected 80x24"));
    }
    Ok(json!({
        "backend": backend,
        "host": "conpty",
        "phase": "workload",
        "repeat": repeat,
        "no_color": true,
        "reduced_motion": true,
        "pass": failures.is_empty(),
        "failures": failures,
        "latencies_us": latencies,
        "idle_pty_bytes": captured.idle_pty_bytes,
        "output_bytes": captured.output.len(),
        "lines": lines,
    }))
}

fn conpty_simple(
    exe: &Path,
    temp: &Path,
    backend: &str,
    phase: &str,
    no_color: bool,
    reduced: bool,
) -> MeasureResult<Value> {
    let report = temp.join(format!("{phase}-{backend}.jsonl"));
    let mut failures = Vec::new();
    let captured = match scripted(
        exe,
        &report,
        backend,
        phase,
        no_color,
        reduced,
        Script::Settle,
    ) {
        Ok(value) => value,
        Err(error) => {
            return Ok(failed_run(
                backend,
                "conpty",
                phase,
                1,
                vec![error.to_string()],
            ));
        }
    };
    let lines = read_lines(&report);
    expect_modes(&lines, &mut failures);
    expect_done(&lines, &captured.status_ok, &mut failures);
    if phase == "color" {
        if !has_color_sgr(&captured.output) {
            failures.push("color run emitted no color SGR sequence".into());
        }
    } else if has_color_sgr(&captured.output) {
        failures.push(format!("{phase} run emitted a color SGR sequence"));
    }
    if phase == "contrast" {
        let animation = find_tag(&lines, "contrast")
            .and_then(|value| value.get("animation").and_then(Value::as_u64))
            .unwrap_or(0);
        if animation == 0 {
            failures.push("reduced motion off produced no animation frame".into());
        }
    }
    if phase == "modes" || phase == "color" {
        expect_text(&lines, &mut failures);
    }
    Ok(json!({
        "backend": backend,
        "host": "conpty",
        "phase": phase,
        "no_color": no_color,
        "reduced_motion": reduced,
        "pass": failures.is_empty(),
        "failures": failures,
        "output_bytes": captured.output.len(),
        "lines": lines,
    }))
}

fn conpty_panic(exe: &Path, temp: &Path, backend: &str) -> MeasureResult<Value> {
    let report = temp.join(format!("panic-{backend}.jsonl"));
    let mut failures = Vec::new();
    let captured = match scripted(exe, &report, backend, "panic", true, true, Script::Settle) {
        Ok(value) => value,
        Err(error) => {
            return Ok(failed_run(
                backend,
                "conpty",
                "panic",
                1,
                vec![error.to_string()],
            ));
        }
    };
    let lines = read_lines(&report);
    if captured.status_ok {
        failures.push("panic phase exited successfully".into());
    }
    if find_tag(&lines, "panic").is_none() {
        failures.push("panic phase did not record a panic line".into());
    }
    expect_modes(&lines, &mut failures);
    Ok(json!({
        "backend": backend,
        "host": "conpty",
        "phase": "panic",
        "pass": failures.is_empty(),
        "failures": failures,
        "status_ok": captured.status_ok,
        "lines": lines,
    }))
}

fn hosted_window(
    exe: &Path,
    temp: &Path,
    backend: &str,
    cols: u16,
    rows: u16,
    label: &str,
) -> MeasureResult<Value> {
    let report = temp.join(format!("hosted-{backend}-{label}.jsonl"));
    let _ = fs::remove_file(&report);
    let cmdline = format!(
        "\"{}\" probe --backend {backend} --phase hosted --report \"{}\"",
        exe.display(),
        report.display()
    );
    let window = format!("sigy-term-{backend}-{label}-{}", std::process::id());
    let spawned = Command::new("wt.exe")
        .env("NO_COLOR", "1")
        .env("SIGY_REDUCED_MOTION", "1")
        .args([
            "--size",
            &format!("{cols},{rows}"),
            "-w",
            &window,
            "cmd",
            "/c",
            &cmdline,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(error) = spawned {
        return Ok(failed_run(
            backend,
            "windows-terminal",
            "hosted",
            1,
            vec![format!("wt.exe failed to start: {error}")],
        ));
    }
    let ready = wait_tag(&report, "done", Duration::from_secs(20));
    if ready.is_err() {
        let _ = wait_tag(&report, "hosted_timeout", Duration::from_secs(2));
    }
    let lines = read_lines(&report);
    let mut failures = Vec::new();
    if lines.is_empty() {
        failures.push("Windows Terminal probe produced no report".into());
    }
    let session =
        find_tag(&lines, "ready").and_then(|value| value.get("wt").and_then(Value::as_bool));
    if session != Some(true) {
        failures.push("hosted probe did not observe WT_SESSION".into());
    }
    expect_idle(&lines, &mut failures);
    expect_text(&lines, &mut failures);
    expect_modes(&lines, &mut failures);
    if find_tag(&lines, "done").is_none() {
        failures.push("hosted probe did not finish".into());
    }
    let observed = find_tag(&lines, "ready").map(|value| {
        (
            value.get("cols").and_then(Value::as_u64).unwrap_or(0),
            value.get("rows").and_then(Value::as_u64).unwrap_or(0),
        )
    });
    if observed.unwrap_or((0, 0)).0 == 0 || observed.unwrap_or((0, 0)).1 == 0 {
        failures.push("hosted probe reported an empty terminal size".into());
    }
    let input_ok = hosted_input_ok(&lines);
    if !input_ok {
        failures.push("Windows Terminal input injection did not complete search and paste".into());
    }
    Ok(json!({
        "backend": backend,
        "host": "windows-terminal",
        "phase": "hosted",
        "label": label,
        "requested_cols": cols,
        "requested_rows": rows,
        "observed_size": observed,
        "pass": failures.is_empty(),
        "failures": failures,
        "lines": lines,
    }))
}

fn hosted_input_ok(lines: &[Value]) -> bool {
    let keys = key_latencies(lines).len();
    let paste = find_tag(lines, "paste");
    let query = paste
        .as_ref()
        .and_then(|value| value.get("query").and_then(Value::as_str))
        .unwrap_or("");
    keys >= 6 && query.contains('q') && query.contains('東')
}

struct Captured {
    output: Vec<u8>,
    idle_pty_bytes: u64,
    status_ok: bool,
    step_error: Option<String>,
}

#[derive(Clone, Copy)]
enum Script {
    Settle,
    Workload,
}

struct PtyChild {
    output: Arc<Mutex<Vec<u8>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn scripted(
    exe: &Path,
    report: &Path,
    backend: &str,
    phase: &str,
    no_color: bool,
    reduced: bool,
    script: Script,
) -> MeasureResult<Captured> {
    let _ = fs::remove_file(report);
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(exe);
    command.arg("probe");
    command.arg("--backend");
    command.arg(backend);
    command.arg("--phase");
    command.arg(phase);
    command.arg("--report");
    command.arg(report.as_os_str());
    command.env("NO_COLOR", if no_color { "1" } else { "" });
    command.env("SIGY_REDUCED_MOTION", if reduced { "1" } else { "0" });
    command.env_remove("WT_SESSION");
    let reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let output = Arc::new(Mutex::new(Vec::new()));
    let writer = Arc::new(Mutex::new(writer));
    spawn_reader(reader, Arc::clone(&output), Arc::clone(&writer));
    let child = pair.slave.spawn_command(command)?;
    let mut pty = PtyChild {
        output,
        writer,
        master: pair.master,
        child,
    };
    let mut step_error = None;
    let idle_pty_bytes = match script {
        Script::Settle => 0,
        Script::Workload => {
            let mut idle_bytes = 0u64;
            if let Err(error) = workload_steps(&mut pty, report, &mut idle_bytes) {
                step_error = Some(error.to_string());
            }
            idle_bytes
        }
    };
    let status_ok = match wait_child(&mut pty.child, Duration::from_secs(12)) {
        Ok(ok) => ok,
        Err(error) => {
            if step_error.is_none() {
                step_error = Some(error.to_string());
            }
            false
        }
    };
    thread::sleep(Duration::from_millis(50));
    let captured = lock_slice(&pty.output);
    Ok(Captured {
        output: captured,
        idle_pty_bytes,
        status_ok,
        step_error,
    })
}

fn workload_steps(pty: &mut PtyChild, report: &Path, idle_bytes: &mut u64) -> MeasureResult<()> {
    wait_tag(report, "ready", Duration::from_secs(8))?;
    let at_ready = lock_slice(&pty.output).len();
    wait_tag(report, "idle", Duration::from_secs(3))?;
    let after_idle = lock_slice(&pty.output).len();
    *idle_bytes = u64::try_from(after_idle.saturating_sub(at_ready)).unwrap_or(u64::MAX);
    send_workload(pty, report)
}

fn send_workload(pty: &mut PtyChild, report: &Path) -> MeasureResult<()> {
    for character in ["n", "a", "v", "a", "j", "o"] {
        send_bytes(&pty.writer, character.as_bytes())?;
        wait_key(report, character, Duration::from_secs(3))?;
    }
    pty.master.resize(PtySize {
        rows: 36,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    wait_resize(report, 120, 36, Duration::from_secs(3))?;
    pty.master.resize(PtySize {
        rows: 50,
        cols: 200,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    wait_resize(report, 200, 50, Duration::from_secs(3))?;
    let mut paste = Vec::from(&b"\x1b[200~"[..]);
    paste.extend("q\u{6771}\u{4eac}".as_bytes());
    paste.extend(b"\x1b[201~");
    send_bytes(&pty.writer, &paste)?;
    wait_tag(report, "paste", Duration::from_secs(3))?;
    send_bytes(&pty.writer, b"q")?;
    Ok(())
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<Vec<u8>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
) {
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut pending = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    pending.extend_from_slice(&buffer[..count]);
                    reply_queries(&mut pending, &writer);
                    if pending.len() > 32 {
                        let keep = pending.len() - 32;
                        pending.drain(..keep);
                    }
                    lock_mut(&output).extend_from_slice(&buffer[..count]);
                }
            }
        }
    });
}

fn reply_queries(pending: &mut Vec<u8>, writer: &Mutex<Box<dyn Write + Send>>) {
    while let Some(index) = find_sequence(pending, b"\x1b[6n") {
        let _ = send_bytes(writer, b"\x1b[1;1R");
        pending.drain(index..index + 4);
    }
}

fn find_sequence(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
}

fn send_bytes(writer: &Mutex<Box<dyn Write + Send>>, bytes: &[u8]) -> MeasureResult<()> {
    let mut guard = lock_mut(writer);
    guard.write_all(bytes)?;
    guard.flush()?;
    Ok(())
}

fn wait_child(
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    timeout: Duration,
) -> MeasureResult<bool> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return Err("probe timed out".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_tag(path: &Path, tag: &str, timeout: Duration) -> MeasureResult<Value> {
    let started = Instant::now();
    loop {
        if let Some(value) = find_tag(&read_lines(path), tag) {
            return Ok(value);
        }
        if started.elapsed() > timeout {
            return Err(format!("timed out waiting for {tag} in {}", path.display()).into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_key(path: &Path, character: &str, timeout: Duration) -> MeasureResult<()> {
    let started = Instant::now();
    loop {
        let found = read_lines(path).into_iter().any(|value| {
            value.get("t").and_then(Value::as_str) == Some("key")
                && value.get("ch").and_then(Value::as_str) == Some(character)
        });
        if found {
            return Ok(());
        }
        if started.elapsed() > timeout {
            return Err(format!("timed out waiting for key {character}").into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_resize(path: &Path, cols: u16, rows: u16, timeout: Duration) -> MeasureResult<()> {
    let started = Instant::now();
    loop {
        let found = read_lines(path).into_iter().any(|value| {
            value.get("t").and_then(Value::as_str) == Some("resize")
                && value.get("cols").and_then(Value::as_u64) == Some(u64::from(cols))
                && value.get("rows").and_then(Value::as_u64) == Some(u64::from(rows))
        });
        if found {
            return Ok(());
        }
        if started.elapsed() > timeout {
            return Err(format!("timed out waiting for resize {cols}x{rows}").into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_lines(path: &Path) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn find_tag(lines: &[Value], tag: &str) -> Option<Value> {
    lines
        .iter()
        .find(|value| value.get("t").and_then(Value::as_str) == Some(tag))
        .cloned()
}

fn key_latencies(lines: &[Value]) -> Vec<u64> {
    lines
        .iter()
        .filter(|value| value.get("t").and_then(Value::as_str) == Some("key"))
        .filter_map(|value| value.get("latency_us").and_then(Value::as_u64))
        .collect()
}

fn expect_idle(lines: &[Value], failures: &mut Vec<String>) {
    match find_tag(lines, "idle") {
        Some(value) => {
            if value.get("draws").and_then(Value::as_u64) != Some(0) {
                failures.push(format!("idle draws were not zero: {value}"));
            }
            if value.get("animation").and_then(Value::as_u64) != Some(0) {
                failures.push(format!("idle animation was not zero: {value}"));
            }
        }
        None => failures.push("idle report is missing".into()),
    }
}

fn expect_text(lines: &[Value], failures: &mut Vec<String>) {
    match find_tag(lines, "text") {
        Some(value) => {
            if value.get("combining_ok").and_then(Value::as_bool) != Some(true) {
                failures.push(format!("combining mark was not one cell: {value}"));
            }
            if value.get("wide_ok").and_then(Value::as_bool) != Some(true) {
                failures.push(format!("wide character was not two columns: {value}"));
            }
        }
        None => failures.push("text report is missing".into()),
    }
}

fn expect_modes(lines: &[Value], failures: &mut Vec<String>) {
    let before = find_tag(lines, "mode_before");
    let after = find_tag(lines, "mode_after");
    match (before, after) {
        (Some(before), Some(after)) => {
            for field in ["input", "output", "input_cp", "output_cp"] {
                if before.get(field) != after.get(field) {
                    failures.push(format!(
                        "console {field} changed from {} to {}",
                        before.get(field).unwrap_or(&Value::Null),
                        after.get(field).unwrap_or(&Value::Null)
                    ));
                }
            }
        }
        _ => failures.push("mode restoration report is incomplete".into()),
    }
}

fn expect_done(lines: &[Value], status_ok: &bool, failures: &mut Vec<String>) {
    if find_tag(lines, "done").is_none() {
        failures.push("probe did not report done".into());
    }
    if !status_ok {
        failures.push("probe process failed".into());
    }
}

fn expect_resize(lines: &[Value], cols: u16, rows: u16, failures: &mut Vec<String>) {
    let found = lines.iter().any(|value| {
        value.get("t").and_then(Value::as_str) == Some("resize")
            && value.get("cols").and_then(Value::as_u64) == Some(u64::from(cols))
            && value.get("rows").and_then(Value::as_u64) == Some(u64::from(rows))
    });
    if !found {
        failures.push(format!("missing resize to {cols}x{rows}"));
    }
}

fn failed_run(backend: &str, host: &str, phase: &str, repeat: u32, failures: Vec<String>) -> Value {
    json!({
        "backend": backend,
        "host": host,
        "phase": phase,
        "repeat": repeat,
        "pass": false,
        "failures": failures,
    })
}

fn lock_slice(output: &Mutex<Vec<u8>>) -> Vec<u8> {
    lock_mut(output).clone()
}

fn lock_mut<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
