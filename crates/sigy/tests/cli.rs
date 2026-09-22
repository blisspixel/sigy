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

#[test]
fn source_cli_registers_explicit_authority_preserves_languages_and_bounds_lists() -> TestResult {
    let directory = tempfile::tempdir()?;
    let invoke = |args: &[&str]| {
        common::output(
            Command::new(env!("CARGO_BIN_EXE_sigy"))
                .arg("--data-dir")
                .arg(directory.path())
                .arg("--json")
                .args(args),
        )
    };
    assert!(invoke(&["library", "init"])?.status.success());
    let public_denied = invoke(&[
        "source",
        "add",
        "local:v1",
        "--name",
        "Diné Bizaad",
        "--url",
        "http://127.0.0.1:8123/audio",
    ])?;
    assert!(!public_denied.status.success());
    assert!(public_denied.stdout.is_empty());
    let arguments = [
        "source",
        "add",
        "local:v1",
        "--name",
        "Diné Bizaad",
        "--url",
        "http://127.0.0.1:8123/audio?token=hidden",
        "--pin-address",
        "127.0.0.1",
    ];
    for newly_created in [true, false] {
        let output = invoke(&arguments)?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout)?;
        assert!(!text.contains("hidden"));
        let value: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(value["source_page"]["newly_created"], newly_created);
        assert_eq!(value["source_page"]["entries"][0]["name"], "Diné Bizaad");
        assert_eq!(
            value["source_page"]["entries"][0]["network"]["kind"],
            "pinned_address"
        );
        assert_eq!(value["captures"]["dispatch_available"], false);
    }
    let output = invoke(&[
        "source",
        "add",
        "public:v1",
        "--name",
        "tlhIngan Hol",
        "--url",
        "https://unresolved.invalid/radio",
    ])?;
    assert!(output.status.success());
    let page = invoke(&["source", "list", "--limit", "1"])?;
    let page: serde_json::Value = serde_json::from_slice(&page.stdout)?;
    assert_eq!(page["source_page"]["next_after"], "local:v1");
    let next = invoke(&["source", "list", "--limit", "1", "--after", "local:v1"])?;
    let next: serde_json::Value = serde_json::from_slice(&next.stdout)?;
    assert_eq!(next["source_page"]["entries"][0]["name"], "tlhIngan Hol");
    assert!(next["source_page"]["next_after"].is_null());
    assert!(
        !invoke(&["source", "list", "--limit", "1000000"])?
            .status
            .success()
    );
    assert!(!invoke(&["source", "show", "missing"])?.status.success());
    let output = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["source", "show", "local:v1"]),
    )?;
    assert!(output.status.success());
    let display = String::from_utf8(output.stdout)?;
    assert!(display.contains("Diné Bizaad"));
    assert!(!display.contains("hidden"));
    Ok(())
}

fn podcast_invoke(
    directory: &std::path::Path,
    args: &[&str],
) -> std::io::Result<std::process::Output> {
    common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory)
            .arg("--json")
            .args(args),
    )
}

#[test]
fn podcast_subscribe_is_local_and_omits_feed_paths() -> TestResult {
    let directory = tempfile::tempdir()?;
    assert!(
        podcast_invoke(directory.path(), &["library", "init"])?
            .status
            .success()
    );
    let denied = podcast_invoke(
        directory.path(),
        &[
            "podcast",
            "subscribe",
            "local",
            "--url",
            "http://127.0.0.1:9/secret/feed.xml?token=hidden",
        ],
    )?;
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&denied.stderr).contains("hidden"));
    let arguments = [
        "podcast",
        "subscribe",
        "local",
        "--url",
        "http://127.0.0.1:9/secret/feed.xml?token=hidden",
        "--pin-address",
        "127.0.0.1",
    ];
    for newly_created in [true, false] {
        let output = podcast_invoke(directory.path(), &arguments)?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout)?;
        assert!(!text.contains("hidden") && !text.contains("secret"));
        let value: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(
            value["schema_version"],
            sigy_service::storage::SCHEMA_VERSION
        );
        assert_eq!(value["podcast_page"]["newly_created"], newly_created);
        assert_eq!(
            value["podcast_page"]["entries"][0]["origin"],
            "http://127.0.0.1:9"
        );
        assert_eq!(value["podcast_page"]["entries"][0]["polls"], "active");
        assert_eq!(value["captures"]["scheduled"], 0);
        assert_eq!(value["captures"]["active"], 0);
        assert_eq!(value["captures"]["terminal"], 0);
        assert!(value["source_page"].is_null());
    }
    let plain = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["podcast", "show", "local"]),
    )?;
    assert!(plain.status.success());
    let display = String::from_utf8(plain.stdout)?;
    assert!(display.contains("pinned address 127.0.0.1"));
    assert!(display.contains("polls: active"));
    assert!(!display.contains("hidden"));
    Ok(())
}

#[test]
fn podcast_unsubscribe_stops_polls_without_a_source_or_capture() -> TestResult {
    let directory = tempfile::tempdir()?;
    assert!(
        podcast_invoke(directory.path(), &["library", "init"])?
            .status
            .success()
    );
    let public = podcast_invoke(
        directory.path(),
        &[
            "podcast",
            "subscribe",
            "remote",
            "--url",
            "https://unresolved.invalid/secret/feed.xml?token=hidden",
        ],
    )?;
    assert!(
        public.status.success(),
        "{}",
        String::from_utf8_lossy(&public.stderr)
    );
    assert!(!String::from_utf8_lossy(&public.stdout).contains("hidden"));
    let changed = podcast_invoke(
        directory.path(),
        &[
            "podcast",
            "subscribe",
            "remote",
            "--url",
            "https://unresolved.invalid/other.xml?token=hidden",
            "--redirects",
            "public",
        ],
    )?;
    assert!(!changed.status.success());
    assert!(!String::from_utf8_lossy(&changed.stderr).contains("hidden"));
    assert!(
        podcast_invoke(
            directory.path(),
            &[
                "podcast",
                "subscribe",
                "local",
                "--url",
                "http://127.0.0.1:9/feed.xml",
                "--pin-address",
                "127.0.0.1",
            ],
        )?
        .status
        .success()
    );
    let page = podcast_invoke(directory.path(), &["podcast", "list", "--limit", "1"])?;
    let page: serde_json::Value = serde_json::from_slice(&page.stdout)?;
    assert_eq!(page["podcast_page"]["next_after"], "local");
    let stopped = podcast_invoke(directory.path(), &["podcast", "unsubscribe", "remote"])?;
    let stopped: serde_json::Value = serde_json::from_slice(&stopped.stdout)?;
    assert_eq!(stopped["podcast_page"]["newly_stopped"], true);
    assert_eq!(stopped["podcast_page"]["entries"][0]["polls"], "stopped");
    assert_eq!(stopped["captures"]["scheduled"], 0);
    let repeat = podcast_invoke(directory.path(), &["podcast", "unsubscribe", "remote"])?;
    let repeat: serde_json::Value = serde_json::from_slice(&repeat.stdout)?;
    assert_eq!(repeat["podcast_page"]["newly_stopped"], false);
    let sources = podcast_invoke(directory.path(), &["source", "list"])?;
    let sources: serde_json::Value = serde_json::from_slice(&sources.stdout)?;
    assert_eq!(sources["source_page"]["entries"], serde_json::json!([]));
    assert!(
        !podcast_invoke(directory.path(), &["podcast", "list", "--limit", "1000000"])?
            .status
            .success()
    );
    assert!(
        !podcast_invoke(directory.path(), &["podcast", "show", "missing"])?
            .status
            .success()
    );
    Ok(())
}
