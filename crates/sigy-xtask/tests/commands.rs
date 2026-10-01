//! Public verifier commands propagate failures without dispatching later steps.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn invoke(root: &Path, arguments: &[&str], fail: Option<&str>) -> std::io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sigy-xtask"));
    command
        .args(arguments)
        .env("CARGO", env!("CARGO_BIN_EXE_sigy-test-cargo"))
        .env("SIGY_TEST_CARGO_LOG", root.join("commands.log"))
        .env("SIGY_TEST_CARGO_METADATA", root.join("metadata.json"))
        .env("SIGY_TEST_CARGO_REPORT", root.join("report.json"));
    if let Some(fail) = fail {
        command.env("SIGY_TEST_CARGO_FAIL", fail);
    } else {
        command.env_remove("SIGY_TEST_CARGO_FAIL");
    }
    command.output()
}

fn assert_stops(root: &Path, decoder: &str, fail: &str, forbidden: &str) -> TestResult {
    fs::remove_file(root.join("commands.log"))?;
    let output = invoke(root, &["verify-coverage", decoder], Some(fail))?;
    assert!(!output.status.success());
    let log = fs::read_to_string(root.join("commands.log"))?;
    assert!(
        log.lines()
            .last()
            .is_some_and(|line| line.starts_with(fail)),
        "{log}"
    );
    assert!(!log.contains(forbidden), "later step ran: {log}");
    Ok(())
}

#[test]
fn verification_runs_all_checks_and_stops_on_a_failed_formatter() -> TestResult {
    let root = tempfile::tempdir()?;
    let output = invoke(root.path(), &["verify"], None)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commands = fs::read_to_string(root.path().join("commands.log"))?;
    for required in [
        "fmt --all -- --check",
        "clippy -p sigy-xtask",
        "clippy --workspace",
        "test -p sigy-xtask",
        "test --workspace",
        "build --workspace",
        "audit --deny warnings",
    ] {
        assert!(commands.contains(required), "missing {required}");
    }
    fs::remove_file(root.path().join("commands.log"))?;
    assert!(
        !invoke(root.path(), &["verify"], Some("fmt"))?
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(root.path().join("commands.log"))?,
        "fmt --all -- --check\n"
    );
    let unavailable = Command::new(env!("CARGO_BIN_EXE_sigy-xtask"))
        .arg("verify")
        .env("CARGO", root.path().join("missing-cargo.exe"))
        .output()?;
    assert!(!unavailable.status.success());
    Ok(())
}

#[test]
fn media_checks_require_a_real_explicit_path_and_propagate_native_test_failure() -> TestResult {
    let root = tempfile::tempdir()?;
    let decoder = root.path().join("decoder.exe");
    fs::write(&decoder, b"fixture")?;
    let decoder = decoder.to_str().ok_or("decoder path")?;
    assert!(
        invoke(root.path(), &["verify-media", decoder], None)?
            .status
            .success()
    );
    assert!(
        !invoke(root.path(), &["verify-media", decoder], Some("test"))?
            .status
            .success()
    );
    assert!(
        !invoke(
            root.path(),
            &["verify-media", "missing-decoder-213.exe"],
            None
        )?
        .status
        .success()
    );
    assert!(!invoke(root.path(), &[], None)?.status.success());
    assert!(!invoke(root.path(), &["unknown"], None)?.status.success());
    assert!(!invoke(root.path(), &["coastline"], None)?.status.success());
    assert!(
        !invoke(root.path(), &["coastline", decoder], None)?
            .status
            .success()
    );
    Ok(())
}

#[test]
fn coverage_command_measures_every_crate_and_fails_after_a_shortfall() -> TestResult {
    let root = tempfile::tempdir()?;
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    fs::write(
        root.path().join("metadata.json"),
        serde_json::to_vec(
            &serde_json::json!({"workspace_members":["fixture"],"packages":[{"id":"fixture","name":"fixture","manifest_path":manifest}],"target_directory":root.path().join("target")}),
        )?,
    )?;
    let filename = manifest.parent().ok_or("package root")?.join("src/main.rs");
    let report = |covered| {
        serde_json::to_vec(
            &serde_json::json!({"type":"llvm.coverage.json.export","data":[{"files":[{"filename":filename,"summary":{"lines":{"count":100,"covered":covered}}}]}]}),
        )
    };
    fs::write(root.path().join("report.json"), report(80)?)?;
    let decoder = root.path().join("decoder.exe");
    fs::write(&decoder, b"fixture")?;
    let decoder = decoder.to_str().ok_or("decoder path")?;
    let output = invoke(root.path(), &["verify-coverage", decoder], None)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commands = fs::read_to_string(root.path().join("commands.log"))?;
    assert!(commands.starts_with(
        "metadata --locked --no-deps --format-version 1\nllvm-cov clean --workspace\n"
    ));
    assert!(commands.contains("--test-threads=2"));
    assert!(commands.contains("--ignored --test-threads=1"));
    assert!(commands.contains("--include-build-script --no-default-ignore-filename-regex"));
    assert!(commands.contains("show-env --pwsh"));
    let receipts = root.path().join("target/coverage-reports");
    let first = fs::read_dir(&receipts)?
        .next()
        .ok_or("missing receipt")??
        .path();
    let first_bytes = fs::read(&first)?;
    fs::write(root.path().join("report.json"), report(79)?)?;
    let output = invoke(root.path(), &["verify-coverage", decoder], None)?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("below threshold: fixture"));
    assert_eq!(fs::read(first)?, first_bytes);
    assert_eq!(fs::read_dir(receipts)?.count(), 2);
    assert_stops(root.path(), decoder, "metadata", "llvm-cov clean")?;
    assert_stops(root.path(), decoder, "llvm-cov clean", "--test-threads")?;
    assert_stops(root.path(), decoder, "llvm-cov show-env", "build")?;
    assert_stops(root.path(), decoder, "build", "--ignored")?;
    fs::write(root.path().join("metadata.json"), b"invalid")?;
    assert!(
        !invoke(root.path(), &["verify-coverage", decoder], None)?
            .status
            .success()
    );
    Ok(())
}
