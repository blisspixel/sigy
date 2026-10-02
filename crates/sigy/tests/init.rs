use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use sigy_service::library::Library;

mod common;
type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Sandbox {
    root: tempfile::TempDir,
    home: PathBuf,
    libraries: Vec<PathBuf>,
}

impl Sandbox {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let home = root.path().join("Home Québec 東京 with spaces");
        std::fs::create_dir(&home)?;
        let library = home.join(".sigy").join("library");
        Ok(Self {
            root,
            home,
            libraries: vec![library],
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sigy"));
        command
            .env("USERPROFILE", &self.home)
            .env("HOME", &self.home)
            .env_remove("HOMEDRIVE")
            .env_remove("HOMEPATH")
            .current_dir(self.root.path());
        command
    }

    fn library(&self) -> &Path {
        &self.libraries[0]
    }

    fn explicit_library(&mut self) -> PathBuf {
        let path = self.root.path().join("Explicit bibliothèque 東京");
        self.libraries.push(path.clone());
        path
    }

    fn output(&self, arguments: &[&str]) -> std::io::Result<Output> {
        common::output(self.command().args(arguments))
    }

    fn json(&self, arguments: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
        let output = common::output(self.command().arg("--json").args(arguments))?;
        parse_success(&output)
    }

    fn stop(&self, library: &Path) -> TestResult {
        if !library.join("catalog.sqlite3").is_file() {
            return Ok(());
        }
        let _ = common::output(
            self.command()
                .arg("--data-dir")
                .arg(library)
                .args(["service", "stop"]),
        )?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match Library::open(library, false) {
                Ok(owner) => {
                    drop(owner);
                    return Ok(());
                }
                Err(sigy_service::Error::LibraryBusy) => {
                    if Instant::now() >= deadline {
                        return Err(
                            "owned initialization service did not release its library".into()
                        );
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for library in &self.libraries {
            if let Err(error) = self.stop(library) {
                eprintln!("owned initialization fixture cleanup failed: {error}");
            }
        }
    }
}

fn parse_success(output: &Output) -> Result<Value, Box<dyn std::error::Error>> {
    assert!(
        output.status.success(),
        "{}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn assert_refused(output: &Output, guidance: &str) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(guidance), "{stderr}");
}

#[test]
fn fresh_default_setup_survives_client_exit_and_reuses_its_owner() -> TestResult {
    let sandbox = Sandbox::new()?;
    let setup = sandbox.json(&["init"])?;
    assert_eq!(setup["service_ready"], true);
    assert!(setup["radio"].is_null());
    assert_eq!(
        setup["library"],
        sandbox.library().canonicalize()?.to_string_lossy().as_ref()
    );
    assert_eq!(setup["snapshot"]["budgets"][0]["limit_usd"], "0.000000");
    assert_eq!(setup["snapshot"]["budgets"][0]["reserved_usd"], "0.000000");
    assert_eq!(setup["snapshot"]["captures"]["active"], 0);
    let status = sandbox.json(&["service", "status"])?;
    let pid = status["service"]["process_id"]
        .as_u64()
        .ok_or("service owner PID missing")?;
    assert_eq!(setup["snapshot"]["service"]["process_id"], pid);
    let repeated = sandbox.json(&["init"])?;
    assert_eq!(repeated["snapshot"]["service"]["process_id"], pid);
    assert_eq!(
        sandbox.json(&["library", "status"])?["service"]["process_id"],
        pid
    );
    sandbox.stop(sandbox.library())
}

#[test]
fn explicit_setup_path_wins_even_without_a_resolvable_home() -> TestResult {
    let mut sandbox = Sandbox::new()?;
    let library = sandbox.explicit_library();
    let output = common::output(
        sandbox
            .command()
            .env_remove("USERPROFILE")
            .env_remove("HOME")
            .arg("--data-dir")
            .arg(&library)
            .args(["--json", "init", "--no-start"]),
    )?;
    let setup = parse_success(&output)?;
    assert_eq!(setup["service_ready"], false);
    assert!(setup["snapshot"]["service"].is_null());
    assert_eq!(
        setup["library"],
        library.canonicalize()?.to_string_lossy().as_ref()
    );
    assert!(!sandbox.home.join(".sigy").exists());
    let owner = Library::open(&library, false)?;
    assert_eq!(owner.directory(), library.canonicalize()?);
    drop(owner);
    sandbox.stop(&library)
}

#[test]
fn repeated_setup_preserves_paid_and_dvr_configuration_and_running_owner() -> TestResult {
    let sandbox = Sandbox::new()?;
    sandbox.json(&["init", "--no-start"])?;
    sandbox.json(&["budget", "set", "global", "--usd", "0.234567"])?;
    // Configuration stores this local file path without dispatching a decoder.
    // The fixture does not claim that this executable decodes media.
    let executable = Path::new(env!("CARGO_BIN_EXE_sigy")).canonicalize()?;
    let executable = executable
        .to_str()
        .ok_or("test executable is not Unicode")?;
    let configured = sandbox.json(&[
        "dvr",
        "configure",
        "--decoder",
        executable,
        "--quota-gb",
        "1",
        "--retention-days",
        "3",
        "--minimum-free-mib",
        "64",
    ])?;
    let started = sandbox.json(&["init"])?;
    let pid = started["snapshot"]["service"]["process_id"]
        .as_u64()
        .ok_or("running setup owner PID missing")?;
    for arguments in [&["init"][..], &["init", "--no-start"][..]] {
        let setup = sandbox.json(arguments)?;
        assert_eq!(setup["snapshot"]["service"]["process_id"], pid);
        assert!(setup["radio"].is_null());
        assert_eq!(setup["snapshot"]["budgets"][0]["limit_usd"], "0.234567");
        assert_eq!(setup["snapshot"]["budgets"][0]["reserved_usd"], "0.000000");
        let dvr = sandbox.json(&["dvr", "status"])?;
        assert_eq!(dvr["dvr"], configured["dvr"]);
        assert_eq!(dvr["dvr"]["quota_bytes"], 1_000_000_000_u64);
        assert_eq!(dvr["dvr"]["retention_days"], 3);
    }
    sandbox.stop(sandbox.library())
}

#[test]
fn offline_setup_creates_no_owner_refresh_or_capture_and_can_reopen() -> TestResult {
    let sandbox = Sandbox::new()?;
    let setup = sandbox.json(&["init", "--no-start"])?;
    assert_eq!(setup["service_ready"], false);
    assert!(setup["radio"].is_null());
    assert!(setup["snapshot"]["service"].is_null());
    assert_eq!(setup["snapshot"]["captures"]["scheduled"], 0);
    assert_eq!(setup["snapshot"]["captures"]["active"], 0);
    let owner = Library::open(sandbox.library(), false)?;
    assert!(owner.store().sources(None, 16)?.is_empty());
    assert!(owner.store().recordings(None, 16)?.is_empty());
    let directory = owner.store().directory_status()?;
    assert_eq!(directory.cached_stations, 0);
    assert!(directory.latest_refresh.is_none());
    drop(owner);
    let directory = sandbox.json(&["radio", "status"])?;
    assert!(directory["directory_refresh"].is_null());
    let repeated = sandbox.json(&["init", "--no-start"])?;
    assert_eq!(repeated["service_ready"], false);
    assert!(repeated["snapshot"]["service"].is_null());
    assert!(sandbox.json(&["radio", "status"])?["directory_refresh"].is_null());
    sandbox.stop(sandbox.library())
}

#[test]
fn unresolved_or_relative_home_fails_before_creating_any_library() -> TestResult {
    let sandbox = Sandbox::new()?;
    for home in [None, Some(""), Some("relative home")] {
        let mut command = sandbox.command();
        command.env_remove("USERPROFILE").env_remove("HOME");
        if let Some(home) = home {
            command.env("USERPROFILE", home).env("HOME", home);
        }
        let output = common::output(command.arg("init"))?;
        assert_refused(&output, "--data-dir");
        assert!(!sandbox.home.join(".sigy").exists());
        assert!(!sandbox.root.path().join("relative home").exists());
    }
    Ok(())
}

#[test]
fn mcp_requires_explicit_library_and_radio_conflict_has_no_side_effects() -> TestResult {
    let sandbox = Sandbox::new()?;
    assert_refused(&sandbox.output(&["mcp"])?, "--data-dir");
    let conflict = sandbox.output(&["init", "--no-start", "--radio"])?;
    assert_refused(&conflict, "--radio");
    assert!(!sandbox.home.join(".sigy").exists());
    sandbox.json(&["init", "--no-start"])?;
    assert_refused(&sandbox.output(&["mcp"])?, "--data-dir");
    Ok(())
}

#[test]
fn setup_retries_after_unserved_ownership_without_resetting_settings() -> TestResult {
    let sandbox = Sandbox::new()?;
    sandbox.json(&["init", "--no-start"])?;
    sandbox.json(&["budget", "set", "global", "--usd", "0.345678"])?;
    // An owner without a controller represents setup interrupted before service readiness.
    let owner = Library::open(sandbox.library(), false)?;
    let blocked = sandbox.output(&["init"])?;
    assert!(!blocked.status.success());
    assert!(blocked.stdout.is_empty());
    assert!(!blocked.stderr.is_empty());
    assert_eq!(
        owner.store().budget("global")?.limit().to_string(),
        "0.345678"
    );
    assert!(owner.store().directory_status()?.latest_refresh.is_none());
    drop(owner);
    let retried = sandbox.json(&["init"])?;
    assert_eq!(retried["service_ready"], true);
    assert!(retried["radio"].is_null());
    assert_eq!(retried["snapshot"]["budgets"][0]["limit_usd"], "0.345678");
    sandbox.stop(sandbox.library())
}

#[test]
fn backup_verification_and_restore_need_neither_home_nor_selected_library() -> TestResult {
    let mut sandbox = Sandbox::new()?;
    sandbox.json(&["init", "--no-start"])?;
    sandbox.json(&["budget", "set", "global", "--usd", "0.456789"])?;
    let backup = sandbox.root.path().join("Backup archive Québec");
    let restored = sandbox.explicit_library();
    let backup_text = backup.to_str().ok_or("backup path is not Unicode")?;
    let original = sandbox.json(&["library", "backup", backup_text])?;
    let verified = parse_success(&common::output(
        sandbox
            .command()
            .env_remove("USERPROFILE")
            .env_remove("HOME")
            .args(["--json", "library", "verify-backup", backup_text]),
    )?)?;
    assert_eq!(verified, original);
    let restored_manifest = parse_success(&common::output(
        sandbox
            .command()
            .env_remove("USERPROFILE")
            .env_remove("HOME")
            .args(["--json", "library", "restore", backup_text, "--into"])
            .arg(&restored),
    )?)?;
    assert_eq!(restored_manifest, original);
    let owner = Library::open(&restored, false)?;
    assert_eq!(
        owner.store().budget("global")?.limit().to_string(),
        "0.456789"
    );
    assert!(owner.store().recordings(None, 16)?.is_empty());
    drop(owner);
    sandbox.stop(&restored)
}

#[test]
fn missing_backup_keeps_its_error_when_an_unused_library_is_supplied() -> TestResult {
    let mut sandbox = Sandbox::new()?;
    let library = sandbox.explicit_library();
    let missing_backup = sandbox.root.path().join("Missing backup");
    let output = common::output(
        sandbox
            .command()
            .env_remove("USERPROFILE")
            .env_remove("HOME")
            .arg("--data-dir")
            .arg(&library)
            .args(["--json", "library", "verify-backup"])
            .arg(&missing_backup),
    )?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let document: Value = serde_json::from_slice(&output.stderr)?;
    let error = document["error"].as_str().ok_or("missing backup error")?;
    assert!(!error.is_empty());
    assert!(!error.contains("sigy init"));
    assert!(!library.exists());
    assert!(!sandbox.home.join(".sigy").exists());
    Ok(())
}
