use super::{RunningChild, TestResult, invoke, success};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct FeedServer {
    origin: String,
    hits: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl FeedServer {
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let origin = format!("http://fixture.invalid:{port}");
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let counter = hits.clone();
        let recorded = paths.clone();
        let worker = thread::spawn(move || serve(&listener, port, &cancelled, &counter, &recorded));
        Ok(Self {
            origin,
            hits,
            paths,
            stop,
            worker: Some(worker),
        })
    }

    fn feed_url(&self) -> String {
        format!("{}/feed.xml?token=hidden", self.origin)
    }
}

impl Drop for FeedServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(
    listener: &TcpListener,
    port: u16,
    stop: &AtomicBool,
    hits: &AtomicUsize,
    paths: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        match listener.accept() {
            Ok((mut socket, _)) => {
                socket.set_nonblocking(false)?;
                socket.set_read_timeout(Some(Duration::from_secs(15)))?;
                socket.set_write_timeout(Some(Duration::from_secs(15)))?;
                let mut bytes = Vec::new();
                while bytes.len() < 8192 && !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if socket.read_exact(&mut byte).is_err() {
                        break;
                    }
                    bytes.push(byte[0]);
                }
                let path = request_path(&bytes);
                if let Ok(mut guard) = paths.lock() {
                    guard.push(path.clone());
                }
                let hit = hits.fetch_add(1, Ordering::Relaxed);
                let response = response_for(hit, port)?;
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
}

fn request_path(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.split_whitespace().nth(1).unwrap_or("").to_owned()
}

fn response_for(hit: usize, port: u16) -> std::io::Result<Vec<u8>> {
    match hit {
        0 => {
            let body = full_feed(port)?;
            Ok(http_body(
                "200 OK",
                "Content-Type: application/rss+xml; charset=utf-8\r\n",
                body.as_bytes(),
            ))
        }
        1 => Ok(http_body(
            "200 OK",
            "Content-Type: application/rss+xml\r\n",
            br#"<rss version="2.0"><channel><item><guid>guid-1</guid><title>Episode 1</title></item></channel></rss>"#,
        )),
        _ => Ok(http_body(
            "200 OK",
            "Content-Type: application/rss+xml\r\n",
            b"<!DOCTYPE rss [<!ENTITY secret SYSTEM \"file:///secret\">]><rss version=\"2.0\"><channel/></rss>",
        )),
    }
}

fn full_feed(port: u16) -> std::io::Result<String> {
    use std::fmt::Write;
    let mut items = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>Fixture</title>"#,
    );
    for index in 1..=238 {
        write!(
            items,
            "<item><title>Episode {index}</title><guid>guid-{index}</guid><pubDate>Sun, 20 Sep 2026 12:00:00 GMT</pubDate><enclosure url=\"http://127.0.0.1:{port}/audio/{index}.mp3?token=hidden\" length=\"12\" type=\"audio/mpeg\"/><podcast:transcript url=\"http://127.0.0.1:{port}/transcript/{index}.srt\" type=\"application/srt\" language=\"en\"/><podcast:chapters url=\"http://127.0.0.1:{port}/chapters/{index}.json\" type=\"application/json\"/></item>"
        )
        .map_err(std::io::Error::other)?;
    }
    write!(
        items,
        "<item><title>Episode 239</title><pubDate>Mon, 21 Sep 2026 12:00:00 GMT</pubDate><enclosure url=\"http://127.0.0.1:{port}/audio/239.mp3?token=hidden\" length=\"12\" type=\"audio/mpeg\"/><podcast:transcript url=\"http://127.0.0.1:{port}/transcript/239.srt\" type=\"application/srt\" language=\"en\"/><podcast:chapters url=\"http://127.0.0.1:{port}/chapters/239.json\" type=\"application/json\"/></item><item><title>Not an episode</title></item><podcast:liveItem status=\"live\"><title>Live</title><enclosure url=\"http://127.0.0.1:{port}/live.mp3?token=hidden\"/></podcast:liveItem></channel></rss>"
    )
    .map_err(std::io::Error::other)?;
    Ok(items)
}

fn http_body(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn wait_refresh(
    directory: &std::path::Path,
    id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let value = success(directory, &["podcast", "refresh-status", id])?;
        if value["podcast_feed"]["refresh"]["state"] != "running" {
            return Ok(value);
        }
        assert!(Instant::now() < deadline, "feed refresh deadline");
        thread::sleep(Duration::from_millis(20));
    }
}

fn list_episodes(
    directory: &std::path::Path,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let mut episodes = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut args = vec!["podcast", "episodes", "show:v1", "--limit", "32"];
        if let Some(cursor) = &after {
            args.push("--after");
            args.push(cursor);
        }
        let page = success(directory, &args)?;
        let rendered = page.to_string();
        assert!(!rendered.contains("token=hidden"), "{rendered}");
        assert!(!rendered.contains("/audio/"), "{rendered}");
        assert!(!rendered.contains("/transcript/"), "{rendered}");
        assert!(!rendered.contains("/chapters/"), "{rendered}");
        assert!(!rendered.contains("/live.mp3"), "{rendered}");
        let entries = page["podcast_feed"]["episodes"]
            .as_array()
            .cloned()
            .ok_or("episode page")?;
        episodes.extend(entries);
        match page["podcast_feed"]["next_after"].as_str() {
            Some(cursor) => after = Some(cursor.to_owned()),
            None => return Ok(episodes),
        }
    }
}

fn refresh_when_due(directory: &std::path::Path, id: &str) -> TestResult {
    thread::sleep(Duration::from_millis(2_100));
    success(directory, &["podcast", "refresh", "show:v1", "--id", id])?;
    Ok(())
}

fn assert_offline_feed(
    directory: &std::path::Path,
    latest: usize,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let episodes = list_episodes(directory)?;
    assert_eq!(episodes.len(), 239);
    assert_eq!(
        episodes
            .iter()
            .filter(|episode| episode["in_latest"] == true)
            .count(),
        latest
    );
    Ok(episodes)
}

#[test]
fn rss_refresh_lists_a_large_feed_offline_and_keeps_omissions() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = FeedServer::start()?;
    success(directory.path(), &["library", "init"])?;
    let feed = fixture.feed_url();
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
            "--redirects",
            "deny",
        ],
    )?;
    assert!(
        !invoke(
            directory.path(),
            &["podcast", "refresh", "show:v1", "--id", "feed:v1"]
        )?
        .status
        .success()
    );
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &["podcast", "refresh", "show:v1", "--id", "feed:v1"],
    )?;
    let completed = wait_refresh(directory.path(), "feed:v1")?;
    assert_eq!(
        completed["podcast_feed"]["refresh"]["state"], "completed",
        "{completed}"
    );
    assert_eq!(completed["podcast_feed"]["refresh"]["committed_items"], 239);
    assert_eq!(completed["podcast_feed"]["refresh"]["live_count"], 1);
    assert_eq!(completed["podcast_feed"]["refresh"]["skipped_items"], 1);
    assert_eq!(completed["podcast_feed"]["refresh"]["truncated"], false);
    assert_eq!(completed["schema_version"], 14);
    success(
        directory.path(),
        &["podcast", "refresh", "show:v1", "--id", "feed:v1"],
    )?;
    assert_eq!(fixture.hits.load(Ordering::Relaxed), 1);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    let episodes = assert_offline_feed(directory.path(), 239)?;
    assert_eq!(
        episodes
            .iter()
            .filter(|episode| episode["identity"] == "derived_enclosure")
            .count(),
        1
    );
    let cursor = episodes[237]["id"].as_str().ok_or("episode cursor")?;
    let displayed = super::common::output(
        std::process::Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args([
                "podcast", "episodes", "show:v1", "--limit", "32", "--after", cursor,
            ]),
    )?;
    assert!(displayed.status.success());
    let displayed = String::from_utf8(displayed.stdout)?;
    assert!(displayed.contains("derived enclosure"));
    assert!(!displayed.contains("token=hidden"));
    later_refreshes(directory.path(), &fixture)
}

fn later_refreshes(directory: &std::path::Path, fixture: &FeedServer) -> TestResult {
    let mut service = RunningChild::start(directory)?;
    refresh_when_due(directory, "feed:v2")?;
    let narrowed = wait_refresh(directory, "feed:v2")?;
    assert_eq!(narrowed["podcast_feed"]["refresh"]["committed_items"], 1);
    success(directory, &["service", "stop"])?;
    service.wait()?;
    assert_offline_feed(directory, 1)?;
    let mut service = RunningChild::start(directory)?;
    refresh_when_due(directory, "feed:v3")?;
    let failed = wait_refresh(directory, "feed:v3")?;
    assert_eq!(failed["podcast_feed"]["refresh"]["state"], "failed");
    assert_eq!(failed["podcast_feed"]["snapshot"]["refresh_id"], "feed:v2");
    success(directory, &["service", "stop"])?;
    service.wait()?;
    assert_eq!(list_episodes(directory)?.len(), 239);
    let paths = fixture.paths.lock().map_err(|_| "path lock")?;
    assert!(
        paths.iter().all(|path| path == "/feed.xml?token=hidden"),
        "{paths:?}"
    );
    assert_eq!(paths.len(), 3);
    drop(paths);
    success(directory, &["podcast", "unsubscribe", "show:v1"])?;
    assert!(
        !invoke(
            directory,
            &["podcast", "refresh", "show:v1", "--id", "feed:v4"]
        )?
        .status
        .success()
    );
    assert_eq!(fixture.hits.load(Ordering::Relaxed), 3);
    let sources = success(directory, &["source", "list"])?;
    assert!(
        sources["source_page"]["entries"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    Ok(())
}

#[test]
fn private_redirect_fails_closed_without_a_snapshot() -> TestResult {
    let directory = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicUsize::new(0));
    let cancelled = stop.clone();
    let counter = hits.clone();
    let worker = thread::spawn(move || -> std::io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket.set_nonblocking(false)?;
                    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
                    let mut bytes = Vec::new();
                    while bytes.len() < 4096 && !bytes.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        if socket.read_exact(&mut byte).is_err() {
                            break;
                        }
                        bytes.push(byte[0]);
                    }
                    counter.fetch_add(1, Ordering::Relaxed);
                    let body = b"HTTP/1.1 302 Found\r\nLocation: http://10.0.0.1/secret.xml\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = socket.write_all(body);
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
    success(directory.path(), &["library", "init"])?;
    let url = format!("http://fixture.invalid:{port}/feed.xml");
    success(
        directory.path(),
        &[
            "podcast",
            "subscribe",
            "redirected",
            "--url",
            &url,
            "--pin-address",
            "127.0.0.1",
            "--redirects",
            "same-origin",
        ],
    )?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &["podcast", "refresh", "redirected", "--id", "redirect:v1"],
    )?;
    let failed = wait_refresh(directory.path(), "redirect:v1")?;
    assert_eq!(
        failed["podcast_feed"]["refresh"]["state"], "failed",
        "{failed}"
    );
    assert!(failed["podcast_feed"]["snapshot"].is_null());
    let episodes = success(directory.path(), &["podcast", "episodes", "redirected"])?;
    assert!(
        episodes["podcast_feed"]["episodes"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(hits.load(Ordering::Relaxed), 1);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    stop.store(true, Ordering::Relaxed);
    let _ = worker.join();
    Ok(())
}

struct OversizeFeed {
    port: u16,
    hits: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl OversizeFeed {
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let cancelled = stop.clone();
        let counter = hits.clone();
        let recorded = paths.clone();
        let worker = thread::spawn(move || -> std::io::Result<()> {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false)?;
                        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
                        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
                        let mut bytes = Vec::new();
                        while bytes.len() < 4096 && !bytes.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            if socket.read_exact(&mut byte).is_err() {
                                break;
                            }
                            bytes.push(byte[0]);
                        }
                        let path = request_path(&bytes);
                        if let Ok(mut guard) = recorded.lock() {
                            guard.push(path);
                        }
                        counter.fetch_add(1, Ordering::Relaxed);
                        let body = format!(
                            r#"<rss version="2.0"><channel><item><guid>ep-1</guid><enclosure url="http://fixture.invalid:{port}/episode.wav?token=hidden" length="536870913" type="audio/mpeg"/></item></channel></rss>"#
                        );
                        let _ = socket.write_all(&http_body(
                            "200 OK",
                            "Content-Type: application/rss+xml\r\n",
                            body.as_bytes(),
                        ));
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
            port,
            hits,
            paths,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for OversizeFeed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn declared_enclosure_length_above_the_cap_does_not_connect() -> TestResult {
    let directory = tempfile::tempdir()?;
    let feed_server = OversizeFeed::start()?;
    let feed = format!(
        "http://fixture.invalid:{}/feed.xml?token=hidden",
        feed_server.port
    );
    success(directory.path(), &["library", "init"])?;
    let decoder = std::env::current_exe()?;
    let decoder = decoder.to_str().ok_or("decoder path")?;
    success(
        directory.path(),
        &["dvr", "configure", "--decoder", decoder, "--quota-gb", "1"],
    )?;
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
    let refreshed = wait_refresh(directory.path(), "feed:v1")?;
    assert_eq!(refreshed["podcast_feed"]["refresh"]["state"], "completed");
    let episode = list_episodes(directory.path())?
        .into_iter()
        .next()
        .ok_or("missing episode")?;
    let episode_id = episode["id"].as_str().ok_or("episode id")?;
    let denied = invoke(
        directory.path(),
        &[
            "podcast",
            "download",
            "show:v1",
            "--episode",
            episode_id,
            "--id",
            "episode:v1",
            "--revision",
            "enc:v1",
        ],
    )?;
    assert!(
        !denied.status.success(),
        "{}",
        String::from_utf8_lossy(&denied.stdout)
    );
    let error = String::from_utf8(denied.stderr)?;
    assert!(error.contains("512 MiB"), "{error}");
    assert!(!error.contains("token=hidden"));
    let paths = feed_server.paths.lock().map_err(|_| "path lock")?;
    assert!(
        paths.iter().all(|path| path.starts_with("/feed.xml")),
        "{paths:?}"
    );
    assert_eq!(feed_server.hits.load(Ordering::Relaxed), 1);
    drop(paths);
    let sources = success(directory.path(), &["source", "list"])?;
    assert!(
        sources["source_page"]["entries"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    let recordings = success(directory.path(), &["record", "list"])?;
    assert!(
        recordings["recording_page"]["entries"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}
