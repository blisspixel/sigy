//! Actual process output for common newcomer mistakes. No service or network is used.
use std::process::Command;
mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn run(
    library: &std::path::Path,
    arguments: &[&str],
) -> Result<(bool, String, String), Box<dyn std::error::Error>> {
    let output = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .env("NO_COLOR", "1")
            .arg("--data-dir")
            .arg(library)
            .args(arguments),
    )?;
    Ok((
        output.status.success(),
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

#[test]
fn offline_mistakes_explain_what_happened_and_what_to_run() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("guided library é");
    let (created, _, error) = run(&library, &["init", "--no-start"])?;
    assert!(created, "{error}");
    for (arguments, expected) in [
        (
            &["service", "status"][..],
            "no service is running for this library",
        ),
        (&["service", "stop"], "nothing was stopped"),
        (
            &["record", "show", "evening"],
            "no recording has ID evening",
        ),
        (
            &["radio", "refresh-status", "first"],
            "no directory refresh has ID first",
        ),
        (
            &["source", "show", "news:v1"],
            "no registered source revision has ID news:v1",
        ),
        (&["radio", "refresh", "first"], "`sigy --data-dir"),
        (
            &["radio", "show", "not-a-station"],
            "Station IDs are directory UUIDs",
        ),
        (
            &["dvr", "configure", "--decoder", "ffmpeg"],
            "decoder ffmpeg does not exist",
        ),
        (
            &["listen", "file", "evening", "--destination", "null"],
            "`sigy dvr configure --decoder ABSOLUTE_PATH_TO_FFMPEG`",
        ),
    ] {
        let (succeeded, stdout, stderr) = run(&library, arguments)?;
        assert!(!succeeded, "{arguments:?}: {stdout}");
        assert!(stdout.is_empty(), "{arguments:?}: {stdout}");
        assert!(stderr.contains(expected), "{arguments:?}: {stderr}");
        assert!(!stderr.contains("os error"), "{arguments:?}: {stderr}");
        assert!(
            !stderr.contains("record not found"),
            "{arguments:?}: {stderr}"
        );
    }
    let (healthy, doctor, error) = run(&library, &["doctor"])?;
    if cfg!(target_os = "windows") {
        assert!(healthy, "{doctor}\n{error}");
    } else {
        // This ordinary offline CLI child is not a delegated service owner.
        // Linux and macOS must report the unavailable native boundary.
        assert!(!healthy, "{doctor}");
        assert!(doctor.contains("Native containment"), "{doctor}");
        assert!(doctor.contains("blocked"), "{doctor}");
        assert!(!doctor.contains("decoder_containment"), "{doctor}");
        assert!(error.contains("blocked"), "{error}");
        assert!(
            !error.contains("doctor found a check that needs action"),
            "{error}"
        );
    }
    assert!(doctor.contains("Service: not running."), "{doctor}");
    for line in doctor.lines() {
        assert!(line.chars().count() <= 100, "{line}");
    }
    // A long library path wraps the next command. The continuation indent is 16 spaces.
    let commands = doctor.replace("\n                ", " ");
    assert!(
        commands.contains("dvr configure --decoder PATH_TO_FFMPEG"),
        "{doctor}"
    );
    assert!(commands.contains("radio search --fetch"), "{doctor}");
    assert!(!doctor.contains("NEW_ID"), "{doctor}");
    assert!(!doctor.contains("with the same --data-dir"), "{doctor}");
    let (initialized, setup, error) = run(&library, &["init", "--no-start"])?;
    assert!(initialized, "{error}");
    assert!(!setup.contains(r"\\?\"), "{setup}");
    // Guidance never starts the service or changes the library.
    let (checked, status, error) = run(&library, &["--json", "library", "status"])?;
    assert!(checked, "{error}");
    let status: serde_json::Value = serde_json::from_str(&status)?;
    assert!(status["service"].is_null());
    assert_eq!(status["captures"]["scheduled"], 0);
    Ok(())
}
