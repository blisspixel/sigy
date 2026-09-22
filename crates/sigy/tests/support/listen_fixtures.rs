use super::{RunningChild, TestResult, invoke, success};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Shared {
    hits: Arc<AtomicUsize>,
    hold: Arc<AtomicBool>,
    drip: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    content_type: String,
    extra: String,
    body: Vec<u8>,
}

struct CountServer {
    url: String,
    shared: Shared,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl CountServer {
    fn start(content_type: &str, extra: &str, body: Vec<u8>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://127.0.0.1:{}/audio", listener.local_addr()?.port());
        let shared = Shared {
            hits: Arc::new(AtomicUsize::new(0)),
            hold: Arc::new(AtomicBool::new(false)),
            drip: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            content_type: content_type.to_owned(),
            extra: extra.to_owned(),
            body,
        };
        let worker_state = shared.clone();
        let worker = thread::spawn(move || accept_loop(&listener, &worker_state));
        Ok(Self {
            url,
            shared,
            worker: Some(worker),
        })
    }

    fn hits(&self) -> usize {
        self.shared.hits.load(Ordering::Relaxed)
    }

    fn hold(&self, value: bool) {
        self.shared.hold.store(value, Ordering::Relaxed);
    }

    fn drip(&self, value: bool) {
        self.shared.drip.store(value, Ordering::Relaxed);
    }
}

impl Drop for CountServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn accept_loop(listener: &TcpListener, shared: &Shared) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut threads = Vec::new();
    while !shared.stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        match listener.accept() {
            Ok((socket, _)) => {
                let shared = shared.clone();
                threads.push(thread::spawn(move || {
                    let _ = answer(socket, &shared);
                }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    for thread in threads {
        let _ = thread.join();
    }
    Ok(())
}

fn answer(mut socket: std::net::TcpStream, shared: &Shared) -> std::io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::new();
    while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        socket.read_exact(&mut byte)?;
        request.push(byte[0]);
    }
    shared.hits.fetch_add(1, Ordering::Relaxed);
    if shared.hold.load(Ordering::Relaxed) {
        let started = Instant::now();
        while !shared.stop.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(8) {
            thread::sleep(Duration::from_millis(20));
        }
        return Ok(());
    }
    if shared.drip.load(Ordering::Relaxed) {
        return drip_audio(&mut socket, &shared.stop);
    }
    write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
        shared.content_type,
        shared.extra,
        shared.body.len()
    )?;
    socket.write_all(&shared.body)?;
    Ok(())
}

fn drip_audio(socket: &mut std::net::TcpStream, stop: &AtomicBool) -> std::io::Result<()> {
    // A slow body keeps the recording inside its read timeout without finishing.
    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n")?;
    let mut sent = 0_usize;
    while sent < 4096 && !stop.load(Ordering::Relaxed) {
        socket.write_all(&[0])?;
        sent += 1;
        let started = Instant::now();
        while !stop.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(1) {
            thread::sleep(Duration::from_millis(20));
        }
    }
    Ok(())
}

fn wait_hits(server: &CountServer, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.hits() < count {
        assert!(Instant::now() < deadline, "audio fixture was not contacted");
        thread::sleep(Duration::from_millis(20));
    }
}

fn assert_quota(
    directory: &std::path::Path,
    charged: &serde_json::Value,
    reserved: &serde_json::Value,
) -> TestResult {
    let status = success(directory, &["dvr", "status"])?;
    assert_eq!(&status["dvr"]["charged_bytes"], charged);
    assert_eq!(&status["dvr"]["reserved_bytes"], reserved);
    let listed = success(directory, &["record", "list"])?;
    assert!(
        listed["recording_page"]["entries"]
            .as_array()
            .ok_or("records")?
            .is_empty()
    );
    Ok(())
}

fn assert_no_audio_files(directory: &std::path::Path) -> TestResult {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let lower = name.to_ascii_lowercase();
        let extension = std::path::Path::new(lower.as_str()).extension();
        assert!(
            !extension.is_some_and(|ext| {
                ext.eq_ignore_ascii_case("media")
                    || ext.eq_ignore_ascii_case("part")
                    || ext.eq_ignore_ascii_case("wav")
            }),
            "{name}"
        );
        if entry.path().is_dir() && lower == "media" {
            assert_eq!(std::fs::read_dir(entry.path())?.count(), 0, "{name}");
        }
    }
    Ok(())
}

fn register(directory: &std::path::Path, id: &str, url: &str) -> TestResult {
    success(
        directory,
        &[
            "source",
            "add",
            id,
            "--name",
            "Radio",
            "--url",
            url,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    Ok(())
}

#[test]
fn playlist_listen_fails_before_decode_and_replay_does_not_reconnect() -> TestResult {
    let directory = tempfile::tempdir()?;
    let server = CountServer::start(
        "audio/x-mpegurl",
        "",
        b"http://127.0.0.1/secret-stream\n".to_vec(),
    )?;
    success(directory.path(), &["library", "init"])?;
    register(directory.path(), "radio:v1", &server.url)?;
    let mut service = RunningChild::start(directory.path())?;
    let before = success(directory.path(), &["dvr", "status"])?;
    let failed = invoke(
        directory.path(),
        &[
            "listen",
            "source",
            "bad",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert!(!failed.status.success());
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(
        rendered.contains("unsupported audio content type"),
        "{rendered}"
    );
    assert!(!rendered.contains("secret-stream"), "{rendered}");
    assert_eq!(server.hits(), 1);
    let status = success(directory.path(), &["listen", "status", "bad"])?;
    assert_eq!(status["listen"]["state"], "failed");
    assert!(status["listen"]["pipe_nonce"].is_null());
    assert!(!status.to_string().contains("secret-stream"));
    let replay = invoke(
        directory.path(),
        &[
            "listen",
            "source",
            "bad",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert!(!replay.status.success());
    thread::sleep(Duration::from_millis(200));
    assert_eq!(server.hits(), 1);
    assert_quota(
        directory.path(),
        &before["dvr"]["charged_bytes"],
        &before["dvr"]["reserved_bytes"],
    )?;
    assert_no_audio_files(directory.path())?;
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[test]
fn icy_listen_fails_before_decode() -> TestResult {
    let directory = tempfile::tempdir()?;
    let server = CountServer::start(
        "audio/mpeg",
        "icy-metaint: 16\r\n",
        b"secret-stream".to_vec(),
    )?;
    success(directory.path(), &["library", "init"])?;
    register(directory.path(), "radio:v1", &server.url)?;
    let mut service = RunningChild::start(directory.path())?;
    let failed = invoke(
        directory.path(),
        &[
            "listen",
            "source",
            "icy",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert!(!failed.status.success());
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(
        rendered.contains("interleaved ICY metadata is unsupported"),
        "{rendered}"
    );
    assert!(!rendered.contains("secret-stream"), "{rendered}");
    assert_eq!(server.hits(), 1);
    let replay = invoke(
        directory.path(),
        &[
            "listen",
            "source",
            "icy",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert!(!replay.status.success());
    thread::sleep(Duration::from_millis(200));
    assert_eq!(server.hits(), 1);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[test]
fn stopping_a_listen_leaves_the_recording_running() -> TestResult {
    let directory = tempfile::tempdir()?;
    let decoder = std::env::current_exe()?;
    let decoder = decoder.to_str().ok_or("decoder path")?;
    let record_server = CountServer::start("audio/wav", "", Vec::new())?;
    record_server.drip(true);
    let side_server = CountServer::start("audio/wav", "", Vec::new())?;
    side_server.hold(true);
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", decoder, "--quota-gb", "1"],
    )?;
    register(directory.path(), "radio:v1", &record_server.url)?;
    register(directory.path(), "radio:v2", &side_server.url)?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "take",
            "--source",
            "radio:v1",
            "--seconds",
            "30",
            "--max-mib",
            "1",
        ],
    )?;
    wait_hits(&record_server, 1);
    let refused = invoke(
        directory.path(),
        &[
            "listen",
            "source",
            "same",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert!(!refused.status.success());
    let refused_text = String::from_utf8_lossy(&refused.stderr);
    assert!(
        refused_text.contains("source revision is in use"),
        "{refused_text}"
    );
    assert_eq!(record_server.hits(), 1);
    let mut side = Command::new(env!("CARGO_BIN_EXE_sigy"))
        .arg("--data-dir")
        .arg(directory.path())
        .args([
            "--json",
            "listen",
            "source",
            "side",
            "--revision",
            "radio:v2",
            "--destination",
            "null",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    wait_hits(&side_server, 1);
    let running = success(directory.path(), &["listen", "status", "side"])?;
    assert_eq!(running["listen"]["state"], "running");
    success(directory.path(), &["listen", "stop", "side"])?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = success(directory.path(), &["listen", "status", "side"])?;
        if status["listen"]["state"] != "running" {
            assert_eq!(status["listen"]["state"], "interrupted");
            break;
        }
        assert!(Instant::now() < deadline, "listen stop deadline");
        thread::sleep(Duration::from_millis(20));
    }
    let recording = success(directory.path(), &["record", "show", "take"])?;
    assert_eq!(
        recording["recording_page"]["entries"][0]["state"],
        "starting"
    );
    success(directory.path(), &["record", "stop", "take"])?;
    let _ = side.wait();
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

fn assert_progress(
    directory: &std::path::Path,
    server: &CountServer,
    before: &serde_json::Value,
) -> TestResult {
    let played = success(
        directory,
        &[
            "listen",
            "source",
            "live",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert_eq!(played["replayed"], false);
    assert_eq!(played["progress_advanced"], true);
    assert!(played["playhead_us"].as_u64().ok_or("playhead")? > 0);
    assert_eq!(server.hits(), 1);
    assert_quota(
        directory,
        &before["dvr"]["charged_bytes"],
        &before["dvr"]["reserved_bytes"],
    )?;
    assert_no_audio_files(directory)?;
    let replay = success(
        directory,
        &[
            "listen",
            "source",
            "live",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert_eq!(replay["replayed"], true);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(server.hits(), 1);
    Ok(())
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

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn direct_listen_reaches_decoder_progress_without_a_file_or_reservation() -> TestResult {
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    let directory = tempfile::tempdir()?;
    let server = CountServer::start("audio/wav", "", wave())?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    register(directory.path(), "radio:v1", &server.url)?;
    let mut service = RunningChild::start(directory.path())?;
    let before = success(directory.path(), &["dvr", "status"])?;
    assert_progress(directory.path(), &server, &before)?;
    server.hold(true);
    let mut child = Command::new(env!("CARGO_BIN_EXE_sigy"))
        .arg("--data-dir")
        .arg(directory.path())
        .args([
            "--json",
            "listen",
            "source",
            "again",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    wait_hits(&server, 2);
    service.0.kill()?;
    service.0.wait()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "listen client did not exit");
        thread::sleep(Duration::from_millis(20));
    }
    assert_no_audio_files(directory.path())?;
    let mut restarted = RunningChild::start(directory.path())?;
    let status = success(directory.path(), &["listen", "status", "again"])?;
    assert_eq!(status["listen"]["state"], "interrupted");
    assert_quota(
        directory.path(),
        &before["dvr"]["charged_bytes"],
        &before["dvr"]["reserved_bytes"],
    )?;
    let replay_killed = invoke(
        directory.path(),
        &[
            "listen",
            "source",
            "again",
            "--revision",
            "radio:v1",
            "--destination",
            "null",
        ],
    )?;
    assert!(!replay_killed.status.success());
    thread::sleep(Duration::from_millis(200));
    assert_eq!(server.hits(), 2);
    success(directory.path(), &["service", "stop"])?;
    restarted.wait()?;
    Ok(())
}
