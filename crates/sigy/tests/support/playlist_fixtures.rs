use super::{RunningChild, TestResult, success};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct CountingServer {
    port: u16,
    hits: Arc<AtomicUsize>,
    nested: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl CountingServer {
    fn playlist(audio_port: u16) -> std::io::Result<Self> {
        Self::start(Some(audio_port))
    }

    fn audio() -> std::io::Result<Self> {
        Self::start(None)
    }

    fn start(audio_port: Option<u16>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let nested = Arc::new(AtomicUsize::new(0));
        let cancelled = stop.clone();
        let counter = hits.clone();
        let nested_counter = nested.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false)?;
                        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
                        socket.set_write_timeout(Some(Duration::from_secs(2)))?;
                        let mut bytes = Vec::new();
                        while bytes.len() < 4096 && !bytes.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            if socket.read_exact(&mut byte).is_err() {
                                break;
                            }
                            bytes.push(byte[0]);
                        }
                        let request = String::from_utf8_lossy(&bytes);
                        if request.contains(" /secret/live/") || request.contains(" /segment") {
                            nested_counter.fetch_add(1, Ordering::Relaxed);
                        }
                        counter.fetch_add(1, Ordering::Relaxed);
                        let Some(audio_port) = audio_port else {
                            continue;
                        };
                        let (kind, body) = if request.contains(" /secret/master.m3u8") {
                            (
                                "application/vnd.apple.mpegurl".to_owned(),
                                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=2500000,CODECS=\"avc1.4d401f,mp4a.40.2\",RESOLUTION=1280x720\nlive/hd.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS=\"mp4a.40.2\"\nlive/audio.m3u8\n".to_owned(),
                            )
                        } else if request.contains(" /secret/hls.m3u") {
                            (
                                "audio/x-mpegurl".to_owned(),
                                format!(
                                    "#EXTM3U\n#EXT-X-TARGETDURATION:10\nhttp://127.0.0.1:{audio_port}/segment.ts\n"
                                ),
                            )
                        } else {
                            (
                                "application/vnd.apple.mpegurl".to_owned(),
                                "#EXTM3U\n#EXTINF:-1,Hidden title\nlive/main\n".to_owned(),
                            )
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        });
        Ok(Self {
            port,
            hits,
            nested,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for CountingServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn wait_playlist(
    directory: &std::path::Path,
    id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let value = success(directory, &["source", "playlist", "status", id])?;
        if value["playlist"]["state"] != "running" {
            return Ok(value);
        }
        assert!(Instant::now() < deadline, "playlist deadline");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn playlist_resolve_accepts_one_entry_without_opening_audio() -> TestResult {
    let audio = CountingServer::audio()?;
    let playlist = CountingServer::playlist(audio.port)?;
    let library = tempfile::tempdir()?;
    success(library.path(), &["library", "init"])?;
    let mut service = RunningChild::start(library.path())?;
    accept_relative_entry(library.path(), &playlist, &audio)?;
    reject_hls_document(library.path(), &playlist, &audio)?;
    resolve_master_variants(library.path(), &playlist, &audio)?;
    success(library.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

fn accept_relative_entry(
    directory: &std::path::Path,
    playlist: &CountingServer,
    audio: &CountingServer,
) -> TestResult {
    let playlist_url = format!(
        "http://127.0.0.1:{}/secret/list.m3u?token=hidden",
        playlist.port
    );
    let registered = success(
        directory,
        &[
            "source",
            "add",
            "playlist:v1",
            "--name",
            "Playlist",
            "--url",
            &playlist_url,
            "--pin-address",
            "127.0.0.1",
            "--redirects",
            "same-origin",
        ],
    )?;
    let parent_before = registered["source_page"]["entries"][0].clone();
    assert!(!parent_before.to_string().contains("secret"));
    assert!(!parent_before.to_string().contains("token"));
    success(
        directory,
        &[
            "source",
            "playlist",
            "resolve",
            "one",
            "--revision",
            "playlist:v1",
        ],
    )?;
    let resolved = wait_playlist(directory, "one")?;
    assert_eq!(resolved["playlist"]["state"], "completed");
    assert_eq!(resolved["playlist"]["entries"][0]["index"], 0);
    assert_eq!(
        resolved["playlist"]["entries"][0]["origin"],
        format!("http://127.0.0.1:{}", playlist.port)
    );
    let rendered = resolved.to_string();
    assert!(!rendered.contains("secret"));
    assert!(!rendered.contains("token"));
    assert!(!rendered.contains("live/main"));
    assert!(!rendered.contains(&format!("http://127.0.0.1:{}", audio.port)));
    assert_eq!(playlist.hits.load(Ordering::Relaxed), 1);
    assert_eq!(playlist.nested.load(Ordering::Relaxed), 0);
    assert_eq!(audio.hits.load(Ordering::Relaxed), 0);
    accept_and_replay(directory, playlist, audio, &parent_before)
}

fn accept_and_replay(
    directory: &std::path::Path,
    playlist: &CountingServer,
    audio: &CountingServer,
    parent_before: &serde_json::Value,
) -> TestResult {
    let parent_after = success(directory, &["source", "show", "playlist:v1"])?;
    assert_eq!(&parent_after["source_page"]["entries"][0], parent_before);
    let accepted = success(
        directory,
        &[
            "source",
            "playlist",
            "accept",
            "one",
            "--index",
            "0",
            "--revision",
            "chosen:v1",
            "--name",
            "Chosen",
        ],
    )?;
    assert_eq!(accepted["source_page"]["newly_created"], true);
    assert_eq!(
        accepted["source_page"]["entries"][0]["origin"],
        format!("http://127.0.0.1:{}", playlist.port)
    );
    assert_eq!(
        accepted["playlist"]["acceptances"][0]["child_revision"],
        "chosen:v1"
    );
    assert_eq!(
        accepted["playlist"]["acceptances"][0]["parent_revision"],
        "playlist:v1"
    );
    assert_eq!(
        accepted["playlist"]["acceptances"][0]["document_sha256"],
        accepted["playlist"]["document_sha256"]
    );
    assert!(!accepted.to_string().contains("secret"));
    assert!(!accepted.to_string().contains("live/main"));
    let replay = success(
        directory,
        &[
            "source",
            "playlist",
            "accept",
            "one",
            "--index",
            "0",
            "--revision",
            "chosen:v1",
            "--name",
            "Chosen",
        ],
    )?;
    assert_eq!(replay["source_page"]["newly_created"], false);
    success(
        directory,
        &[
            "source",
            "playlist",
            "resolve",
            "one",
            "--revision",
            "playlist:v1",
        ],
    )?;
    let again = wait_playlist(directory, "one")?;
    assert_eq!(again["playlist"]["state"], "completed");
    assert_eq!(playlist.hits.load(Ordering::Relaxed), 1);
    assert_eq!(audio.hits.load(Ordering::Relaxed), 0);
    let parent_final = success(directory, &["source", "show", "playlist:v1"])?;
    assert_eq!(&parent_final["source_page"]["entries"][0], parent_before);
    Ok(())
}

fn reject_hls_document(
    directory: &std::path::Path,
    playlist: &CountingServer,
    audio: &CountingServer,
) -> TestResult {
    let hls_url = format!("http://127.0.0.1:{}/secret/hls.m3u", playlist.port);
    success(
        directory,
        &[
            "source",
            "add",
            "hls:v1",
            "--name",
            "HLS",
            "--url",
            &hls_url,
            "--pin-address",
            "127.0.0.1",
            "--redirects",
            "deny",
        ],
    )?;
    success(
        directory,
        &[
            "source",
            "playlist",
            "resolve",
            "marker",
            "--revision",
            "hls:v1",
        ],
    )?;
    let marker = wait_playlist(directory, "marker")?;
    assert_eq!(marker["playlist"]["state"], "failed");
    assert_eq!(
        marker["playlist"]["failure"],
        "HLS media playlist is recorded with record hls"
    );
    assert_eq!(marker["playlist"]["entries"], serde_json::json!([]));
    assert!(!marker.to_string().contains("segment"));
    success(
        directory,
        &[
            "source",
            "playlist",
            "resolve",
            "marker",
            "--revision",
            "hls:v1",
        ],
    )?;
    let marker_replay = wait_playlist(directory, "marker")?;
    assert_eq!(marker_replay["playlist"]["state"], "failed");
    assert_eq!(playlist.hits.load(Ordering::Relaxed), 2);
    assert_eq!(playlist.nested.load(Ordering::Relaxed), 0);
    assert_eq!(audio.hits.load(Ordering::Relaxed), 0);
    Ok(())
}

fn resolve_master_variants(
    directory: &std::path::Path,
    playlist: &CountingServer,
    audio: &CountingServer,
) -> TestResult {
    let master_url = format!("http://127.0.0.1:{}/secret/master.m3u8", playlist.port);
    success(
        directory,
        &[
            "source",
            "add",
            "master:v1",
            "--name",
            "Master",
            "--url",
            &master_url,
            "--pin-address",
            "127.0.0.1",
            "--redirects",
            "deny",
        ],
    )?;
    success(
        directory,
        &[
            "source",
            "playlist",
            "resolve",
            "variants",
            "--revision",
            "master:v1",
        ],
    )?;
    let resolved = wait_playlist(directory, "variants")?;
    assert_eq!(resolved["playlist"]["state"], "completed", "{resolved}");
    let entries = resolved["playlist"]["entries"]
        .as_array()
        .ok_or("variant list")?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["kind"], "hls_variant");
    assert_eq!(entries[0]["bandwidth"], 2_500_000);
    assert_eq!(entries[0]["codecs"], "avc1.4d401f,mp4a.40.2");
    assert_eq!(entries[0]["audio_only"], false);
    assert_eq!(entries[1]["bandwidth"], 64_000);
    assert_eq!(entries[1]["audio_only"], true);
    assert!(!resolved.to_string().contains("/secret/live"));
    // Nothing chooses a variant, and no variant was requested.
    assert_eq!(playlist.hits.load(Ordering::Relaxed), 3);
    assert_eq!(playlist.nested.load(Ordering::Relaxed), 0);
    let accepted = success(
        directory,
        &[
            "source",
            "playlist",
            "accept",
            "variants",
            "--index",
            "1",
            "--revision",
            "variant:v1",
            "--name",
            "Audio variant",
        ],
    )?;
    assert_eq!(accepted["source_page"]["newly_created"], true);
    let text = super::common::output(
        std::process::Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory)
            .args(["source", "playlist", "status", "variants"]),
    )?;
    let rendered = String::from_utf8(text.stdout)?;
    assert!(
        rendered.contains("HLS variant | 64000 bit/s declared | mp4a.40.2 | audio only"),
        "{rendered}"
    );
    assert!(!rendered.contains("/secret/live"));
    assert_eq!(playlist.hits.load(Ordering::Relaxed), 3);
    assert_eq!(playlist.nested.load(Ordering::Relaxed), 0);
    assert_eq!(audio.hits.load(Ordering::Relaxed), 0);
    Ok(())
}
