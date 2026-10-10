use std::process::Command;
mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn panel_status_and_bar_render_offline_state_and_safe_json() -> TestResult {
    let directory = tempfile::tempdir()?;
    let initialized = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["library", "init"]),
    )?;
    assert!(initialized.status.success());

    // Text status output
    let status_out = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .env("NO_COLOR", "1")
            .args(["panel", "status"]),
    )?;
    assert!(
        status_out.status.success(),
        "{}",
        String::from_utf8_lossy(&status_out.stderr)
    );
    let status_text = String::from_utf8(status_out.stdout)?;
    assert!(
        status_text.contains("World-radio panel. Read only. This is not a qualified installation.")
    );
    assert!(status_text.contains("Service: absent."));
    assert!(status_text.contains("Directory: empty."));
    assert!(status_text.contains("Favorites: none shown."));
    assert!(status_text.contains("Recordings: 0 active. 0 visible rows."));
    assert!(status_text.contains("Listen: none."));
    assert!(status_text.contains("Session hint: absent."));
    assert!(status_text.contains(
        "Recognition and translation are not part of this panel. No language is qualified."
    ));
    assert!(status_text.contains("Device follow is none. The next explicit play uses the current default output. Acoustic delivery is not qualified."));
    assert!(status_text.contains("Explorer: sigy tui."));

    // JSON status output
    let json_out = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["--json", "panel", "status"]),
    )?;
    assert!(
        json_out.status.success(),
        "{}",
        String::from_utf8_lossy(&json_out.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&json_out.stdout)?;
    assert_eq!(doc["qualified_platform"], false);
    assert_eq!(doc["qualified_linux"], false);
    assert_eq!(doc["qualified_omarchy"], false);
    assert_eq!(doc["read_only"], true);
    assert_eq!(doc["service"], "absent");
    assert_eq!(doc["session_hint"], "absent");
    assert_eq!(doc["directory"]["freshness"], "empty");
    assert_eq!(doc["directory"]["cached_stations"], 0);

    // Waybar bar output
    let bar_out = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["panel", "bar"]),
    )?;
    assert!(
        bar_out.status.success(),
        "{}",
        String::from_utf8_lossy(&bar_out.stderr)
    );
    let bar: serde_json::Value = serde_json::from_slice(&bar_out.stdout)?;
    assert_eq!(bar["text"], "offline");
    assert_eq!(bar["class"], "offline");
    assert!(
        bar["tooltip"]
            .as_str()
            .is_some_and(|t| t.contains("World-radio panel"))
    );

    Ok(())
}

#[test]
fn panel_play_and_stop_enforce_service_and_session_checks() -> TestResult {
    let directory = tempfile::tempdir()?;
    let initialized = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["library", "init"]),
    )?;
    assert!(initialized.status.success());

    // Play without service gives service_required
    let play_nosvc = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["panel", "play", "station-1"]),
    )?;
    assert!(!play_nosvc.status.success());
    let err = String::from_utf8_lossy(&play_nosvc.stderr);
    assert!(err.contains("the background service is not running"));

    // Corrupted session hint causes SESSION_UNREADABLE on play and stop
    std::fs::write(
        directory.path().join("panel-session.json"),
        b"{corrupt json}",
    )?;

    let play_corrupt = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["panel", "play", "station-1"]),
    )?;
    assert!(!play_corrupt.status.success());
    let err = String::from_utf8_lossy(&play_corrupt.stderr);
    assert!(err.contains("panel session hint is unreadable"));

    let stop_corrupt = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["panel", "stop"]),
    )?;
    assert!(!stop_corrupt.status.success());
    let err = String::from_utf8_lossy(&stop_corrupt.stderr);
    assert!(err.contains("panel session hint is unreadable"));

    // Remove corrupt session file: stop without session fails cleanly
    std::fs::remove_file(directory.path().join("panel-session.json"))?;
    let stop_nosession = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["panel", "stop"]),
    )?;
    assert!(!stop_nosession.status.success());
    let err = String::from_utf8_lossy(&stop_nosession.stderr);
    assert!(err.contains("no active panel listen session"));

    Ok(())
}
