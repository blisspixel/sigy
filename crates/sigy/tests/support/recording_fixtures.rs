use super::{RunningChild, TestResult, invoke, success};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct AudioServer {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}
impl AudioServer {
    fn start(body: Vec<u8>) -> std::io::Result<Self> {
        Self::start_delayed(body, Duration::ZERO)
    }

    fn start_delayed(body: Vec<u8>, body_delay: Duration) -> std::io::Result<Self> {
        Self::start_configured(body, body_delay, false)
    }

    fn start_configured(
        body: Vec<u8>,
        body_delay: Duration,
        redirect: bool,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://127.0.0.1:{}/audio", listener.local_addr()?.port());
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                        let mut request = Vec::new();
                        while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            stream.read_exact(&mut byte)?;
                            request.push(byte[0]);
                        }
                        if redirect && request.starts_with(b"GET /audio ") {
                            stream.write_all(b"HTTP/1.1 302 Redirect\r\nLocation: /resolved\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
                            continue;
                        }
                        thread::sleep(Duration::from_millis(500));
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )?;
                        let first = body.len().min(1024);
                        stream.write_all(&body[..first])?;
                        thread::sleep(body_delay);
                        stream.write_all(&body[first..])?;
                        return Ok(());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(std::io::Error::other("audio fixture was not contacted"))
        });
        Ok(Self {
            url,
            stop,
            worker: Some(worker),
        })
    }
    fn finish(&mut self) -> TestResult {
        self.worker
            .take()
            .ok_or("fixture already joined")?
            .join()
            .map_err(|_| "fixture panicked")??;
        Ok(())
    }
}
impl Drop for AudioServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn wave() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&8036_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend((0..8000).map(|index| if (index / 20) % 2 == 0 { 120 } else { 136 }));
    bytes
}

fn wait_recording(
    directory: &std::path::Path,
    id: &str,
    expected: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let response = success(directory, &["record", "show", id])?;
        let record = &response["recording_page"]["entries"][0];
        if record["state"] == expected {
            return Ok(record.clone());
        }
        assert_ne!(record["state"], "failed", "{record}");
        assert!(
            Instant::now() < deadline,
            "recording completion deadline: {record}"
        );
        thread::sleep(Duration::from_millis(30));
    }
}

fn initialize(directory: &std::path::Path, fixture: &AudioServer) -> TestResult {
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
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
            "Radio francophone",
            "--url",
            &fixture.url,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run scripts/verify-media.ps1"]
fn redirected_recording_publishes_audio_and_route_together() -> TestResult {
    let directory = tempfile::tempdir()?;
    let bytes = wave();
    let mut fixture = AudioServer::start_configured(bytes.clone(), Duration::ZERO, true)?;
    initialize(directory.path(), &fixture)?;
    success(
        directory.path(),
        &[
            "source",
            "add",
            "radio:v2",
            "--name",
            "Redirect radio",
            "--url",
            &fixture.url,
            "--pin-address",
            "127.0.0.1",
            "--redirects",
            "same-origin",
        ],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "redirected",
            "--source",
            "radio:v2",
            "--seconds",
            "3",
            "--max-mib",
            "1",
        ],
    )?;
    let record = wait_recording(directory.path(), "redirected", "completed")?;
    assert_eq!(record["media_bytes"], bytes.len());
    let metadata = success(directory.path(), &["record", "metadata", "redirected"])?;
    assert_eq!(metadata["source"]["redirects"], "same_origin");
    let route = metadata["capture"]["http_route"]
        .as_array()
        .ok_or("missing route")?;
    assert_eq!(route.len(), 2);
    assert_eq!(route[0]["status"], 302);
    assert_eq!(route[1]["status"], 200);
    assert_eq!(route[0]["origin"], route[1]["origin"]);
    assert!(!metadata.to_string().contains("/resolved"));
    let path = success(directory.path(), &["record", "path", "redirected"])?;
    assert_eq!(
        std::fs::read(path["path"].as_str().ok_or("missing path")?)?,
        bytes
    );
    fixture.finish()?;
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run scripts/verify-media.ps1"]
fn recording_is_service_owned_verified_exportable_and_prunable() -> TestResult {
    let directory = tempfile::tempdir()?;
    let bytes = wave();
    let mut fixture = AudioServer::start(bytes.clone())?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    let start = [
        "record",
        "start",
        "morning",
        "--source",
        "radio:v1",
        "--seconds",
        "3",
        "--max-mib",
        "1",
    ];
    success(directory.path(), &start)?;
    success(directory.path(), &start)?;
    let record = wait_recording(directory.path(), "morning", "completed")?;
    assert_eq!(record["media_bytes"], bytes.len());
    assert_eq!(record["decoded_microseconds"], 1_000_000);
    assert_eq!(record["end_reason"], "end_of_body");
    let path = success(directory.path(), &["record", "path", "morning"])?;
    let path = path["path"].as_str().ok_or("missing media path")?;
    assert_eq!(std::fs::read(path)?, bytes);
    let metadata = success(directory.path(), &["record", "metadata", "morning"])?;
    assert_eq!(metadata["schema_version"], 2);
    assert_eq!(metadata["source"]["redirects"], "deny");
    assert_eq!(metadata["capture"]["http_route"][0]["status"], 200);
    assert_eq!(metadata["payload"]["kind"], "encoded_audio");
    assert_eq!(metadata["storage"]["sha256"], record["sha256"]);
    assert!(!metadata.to_string().contains("/audio"));
    success(
        directory.path(),
        &[
            "record",
            "processed",
            "morning",
            "--receipt",
            "manual:analysis-1",
        ],
    )?;
    success(directory.path(), &["record", "keep", "morning"])?;
    success(directory.path(), &["dvr", "prune"])?;
    assert!(std::path::Path::new(path).is_file());
    success(directory.path(), &["record", "temporary", "morning"])?;
    success(directory.path(), &["dvr", "prune"])?;
    assert!(!std::path::Path::new(path).exists());
    let record = success(directory.path(), &["record", "show", "morning"])?;
    assert_eq!(
        record["recording_page"]["entries"][0]["storage_state"],
        "deleted"
    );
    assert!(
        !invoke(directory.path(), &["record", "path", "morning"])?
            .status
            .success()
    );
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    assert_eq!(
        success(directory.path(), &["dvr", "status"])?["dvr"]["charged_bytes"],
        0
    );
    fixture.finish()?;
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run scripts/verify-media.ps1"]
fn non_audio_headers_do_not_make_a_playable_recording() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut fixture = AudioServer::start(b"not audio".to_vec())?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "bad",
            "--source",
            "radio:v1",
            "--seconds",
            "3",
            "--max-mib",
            "1",
        ],
    )?;
    let record = wait_recording(directory.path(), "bad", "failed")?;
    assert_eq!(record["storage_state"], "reserved");
    assert_eq!(record["charged_bytes"], 1_048_576);
    assert!(record["media_bytes"].is_null());
    assert!(
        record["failure_detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("decoded"))
    );
    assert!(
        !invoke(directory.path(), &["record", "path", "bad"])?
            .status
            .success()
    );
    success(directory.path(), &["record", "delete", "bad"])?;
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    assert_eq!(
        success(directory.path(), &["dvr", "status"])?["dvr"]["charged_bytes"],
        0
    );
    fixture.finish()?;
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run scripts/verify-media.ps1"]
fn killed_capture_preserves_partial_bytes_and_never_replays() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = AudioServer::start_delayed(wave(), Duration::from_secs(2))?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    let start = [
        "record",
        "start",
        "interrupted",
        "--source",
        "radio:v1",
        "--seconds",
        "10",
        "--max-mib",
        "1",
    ];
    let accepted = success(directory.path(), &start)?;
    let key = accepted["recording_page"]["entries"][0]["object_key"]
        .as_str()
        .ok_or("missing object key")?;
    let path = directory.path().join("media").join(format!("{key}.part"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.metadata().is_ok_and(|metadata| metadata.len() >= 1024) {
        assert!(
            Instant::now() < deadline,
            "capture never wrote the partial fixture"
        );
        thread::sleep(Duration::from_millis(10));
    }
    service.0.kill()?;
    service.0.wait()?;
    let retained = std::fs::read(&path)?;
    assert!(!retained.is_empty());
    let mut restarted = RunningChild::start(directory.path())?;
    let record = success(directory.path(), &start)?;
    assert_eq!(
        record["recording_page"]["entries"][0]["state"],
        "interrupted"
    );
    assert_eq!(
        record["recording_page"]["entries"][0]["charged_bytes"],
        1_048_576
    );
    assert_eq!(std::fs::read(&path)?, retained);
    assert!(
        !invoke(directory.path(), &["record", "path", "interrupted"])?
            .status
            .success()
    );
    success(directory.path(), &["record", "delete", "interrupted"])?;
    assert!(!path.exists());
    success(directory.path(), &["service", "stop"])?;
    restarted.wait()?;
    Ok(())
}
