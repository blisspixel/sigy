use std::process::Command;
mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn help_and_version_work_without_initializing_a_library() -> TestResult {
    for arguments in [&["--help"][..], &["--version"][..]] {
        let output = common::output(Command::new(env!("CARGO_BIN_EXE_sigy")).args(arguments))?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn cli_initializes_reopens_and_reports_exact_budgets() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("library");
    for arguments in [
        vec!["library", "init"],
        vec!["budget", "set", "global", "--usd", "0.123456"],
        vec!["library", "status"],
    ] {
        let output = common::output(
            Command::new(env!("CARGO_BIN_EXE_sigy"))
                .arg("--data-dir")
                .arg(&library)
                .arg("--json")
                .args(&arguments),
        )?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(value["provider_dispatch_available"], false);
        assert_eq!(value["budgets"][0]["reserved_usd"], "0.000000");
        assert_eq!(value["budgets"][0]["settled_usd"], "0.000000");
        if arguments != ["library", "init"] {
            assert_eq!(value["budgets"][0]["limit_usd"], "0.123456");
        }
    }
    Ok(())
}

#[test]
fn missing_library_fails_with_json_and_no_success_output() -> TestResult {
    let directory = tempfile::tempdir()?;
    let output = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path().join("missing"))
            .args(["--json", "library", "status"]),
    )?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr)?;
    assert!(error["error"].is_string());
    Ok(())
}
