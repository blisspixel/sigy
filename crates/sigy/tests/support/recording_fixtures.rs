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
