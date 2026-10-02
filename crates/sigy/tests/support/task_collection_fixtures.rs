//! Native loopback collection: clients exit, task provenance is exact, cancellation is scoped.

use super::{RunningChild, TestResult, success};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture {
    url: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl Fixture {
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://127.0.0.1:{}/audio", listener.local_addr()?.port());
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = requests.clone();
        let cancelled = stop.clone();
        let worker = thread::spawn(move || {
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
                        let count = seen.fetch_add(1, Ordering::Relaxed) + 1;
                        let body = wave();
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )?;
                        if count == 2 {
                            thread::sleep(Duration::from_secs(3));
                        }
                        stream.write_all(&body)?;
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
            url,
            requests,
            stop,
            worker: Some(worker),
        })
    }
    fn finish(&mut self) -> TestResult {
        self.stop.store(true, Ordering::Relaxed);
        self.worker
            .take()
            .ok_or("fixture joined")?
            .join()
            .map_err(|_| "fixture panicked")??;
        Ok(())
    }
}

impl Drop for Fixture {
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

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

fn create_task(root: &Path, id: &str, from: i64, to: i64) -> TestResult {
    success(
        root,
        &[
            "task",
            "create",
            id,
            "--goal",
            "Collect finite evidence",
            "--monitor",
            "bounded",
            "--monitor-version",
            "1",
            "--monitor-actions",
            "0",
            "--from-ms",
            &from.to_string(),
            "--to-ms",
            &to.to_string(),
        ],
    )?;
    Ok(())
}

fn collect(root: &Path, id: &str, start: i64, seconds: &str) -> TestResult {
    success(
        root,
        &[
            "task",
            "collect",
            id,
            "grant",
            "--source",
            "radio:v1",
            "--start-ms",
            &start.to_string(),
            "--seconds",
            seconds,
            "--max-bytes",
            "1048576",
            "--expected-generation",
            "0",
        ],
    )?;
    Ok(())
}

fn collection(root: &Path, id: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(success(root, &["task", "collection", id])?["task"]["collection"].clone())
}

fn wait_state(
    root: &Path,
    id: &str,
    state: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let page = collection(root, id)?;
        let capture = &page["captures"][0];
        if capture["recording_state"] == state
            || (state == "active"
                && capture["recording_state"]
                    .as_str()
                    .is_some_and(|value| matches!(value, "starting" | "running")))
        {
            return Ok(page);
        }
        assert_ne!(capture["recording_state"], "failed", "{page}");
        assert!(
            Instant::now() < deadline,
            "task collection state deadline: {page}"
        );
        thread::sleep(Duration::from_millis(30));
    }
}

fn prepare(root: &Path, fixture: &Fixture) -> TestResult {
    let decoder = std::env::var("SIGY_TEST_FFMPEG")?;
    success(root, &["library", "init"])?;
    success(
        root,
        &["dvr", "configure", "--decoder", &decoder, "--quota-gb", "1"],
    )?;
    success(
        root,
        &[
            "source",
            "add",
            "radio:v1",
            "--name",
            "Collection fixture",
            "--url",
            &fixture.url,
            "--pin-address",
            "127.0.0.1",
        ],
    )?;
    success(
        root,
        &[
            "monitor",
            "create",
            "bounded",
            "--name",
            "Bounded evidence",
            "--goal",
            "Observe",
            "--term",
            "und:water",
            "--source",
            "radio:v1",
            "--daily-minutes",
            "1",
            "--total-hours",
            "1",
            "--capture-daily-minutes",
            "1",
            "--capture-total-hours",
            "1",
            "--capture-total-mib",
            "3",
        ],
    )?;
    Ok(())
}

fn independent_recording(root: &Path, id: &str) -> TestResult {
    success(
        root,
        &[
            "record",
            "start",
            id,
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
        let independent = success(root, &["record", "show", id])?;
        if independent["recording_page"]["entries"][0]["state"] == "completed" {
            break;
        }
        assert!(Instant::now() < deadline, "{independent}");
        thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}

fn restored_media_and_exact_grants(
    workspace: &Path,
    source: &Path,
    admitted: &serde_json::Value,
    future: &serde_json::Value,
    usage: &serde_json::Value,
) -> TestResult {
    let backup = workspace.join("backup");
    let restored = workspace.join("restored");
    let backup_path = backup.to_str().ok_or("backup path")?;
    success(source, &["library", "backup", backup_path])?;
    success(source, &["library", "verify-backup", backup_path])?;
    success(
        source,
        &[
            "library",
            "restore",
            backup_path,
            "--into",
            restored.to_str().ok_or("restore path")?,
        ],
    )?;
    assert_eq!(collection(&restored, "admitted-task")?, *admitted);
    assert_eq!(collection(&restored, "future-task")?, *future);
    let monitor = success(&restored, &["monitor", "show", "bounded"])?;
    assert_eq!(monitor["monitor"]["monitor"]["capture_usage"], *usage);
    let recording = admitted["captures"][0]["recording_id"]
        .as_str()
        .ok_or("task recording")?;
    let expected = wave();
    for id in ["independent", "independent-active", recording] {
        let media = success(&restored, &["record", "show", id])?;
        assert_eq!(media["recording_page"]["entries"][0]["state"], "completed");
        assert_eq!(
            media["recording_page"]["entries"][0]["media_bytes"],
            expected.len()
        );
        let path = success(&restored, &["record", "path", id])?;
        assert_eq!(
            std::fs::read(path["path"].as_str().ok_or("retained media path")?)?,
            expected
        );
    }
    Ok(())
}

fn settled_collection_and_independent_work(
    root: &Path,
    start: i64,
    fixture: &Fixture,
    finished: &serde_json::Value,
) -> Result<(serde_json::Value, serde_json::Value), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while now_ms()? <= start + 11_000 {
        assert!(Instant::now() < deadline, "future window deadline");
        thread::sleep(Duration::from_millis(30));
    }
    // The service marks the elapsed occurrence missed on a later schedule tick, which a
    // loaded host can delay. Compare the settled state rather than a racing snapshot.
    let future = loop {
        let page = collection(root, "future-task")?;
        if page["captures"][0]["state"] == "missed" {
            break page;
        }
        assert_eq!(page["captures"][0]["state"], "waiting", "{page}");
        assert!(Instant::now() < deadline, "future miss deadline: {page}");
        thread::sleep(Duration::from_millis(30));
    };
    assert_eq!(future["cancelled"], true);
    assert!(future["captures"][0]["recording_id"].is_null());
    assert_eq!(
        fixture.requests.load(Ordering::Relaxed),
        3,
        "cancelled future grant must not connect"
    );
    for id in ["independent", "independent-active"] {
        let independent = success(root, &["record", "show", id])?;
        assert_eq!(
            independent["recording_page"]["entries"][0]["state"],
            "completed"
        );
        assert_ne!(finished["captures"][0]["recording_id"], id);
    }
    let monitor = success(root, &["monitor", "show", "bounded"])?;
    let usage = &monitor["monitor"]["monitor"]["capture_usage"];
    assert_eq!(usage["admissions"], 1);
    assert_eq!(usage["used_total_seconds"], 10);
    assert_eq!(usage["reserved_total_bytes"], 1_048_576);
    Ok((future, usage.clone()))
}

#[test]
#[ignore = "requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn task_collection_records_after_clients_exit_and_cancels_only_future_owned_admission() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("library");
    let root = library.as_path();
    let mut fixture = Fixture::start()?;
    prepare(root, &fixture)?;
    let mut service = RunningChild::start(root)?;
    independent_recording(root, "independent")?;
    let start = now_ms()? / 1000 * 1000;
    create_task(root, "admitted-task", start - 1000, start + 30_000)?;
    create_task(root, "future-task", start - 1000, start + 30_000)?;
    collect(root, "admitted-task", start, "10")?;
    collect(root, "future-task", start + 8000, "3")?;
    let admitted = wait_state(root, "admitted-task", "active")?;
    let recording = admitted["captures"][0]["recording_id"]
        .as_str()
        .ok_or("missing task recording")?
        .to_owned();
    assert_ne!(recording, "independent");
    success(
        root,
        &[
            "record",
            "start",
            "independent-active",
            "--source",
            "radio:v1",
            "--seconds",
            "10",
            "--max-mib",
            "1",
        ],
    )?;
    let independent_active = success(root, &["record", "show", "independent-active"])?;
    assert!(
        independent_active["recording_page"]["entries"][0]["state"]
            .as_str()
            .is_some_and(|state| matches!(state, "starting" | "running"))
    );
    for id in ["future-task", "admitted-task"] {
        success(
            root,
            &[
                "task",
                "cancel-collection",
                id,
                "stop",
                "--expected-generation",
                "1",
            ],
        )?;
    }
    let finished = wait_state(root, "admitted-task", "completed")?;
    assert_eq!(finished["generation"], 2);
    assert_eq!(finished["captures"][0]["recording_id"], recording);
    assert_eq!(finished["captures"].as_array().ok_or("captures")?.len(), 1);
    let (future, usage) =
        settled_collection_and_independent_work(root, start, &fixture, &finished)?;
    success(root, &["service", "stop"])?;
    service.wait()?;
    fixture.finish()?;
    restored_media_and_exact_grants(directory.path(), root, &finished, &future, &usage)?;
    Ok(())
}
