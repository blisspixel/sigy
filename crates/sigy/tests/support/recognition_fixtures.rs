//! Native recognition through the real service, decoder and process containment,
//! with a fault-injecting stand-in recognizer. No model or network is used.

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
    let started = success(
        directory,
        &[
            "analysis",
            "transcribe",
            job,
            "--input",
            "pin",
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
        if !matches!(state.as_str(), Some("running" | "cancelling")) {
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

    speech_and_replay(directory.path())?;
    faults(directory.path(), &assets)?;
    cancel_and_kill(directory.path(), &mut service)
}

fn speech_and_replay(directory: &Path) -> TestResult {
    expect(directory, "asr-speech", "speech", "succeeded", None)?;
    let page = newest(directory)?;
    assert_eq!(page["transcript"]["kind"], "recognition");
    assert_eq!(page["transcript"]["outcome"], "text");
    assert_eq!(page["transcript"]["revision"], 1);
    assert_eq!(page["transcript"]["amount_usd"], "0.000000");
    assert_eq!(page["coverage"]["sample_rate"], 16_000);
    assert_eq!(page["coverage"]["sample_count"], 16_000);
    assert_eq!(page["cues"][0]["script"], "bonjour");
    assert_eq!(page["cues"][0]["start_us"], 0);
    assert_eq!(page["cues"][0]["end_us"], 400_000);
    assert_eq!(page["cues"][1]["script"], "le monde");
    assert_eq!(page["cues"][1]["end_us"], 1_000_000);

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

fn cancel_and_kill(directory: &Path, service: &mut RunningChild) -> TestResult {
    // Cancellation of a running native process reaches a terminal state.
    transcribe(directory, "asr-cancel", "slow")?;
    thread::sleep(Duration::from_millis(1500));
    success(
        directory,
        &["analysis", "cancel", "asr-cancel", "--generation", "1"],
    )?;
    let cancelled = wait_job(directory, "asr-cancel")?;
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");

    // Killing the service ends its contained recognizer; restart interrupts the job.
    transcribe(directory, "asr-killed", "slow")?;
    thread::sleep(Duration::from_millis(1500));
    #[cfg(windows)]
    assert_eq!(
        running_images(&format!("recognizer-hang{}", std::env::consts::EXE_SUFFIX))?,
        1
    );
    service.0.kill()?;
    service.0.wait()?;
    #[cfg(windows)]
    {
        let deadline = Instant::now() + Duration::from_secs(5);
        while running_images(&format!("recognizer-hang{}", std::env::consts::EXE_SUFFIX))? != 0 {
            assert!(
                Instant::now() < deadline,
                "contained recognizer outlived its service"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    let mut restarted = RunningChild::start(directory)?;
    let interrupted = wait_job(directory, "asr-killed")?;
    assert_eq!(interrupted["state"], "interrupted");
    assert_eq!(interrupted["generation"], 2);
    assert!(!directory.join("analysis-scratch").exists());

    let status = success(directory, &["library", "status"])?;
    assert_eq!(status["budgets"][0]["reserved_usd"], "0.000000");
    assert_eq!(status["budgets"][0]["settled_usd"], "0.000000");
    success(directory, &["service", "stop"])?;
    restarted.wait()?;
    Ok(())
}
