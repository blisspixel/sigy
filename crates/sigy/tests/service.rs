use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use sigy_service::{domain::money::Usd, library::Library, storage::ledger::RequestState};

type TestResult = Result<(), Box<dyn std::error::Error>>;
mod common;
#[path = "support/recording_fixtures.rs"]
mod recording_fixtures;

fn invoke(directory: &Path, arguments: &[&str]) -> std::io::Result<Output> {
    common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory)
            .arg("--json")
            .args(arguments),
    )
}

fn success(
    directory: &Path,
    arguments: &[&str],
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let output = invoke(directory, arguments)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

struct RunningChild(Child);

impl RunningChild {
    fn start(directory: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let child = Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory)
            .args(["service", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut child = Self(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "service startup deadline");
            assert!(
                child.0.try_wait()?.is_none(),
                "service exited before readiness"
            );
            if invoke(directory, &["service", "status"])?.status.success() {
                return Ok(child);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait(&mut self) -> TestResult {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait()? {
                assert!(status.success(), "service shutdown failed: {status}");
                return Ok(());
            }
            assert!(Instant::now() < deadline, "service shutdown deadline");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn independent_clients_share_one_owner_and_changes_survive_restarts() -> TestResult {
    let directory = tempfile::tempdir()?;
    success(directory.path(), &["library", "init"])?;
    let mut service = RunningChild::start(directory.path())?;
    let first = success(directory.path(), &["service", "status"])?;
    assert_eq!(first["service"]["process_id"], service.0.id());
    assert!(
        !invoke(directory.path(), &["service", "run"])?
            .status
            .success()
    );
    let update = success(
        directory.path(),
        &["budget", "set", "global", "--usd", "0.234567"],
    )?;
    assert_eq!(update["service"]["process_id"], service.0.id());
    let second = success(directory.path(), &["library", "status"])?;
    assert_eq!(second["budgets"][0]["limit_usd"], "0.234567");
    assert_eq!(
        success(directory.path(), &["service", "stop"])?["service"]["stopping"],
        true
    );
    service.wait()?;
    assert!(!directory.path().join("service.json").exists());
    let reopened = success(directory.path(), &["library", "status"])?;
    assert!(reopened["service"].is_null());
    assert_eq!(reopened["budgets"][0]["limit_usd"], "0.234567");
    Ok(())
}

#[test]
fn killed_service_releases_ownership_and_retains_uncertain_liability() -> TestResult {
    let directory = tempfile::tempdir()?;
    {
        let mut library = Library::open(directory.path(), true)?;
        let store = library.store_mut();
        store.set_budget_limit("global", "1".parse::<Usd>()?)?;
        store.reserve("pending", "provider-a", "0.6".parse::<Usd>()?, &[])?;
        store.mark_submitted("pending")?;
    }
    let mut service = RunningChild::start(directory.path())?;
    let pid = service.0.id();
    service.0.kill()?;
    service.0.wait()?;
    assert!(
        directory.path().join("service.json").exists(),
        "kill leaves a stale discovery record"
    );
    let mut restarted = RunningChild::start(directory.path())?;
    let snapshot = success(directory.path(), &["service", "status"])?;
    assert_ne!(snapshot["service"]["process_id"], pid);
    assert_eq!(snapshot["budgets"][0]["reserved_usd"], "0.600000");
    assert_eq!(snapshot["budgets"][0]["available_usd"], "0.400000");
    success(directory.path(), &["service", "stop"])?;
    restarted.wait()?;
    let library = Library::open(directory.path(), false)?;
    assert_eq!(
        library
            .store()
            .reservation("pending")?
            .map(|reservation| reservation.state),
        Some(RequestState::Uncertain)
    );
    Ok(())
}

#[test]
fn service_startup_exposes_recovered_capture_intent_without_claiming_recording() -> TestResult {
    use sigy_service::{
        domain::capture::{CaptureEvent, CaptureState},
        storage::captures::CapturePlan,
    };
    let directory = tempfile::tempdir()?;
    let stale_worker = {
        let mut library = Library::open(directory.path(), true)?;
        let store = library.store_mut();
        let plan = CapturePlan::new("fixture:radio:v1", 0, i64::MAX, 1_048_576)?;
        store.create_capture("waiting", &plan)?;
        let job = store.create_capture("abandoned", &plan)?.job;
        store
            .transition_capture(&job.version, CaptureEvent::Start, "admitted")?
            .version
    };
    let mut service = RunningChild::start(directory.path())?;
    let snapshot = success(directory.path(), &["service", "status"])?;
    assert_eq!(
        snapshot["schema_version"],
        sigy_service::storage::SCHEMA_VERSION
    );
    assert_eq!(snapshot["captures"]["dispatch_available"], false);
    assert_eq!(snapshot["captures"]["scheduled"], 1);
    assert_eq!(snapshot["captures"]["active"], 0);
    assert_eq!(snapshot["captures"]["interrupted"], 1);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    let mut library = Library::open(directory.path(), false)?;
    assert_eq!(
        library
            .store()
            .capture("abandoned")?
            .ok_or("missing capture")?
            .state,
        CaptureState::Interrupted
    );
    assert!(matches!(
        library.store_mut().transition_capture(
            &stale_worker,
            CaptureEvent::Connected,
            "late_worker"
        ),
        Err(sigy_service::Error::StaleCapture)
    ));
    Ok(())
}

struct DetachedService(PathBuf);

#[test]
fn source_registration_uses_the_running_owner_and_survives_restart() -> TestResult {
    let directory = tempfile::tempdir()?;
    success(directory.path(), &["library", "init"])?;
    let mut service = RunningChild::start(directory.path())?;
    let args = [
        "source",
        "add",
        "quebec:v1",
        "--name",
        "Radio Québec",
        "--url",
        "https://unresolved.invalid/audio?token=hidden",
    ];
    let added = success(directory.path(), &args)?;
    assert_eq!(added["service"]["process_id"], service.0.id());
    assert_eq!(added["source_page"]["newly_created"], true);
    assert_eq!(
        success(directory.path(), &args)?["source_page"]["newly_created"],
        false
    );
    let changed = invoke(
        directory.path(),
        &[
            "source",
            "add",
            "quebec:v1",
            "--name",
            "Changed",
            "--url",
            "https://unresolved.invalid/audio?token=hidden",
        ],
    )?;
    assert!(!changed.status.success());
    assert!(!String::from_utf8_lossy(&changed.stderr).contains("hidden"));
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    let reopened = success(directory.path(), &["source", "show", "quebec:v1"])?;
    assert!(reopened["service"].is_null());
    assert_eq!(
        reopened["source_page"]["entries"][0]["name"],
        "Radio Québec"
    );
    let mut restarted = RunningChild::start(directory.path())?;
    assert_eq!(
        success(directory.path(), &["source", "list"])?["source_page"]["entries"],
        reopened["source_page"]["entries"]
    );
    success(directory.path(), &["service", "stop"])?;
    restarted.wait()?;
    Ok(())
}

impl Drop for DetachedService {
    fn drop(&mut self) {
        let _ = invoke(&self.0, &["service", "stop"]);
    }
}

#[test]
fn detached_service_releases_the_starting_clients_output_pipe() -> TestResult {
    use std::io::Read;
    let directory = tempfile::tempdir()?;
    success(directory.path(), &["library", "init"])?;
    let _cleanup = DetachedService(directory.path().to_owned());
    let mut client = RunningChild(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["--json", "service", "start"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let stdout = client.0.stdout.take().ok_or("missing output pipe")?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    client.wait()?;
    let bytes = receiver.recv_timeout(Duration::from_secs(10))??;
    reader.join().map_err(|_| "output reader panicked")?;
    assert!(bytes.len() <= 256 * 1024);
    let started: serde_json::Value = serde_json::from_slice(&bytes)?;
    let still_running = success(directory.path(), &["service", "status"])?;
    assert_eq!(
        started["service"]["process_id"],
        still_running["service"]["process_id"]
    );
    success(directory.path(), &["service", "stop"])?;
    Ok(())
}

#[test]
fn detached_start_returns_and_repeated_start_reuses_the_service() -> TestResult {
    let directory = tempfile::tempdir()?;
    success(directory.path(), &["library", "init"])?;
    let _cleanup = DetachedService(directory.path().to_owned());
    let started = success(directory.path(), &["service", "start"])?;
    let again = success(directory.path(), &["service", "start"])?;
    let status = success(directory.path(), &["service", "status"])?;
    assert_eq!(
        started["service"]["process_id"],
        again["service"]["process_id"]
    );
    assert_eq!(
        again["service"]["process_id"],
        status["service"]["process_id"]
    );
    success(directory.path(), &["service", "stop"])?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Library::open(directory.path(), false).is_err() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
