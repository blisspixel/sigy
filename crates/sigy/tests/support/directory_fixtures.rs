use super::{RunningChild, TestResult, invoke, success};
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

struct DirectoryServer {
    origin: String,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl DirectoryServer {
    fn start(response: Vec<u8>, delay: Duration) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let origin = format!("http://fixture.invalid:{}", listener.local_addr()?.port());
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let cancelled = stop.clone();
        let counter = hits.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false)?;
                        socket.set_read_timeout(Some(Duration::from_secs(15)))?;
                        socket.set_write_timeout(Some(Duration::from_secs(15)))?;
                        let mut bytes = Vec::new();
                        while bytes.len() < 4096 && !bytes.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            if socket.read_exact(&mut byte).is_err() {
                                break;
                            }
                            bytes.push(byte[0]);
                        }
                        let expected = b"name=Qu%C3%A9bec&hidebroken=";
                        let matches = bytes.starts_with(b"GET /json/stations/search?")
                            && bytes
                                .windows(expected.len())
                                .any(|window| window == expected);
                        if !matches {
                            let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            continue;
                        }
                        counter.fetch_add(1, Ordering::Relaxed);
                        thread::sleep(delay);
                        // A killed client can close the fixture before the response.
                        let _ = socket.write_all(&response);
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
        });
        Ok(Self {
            origin,
            hits,
            stop,
            worker: Some(worker),
        })
    }

    fn request<'a>(&'a self, id: &'a str) -> Vec<&'a str> {
        vec![
            "radio",
            "refresh",
            id,
            "--mirror",
            &self.origin,
            "--pin-address",
            "127.0.0.1",
            "--name",
            "Québec",
        ]
    }
}

impl Drop for DirectoryServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn response() -> Vec<u8> {
    let body = serde_json::json!([{"stationuuid":"12345678-1234-1234-1234-123456789abc", "name":"Radio Québec", "url":"https://stream.example/audio", "countrycode":"CA", "language":"french,english", "tags":"news", "lastcheckok":1}]).to_string();
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
}

fn wait_refresh(
    directory: &std::path::Path,
    id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let value = success(directory, &["radio", "refresh-status", id])?;
        if value["directory_refresh"]["state"] != "running" {
            return Ok(value);
        }
        assert!(Instant::now() < deadline, "refresh deadline");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn directory_jobs_cache_unicode_and_register_without_tuning() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = DirectoryServer::start(response(), Duration::from_millis(250))?;
    success(directory.path(), &["library", "init"])?;
    let mut service = RunningChild::start(directory.path())?;
    success(directory.path(), &fixture.request("one"))?;
    let completed = wait_refresh(directory.path(), "one")?;
    assert_eq!(
        completed["directory_refresh"]["state"], "completed",
        "{completed}"
    );
    assert_eq!(completed["directory_refresh"]["accepted"], 1);
    let found = success(
        directory.path(),
        &[
            "radio",
            "search",
            "--name",
            "QUÉBEC",
            "--language",
            "french",
            "--country",
            "ca",
            "--tag",
            "news",
        ],
    )?;
    let id = found["station_page"]["entries"][0]["id"]
        .as_str()
        .ok_or("station missing")?;
    assert_eq!(found["station_page"]["favorite_ids"], serde_json::json!([]));
    let saved = success(directory.path(), &["radio", "favorite", id])?;
    assert_eq!(saved["directory"]["favorite_stations"], 1);
    assert_eq!(
        saved["station_page"]["favorite_ids"],
        serde_json::json!([id])
    );
    assert!(saved["source_page"].is_null());
    let displayed = super::common::output(
        std::process::Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["radio", "show", id]),
    )?;
    assert!(displayed.status.success());
    let displayed = String::from_utf8(displayed.stdout)?;
    assert!(displayed.contains("Radio Québec [favorite]"));
    assert!(displayed.contains("favorites: 1"));
    println!("{displayed}");
    let favorites = success(
        directory.path(),
        &["radio", "search", "--favorites", "--name", "QUÉBEC"],
    )?;
    assert_eq!(favorites["station_page"]["entries"][0]["id"], id);
    let source = success(
        directory.path(),
        &[
            "radio",
            "add",
            id,
            "--revision",
            "radio:v1",
            "--redirects",
            "public",
        ],
    )?;
    assert_eq!(
        source["source_page"]["entries"][0]["origin"],
        "https://stream.example"
    );
    assert_eq!(source["captures"]["active"], 0);
    assert_eq!(source["source_page"]["entries"][0]["redirects"], "public");
    success(directory.path(), &fixture.request("one"))?;
    assert_eq!(fixture.hits.load(Ordering::Relaxed), 1);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    let cached = success(directory.path(), &["radio", "search", "--name", "québec"])?;
    assert_eq!(cached["station_page"]["entries"][0]["name"], "Radio Québec");
    assert_eq!(
        cached["station_page"]["favorite_ids"],
        serde_json::json!([id])
    );
    // Exercise the same mutation with the controller stopped, then reopen it.
    let removed = success(directory.path(), &["radio", "unfavorite", id])?;
    assert_eq!(removed["directory"]["favorite_stations"], 0);
    assert_eq!(
        removed["station_page"]["favorite_ids"],
        serde_json::json!([])
    );
    let mut reopened = RunningChild::start(directory.path())?;
    let empty = success(directory.path(), &["radio", "search", "--favorites"])?;
    assert_eq!(empty["station_page"]["entries"], serde_json::json!([]));
    assert_eq!(empty["directory"]["cached_stations"], 1);
    let retained = success(directory.path(), &["source", "show", "radio:v1"])?;
    assert_eq!(
        retained["source_page"]["entries"][0]["origin"],
        "https://stream.example"
    );
    success(directory.path(), &["service", "stop"])?;
    reopened.wait()?;
    assert_eq!(fixture.hits.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn killed_directory_refresh_is_interrupted_and_never_replayed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = DirectoryServer::start(response(), Duration::from_secs(2))?;
    success(directory.path(), &["library", "init"])?;
    let mut service = RunningChild::start(directory.path())?;
    success(directory.path(), &fixture.request("one"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.hits.load(Ordering::Relaxed) == 0 {
        if Instant::now() >= deadline {
            let status = success(directory.path(), &["radio", "refresh-status", "one"])?;
            panic!("directory fixture was not contacted: {status}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    service.0.kill()?;
    service.0.wait()?;
    let mut recovered = RunningChild::start(directory.path())?;
    let replay = success(directory.path(), &fixture.request("one"))?;
    assert_eq!(replay["directory_refresh"]["state"], "interrupted");
    assert_eq!(replay["directory"]["cached_stations"], 0);
    assert_eq!(fixture.hits.load(Ordering::Relaxed), 1);
    success(directory.path(), &["service", "stop"])?;
    recovered.wait()?;
    Ok(())
}

#[test]
fn oversized_directory_response_fails_without_cache_changes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = DirectoryServer::start(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2097153\r\n\r\n"
            .to_vec(),
        Duration::ZERO,
    )?;
    success(directory.path(), &["library", "init"])?;
    assert!(
        !invoke(directory.path(), &fixture.request("offline"))?
            .status
            .success()
    );
    let mut service = RunningChild::start(directory.path())?;
    success(directory.path(), &fixture.request("one"))?;
    let failed = wait_refresh(directory.path(), "one")?;
    assert_eq!(failed["directory_refresh"]["state"], "failed");
    assert_eq!(failed["directory"]["cached_stations"], 0);
    assert_eq!(fixture.hits.load(Ordering::Relaxed), 1);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

struct ClickServer {
    origin: String,
    paths: Arc<std::sync::Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl ClickServer {
    fn start(station_id: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let origin = format!("http://fixture.invalid:{port}");
        let stop = Arc::new(AtomicBool::new(false));
        let paths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let recorded = paths.clone();
        let station = station_id.to_owned();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false)?;
                        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
                        socket.set_write_timeout(Some(Duration::from_secs(2)))?;
                        let mut bytes = Vec::new();
                        while bytes.len() < 4096 && !bytes.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            socket.read_exact(&mut byte)?;
                            bytes.push(byte[0]);
                        }
                        let request = String::from_utf8_lossy(&bytes);
                        let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                        if let Ok(mut paths) = recorded.lock() {
                            paths.push(path);
                        }
                        let body = format!(
                            r#"{{"ok":"true","message":"retrieved station url","stationuuid":"{station}","name":"Radio","url":"http://127.0.0.1:{port}/secret-stream"}}"#
                        );
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
            origin,
            paths,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for ClickServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn wait_click(
    directory: &std::path::Path,
    id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let value = success(directory, &["radio", "click-status", id])?;
        if value["directory_click"]["state"] != "running" {
            return Ok(value);
        }
        assert!(Instant::now() < deadline, "click deadline");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn explicit_click_is_one_request_and_discards_the_stream_url() -> TestResult {
    const STATION: &str = "12345678-1234-1234-1234-123456789abc";
    let directory = tempfile::tempdir()?;
    let catalog = DirectoryServer::start(response(), Duration::ZERO)?;
    success(directory.path(), &["library", "init"])?;
    let mut service = RunningChild::start(directory.path())?;
    success(directory.path(), &catalog.request("cache"))?;
    let cached = wait_refresh(directory.path(), "cache")?;
    assert_eq!(
        cached["directory_refresh"]["state"], "completed",
        "{cached}"
    );
    success(directory.path(), &["radio", "search", "--name", "Québec"])?;
    success(directory.path(), &["radio", "show", STATION])?;
    success(directory.path(), &["radio", "favorite", STATION])?;
    assert_eq!(catalog.hits.load(Ordering::Relaxed), 1);
    let clicks = ClickServer::start(STATION)?;
    let command = [
        "radio",
        "click",
        "heard",
        "--station",
        STATION,
        "--mirror",
        clicks.origin.as_str(),
        "--pin-address",
        "127.0.0.1",
    ];
    success(directory.path(), &command)?;
    success(directory.path(), &command)?;
    let clicked = wait_click(directory.path(), "heard")?;
    assert_eq!(clicked["directory_click"]["state"], "completed");
    assert_eq!(clicked["directory_click"]["acknowledged"], true);
    assert_eq!(clicked["directory_click"]["station_id"], STATION);
    let rendered = clicked.to_string();
    assert!(!rendered.contains("secret-stream"));
    assert!(!rendered.contains("/json/vote/"));
    let paths = clicks.paths.lock().map_err(|_| "click paths")?.clone();
    assert_eq!(paths, vec![format!("/json/url/{STATION}")]);
    let early = invoke(
        directory.path(),
        &[
            "radio",
            "click",
            "again",
            "--station",
            STATION,
            "--mirror",
            &clicks.origin,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    assert!(!early.status.success());
    assert_eq!(clicks.paths.lock().map_err(|_| "click paths")?.len(), 1);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}
