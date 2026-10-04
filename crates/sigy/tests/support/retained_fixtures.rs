use super::{
    AudioServer, RunningChild, TestResult, initialize, invoke, success, wait_recording, wave,
};
use sigy_service::control::{self, Operation, RetainedOperation};
use std::{
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn retained_long_prefix_seek_is_not_throttled_by_discarded_input() -> TestResult {
    let directory = tempfile::tempdir()?;
    let one_second = wave();
    let mut audio = one_second[..44].to_vec();
    let payload_bytes = 480_000_u32;
    audio[4..8].copy_from_slice(&(payload_bytes + 36).to_le_bytes());
    audio[40..44].copy_from_slice(&payload_bytes.to_le_bytes());
    for _ in 0..60 {
        audio.extend_from_slice(&one_second[44..]);
    }
    let fixture = AudioServer::start(audio)?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "minute",
            "--source",
            "radio:v1",
            "--seconds",
            "90",
            "--max-mib",
            "1",
        ],
    )?;
    let record = wait_recording(directory.path(), "minute", "completed")?;
    assert_eq!(record["decoded_microseconds"], 60_000_000);
    // The existing CLI fixture helper has a 20-second process deadline. A
    // real-time paced 59.25-second discarded prefix cannot pass this check.
    let began = Instant::now();
    let played = success(
        directory.path(),
        &[
            "listen",
            "file",
            "minute",
            "--destination",
            "null",
            "--seek-us",
            "59250000",
            "--request",
            "minute-tail",
        ],
    )?;
    eprintln!(
        "60-second retained fixture, 59.25-second seek: elapsed_ms={} report={played}",
        began.elapsed().as_millis()
    );
    assert_eq!(played["reported_elapsed_us"], 750_000);
    assert_eq!(played["playhead_us"], 60_000_000);
    assert_eq!(played["decoder_completed"], true);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn protected_reader_resolves_late_start_replay_stop_and_restart_holds() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = AudioServer::start(wave())?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    let civil: String = rusqlite::Connection::open_in_memory()?.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S','now','-2 seconds')",
        [],
        |row| row.get(0),
    )?;
    let scheduled = success(
        directory.path(),
        &[
            "schedule",
            "create",
            "late",
            "--source",
            "radio:v1",
            "--zone",
            "Etc/UTC",
            "--once",
            &civil,
            "--seconds",
            "15",
            "--max-mib",
            "1",
        ],
    )?;
    let recording = scheduled["schedule"]["recording_id"]
        .as_str()
        .map(str::to_owned);
    let id = match recording {
        Some(id) => id,
        None => wait_scheduled(directory.path())?,
    };
    let record = wait_recording(directory.path(), &id, "completed")?;
    let origin = record["intervals"][0]["decoded_start_us"]
        .as_u64()
        .ok_or("interval origin")?;
    let end = record["intervals"][0]["decoded_end_us"]
        .as_u64()
        .ok_or("interval end")?;
    assert!(origin >= 2_000_000, "{record}");
    assert_eq!(end - origin, 1_000_000);
    assert!(
        !invoke(
            directory.path(),
            &[
                "listen",
                "file",
                &id,
                "--destination",
                "null",
                "--seek-us",
                "0"
            ]
        )?
        .status
        .success()
    );
    let seek = (origin + 250_000).to_string();
    let arguments = [
        "listen",
        "file",
        &id,
        "--destination",
        "null",
        "--seek-us",
        &seek,
        "--request",
        "late-reader",
    ];
    let played = success(directory.path(), &arguments)?;
    assert_eq!(played["reader"]["spec"]["file_seek_us"], 250_000);
    assert_eq!(played["reader"]["spec"]["file_duration_us"], 1_000_000);
    assert_eq!(played["decoder_completed"], true);
    assert_eq!(played["reported_elapsed_us"], 750_000);
    assert_eq!(played["playhead_us"], end);
    let replay = success(directory.path(), &arguments)?;
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["decoder_completed"], false);
    assert_eq!(replay["reader"], played["reader"]);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    rusqlite::Connection::open(directory.path().join("catalog.sqlite3"))?
        .execute("UPDATE dvr_policy SET decoder=NULL WHERE singleton=1", [])?;
    let offline_replay = success(directory.path(), &arguments)?;
    assert_eq!(offline_replay["reader"], played["reader"]);
    assert_eq!(offline_replay["decoder_completed"], false);
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    service = RunningChild::start(directory.path())?;
    stop_protection(directory.path(), &id, origin)?;
    restart_protection(directory.path(), &id, origin, &mut service)?;
    Ok(())
}

fn wait_scheduled(directory: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let page = success(directory, &["schedule", "show", "late"])?;
        if let Some(id) = page["schedule"]["occurrences"][0]["recording_id"].as_str() {
            return Ok(id.into());
        }
        assert!(Instant::now() < deadline, "scheduled admission: {page}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn start_raw(
    directory: &std::path::Path,
    recording: &str,
    id: &str,
    seek_us: u64,
) -> Result<control::RetainedPage, Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let snapshot = runtime.block_on(control::request(
        directory,
        Operation::Retained {
            command: RetainedOperation::Start {
                id: id.into(),
                recording_id: recording.into(),
                seek_us,
            },
        },
    ))?;
    Ok(*snapshot.retained.ok_or("reader page")?)
}

fn stop_protection(directory: &std::path::Path, recording: &str, seek_us: u64) -> TestResult {
    let start = start_raw(directory, recording, "stopped-reader", seek_us)?;
    assert_eq!(start.newly_started, Some(true));
    let deletion = invoke(directory, &["record", "delete", recording])?;
    assert!(
        !deletion.status.success(),
        "deletion raced an admitted reader"
    );
    let stale = invoke(
        directory,
        &[
            "listen",
            "reader",
            "stop",
            "stopped-reader",
            "--generation",
            "2",
        ],
    )?;
    assert!(!stale.status.success());
    success(
        directory,
        &[
            "listen",
            "reader",
            "stop",
            "stopped-reader",
            "--generation",
            "1",
        ],
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let shown = success(directory, &["listen", "reader", "show", "stopped-reader"])?;
        if shown["entries"][0]["state"] == "failed" {
            break;
        }
        assert!(Instant::now() < deadline, "reader stop: {shown}");
        thread::sleep(Duration::from_millis(10));
    }
    let replay = start_raw(directory, recording, "stopped-reader", seek_us)?;
    assert_eq!(replay.newly_started, Some(false));
    assert!(replay.pipe_nonce.is_none());
    Ok(())
}

fn restart_protection(
    directory: &std::path::Path,
    recording: &str,
    seek_us: u64,
    service: &mut RunningChild,
) -> TestResult {
    let admitted = start_raw(directory, recording, "abandoned-reader", seek_us)?;
    assert_eq!(admitted.newly_started, Some(true));
    service.0.kill()?;
    service.0.wait()?;
    let mut restarted = RunningChild::start(directory)?;
    let held = success(directory, &["listen", "reader", "show", "abandoned-reader"])?;
    assert_eq!(held["entries"][0]["state"], "recovery_held");
    assert_eq!(
        held["entries"][0]["recovery_reason"],
        "restart-completion-unproven"
    );
    assert!(
        !invoke(directory, &["record", "delete", recording])?
            .status
            .success()
    );
    let replay = start_raw(directory, recording, "abandoned-reader", seek_us)?;
    assert_eq!(replay.newly_started, Some(false));
    assert!(replay.pipe_nonce.is_none());
    success(directory, &["service", "stop"])?;
    restarted.wait()?;
    let offline = success(directory, &["listen", "reader", "show", "abandoned-reader"])?;
    assert_eq!(offline["entries"][0], held["entries"][0]);
    Ok(())
}
