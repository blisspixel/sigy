//! Live HLS over loopback. Segments are cut by the configured `FFmpeg` from a local tone.

use super::{RunningChild, TestResult, success};
use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

type Windows = fn(usize) -> (u64, u64);

struct LiveServer {
    origin: String,
    paths: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl LiveServer {
    /// `windows` maps the playlist request count to its first sequence and length.
    fn start(
        segments: Vec<Vec<u8>>,
        kind: &'static str,
        windows: Windows,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let origin = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let stop = Arc::new(AtomicBool::new(false));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let recorded = paths.clone();
        let worker = thread::spawn(move || {
            serve(&listener, &cancelled, &recorded, &segments, kind, windows)
        });
        Ok(Self {
            origin,
            paths,
            stop,
            worker: Some(worker),
        })
    }

    fn requested(&self) -> Vec<String> {
        self.paths
            .lock()
            .map(|paths| paths.clone())
            .unwrap_or_default()
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(
    listener: &TcpListener,
    cancelled: &AtomicBool,
    recorded: &Mutex<Vec<String>>,
    segments: &[Vec<u8>],
    kind: &'static str,
    windows: Windows,
) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut counts: HashMap<String, usize> = HashMap::new();
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
                let count = counts.entry(path.clone()).or_insert(0);
                let (content_type, body) = respond(&path, *count, segments, kind, windows);
                *count += 1;
                if let Ok(mut paths) = recorded.lock() {
                    paths.push(path);
                }
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

fn respond(
    path: &str,
    count: usize,
    segments: &[Vec<u8>],
    kind: &'static str,
    windows: Windows,
) -> (&'static str, Vec<u8>) {
    // A query names a separate playlist with its own request count.
    if path.split('?').next() == Some("/live.m3u8") {
        let (first, length) = windows(count);
        let entries = (first..first + length).fold(String::new(), |mut text, sequence| {
            let _ = writeln!(text, "#EXTINF:1.000,\nseg{sequence}");
            text
        });
        let body = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n{entries}"
        );
        return ("application/vnd.apple.mpegurl", body.into_bytes());
    }
    let index = path
        .strip_prefix("/seg")
        .and_then(|number| number.parse::<usize>().ok());
    match index.and_then(|index| segments.get(index)) {
        Some(bytes) => (kind, bytes.clone()),
        None => ("text/plain", b"missing".to_vec()),
    }
}

/// Eight seconds of a 440 Hz tone as 16 kHz mono 16-bit WAV.
fn tone() -> Vec<u8> {
    let rate = 16_000_u32;
    let samples = rate * 8;
    let data = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for index in 0..samples {
        // A square wave near 440 Hz keeps the fixture free of floating point.
        let high = (index * 880 / rate).is_multiple_of(2);
        let sample: i16 = if high { 6000 } else { -6000 };
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// Cuts the tone into one-second segments with the configured `FFmpeg`.
fn cut_segments(
    decoder: &str,
    directory: &Path,
    muxer: &str,
    extension: &str,
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let source = directory.join("tone.wav");
    std::fs::write(&source, tone())?;
    let pattern = directory.join(format!("part%03d.{extension}"));
    let output = super::common::output(
        std::process::Command::new(decoder)
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&source)
            .args([
                "-c:a",
                "aac",
                "-b:a",
                "32k",
                "-f",
                "segment",
                "-segment_time",
                "1",
                "-segment_format",
                muxer,
            ])
            .arg(&pattern),
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut segments = Vec::new();
    for index in 0.. {
        let path: PathBuf = directory.join(format!("part{index:03}.{extension}"));
        if !path.is_file() {
            break;
        }
        segments.push(std::fs::read(path)?);
    }
    assert!(
        segments.len() >= 7,
        "FFmpeg cut {} segments",
        segments.len()
    );
    Ok(segments)
}

fn wait_terminal(
    directory: &Path,
    id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let response = success(directory, &["record", "show", id])?;
        let record = &response["recording_page"]["entries"][0];
        let state = record["state"].as_str().unwrap_or("");
        if matches!(state, "completed" | "failed" | "interrupted" | "cancelled") {
            return Ok(record.clone());
        }
        assert!(Instant::now() < deadline, "{record}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn register(directory: &Path, revision: &str, server: &LiveServer, query: &str) -> TestResult {
    success(
        directory,
        &[
            "source",
            "add",
            revision,
            "--name",
            "Live loopback",
            "--url",
            &format!("{}/live.m3u8{query}", server.origin),
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    Ok(())
}

/// Window 0 lists 0-2, then the edge advances by one per reload until reload 3
/// jumps to 6-8, so sequence 5 expires before it is fetched.
fn skipping_window(count: usize) -> (u64, u64) {
    match count {
        0..=2 => (u64::try_from(count).unwrap_or(0), 3),
        _ => (6, 3),
    }
}

/// Checks the published bytes, decoded duration, and the skip gap. Returns the duration.
fn assert_skip_publication(
    directory: &Path,
    published: &[Vec<u8>],
) -> Result<u64, Box<dyn std::error::Error>> {
    let record = wait_terminal(directory, "live-ts")?;
    assert_eq!(record["state"], "completed", "{record}");
    assert_eq!(record["format"], "mpegts");
    assert_eq!(record["end_reason"], "stream_gap");
    let expected: Vec<u8> = published.concat();
    assert_eq!(record["media_bytes"], expected.len());
    let path = success(directory, &["record", "path", "live-ts"])?;
    assert_eq!(
        std::fs::read(path["path"].as_str().ok_or("missing path")?)?,
        expected
    );
    let measured_us = record["decoded_microseconds"]
        .as_u64()
        .ok_or("decoded duration")?;
    assert!(
        (4_500_000..=5_600_000).contains(&measured_us),
        "five one-second segments decoded as {measured_us} us"
    );
    let gaps = record["gaps"].as_array().ok_or("gaps")?;
    assert_eq!(gaps.len(), 1, "{record}");
    assert_eq!(gaps[0]["cause"], "sequence_skip");
    assert_eq!(gaps[0]["start_us"], measured_us);
    assert_eq!(gaps[0]["end_us"], 30_000_000);
    Ok(measured_us)
}

/// The edge advances by one segment per reload and never skips.
fn steady_window(count: usize) -> (u64, u64) {
    (u64::try_from(count.min(5)).unwrap_or(0), 3)
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn live_transport_stream_publishes_whole_segments_and_records_a_skip_gap() -> TestResult {
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    let scratch = tempfile::tempdir()?;
    let segments = cut_segments(&decoder, scratch.path(), "mpegts", "ts")?;
    let server = LiveServer::start(segments.clone(), "video/mp2t", skipping_window)?;
    let directory = tempfile::tempdir()?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    register(directory.path(), "live:v1", &server, "")?;
    register(directory.path(), "finite:v1", &server, "?finite")?;
    let finite = super::invoke(
        directory.path(),
        &[
            "record",
            "hls",
            "finite-try",
            "--source",
            "finite:v1",
            "--seconds",
            "30",
        ],
    )?;
    assert!(finite.status.success());
    let refused = wait_terminal(directory.path(), "finite-try")?;
    assert_eq!(refused["state"], "failed", "{refused}");
    assert!(
        server
            .requested()
            .iter()
            .all(|path| !path.starts_with("/seg")),
        "a refused finite attempt fetched a segment"
    );
    let before = server.requested().len();
    success(
        directory.path(),
        &[
            "record",
            "hls",
            "live-ts",
            "--source",
            "live:v1",
            "--seconds",
            "30",
            "--max-mib",
            "4",
            "--live",
        ],
    )?;
    let measured_us = assert_skip_publication(directory.path(), &segments[..5])?;
    let requested = server.requested();
    let live: Vec<&String> = requested[before..].iter().collect();
    let fetched: Vec<&str> = live
        .iter()
        .filter(|path| path.starts_with("/seg"))
        .map(|path| path.as_str())
        .collect();
    assert_eq!(fetched, ["/seg0", "/seg1", "/seg2", "/seg3", "/seg4"]);
    assert_eq!(
        live.iter()
            .filter(|path| path.as_str() == "/live.m3u8")
            .count(),
        4
    );
    success(
        directory.path(),
        &[
            "listen",
            "file",
            "live-ts",
            "--destination",
            "null",
            "--seek-us",
            &(measured_us - 800_000).to_string(),
        ],
    )?;
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn live_adts_segments_stop_at_the_time_ceiling_without_a_gap() -> TestResult {
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    let scratch = tempfile::tempdir()?;
    let segments = cut_segments(&decoder, scratch.path(), "adts", "aac")?;
    let server = LiveServer::start(segments.clone(), "audio/aac", steady_window)?;
    let directory = tempfile::tempdir()?;
    success(directory.path(), &["library", "init"])?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    register(directory.path(), "adts:v1", &server, "")?;
    success(
        directory.path(),
        &[
            "record",
            "hls",
            "live-aac",
            "--source",
            "adts:v1",
            "--seconds",
            "3",
            "--max-mib",
            "4",
            "--live",
        ],
    )?;
    let record = wait_terminal(directory.path(), "live-aac")?;
    assert_eq!(record["state"], "completed", "{record}");
    assert_eq!(record["format"], "aac");
    assert_eq!(record["end_reason"], "duration_limit");
    assert_eq!(record["gaps"], serde_json::json!([]));
    let fetched: Vec<String> = server
        .requested()
        .into_iter()
        .filter(|path| path.starts_with("/seg"))
        .collect();
    assert!(fetched.len() >= 4, "{fetched:?}");
    let expected: Vec<u8> = segments[..fetched.len()].concat();
    for (index, path) in fetched.iter().enumerate() {
        assert_eq!(path, &format!("/seg{index}"));
    }
    let path = success(directory.path(), &["record", "path", "live-aac"])?;
    assert_eq!(
        std::fs::read(path["path"].as_str().ok_or("missing path")?)?,
        expected
    );
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}
