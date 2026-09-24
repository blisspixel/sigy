use super::{RunningChild, TestResult, invoke, success};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
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
                        stream.set_nonblocking(false)?;
                        stream.set_nonblocking(false)?;
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
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
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
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
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
    verify_retained_analysis(directory.path(), bytes.len())?;
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

fn verify_retained_analysis(directory: &std::path::Path, bytes: usize) -> TestResult {
    success(
        directory,
        &["analysis", "admit", "pin", "--recording", "morning"],
    )?;
    success(
        directory,
        &["analysis", "publish", "pin", "--revision", "1"],
    )?;
    let command = [
        "analysis",
        "verify",
        "verify",
        "--input",
        "pin",
        "--revision",
        "1",
    ];
    success(directory, &command)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = success(directory, &["analysis", "job", "verify"])?;
        let job = &response["analysis_job"];
        if job["state"] == "verified" {
            assert_eq!(job["verified_bytes"], bytes);
            assert_eq!(job["amount_usd"], "0.000000");
            break;
        }
        assert_eq!(job["state"], "running");
        assert!(Instant::now() < deadline, "verification did not finish");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        success(directory, &command)?["analysis_job"]["state"],
        "verified"
    );
    assert!(
        !invoke(
            directory,
            &[
                "analysis",
                "transcribe",
                "asr",
                "--input",
                "pin",
                "--revision",
                "1",
                "--profile",
                "unconfigured"
            ]
        )?
        .status
        .success()
    );
    assert!(success(directory, &["analysis", "show", "pin"])?["analysis"]["transcript"].is_null());
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
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
            .is_some_and(|detail| detail.contains("decoded")),
        "malformed audio failed outside decoder validation: {record}"
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
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
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

fn assert_charged(directory: &std::path::Path, charged: &serde_json::Value) -> TestResult {
    assert_eq!(
        &success(directory, &["dvr", "status"])?["dvr"]["charged_bytes"],
        charged
    );
    Ok(())
}

fn assert_system_playback(directory: &std::path::Path) -> TestResult {
    let system = invoke(
        directory,
        &[
            "listen",
            "file",
            "played",
            "--destination",
            "system",
            "--seek-us",
            "900000",
        ],
    )?;
    if system.status.success() {
        let played: serde_json::Value = serde_json::from_slice(&system.stdout)?;
        let playhead = played["playhead_us"].as_u64().ok_or("missing playhead")?;
        assert!(playhead > 900_000 && playhead <= 1_000_000, "{played}");
    } else {
        let error = String::from_utf8_lossy(&system.stderr);
        assert!(
            error.contains("audio output") || error.contains("system audio"),
            "{error}"
        );
    }
    Ok(())
}

fn kill_running_playback(directory: &std::path::Path) -> TestResult {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_sigy"))
        .arg("--data-dir")
        .arg(directory)
        .args([
            "listen",
            "file",
            "played",
            "--destination",
            "null",
            "--seek-us",
            "100000",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    thread::sleep(Duration::from_millis(200));
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn retained_playback_seeks_without_changing_quota_or_capture() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut fixture = AudioServer::start(wave())?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "played",
            "--source",
            "radio:v1",
            "--seconds",
            "3",
            "--max-mib",
            "1",
        ],
    )?;
    let record = wait_recording(directory.path(), "played", "completed")?;
    assert_eq!(record["storage_state"], "retained");
    assert_eq!(record["decoded_microseconds"], 1_000_000);
    fixture.finish()?;
    let charged = success(directory.path(), &["dvr", "status"])?["dvr"]["charged_bytes"].clone();
    assert!(
        !invoke(
            directory.path(),
            &[
                "listen",
                "file",
                "played",
                "--destination",
                "null",
                "--seek-us",
                "1000000",
            ],
        )?
        .status
        .success()
    );
    let played = success(
        directory.path(),
        &[
            "listen",
            "file",
            "played",
            "--destination",
            "null",
            "--seek-us",
            "200000",
        ],
    )?;
    assert_eq!(played["destination"], "null");
    assert_eq!(played["progress_advanced"], true);
    let playhead = played["playhead_us"].as_u64().ok_or("missing playhead")?;
    assert!((200_001..=1_000_000).contains(&playhead), "{played}");
    assert_charged(directory.path(), &charged)?;
    assert_system_playback(directory.path())?;
    assert_charged(directory.path(), &charged)?;
    assert_eq!(
        success(directory.path(), &["record", "show", "played"])?["recording_page"]["entries"][0]["state"],
        "completed"
    );
    kill_running_playback(directory.path())?;
    let shown = success(directory.path(), &["record", "show", "played"])?;
    assert_eq!(
        shown["recording_page"]["entries"][0]["storage_state"],
        "retained"
    );
    assert_eq!(
        success(directory.path(), &["dvr", "status"])?["dvr"]["charged_bytes"],
        charged
    );
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

struct HlsServer {
    origin: String,
    paths: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl HlsServer {
    fn start(audio: Vec<u8>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let origin = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let stop = Arc::new(AtomicBool::new(false));
        let paths = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let recorded = paths.clone();
        let worker = thread::spawn(move || {
            hls_accept(&listener, cancelled.as_ref(), recorded.as_ref(), &audio)
        });
        Ok(Self {
            origin,
            paths,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for HlsServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn hls_accept(
    listener: &TcpListener,
    cancelled: &AtomicBool,
    recorded: &std::sync::Mutex<Vec<String>>,
    audio: &[u8],
) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                let mut request = Vec::new();
                while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte)?;
                    request.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("").to_owned();
                if let Ok(mut paths) = recorded.lock() {
                    paths.push(path.clone());
                }
                let (content_type, body) = hls_response(&path, audio);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )?;
                stream.write_all(&body)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn hls_response(path: &str, audio: &[u8]) -> (&'static str, Vec<u8>) {
    if path.starts_with("/master.m3u8") {
        (
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000\nvariant.m3u8\n".to_vec(),
        )
    } else if path.starts_with("/media.m3u8") {
        (
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nseg0\n#EXTINF:1,\nseg1\n#EXT-X-ENDLIST\n"
                .to_vec(),
        )
    } else if path.starts_with("/seg0") {
        ("audio/wav", audio[..audio.len() / 2].to_vec())
    } else if path.starts_with("/seg1") {
        ("audio/wav", audio[audio.len() / 2..].to_vec())
    } else {
        ("text/plain", b"unexpected".to_vec())
    }
}

fn wait_terminal(
    directory: &std::path::Path,
    id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let response = success(directory, &["record", "show", id])?;
        let record = &response["recording_page"]["entries"][0];
        let state = record["state"].as_str().unwrap_or("");
        if matches!(state, "completed" | "failed" | "interrupted" | "cancelled") {
            return Ok(record.clone());
        }
        assert!(Instant::now() < deadline, "{record}");
        thread::sleep(Duration::from_millis(30));
    }
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn hls_media_playlist_publishes_audio_and_master_is_rejected() -> TestResult {
    let directory = tempfile::tempdir()?;
    let audio = wave();
    let server = HlsServer::start(audio.clone())?;
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "source",
            "add",
            "master:v1",
            "--name",
            "Master",
            "--url",
            &format!("{}/master.m3u8", server.origin),
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    success(
        directory.path(),
        &[
            "source",
            "add",
            "media:v1",
            "--name",
            "Media",
            "--url",
            &format!("{}/media.m3u8", server.origin),
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    success(
        directory.path(),
        &[
            "record",
            "hls",
            "master-rec",
            "--source",
            "master:v1",
            "--seconds",
            "30",
            "--max-mib",
            "1",
        ],
    )?;
    let failed = wait_terminal(directory.path(), "master-rec")?;
    assert_eq!(failed["state"], "failed", "{failed}");
    success(
        directory.path(),
        &[
            "record",
            "hls",
            "media-rec",
            "--source",
            "media:v1",
            "--seconds",
            "30",
            "--max-mib",
            "1",
        ],
    )?;
    let recorded = wait_terminal(directory.path(), "media-rec")?;
    assert_eq!(recorded["state"], "completed", "{recorded}");
    assert_eq!(recorded["media_bytes"], audio.len());
    let path = success(directory.path(), &["record", "path", "media-rec"])?;
    assert_eq!(
        std::fs::read(path["path"].as_str().ok_or("missing path")?)?,
        audio
    );
    let paths = server.paths.lock().map_err(|_| "hls paths")?.clone();
    assert!(paths.iter().any(|path| path.starts_with("/master.m3u8")));
    assert!(paths.iter().any(|path| path.starts_with("/seg0")));
    assert!(paths.iter().any(|path| path.starts_with("/seg1")));
    assert!(paths.iter().all(|path| !path.contains("variant")));
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

struct IcyServer {
    origin: String,
    paths: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    headers: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl IcyServer {
    fn start(audio: &[u8], title: &str) -> std::io::Result<Self> {
        let (interval, body) = icy_body(audio, title)?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let origin = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let stop = Arc::new(AtomicBool::new(false));
        let paths = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let headers = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let recorded = paths.clone();
        let noted = headers.clone();
        let worker = thread::spawn(move || {
            icy_accept(&listener, &cancelled, &recorded, &noted, interval, &body)
        });
        Ok(Self {
            origin,
            paths,
            headers,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for IcyServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

fn icy_body(audio: &[u8], title: &str) -> std::io::Result<(u64, Vec<u8>)> {
    let mut block = title.as_bytes().to_vec();
    let size = block.len().div_ceil(16) * 16;
    block.resize(size, 0);
    let chunks = u8::try_from(size / 16).map_err(|_| std::io::Error::other("icy block"))?;
    let mut body = audio.to_vec();
    body.push(chunks);
    body.extend(block);
    let interval = u64::try_from(audio.len()).map_err(|_| std::io::Error::other("icy interval"))?;
    Ok((interval, body))
}

fn icy_accept(
    listener: &TcpListener,
    cancelled: &AtomicBool,
    recorded: &std::sync::Mutex<Vec<String>>,
    noted: &std::sync::Mutex<Vec<String>>,
    interval: u64,
    body: &[u8],
) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                let mut request = Vec::new();
                while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte)?;
                    request.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("").to_owned();
                let header = text
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("icy-metadata")
                            .then(|| value.trim().to_owned())
                    })
                    .unwrap_or_default();
                if let Ok(mut paths) = recorded.lock() {
                    paths.push(path);
                }
                if let Ok(mut headers) = noted.lock() {
                    headers.push(header);
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nicy-metaint: {interval}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn icy_metadata_stays_out_of_the_audio_hash() -> TestResult {
    let directory = tempfile::tempdir()?;
    let audio = wave();
    let title = "StreamTitle='Owned Station';StreamUrl='http://127.0.0.1/secret-stream';";
    let server = IcyServer::start(&audio, title)?;
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "source",
            "add",
            "icy:v1",
            "--name",
            "Icy",
            "--url",
            &format!("{}/audio", server.origin),
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "plain",
            "--source",
            "icy:v1",
            "--seconds",
            "30",
            "--max-mib",
            "1",
        ],
    )?;
    let failed = wait_terminal(directory.path(), "plain")?;
    assert_eq!(failed["state"], "failed", "{failed}");
    success(
        directory.path(),
        &[
            "record",
            "start",
            "titled",
            "--source",
            "icy:v1",
            "--seconds",
            "30",
            "--max-mib",
            "1",
            "--icy",
        ],
    )?;
    let recorded = wait_terminal(directory.path(), "titled")?;
    assert_eq!(recorded["state"], "completed", "{recorded}");
    assert_eq!(recorded["media_bytes"], audio.len());
    let path = success(directory.path(), &["record", "path", "titled"])?;
    let published = std::fs::read(path["path"].as_str().ok_or("missing path")?)?;
    assert_eq!(published, audio);
    let metadata = success(directory.path(), &["record", "metadata", "titled"])?;
    assert_eq!(metadata["schema_version"], 3);
    assert_eq!(metadata["source"]["name"], "Icy");
    let observations = metadata["capture"]["icy_observations"]
        .as_array()
        .ok_or("missing observations")?;
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0]["text"], title);
    assert_eq!(observations[0]["audio_offset"], audio.len());
    let paths = server.paths.lock().map_err(|_| "icy paths")?.clone();
    let headers = server.headers.lock().map_err(|_| "icy headers")?.clone();
    assert_eq!(paths, vec!["/audio".to_owned(), "/audio".to_owned()]);
    assert_eq!(headers, vec!["0".to_owned(), "1".to_owned()]);
    let digest = metadata["storage"]["sha256"]
        .as_str()
        .ok_or("missing audio hash")?;
    assert_eq!(digest, sha256_hex(&audio));
    assert_eq!(digest, sha256_hex(&published));
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

fn transcode(ffmpeg: &str, wav: &[u8], args: &[&str]) -> std::io::Result<Vec<u8>> {
    let mut child = std::process::Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "wav",
            "-i",
            "pipe:0",
        ])
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("transcode stdin"))?;
    stdin.write_all(wav)?;
    drop(stdin);
    let output = child.wait_with_output()?;
    if !output.status.success() || output.stdout.is_empty() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!("transcode failed: {detail}")));
    }
    Ok(output.stdout)
}

struct ServedClip {
    name: String,
    content_type: &'static str,
    body: Vec<u8>,
}

struct LadderServer {
    origin: String,
    paths: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl LadderServer {
    fn start(clips: std::sync::Arc<Vec<ServedClip>>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let origin = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let stop = Arc::new(AtomicBool::new(false));
        let paths = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let recorded = paths.clone();
        let worker = thread::spawn(move || ladder_accept(&listener, &cancelled, &recorded, &clips));
        Ok(Self {
            origin,
            paths,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for LadderServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn ladder_response(path: &str, clips: &[ServedClip]) -> (&'static str, &'static str, Vec<u8>) {
    if path.starts_with("/list") {
        return ("302 Found", "text/plain", Vec::new());
    }
    if path.starts_with("/final.m3u") {
        return (
            "200 OK",
            "audio/x-mpegurl",
            b"#EXTM3U\n#EXTINF:1,tone\nwav\n".to_vec(),
        );
    }
    if let Some(clip) = clips
        .iter()
        .find(|clip| path.starts_with(&format!("/{}", clip.name)))
    {
        return ("200 OK", clip.content_type, clip.body.clone());
    }
    ("404 Not Found", "text/plain", b"missing".to_vec())
}

fn ladder_accept(
    listener: &TcpListener,
    cancelled: &AtomicBool,
    recorded: &std::sync::Mutex<Vec<String>>,
    clips: &[ServedClip],
) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                let mut request = Vec::new();
                while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    request.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("").to_owned();
                if let Ok(mut paths) = recorded.lock() {
                    paths.push(path.clone());
                }
                let (status, content_type, body) = ladder_response(&path, clips);
                if status.starts_with("302") {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 302 Found\r\nLocation: /final.m3u\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                } else {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(&body);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn publish_clip(directory: &std::path::Path, origin: &str, name: &str, format: &str) -> TestResult {
    let revision = format!("fmt-{name}:v1");
    let id = format!("rec-{name}");
    success(
        directory,
        &[
            "source",
            "add",
            &revision,
            "--name",
            name,
            "--url",
            &format!("{origin}/{name}"),
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    success(
        directory,
        &[
            "record",
            "start",
            &id,
            "--source",
            &revision,
            "--seconds",
            "30",
            "--max-mib",
            "2",
        ],
    )?;
    let record = wait_terminal(directory, &id)?;
    assert_eq!(record["state"], "completed", "{record}");
    assert_eq!(record["format"], format, "{record}");
    let decoded = record["decoded_microseconds"]
        .as_u64()
        .ok_or("missing decoded duration")?;
    assert!(decoded > 0, "{record}");
    Ok(())
}

fn served(name: &str, content_type: &'static str, body: Vec<u8>) -> ServedClip {
    ServedClip {
        name: name.to_owned(),
        content_type,
        body,
    }
}

fn generated_clips(ffmpeg: &str, wav: &[u8]) -> std::io::Result<std::sync::Arc<Vec<ServedClip>>> {
    Ok(std::sync::Arc::new(vec![
        served("wav", "audio/wav", wav.to_vec()),
        served(
            "mp3",
            "audio/mpeg",
            transcode(
                ffmpeg,
                wav,
                &["-c:a", "libmp3lame", "-b:a", "64k", "-f", "mp3", "pipe:1"],
            )?,
        ),
        served(
            "aac",
            "audio/aac",
            transcode(
                ffmpeg,
                wav,
                &["-c:a", "aac", "-b:a", "64k", "-f", "adts", "pipe:1"],
            )?,
        ),
        served(
            "flac",
            "audio/flac",
            transcode(ffmpeg, wav, &["-c:a", "flac", "-f", "flac", "pipe:1"])?,
        ),
        served(
            "ogg",
            "audio/ogg",
            transcode(
                ffmpeg,
                wav,
                &["-c:a", "libvorbis", "-q:a", "2", "-f", "ogg", "pipe:1"],
            )?,
        ),
    ]))
}

fn play_redirected_playlist(directory: &std::path::Path, server: &LadderServer) -> TestResult {
    success(
        directory,
        &[
            "source",
            "add",
            "list:v1",
            "--name",
            "List",
            "--url",
            &format!("{}/list", server.origin),
            "--pin-address",
            "127.0.0.1",
            "--redirects",
            "same-origin",
        ],
    )?;
    success(
        directory,
        &[
            "source",
            "playlist",
            "resolve",
            "list-1",
            "--revision",
            "list:v1",
        ],
    )?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut resolved = success(directory, &["source", "playlist", "status", "list-1"])?;
    while resolved["playlist"]["state"] == "running" {
        assert!(Instant::now() < deadline, "{resolved}");
        thread::sleep(Duration::from_millis(30));
        resolved = success(directory, &["source", "playlist", "status", "list-1"])?;
    }
    assert_eq!(resolved["playlist"]["state"], "completed", "{resolved}");
    success(
        directory,
        &[
            "source",
            "playlist",
            "accept",
            "list-1",
            "--index",
            "0",
            "--revision",
            "entry:v1",
            "--name",
            "Entry",
        ],
    )?;
    success(
        directory,
        &[
            "record",
            "start",
            "entry-rec",
            "--source",
            "entry:v1",
            "--seconds",
            "30",
            "--max-mib",
            "2",
        ],
    )?;
    let entry = wait_terminal(directory, "entry-rec")?;
    assert_eq!(entry["state"], "completed", "{entry}");
    assert_eq!(entry["format"], "wav", "{entry}");
    let played = success(
        directory,
        &[
            "listen",
            "source",
            "entry-listen",
            "--revision",
            "entry:v1",
            "--destination",
            "null",
        ],
    )?;
    assert_eq!(played["destination"], "null", "{played}");
    assert_eq!(played["format"], "wav", "{played}");
    assert_eq!(played["progress_advanced"], true, "{played}");
    let playhead = played["playhead_us"].as_u64().ok_or("missing playhead")?;
    assert!(playhead > 0, "{played}");
    let paths = server.paths.lock().map_err(|_| "ladder paths")?.clone();
    assert_local_paths(&paths);
    Ok(())
}

fn assert_local_paths(paths: &[String]) {
    assert!(paths.iter().any(|path| path.starts_with("/list")));
    assert!(paths.iter().any(|path| path.starts_with("/final.m3u")));
    for name in ["wav", "mp3", "aac", "flac", "ogg"] {
        assert!(
            paths
                .iter()
                .any(|path| path.starts_with(&format!("/{name}"))),
            "{paths:?}"
        );
    }
    for path in paths {
        let expected = path.starts_with("/list")
            || path.starts_with("/final.m3u")
            || ["wav", "mp3", "aac", "flac", "ogg"]
                .iter()
                .any(|name| path.starts_with(&format!("/{name}")));
        assert!(expected, "{paths:?}");
    }
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn local_ladder_decodes_generated_formats_without_a_public_station() -> TestResult {
    let ffmpeg = std::env::var("SIGY_TEST_FFMPEG")?;
    let server = LadderServer::start(generated_clips(&ffmpeg, &wave())?)?;
    let directory = tempfile::tempdir()?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &ffmpeg, "--quota-gb", "1"],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    for (name, format) in [
        ("wav", "wav"),
        ("mp3", "mp3"),
        ("aac", "aac"),
        ("flac", "flac"),
        ("ogg", "ogg"),
    ] {
        publish_clip(directory.path(), &server.origin, name, format)?;
    }
    play_redirected_playlist(directory.path(), &server)?;
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

struct EnclosureHost {
    port: u16,
    paths: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl EnclosureHost {
    fn start(body: Vec<u8>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let recorded = paths.clone();
        let worker =
            thread::spawn(move || serve_enclosure(listener, cancelled, recorded, port, body));
        Ok(Self {
            port,
            paths,
            stop,
            worker: Some(worker),
        })
    }

    fn feed_url(&self) -> String {
        format!("http://fixture.invalid:{}/feed.xml?token=hidden", self.port)
    }
}

impl Drop for EnclosureHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve_enclosure(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    paths: Arc<Mutex<Vec<String>>>,
    port: u16,
    body: Vec<u8>,
) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        match listener.accept() {
            Ok((mut socket, _)) => {
                socket.set_nonblocking(false)?;
                socket.set_read_timeout(Some(Duration::from_secs(5)))?;
                socket.set_write_timeout(Some(Duration::from_secs(5)))?;
                let mut request = Vec::new();
                while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if socket.read_exact(&mut byte).is_err() {
                        break;
                    }
                    request.push(byte[0]);
                }
                let path = String::from_utf8_lossy(&request)
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_owned();
                if let Ok(mut guard) = paths.lock() {
                    guard.push(path.clone());
                }
                let _ = socket.write_all(&enclosure_response(&path, port, &body));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    drop((listener, stop, paths, body));
    Ok(())
}

fn wait_episode_feed(directory: &std::path::Path, id: &str) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = success(directory, &["podcast", "refresh-status", id])?;
        if status["podcast_feed"]["refresh"]["state"] != "running" {
            assert_eq!(
                status["podcast_feed"]["refresh"]["state"], "completed",
                "{status}"
            );
            return Ok(());
        }
        assert!(Instant::now() < deadline, "{status}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn assert_retained_episode(directory: &std::path::Path, bytes: &[u8]) -> TestResult {
    let record = wait_recording(directory, "episode:v1", "completed")?;
    assert_eq!(record["profile"], "episode");
    assert_eq!(record["source_revision"], "enc:v1");
    assert_eq!(record["duration_seconds"], 1800);
    assert_eq!(record["maximum_bytes"], 512 * 1024 * 1024);
    assert_eq!(record["storage_state"], "retained");
    assert_eq!(record["end_reason"], "end_of_body");
    assert_eq!(record["media_bytes"], bytes.len());
    assert_eq!(record["sha256"], sha256_hex(bytes));
    assert!(
        record["decoded_microseconds"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );
    let path = success(directory, &["record", "path", "episode:v1"])?;
    let media = path["path"].as_str().ok_or("missing media path")?;
    assert_eq!(std::fs::read(media)?, bytes);
    let metadata = success(directory, &["record", "metadata", "episode:v1"])?;
    assert_eq!(metadata["storage"]["sha256"], record["sha256"]);
    assert!(!metadata.to_string().contains("token=hidden"));
    let listed = success(directory, &["record", "list"])?;
    assert_eq!(listed["recording_page"]["entries"][0]["id"], "episode:v1");
    success(directory, &["record", "keep", "episode:v1"])?;
    let kept = success(directory, &["record", "show", "episode:v1"])?;
    assert_eq!(kept["recording_page"]["entries"][0]["retention"], "kept");
    let policy = success(directory, &["dvr", "status"])?;
    assert_eq!(policy["dvr"]["charged_bytes"], bytes.len());
    assert_eq!(policy["dvr"]["reserved_bytes"], 0);
    let source = success(directory, &["source", "show", "enc:v1"])?;
    assert_eq!(source["source_page"]["entries"][0]["revision_id"], "enc:v1");
    assert!(!source.to_string().contains("token=hidden"));
    Ok(())
}

fn enclosure_response(path: &str, port: u16, body: &[u8]) -> Vec<u8> {
    if path.starts_with("/feed.xml") {
        let document = format!(
            r#"<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><item><guid>ep-1</guid><title>Episode</title><enclosure url="http://fixture.invalid:{port}/episode.wav?token=hidden" length="{}" type="audio/wav"/><podcast:transcript url="http://fixture.invalid:{port}/notes.vtt?token=hidden" type="text/vtt"/></item></channel></rss>"#,
            body.len()
        );
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            document.len()
        )
        .into_bytes();
        response.extend(document.as_bytes());
        response
    } else if path.starts_with("/episode.wav") {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend(body);
        response
    } else {
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
    }
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn episode_enclosure_is_one_retained_recording() -> TestResult {
    let bytes = wave();
    let server = EnclosureHost::start(bytes.clone())?;
    let directory = tempfile::tempdir()?;
    let ffmpeg = std::env::var("SIGY_TEST_FFMPEG")?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &ffmpeg, "--quota-gb", "1"],
    )?;
    let feed = server.feed_url();
    success(
        directory.path(),
        &[
            "podcast",
            "subscribe",
            "show:v1",
            "--url",
            &feed,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &["podcast", "refresh", "show:v1", "--id", "feed:v1"],
    )?;
    wait_episode_feed(directory.path(), "feed:v1")?;
    let episodes = success(directory.path(), &["podcast", "episodes", "show:v1"])?;
    let episode_id = episodes["podcast_feed"]["episodes"][0]["id"]
        .as_str()
        .ok_or("episode id")?
        .to_owned();
    assert_eq!(episodes["podcast_feed"]["episodes"][0]["enclosure"], true);
    let download = [
        "podcast",
        "download",
        "show:v1",
        "--episode",
        episode_id.as_str(),
        "--id",
        "episode:v1",
        "--revision",
        "enc:v1",
    ];
    success(directory.path(), &download)?;
    assert_retained_episode(directory.path(), &bytes)?;
    let audio_hits = server
        .paths
        .lock()
        .map_err(|_| "path lock")?
        .iter()
        .filter(|path| path.starts_with("/episode.wav"))
        .count();
    assert_eq!(audio_hits, 1);
    success(directory.path(), &download)?;
    thread::sleep(Duration::from_millis(400));
    let again = invoke(
        directory.path(),
        &[
            "podcast",
            "download",
            "show:v1",
            "--episode",
            &episode_id,
            "--id",
            "episode:v2",
            "--revision",
            "enc:v2",
        ],
    )?;
    assert!(!again.status.success());
    thread::sleep(Duration::from_millis(400));
    let seen = server.paths.lock().map_err(|_| "path lock")?;
    assert_eq!(
        seen.iter()
            .filter(|path| path.starts_with("/episode.wav"))
            .count(),
        1,
        "{seen:?}"
    );
    assert!(
        seen.iter().all(|path| !path.contains("notes.vtt")),
        "{seen:?}"
    );
    drop(seen);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn retained_episode_plays_under_the_default_policy_without_a_live_edge() -> TestResult {
    let bytes = wave();
    let server = EnclosureHost::start(bytes.clone())?;
    let directory = tempfile::tempdir()?;
    subscribe_default_episode(directory.path(), &server)?;
    let mut service = RunningChild::start(directory.path())?;
    let decoded = download_inspected_episode(directory.path(), &server, &bytes)?;
    play_retained_episode(directory.path(), &bytes, decoded)?;
    assert_episode_has_no_live_edge(directory.path(), &server, bytes.len())?;
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

fn subscribe_default_episode(directory: &std::path::Path, server: &EnclosureHost) -> TestResult {
    let ffmpeg = std::env::var("SIGY_TEST_FFMPEG")?;
    success(directory, &["library", "init"])?;
    success(directory, &["dvr", "configure", "--decoder", &ffmpeg])?;
    let policy = success(directory, &["dvr", "status"])?;
    assert_eq!(policy["dvr"]["retention_days"], 14);
    assert_eq!(policy["dvr"]["quota_bytes"], 50_000_000_000_u64);
    assert_eq!(policy["dvr"]["charged_bytes"], 0);
    assert_eq!(
        policy["schema_version"],
        sigy_service::storage::SCHEMA_VERSION
    );
    success(
        directory,
        &[
            "podcast",
            "subscribe",
            "show:v1",
            "--url",
            &server.feed_url(),
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    let shown = success(directory, &["podcast", "show", "show:v1"])?;
    assert_eq!(shown["podcast_page"]["entries"][0]["polls"], "active");
    assert_eq!(shown["captures"]["scheduled"], 0);
    assert_eq!(shown["captures"]["active"], 0);
    assert!(shown["source_page"].is_null());
    assert_no_recordings(directory)
}

fn download_inspected_episode(
    directory: &std::path::Path,
    server: &EnclosureHost,
    bytes: &[u8],
) -> Result<u64, Box<dyn std::error::Error>> {
    success(
        directory,
        &["podcast", "refresh", "show:v1", "--id", "feed:v1"],
    )?;
    wait_episode_feed(directory, "feed:v1")?;
    let episodes = success(directory, &["podcast", "episodes", "show:v1"])?;
    let episode_id = episodes["podcast_feed"]["episodes"][0]["id"]
        .as_str()
        .ok_or("episode id")?
        .to_owned();
    assert_eq!(episodes["podcast_feed"]["episodes"][0]["enclosure"], true);
    assert_eq!(episodes["captures"]["active"], 0);
    assert_eq!(episodes["captures"]["scheduled"], 0);
    assert!(!episodes.to_string().contains("token=hidden"));
    assert_no_recordings(directory)?;
    assert_eq!(enclosure_hits(server, "/episode.wav")?, 0);
    let early = invoke(
        directory,
        &["listen", "file", "episode:v1", "--destination", "null"],
    )?;
    assert!(!early.status.success());
    success(
        directory,
        &[
            "podcast",
            "download",
            "show:v1",
            "--episode",
            &episode_id,
            "--id",
            "episode:v1",
            "--revision",
            "enc:v1",
        ],
    )?;
    let record = wait_recording(directory, "episode:v1", "completed")?;
    assert_eq!(record["profile"], "episode");
    assert_eq!(record["retention"], "temporary");
    assert_eq!(record["storage_state"], "retained");
    assert_eq!(record["end_reason"], "end_of_body");
    assert_eq!(record["media_bytes"], bytes.len());
    let decoded = record["decoded_microseconds"]
        .as_u64()
        .filter(|value| *value > 200_000)
        .ok_or("decoded duration")?;
    let charged = success(directory, &["dvr", "status"])?;
    assert_eq!(charged["dvr"]["retention_days"], 14);
    assert_eq!(charged["dvr"]["quota_bytes"], 50_000_000_000_u64);
    assert_eq!(charged["dvr"]["charged_bytes"], bytes.len());
    assert_eq!(charged["dvr"]["reserved_bytes"], 0);
    assert_eq!(enclosure_hits(server, "/episode.wav")?, 1);
    Ok(decoded)
}

fn play_retained_episode(directory: &std::path::Path, bytes: &[u8], decoded: u64) -> TestResult {
    let outside = invoke(
        directory,
        &[
            "listen",
            "file",
            "episode:v1",
            "--destination",
            "null",
            "--seek-us",
            &decoded.to_string(),
        ],
    )?;
    assert!(!outside.status.success());
    let played = success(
        directory,
        &[
            "listen",
            "file",
            "episode:v1",
            "--destination",
            "null",
            "--seek-us",
            "200000",
        ],
    )?;
    assert_eq!(played["id"], "episode:v1");
    assert_eq!(played["destination"], "null");
    assert_eq!(played["progress_advanced"], true);
    let playhead = played["playhead_us"].as_u64().ok_or("missing playhead")?;
    assert!((200_001..=decoded).contains(&playhead), "{played}");
    let after = success(directory, &["dvr", "status"])?;
    assert_eq!(after["dvr"]["charged_bytes"], bytes.len());
    assert_eq!(after["dvr"]["reserved_bytes"], 0);
    let shown = success(directory, &["record", "show", "episode:v1"])?;
    assert_eq!(
        shown["recording_page"]["entries"][0]["retention"],
        "temporary"
    );
    assert_eq!(
        shown["recording_page"]["entries"][0]["storage_state"],
        "retained"
    );
    Ok(())
}

fn assert_episode_has_no_live_edge(
    directory: &std::path::Path,
    server: &EnclosureHost,
    media_bytes: usize,
) -> TestResult {
    assert_eq!(enclosure_hits(server, "/episode.wav")?, 1);
    let live = invoke(
        directory,
        &[
            "listen",
            "source",
            "live",
            "--revision",
            "enc:v1",
            "--destination",
            "null",
        ],
    )?;
    assert!(!live.status.success());
    let stderr = String::from_utf8_lossy(&live.stderr);
    assert!(stderr.contains("episode has no live edge"), "{stderr}");
    assert!(!stderr.contains("token=hidden"));
    thread::sleep(Duration::from_millis(400));
    assert_eq!(enclosure_hits(server, "/episode.wav")?, 1);
    let missing = invoke(directory, &["listen", "status", "live"])?;
    assert!(!missing.status.success());
    let feed_hits = enclosure_hits(server, "/feed.xml")?;
    success(
        directory,
        &["podcast", "refresh", "show:v1", "--id", "feed:v1"],
    )?;
    thread::sleep(Duration::from_millis(400));
    assert_eq!(enclosure_hits(server, "/feed.xml")?, feed_hits);
    thread::sleep(Duration::from_secs(3));
    success(
        directory,
        &["podcast", "refresh", "show:v1", "--id", "feed:v2"],
    )?;
    wait_episode_feed(directory, "feed:v2")?;
    assert_eq!(enclosure_hits(server, "/episode.wav")?, 1);
    let paths = server.paths.lock().map_err(|_| "path lock")?;
    assert!(
        paths.iter().all(|path| !path.contains("notes.vtt")),
        "{paths:?}"
    );
    drop(paths);
    let listed = success(directory, &["record", "list"])?;
    let entries = listed["recording_page"]["entries"]
        .as_array()
        .ok_or("records")?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], "episode:v1");
    let policy = success(directory, &["dvr", "status"])?;
    assert_eq!(policy["dvr"]["charged_bytes"], media_bytes);
    assert_eq!(policy["dvr"]["retention_days"], 14);
    Ok(())
}

fn assert_no_recordings(directory: &std::path::Path) -> TestResult {
    let listed = success(directory, &["record", "list"])?;
    let entries = listed["recording_page"]["entries"]
        .as_array()
        .ok_or("records")?;
    assert!(entries.is_empty(), "{listed}");
    Ok(())
}

fn enclosure_hits(
    server: &EnclosureHost,
    prefix: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(server
        .paths
        .lock()
        .map_err(|_| "path lock")?
        .iter()
        .filter(|path| path.starts_with(prefix))
        .count())
}

struct SegmentServer {
    url: String,
    hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    stop: Arc<AtomicBool>,
    /// Holds the stream open after the second segment until the test releases it.
    release: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl SegmentServer {
    fn start(first: Vec<u8>, second: Vec<u8>) -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://127.0.0.1:{}/audio", listener.local_addr()?.port());
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let hits_worker = hits.clone();
        let stop_worker = stop.clone();
        let release_worker = release.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(40);
            while !stop_worker.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        hits_worker.fetch_add(1, Ordering::Relaxed);
                        stream.set_nonblocking(false)?;
                        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                        let mut request = Vec::new();
                        while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            stream.read_exact(&mut byte)?;
                            request.push(byte[0]);
                        }
                        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")?;
                        write_paced(&mut stream, &first, Duration::from_millis(3500))?;
                        thread::sleep(Duration::from_millis(3500));
                        write_paced(&mut stream, &second, Duration::from_millis(3500))?;
                        // Keep the tail open while the test inspects it, without depending on
                        // how long playback takes on a loaded host. Stay under the stall timeout.
                        let hold = Instant::now() + Duration::from_secs(10);
                        thread::sleep(Duration::from_millis(3000));
                        while !release_worker.load(Ordering::Relaxed) && Instant::now() < hold {
                            thread::sleep(Duration::from_millis(20));
                        }
                        stream.write_all(b"0\r\n\r\n")?;
                        stream.flush()?;
                        let _ = stream.shutdown(std::net::Shutdown::Write);
                        let extra = Instant::now() + Duration::from_millis(300);
                        while Instant::now() < extra {
                            if listener.accept().is_ok() {
                                hits_worker.fetch_add(1, Ordering::Relaxed);
                            }
                            thread::sleep(Duration::from_millis(20));
                        }
                        return Ok(());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(std::io::Error::other("segment fixture was not contacted"))
        });
        Ok(Self {
            url,
            hits,
            stop,
            release,
            worker: Some(worker),
        })
    }

    fn release_tail(&self) {
        self.release
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn hits(&self) -> usize {
        self.hits.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn finish(&mut self) -> TestResult {
        self.worker
            .take()
            .ok_or("segment fixture already joined")?
            .join()
            .map_err(|_| "segment fixture panicked")??;
        Ok(())
    }
}

impl Drop for SegmentServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn write_paced(stream: &mut impl Write, bytes: &[u8], pace: Duration) -> std::io::Result<()> {
    let pieces = 8_usize;
    let pause = pace / u32::try_from(pieces).map_err(std::io::Error::other)?;
    for index in 0..pieces {
        let start = bytes.len() * index / pieces;
        let end = if index + 1 == pieces {
            bytes.len()
        } else {
            bytes.len() * (index + 1) / pieces
        };
        write_chunk(stream, &bytes[start..end])?;
        thread::sleep(pause);
    }
    Ok(())
}

fn write_chunk(stream: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    write!(stream, "{:X}\r\n", bytes.len())?;
    stream.write_all(bytes)?;
    stream.write_all(b"\r\n")?;
    stream.flush()
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn running_capture_seals_ordered_segments_on_one_socket() -> TestResult {
    use sha2::{Digest, Sha256};
    let directory = tempfile::tempdir()?;
    let audio = wave();
    let mut server = SegmentServer::start(audio.clone(), audio.clone())?;
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    success(
        directory.path(),
        &[
            "source",
            "add",
            "radio:v1",
            "--name",
            "Radio francophone",
            "--url",
            &server.url,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "segments",
            "--source",
            "radio:v1",
            "--seconds",
            "60",
            "--max-mib",
            "64",
        ],
    )?;
    let first = wait_segments(directory.path(), 1)?;
    assert_eq!(first["state"], "running");
    assert_eq!(server.hits(), 1);
    assert_eq!(first["open_ceiling"], 32 * 1024 * 1024);
    assert_eq!(first["charged_bytes"], 64 * 1024 * 1024);
    assert_eq!(first["lease_renewals"], 1);
    assert_eq!(
        success(directory.path(), &["dvr", "status"])?["dvr"]["reserved_bytes"],
        64 * 1024 * 1024
    );
    let second = wait_segments(directory.path(), 2)?;
    assert_eq!(second["state"], "running");
    assert_eq!(server.hits(), 1);
    let intervals = second["intervals"].as_array().ok_or("intervals")?;
    assert_eq!(intervals[0]["ordinal"], 0);
    assert_eq!(intervals[1]["ordinal"], 1);
    assert_eq!(intervals[1]["byte_start"], intervals[0]["byte_end"]);
    assert_eq!(
        intervals[1]["decoded_start_us"],
        intervals[0]["decoded_end_us"]
    );
    let checked = play_two_segments_without_stopping_capture(directory.path(), intervals);
    server.release_tail();
    checked?;
    server.finish()?;
    let done = wait_recording(directory.path(), "segments", "completed")?;
    assert_eq!(server.hits(), 1);
    assert_eq!(done["open_ceiling"], 0);
    assert_eq!(done["escrow_bytes"], 0);
    assert_eq!(done["media_bytes"], audio.len() * 2);
    assert_eq!(done["charged_bytes"], audio.len() * 2);
    assert_eq!(done["end_reason"], "end_of_body");
    let intervals = done["intervals"].as_array().ok_or("intervals")?;
    assert_eq!(intervals.len(), 2);
    let segment_sha = hex_encode(&Sha256::digest(audio.as_slice()));
    assert_eq!(intervals[0]["sha256"], segment_sha);
    assert_eq!(intervals[1]["sha256"], segment_sha);
    assert_eq!(intervals[0]["byte_end"], audio.len());
    let mut whole = audio.clone();
    whole.extend_from_slice(&audio);
    assert_eq!(done["sha256"], hex_encode(&Sha256::digest(&whole)));
    assert_eq!(
        success(directory.path(), &["dvr", "status"])?["dvr"]["reserved_bytes"],
        0
    );
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut output, byte| {
        let _ = write!(output, "{byte:02x}");
        output
    })
}

fn play_two_segments_without_stopping_capture(
    directory: &std::path::Path,
    intervals: &[serde_json::Value],
) -> Result<(), Box<dyn std::error::Error>> {
    let played = success(
        directory,
        &[
            "listen",
            "file",
            "segments",
            "--destination",
            "null",
            "--seek-us",
            "200000",
        ],
    )?;
    if played["segment_ordinal"] != 0 {
        return Err(format!("segment 0 was not played: {played}").into());
    }
    let second_start = intervals[1]["decoded_start_us"].as_u64().ok_or("start")?;
    let second = success(
        directory,
        &[
            "listen",
            "file",
            "segments",
            "--destination",
            "null",
            "--seek-us",
            &second_start.to_string(),
        ],
    )?;
    if second["segment_ordinal"] != 1 {
        return Err(format!("segment 1 was not played: {second}").into());
    }
    let live = intervals[1]["decoded_end_us"].as_u64().ok_or("live")?;
    let tail = super::invoke(
        directory,
        &[
            "listen",
            "file",
            "segments",
            "--destination",
            "null",
            "--seek-us",
            &live.to_string(),
        ],
    )?;
    let tail_error = String::from_utf8_lossy(&tail.stderr);
    if tail.status.success() || !tail_error.contains("open tail") {
        return Err(format!("open tail was readable: {tail_error}").into());
    }
    independent_playheads(directory, intervals)
}

fn independent_playheads(
    directory: &std::path::Path,
    intervals: &[serde_json::Value],
) -> Result<(), Box<dyn std::error::Error>> {
    success(
        directory,
        &["listen", "attach", "listener-a", "--recording", "segments"],
    )?;
    success(directory, &["listen", "pause", "listener-a"])?;
    success(
        directory,
        &["listen", "attach", "listener-b", "--recording", "segments"],
    )?;
    let live = intervals[1]["decoded_end_us"].as_u64().ok_or("live")?;
    let tail = super::invoke(
        directory,
        &[
            "listen",
            "seek",
            "listener-b",
            "--seek-us",
            &live.to_string(),
        ],
    )?;
    let tail_error = String::from_utf8_lossy(&tail.stderr);
    if tail.status.success() || !tail_error.contains("open tail") {
        return Err(format!("session seek read the open tail: {tail_error}").into());
    }
    let moved = success(
        directory,
        &["listen", "seek", "listener-b", "--seek-us", "200000"],
    )?;
    if moved["playhead_us"] != 200_000 || moved["state"] == "paused" {
        return Err(format!("seek did not enter the segment: {moved}").into());
    }
    let paused = success(directory, &["listen", "session", "listener-a"])?;
    if paused["state"] != "paused" || paused["playhead_us"] == 200_000 {
        return Err(format!("playheads were not independent: {paused}").into());
    }
    let played = success(
        directory,
        &["listen", "play", "listener-b", "--destination", "null"],
    )?;
    if played["progress_advanced"] != true || played["detached"] != true {
        return Err(format!("session play did not finish: {played}").into());
    }
    let closed = super::invoke(directory, &["listen", "session", "listener-b"])?;
    if closed.status.success() {
        return Err("client exit left the playhead attached".into());
    }
    success(directory, &["listen", "detach", "listener-a"])?;
    let still = success(directory, &["record", "show", "segments"])?;
    let still = &still["recording_page"]["entries"][0];
    let state = still["state"].as_str().unwrap_or("");
    if state == "failed" || state == "interrupted" {
        return Err(format!("capture stopped: {still}").into());
    }
    if still["gaps"]
        .as_array()
        .is_some_and(|gaps| !gaps.is_empty())
    {
        return Err("playback pause wrote a gap".into());
    }
    Ok(())
}

fn wait_segments(
    directory: &std::path::Path,
    count: usize,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let response = success(directory, &["record", "show", "segments"])?;
        let record = &response["recording_page"]["entries"][0];
        let intervals = record["intervals"].as_array().map_or(0, Vec::len);
        if record["state"] == "failed" {
            return Err(format!("segment capture failed: {record}").into());
        }
        let open = record["open_ceiling"].as_u64().unwrap_or(0) == 32 * 1024 * 1024;
        let renewed = record["lease_renewals"].as_u64().unwrap_or(0) >= 1;
        if intervals >= count && record["state"] == "running" && renewed && open {
            return Ok(record.clone());
        }
        if Instant::now() >= deadline {
            return Err(format!("segment seal deadline: {record}").into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}
