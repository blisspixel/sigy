//! Native recognition through the real service, decoder and process containment,
//! with a fault-injecting stand-in recognizer. No real model or external network is used.

use super::{RunningChild, TestResult, invoke, success};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

fn tone() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&16_036_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend((0..16_000).map(|index| if (index / 16) % 2 == 0 { 100 } else { 156 }));
    bytes
}

/// Serves one WAV body once, on loopback only.
fn serve_once(body: Vec<u8>) -> std::io::Result<(String, thread::JoinHandle<std::io::Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://127.0.0.1:{}/audio", listener.local_addr()?.port());
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut request = Vec::new();
        while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte)?;
            request.push(byte[0]);
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(&body)
    });
    Ok((url, worker))
}

fn stand_in() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let sigy = PathBuf::from(env!("CARGO_BIN_EXE_sigy"));
    let path = sigy.with_file_name(format!(
        "sigy-test-recognizer{}",
        std::env::consts::EXE_SUFFIX
    ));
    if !path.is_file() {
        return Err("build sigy-test-recognizer first; cargo verify-media does this".into());
    }
    Ok(path)
}

struct Assets {
    model: PathBuf,
    vad: PathBuf,
}

fn stage(root: &Path, id: &str, mode: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let runtime = root.join(format!("runtime-{id}"));
    std::fs::create_dir(&runtime)?;
    std::fs::copy(
        stand_in()?,
        runtime.join(format!("recognizer-{mode}{}", std::env::consts::EXE_SUFFIX)),
    )?;
    Ok(runtime)
}

fn add_profile(
    directory: &Path,
    root: &Path,
    assets: &Assets,
    (id, mode): (&str, &str),
    memory_mib: &str,
    deadline_seconds: &str,
) -> TestResult {
    let runtime = stage(root, id, mode)?;
    let executable = format!("recognizer-{mode}{}", std::env::consts::EXE_SUFFIX);
    let created = success(
        directory,
        &[
            "analysis",
            "profile",
            "add",
            id,
            "--runtime-dir",
            runtime.to_str().ok_or("path")?,
            "--executable",
            &executable,
            "--model",
            assets.model.to_str().ok_or("path")?,
            "--vad-model",
            assets.vad.to_str().ok_or("path")?,
            "--threads",
            "1",
            "--memory-mib",
            memory_mib,
            "--deadline-seconds",
            deadline_seconds,
        ],
    )?;
    assert_eq!(created["recognition"]["created"], true);
    Ok(())
}

fn transcribe(directory: &Path, job: &str, profile: &str) -> TestResult {
    transcribe_on(directory, job, "pin", profile)
}

fn transcribe_on(directory: &Path, job: &str, input: &str, profile: &str) -> TestResult {
    let started = success(
        directory,
        &[
            "analysis",
            "transcribe",
            job,
            "--input",
            input,
            "--revision",
            "1",
            "--profile",
            profile,
        ],
    )?;
    assert_eq!(started["recognition"]["job"]["request"]["id"], job);
    assert_eq!(started["recognition"]["job"]["amount_usd"], "0.000000");
    Ok(())
}

fn wait_job(directory: &Path, job: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let response = success(directory, &["analysis", "job", job])?;
        let state = response["recognition"]["job"]["state"].clone();
        if !matches!(state.as_str(), Some("queued" | "running" | "cancelling")) {
            return Ok(response["recognition"]["job"].clone());
        }
        assert!(
            Instant::now() < deadline,
            "recognition deadline: {response}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn expect(
    directory: &Path,
    job: &str,
    profile: &str,
    state: &str,
    reason: Option<&str>,
) -> TestResult {
    transcribe(directory, job, profile)?;
    let finished = wait_job(directory, job)?;
    assert_eq!(finished["state"], state, "{job}: {finished}");
    assert_eq!(finished["reason"].as_str(), reason, "{job}: {finished}");
    Ok(())
}

fn newest(directory: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let page = success(directory, &["analysis", "transcript", "pin"])?;
    Ok(page["recognition"]["page"].clone())
}

#[cfg(windows)]
fn running_images(name: &str) -> std::io::Result<usize> {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {name}"), "/NH", "/FO", "CSV"])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(name))
        .count())
}

fn add_translator(
    directory: &Path,
    root: &Path,
    model: &Path,
    (id, mode, languages): (&str, &str, &str),
    deadline_seconds: &str,
) -> TestResult {
    let runtime = root.join(format!("translator-runtime-{id}"));
    std::fs::create_dir(&runtime)?;
    let executable = format!("translator-{mode}{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(stand_in()?, runtime.join(&executable))?;
    let created = success(
        directory,
        &[
            "analysis",
            "translation-profile",
            "add",
            id,
            "--runtime-dir",
            runtime.to_str().ok_or("path")?,
            "--executable",
            &executable,
            "--model",
            model.to_str().ok_or("path")?,
            "--languages",
            languages,
            "--threads",
            "1",
            "--memory-mib",
            "256",
            "--cue-deadline-seconds",
            deadline_seconds,
        ],
    )?;
    assert_eq!(created["recognition"]["created"], true);
    Ok(())
}

fn translate(
    directory: &Path,
    job: &str,
    profile: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let started = success(
        directory,
        &[
            "analysis",
            "translate",
            job,
            "--input",
            "pin",
            "--transcript-revision",
            "1",
            "--profile",
            profile,
        ],
    )?;
    assert_eq!(started["recognition"]["job"]["request"]["id"], job);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let response = success(directory, &["analysis", "job", job])?;
        let job_view = response["recognition"]["job"].clone();
        if !matches!(
            job_view["state"].as_str(),
            Some("queued" | "running" | "cancelling")
        ) {
            return Ok(job_view);
        }
        assert!(
            Instant::now() < deadline,
            "translation deadline: {response}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// Translation of the speech transcript (revision 1: `bonjour`, `le monde`, label `fr`).
fn translation_phase(directory: &Path, root: &Path) -> TestResult {
    let model = root.join("translator.gguf");
    std::fs::write(&model, b"fixture translation model")?;
    for (id, mode, languages, deadline) in [
        ("mt-echo", "echo", "es,fr", "30"),
        ("mt-es", "echo", "es", "30"),
        ("mt-fail", "fail", "fr", "30"),
        ("mt-flood", "flood", "fr", "30"),
        ("mt-hang", "hang", "fr", "1"),
    ] {
        add_translator(directory, root, &model, (id, mode, languages), deadline)?;
    }
    let done = translate(directory, "tr-echo", "mt-echo")?;
    assert_eq!(done["state"], "succeeded", "{done}");
    assert_eq!(done["amount_usd"], "0.000000");
    let page = success(
        directory,
        &[
            "analysis",
            "translation",
            "pin",
            "--transcript-revision",
            "1",
        ],
    )?;
    let page = &page["recognition"]["page"];
    assert_eq!(page["translated_count"], 2, "{page}");
    assert_eq!(page["pairs"][0]["original"], "bonjour");
    assert_eq!(page["pairs"][0]["english"], "EN bonjour");
    assert_eq!(page["pairs"][1]["english"], "EN le monde");
    assert_eq!(page["pairs"][1]["start_us"], 500_000);
    // Exact replay returns the stored job; a changed request under the same ID is refused.
    translate(directory, "tr-echo", "mt-echo")?;
    assert!(
        !invoke(
            directory,
            &[
                "analysis",
                "translate",
                "tr-echo",
                "--input",
                "pin",
                "--transcript-revision",
                "1",
                "--profile",
                "mt-es"
            ],
        )?
        .status
        .success()
    );
    for (job, profile, reason) in [
        ("tr-es", "mt-es", "unsupported-language"),
        ("tr-fail", "mt-fail", "translator-failed"),
        ("tr-flood", "mt-flood", "output-limit"),
        ("tr-hang", "mt-hang", "deadline"),
    ] {
        let finished = translate(directory, job, profile)?;
        assert_eq!(finished["state"], "succeeded", "{job}: {finished}");
        let page = success(
            directory,
            &[
                "analysis",
                "translation",
                "pin",
                "--transcript-revision",
                "1",
            ],
        )?;
        let page = &page["recognition"]["page"];
        assert_eq!(page["job_id"], job, "{page}");
        assert_eq!(page["translated_count"], 0, "{job}: {page}");
        assert_eq!(page["pairs"][0]["reason"], reason, "{job}: {page}");
        assert!(page["pairs"][0]["english"].is_null());
    }
    // A changed model file fails closed before any translator process starts.
    std::fs::write(&model, b"replaced model")?;
    let changed = translate(directory, "tr-changed", "mt-echo")?;
    assert_eq!(changed["state"], "failed");
    assert_eq!(changed["reason"], "profile-unavailable");
    std::fs::write(&model, b"fixture translation model")?;
    Ok(())
}

fn record_and_pin(directory: &Path) -> Result<RunningChild, Box<dyn std::error::Error>> {
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    let (url, server) = serve_once(tone())?;
    success(directory, &["library", "init"])?;
    success(
        directory,
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    success(
        directory,
        &[
            "source",
            "add",
            "radio:v1",
            "--name",
            "Radio",
            "--url",
            &url,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    let service = RunningChild::start(directory)?;
    success(
        directory,
        &[
            "record",
            "start",
            "morning",
            "--source",
            "radio:v1",
            "--seconds",
            "3",
            "--max-mib",
            "1",
        ],
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let response = success(directory, &["record", "show", "morning"])?;
        let state = &response["recording_page"]["entries"][0]["state"];
        if state == "completed" {
            break;
        }
        assert_ne!(state, "failed", "{response}");
        assert!(Instant::now() < deadline, "recording deadline");
        thread::sleep(Duration::from_millis(30));
    }
    server.join().map_err(|_| "server panicked")??;
    success(
        directory,
        &["analysis", "admit", "pin", "--recording", "morning"],
    )?;
    success(
        directory,
        &["analysis", "publish", "pin", "--revision", "1"],
    )?;
    Ok(service)
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG and sigy-test-recognizer; run cargo verify-media"]
fn contained_recognition_publishes_bounded_text_and_fails_closed_on_faults() -> TestResult {
    let directory = tempfile::tempdir()?;
    let files = tempfile::tempdir()?;
    let assets = Assets {
        model: files.path().join("model.bin"),
        vad: files.path().join("vad.bin"),
    };
    std::fs::write(&assets.model, b"fixture model")?;
    std::fs::write(&assets.vad, b"fixture speech gate")?;
    let mut service = record_and_pin(directory.path())?;
    for (mode, memory, deadline) in [
        ("speech", "256", "30"),
        ("silent", "256", "30"),
        ("garbage", "256", "30"),
        ("overlap", "256", "30"),
        ("huge", "256", "30"),
        ("fail", "256", "30"),
        ("child", "256", "30"),
        ("memory", "256", "30"),
        ("hang", "256", "2"),
    ] {
        add_profile(
            directory.path(),
            files.path(),
            &assets,
            (mode, mode),
            memory,
            deadline,
        )?;
    }
    add_profile(
        directory.path(),
        files.path(),
        &assets,
        ("slow", "hang"),
        "256",
        "600",
    )?;
    add_profile(
        directory.path(),
        files.path(),
        &assets,
        ("once", "once"),
        "256",
        "600",
    )?;

    speech_and_replay(directory.path())?;
    translation_phase(directory.path(), files.path())?;
    monitor_owned_capture(directory.path())?;
    task_owned_processing(directory.path())?;
    faults(directory.path(), &assets)?;
    queue_in_order(directory.path())?;
    cancel_and_kill(directory.path(), &mut service)
}

fn capture_monitor(directory: &Path, expected: Option<&str>, capture: bool) -> TestResult {
    let mut args = vec![
        "monitor",
        if expected.is_some() {
            "revise"
        } else {
            "create"
        },
        "capture-monitor",
    ];
    if let Some(version) = expected {
        args.extend(["--expected-version", version]);
    }
    args.extend([
        "--name",
        "Fixture news",
        "--goal",
        "Follow French news",
        "--term",
        "fr:bonjour",
        "--source",
        "monitor-a:v1",
        "--source",
        "monitor-b:v1",
        "--source",
        "radio:v1",
        "--daily-minutes",
        "1",
        "--total-hours",
        "1",
        "--recognition-profile",
        "speech",
        "--translation-profile",
        "mt-echo",
    ]);
    if capture {
        args.extend([
            "--capture-daily-minutes",
            "1",
            "--capture-total-hours",
            "1",
            "--capture-total-mib",
            "2",
        ]);
    }
    success(directory, &args)?;
    Ok(())
}

fn owned_schedule(
    directory: &Path,
    source: &str,
    rule: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let civil: String = rusqlite::Connection::open_in_memory()?.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S', 'now', '-2 seconds')",
        [],
        |row| row.get(0),
    )?;
    let date = civil.get(..10).ok_or("civil date")?;
    let args = [
        "schedule",
        "create",
        rule,
        "--source",
        source,
        "--zone",
        "Etc/UTC",
        "--once",
        &civil,
        "--seconds",
        "15",
        "--max-mib",
        "1",
        "--monitor",
        "capture-monitor",
        "--monitor-version",
        "1",
    ];
    success(directory, &args)?;
    success(directory, &args)?;
    Ok(format!("{rule}:{date}"))
}

fn wait_owned_recording(directory: &Path, id: &str) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let page = success(directory, &["record", "show", id])?;
        let state = &page["recording_page"]["entries"][0]["state"];
        if state == "completed" {
            return Ok(());
        }
        assert_ne!(state, "failed", "{page}");
        assert!(Instant::now() < deadline, "capture deadline: {page}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_owned_translation(directory: &Path, recording: &str) -> TestResult {
    let pin = sigy_service::monitor::pipeline::pin_id(recording);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let response = invoke(directory, &["analysis", "translation", &pin])?;
        if response.status.success() {
            let page: serde_json::Value = serde_json::from_slice(&response.stdout)?;
            if page["recognition"]["kind"] == "translation" {
                assert_eq!(page["recognition"]["page"]["amount_usd"], "0.000000");
                assert_eq!(
                    page["recognition"]["page"]["pairs"][0]["original"],
                    "bonjour"
                );
                return Ok(());
            }
        }
        assert!(
            Instant::now() < deadline,
            "translation deadline: {}",
            String::from_utf8_lossy(&response.stdout)
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn monitor_owned_capture(directory: &Path) -> TestResult {
    let (first_url, first_server) = serve_once(tone())?;
    let (second_url, second_server) = serve_once(tone())?;
    for (id, url) in [
        ("monitor-a:v1", first_url.as_str()),
        ("monitor-b:v1", second_url.as_str()),
    ] {
        success(
            directory,
            &[
                "source",
                "add",
                id,
                "--name",
                "Monitor fixture",
                "--url",
                url,
                "--pin-address",
                "127.0.0.1",
            ],
        )?;
    }
    capture_monitor(directory, None, true)?;
    let first = owned_schedule(directory, "monitor-a:v1", "owned-first")?;
    wait_owned_recording(directory, &first)?;
    first_server.join().map_err(|_| "server panicked")??;
    wait_owned_translation(directory, &first)?;
    let recorded = success(directory, &["record", "show", &first])?;
    let gaps = &recorded["recording_page"]["entries"][0]["gaps"];
    assert_eq!(gaps[0]["cause"], "late_start", "{recorded}");
    assert_eq!(gaps[0]["start_us"], 0);
    assert!(
        gaps[0]["end_us"]
            .as_u64()
            .is_some_and(|end| end >= 2_000_000)
    );
    let policy = success(directory, &["monitor", "show", "capture-monitor"])?;
    assert_eq!(
        policy["monitor"]["monitor"]["processing"]["recognition_queued"],
        1
    );
    assert_eq!(
        policy["monitor"]["monitor"]["processing"]["translation_queued"],
        1
    );
    let historical_pin = sigy_service::monitor::pipeline::pin_id("morning");
    assert!(
        !invoke(directory, &["analysis", "show", &historical_pin])?
            .status
            .success()
    );
    let shown = success(directory, &["schedule", "show", "owned-first"])?;
    assert_eq!(
        shown["schedule"]["rules"][0]["monitor_owner"]["monitor_id"],
        "capture-monitor"
    );
    assert_eq!(
        shown["schedule"]["occurrences"][0]["capture_admission"]["planned_seconds"],
        15
    );
    assert_eq!(
        shown["schedule"]["occurrences"][0]["capture_admission"]["maximum_bytes"],
        1_048_576
    );
    success(
        directory,
        &[
            "monitor",
            "pause",
            "capture-monitor",
            "--action-id",
            "processing-pause",
        ],
    )?;
    let second = owned_schedule(directory, "monitor-b:v1", "owned-paused")?;
    wait_owned_recording(directory, &second)?;
    second_server.join().map_err(|_| "server panicked")??;
    let monitor = success(directory, &["monitor", "show", "capture-monitor"])?;
    let usage = &monitor["monitor"]["monitor"]["capture_usage"];
    assert_eq!(usage["admissions"], 2);
    assert_eq!(usage["used_total_seconds"], 30);
    assert_eq!(usage["reserved_total_bytes"], 2_097_152);
    let second_pin = sigy_service::monitor::pipeline::pin_id(&second);
    assert!(
        !invoke(directory, &["analysis", "show", &second_pin])?
            .status
            .success()
    );
    capture_monitor(directory, Some("1"), false)?;
    let monitor = success(directory, &["monitor", "show", "capture-monitor"])?;
    assert_eq!(
        monitor["monitor"]["monitor"]["capture_usage"]["admissions"],
        2
    );
    assert!(
        monitor["monitor"]["monitor"]["version"]["spec"]
            .get("capture")
            .is_none()
    );
    Ok(())
}

/// A WAV body of whole seconds at 16 kHz, 8-bit mono, as `tone` with a longer data chunk.
fn long_tone(seconds: u32) -> Vec<u8> {
    let samples = 16_000 * seconds;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&samples.to_le_bytes());
    bytes.extend((0..samples).map(|index| if (index / 16) % 2 == 0 { 100 } else { 156 }));
    bytes
}

fn wait_task_evidence(
    directory: &Path,
    task: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let page = success(directory, &["task", "evidence", task])?;
        let evidence = page["task"]["evidence"].clone();
        if evidence["outcome"] != "pending" {
            return Ok(evidence);
        }
        assert!(Instant::now() < deadline, "task evidence deadline: {page}");
        thread::sleep(Duration::from_millis(250));
    }
}

/// Two loopback sources and a monitor that may capture them but names no recognition
/// profile, so no authority other than the task holds their processing jobs.
fn task_sources(directory: &Path, urls: [&str; 2]) -> TestResult {
    for (id, url) in ["task-a:v1", "task-b:v1"].into_iter().zip(urls) {
        let source = ["source", "add", id, "--name", "Task fixture", "--url", url];
        let mut args = source.to_vec();
        args.extend(["--pin-address", "127.0.0.1"]);
        success(directory, &args)?;
    }
    let mut monitor = vec![
        "monitor",
        "create",
        "task-monitor",
        "--name",
        "Task fixture",
    ];
    monitor.extend(["--goal", "Follow French news", "--term", "fr:bonjour"]);
    monitor.extend(["--source", "task-a:v1", "--source", "task-b:v1"]);
    monitor.extend(["--daily-minutes", "1", "--total-hours", "1"]);
    monitor.extend(["--capture-daily-minutes", "1", "--capture-total-hours", "1"]);
    monitor.extend(["--capture-total-mib", "2"]);
    success(directory, &monitor)?;
    Ok(())
}

/// Scope, a three-second collection from each source and a finite processing grant.
fn task_grants(directory: &Path, start: i64) -> TestResult {
    let (from, to, at) = (
        (start - 60_000).to_string(),
        (start + 600_000).to_string(),
        start.to_string(),
    );
    let mut task = vec!["task", "create", "native-task", "--goal", "Follow bonjour"];
    task.extend(["--monitor", "task-monitor", "--monitor-version", "1"]);
    task.extend(["--monitor-actions", "0", "--from-ms", &from, "--to-ms", &to]);
    success(directory, &task)?;
    let mut collect = vec!["task", "collect", "native-task", "collect"];
    collect.extend(["--source", "task-a:v1", "--source", "task-b:v1"]);
    collect.extend([
        "--start-ms",
        &at,
        "--seconds",
        "3",
        "--max-bytes",
        "1048576",
    ]);
    collect.extend(["--expected-generation", "0"]);
    success(directory, &collect)?;
    let mut process = vec!["task", "process", "native-task", "process"];
    process.extend([
        "--recognition-profile",
        "speech",
        "--translation-profile",
        "mt-echo",
    ]);
    process.extend(["--max-audio-seconds", "20", "--expected-generation", "0"]);
    let granted = success(directory, &process)?;
    assert_eq!(granted["task"]["processing"]["generation"], 1, "{granted}");
    Ok(())
}

/// A finite task collects two loopback recordings and the service processes exactly those
/// under the task grant after the granting client exits.
fn task_owned_processing(directory: &Path) -> TestResult {
    let (first_url, first_server) = serve_once(long_tone(4))?;
    let (second_url, second_server) = serve_once(long_tone(4))?;
    task_sources(directory, [&first_url, &second_url])?;
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    task_grants(directory, (now / 1000 + 8) * 1000)?;
    let evidence = wait_task_evidence(directory, "native-task")?;
    first_server.join().map_err(|_| "server panicked")??;
    second_server.join().map_err(|_| "server panicked")??;
    assert_eq!(evidence["outcome"], "cited", "{evidence}");
    assert_eq!(evidence["uncovered_us"], 0, "{evidence}");
    assert_eq!(evidence["unprocessed_us"], 0, "{evidence}");
    assert_eq!(evidence["citations"].as_array().map(Vec::len), Some(2));
    let processing = success(directory, &["task", "processing", "native-task"])?;
    let processing = &processing["task"]["processing"];
    assert_eq!(processing["charged_audio_us"], 8_000_000, "{processing}");
    let steps = processing["steps"].as_array().ok_or("steps")?;
    assert_eq!(steps.len(), 4, "{processing}");
    for step in steps {
        assert_eq!(step["decision"], "queued", "{processing}");
        assert_eq!(step["job_state"], "succeeded", "{processing}");
        assert_eq!(step["sharing"]["tasks"], 1);
        assert_eq!(step["sharing"]["monitors"], 0);
    }
    let monitor = success(directory, &["monitor", "show", "task-monitor"])?;
    assert_eq!(
        monitor["monitor"]["monitor"]["processing"]["recognition_queued"],
        0
    );
    Ok(())
}

fn speech_and_replay(directory: &Path) -> TestResult {
    expect(directory, "asr-speech", "speech", "succeeded", None)?;
    let page = newest(directory)?;
    assert_eq!(page["transcript"]["kind"], "recognition");
    assert_eq!(page["transcript"]["outcome"], "text");
    assert_eq!(page["transcript"]["revision"], 1);
    assert_eq!(page["transcript"]["amount_usd"], "0.000000");
    assert_eq!(page["coverages"][0]["sample_rate"], 16_000);
    assert_eq!(page["coverages"][0]["sample_count"], 16_000);
    assert_eq!(page["cues"][0]["script"], "bonjour");
    assert_eq!(page["cues"][0]["start_us"], 0);
    assert_eq!(page["cues"][0]["end_us"], 400_000);
    assert_eq!(page["cues"][1]["script"], "le monde");
    assert_eq!(page["cues"][1]["end_us"], 1_000_000);

    // The recognizer's block language label is stored as evidence for that transcript.
    let languages = success(
        directory,
        &["analysis", "languages", "list", "pin", "--revision", "1"],
    )?
    .to_string();
    assert!(languages.contains("\"asr-speech\""), "{languages}");
    assert!(languages.contains("\"recognizer\""), "{languages}");
    let evidence = success(
        directory,
        &[
            "analysis",
            "languages",
            "show",
            "asr-speech",
            "--revision",
            "1",
        ],
    )?
    .to_string();
    assert!(evidence.contains("\"tag\":\"fr\""), "{evidence}");
    assert!(evidence.contains("\"unevaluated\""), "{evidence}");

    // Exact replay returns the stored job and never runs the recognizer again.
    transcribe(directory, "asr-speech", "speech")?;
    assert_eq!(newest(directory)?["transcript"]["revision"], 1);
    assert!(
        !invoke(
            directory,
            &[
                "analysis",
                "transcribe",
                "asr-speech",
                "--input",
                "pin",
                "--revision",
                "1",
                "--profile",
                "silent"
            ],
        )?
        .status
        .success()
    );

    Ok(())
}

fn faults(directory: &Path, assets: &Assets) -> TestResult {
    expect(directory, "asr-silent", "silent", "succeeded", None)?;
    let silent = newest(directory)?;
    assert_eq!(silent["transcript"]["outcome"], "no_text");
    assert_eq!(silent["transcript"]["revision"], 2);
    assert_eq!(silent["transcript"]["parent_revision"], 1);
    assert!(
        !invoke(
            directory,
            &[
                "analysis",
                "languages",
                "show",
                "asr-silent",
                "--revision",
                "1"
            ]
        )?
        .status
        .success(),
        "no text publishes no language observation"
    );

    for (job, mode) in [
        ("asr-garbage", "garbage"),
        ("asr-overlap", "overlap"),
        ("asr-huge", "huge"),
    ] {
        expect(
            directory,
            job,
            mode,
            "failed",
            Some("invalid-worker-output"),
        )?;
    }
    expect(
        directory,
        "asr-fail",
        "fail",
        "failed",
        Some("recognizer-failed"),
    )?;
    expect(directory, "asr-hang", "hang", "failed", Some("deadline"))?;

    // The process-count limit refuses a grandchild; the stand-in reports what it saw.
    expect(directory, "asr-child", "child", "succeeded", None)?;
    assert_eq!(
        newest(directory)?["cues"][0]["script"],
        "grandchild refused"
    );

    // The committed-memory limit refuses a 4 GiB allocation under a 256 MiB ceiling.
    transcribe(directory, "asr-memory", "memory")?;
    let memory = wait_job(directory, "asr-memory")?;
    assert_eq!(memory["state"], "failed", "{memory}");

    // A changed profile file fails closed before any recognizer process starts.
    std::fs::write(&assets.vad, b"replaced speech gate")?;
    expect(
        directory,
        "asr-changed",
        "speech",
        "failed",
        Some("profile-unavailable"),
    )?;
    std::fs::write(&assets.vad, b"fixture speech gate")?;

    Ok(())
}

/// Five recognitions admitted at once all publish, one at a time, in admission order.
fn queue_in_order(directory: &Path) -> TestResult {
    let jobs: Vec<(String, String)> = (1..=5)
        .map(|index| (format!("asr-q{index}"), format!("pin-q{index}")))
        .collect();
    for (_, pin) in &jobs {
        success(
            directory,
            &["analysis", "admit", pin, "--recording", "morning"],
        )?;
        success(directory, &["analysis", "publish", pin, "--revision", "1"])?;
    }
    for (job, pin) in &jobs {
        transcribe_on(directory, job, pin, "speech")?;
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let mut states = Vec::new();
        for (job, _) in &jobs {
            let response = success(directory, &["analysis", "job", job])?;
            states.push(response["recognition"]["job"]["state"].clone());
        }
        // Separate reads are not one snapshot; overlap is checked below from the stored
        // start and finish times.
        if states.iter().all(|state| state == "succeeded") {
            break;
        }
        assert!(
            states
                .iter()
                .all(|state| matches!(state.as_str(), Some("queued" | "running" | "succeeded"))),
            "{states:?}"
        );
        assert!(Instant::now() < deadline, "queue deadline: {states:?}");
        thread::sleep(Duration::from_millis(20));
    }
    // Each job started only after its predecessor finished.
    let mut previous = 0;
    for (job, pin) in &jobs {
        let view = wait_job(directory, job)?;
        let started = view["started_ms"].as_i64().ok_or("started")?;
        assert!(started >= previous, "{job} overlapped its predecessor");
        previous = view["finished_ms"].as_i64().ok_or("finished")?;
        let page = success(directory, &["analysis", "transcript", pin])?;
        assert_eq!(page["recognition"]["page"]["transcript"]["revision"], 1);
    }
    Ok(())
}

/// A second capture runs to completion while a recognizer holds its slot.
fn capture_during_recognition(directory: &Path) -> TestResult {
    let (url, server) = serve_once(tone())?;
    success(
        directory,
        &[
            "source",
            "add",
            "radio:v2",
            "--name",
            "Radio two",
            "--url",
            &url,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    success(
        directory,
        &[
            "record",
            "start",
            "evening",
            "--source",
            "radio:v2",
            "--seconds",
            "3",
            "--max-mib",
            "1",
        ],
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let response = success(directory, &["record", "show", "evening"])?;
        let state = &response["recording_page"]["entries"][0]["state"];
        if state == "completed" {
            break;
        }
        assert_ne!(state, "failed", "{response}");
        assert!(Instant::now() < deadline, "capture deadline");
        thread::sleep(Duration::from_millis(30));
    }
    server.join().map_err(|_| "server panicked")??;
    Ok(())
}

fn cancel_and_kill(directory: &Path, service: &mut RunningChild) -> TestResult {
    // Cancellation of a running native process reaches a terminal state.
    transcribe(directory, "asr-cancel", "slow")?;
    thread::sleep(Duration::from_millis(1500));
    capture_during_recognition(directory)?;
    let running = success(directory, &["analysis", "job", "asr-cancel"])?;
    assert_eq!(running["recognition"]["job"]["state"], "running");
    success(
        directory,
        &["analysis", "cancel", "asr-cancel", "--generation", "1"],
    )?;
    let cancelled = wait_job(directory, "asr-cancel")?;
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");

    // Killing the service ends its contained recognizer; restart requeues the job under
    // a new generation, and the second attempt publishes exactly one transcript revision.
    let before = newest(directory)?["transcript"]["revision"]
        .as_i64()
        .ok_or("revision")?;
    transcribe(directory, "asr-killed", "once")?;
    thread::sleep(Duration::from_millis(1500));
    #[cfg(windows)]
    assert_eq!(
        running_images(&format!("recognizer-once{}", std::env::consts::EXE_SUFFIX))?,
        1
    );
    service.0.kill()?;
    service.0.wait()?;
    #[cfg(windows)]
    {
        let deadline = Instant::now() + Duration::from_secs(5);
        while running_images(&format!("recognizer-once{}", std::env::consts::EXE_SUFFIX))? != 0 {
            assert!(
                Instant::now() < deadline,
                "contained recognizer outlived its service"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    let mut restarted = RunningChild::start(directory)?;
    let requeued = wait_job(directory, "asr-killed")?;
    assert_eq!(requeued["state"], "succeeded", "{requeued}");
    assert_eq!(requeued["generation"], 2);
    assert_eq!(requeued["attempt"], 2);
    let page = newest(directory)?;
    assert_eq!(page["transcript"]["revision"], before + 1);
    assert_eq!(page["transcript"]["job_id"], "asr-killed");
    assert_eq!(page["transcript"]["job_generation"], 2);
    assert_eq!(page["cues"][0]["script"], "encore");
    assert!(
        !directory
            .join("analysis-scratch")
            .join("asr-killed-g1")
            .exists()
    );

    let status = success(directory, &["library", "status"])?;
    assert_eq!(status["budgets"][0]["reserved_usd"], "0.000000");
    assert_eq!(status["budgets"][0]["settled_usd"], "0.000000");
    success(directory, &["service", "stop"])?;
    restarted.wait()?;
    Ok(())
}
