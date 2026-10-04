use std::process::Command;
mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn plain_directory_reads_render_their_scoped_results_instead_of_library_status() -> TestResult {
    let directory = tempfile::tempdir()?;
    let initialized = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(["library", "init"]),
    )?;
    assert!(initialized.status.success());
    for (arguments, expected) in [
        (
            vec!["radio", "linked", "00000000-0000-0000-0000-000000000001"],
            "Linked station 00000000-0000-0000-0000-000000000001",
        ),
        (vec!["radio", "search", "--order", "name"], "Name order"),
    ] {
        let output = common::output(
            Command::new(env!("CARGO_BIN_EXE_sigy"))
                .arg("--data-dir")
                .arg(directory.path())
                .env("NO_COLOR", "1")
                .args(arguments),
        )?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let display = String::from_utf8(output.stdout)?;
        assert!(display.contains(expected), "{display}");
        assert!(!display.contains("Library ready.") && !display.contains("Provider dispatch"));
    }
    assert!(!directory.path().join("service.json").exists());
    Ok(())
}

#[test]
fn update_help_needs_no_home_and_missing_default_guides_initialization() -> TestResult {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("isolated user home");
    std::fs::create_dir(&home)?;
    let help = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .env_remove("USERPROFILE")
            .env_remove("HOME")
            .args(["update", "--help"]),
    )?;
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    let missing = common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .env("USERPROFILE", &home)
            .env("HOME", &home)
            .current_dir(directory.path())
            .args(["library", "status"]),
    )?;
    assert!(!missing.status.success());
    let error = String::from_utf8_lossy(&missing.stderr);
    assert!(error.contains("sigy init"), "{error}");
    assert!(!home.join(".sigy").exists());
    Ok(())
}

#[test]
fn update_status_is_local_bounded_and_preserves_structured_data() -> TestResult {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("isolated update home");
    std::fs::create_dir(&home)?;
    let invoke = |arguments: &[&str]| {
        common::output(
            Command::new(env!("CARGO_BIN_EXE_sigy"))
                .env("USERPROFILE", &home)
                .env("HOME", &home)
                .env("PATH", "")
                .current_dir(directory.path())
                .args(arguments),
        )
    };
    let empty = invoke(&["update", "--status", "--json"])?;
    assert!(empty.status.success());
    let value: serde_json::Value = serde_json::from_slice(&empty.stdout)?;
    assert!(value["recorded_update"].is_null());
    assert_eq!(value["liveness_observed"], false);
    assert!(!home.join(".sigy").exists());

    let metadata = home.join(".sigy");
    std::fs::create_dir(&metadata)?;
    let root = home.join("root\u{1b}[31m\u{202e}suffix");
    let receipt = serde_json::json!({
        "protocol": 1, "operation": "12-34", "state": "pending",
        "commit": "a".repeat(40), "install_root": root,
        "reason": "\u{202e}held"
    });
    let path = metadata.join("update-status.json");
    let original = serde_json::to_vec(&receipt)?;
    std::fs::write(&path, &original)?;
    let plain = invoke(&["update", "--status"])?;
    assert!(plain.status.success());
    let text = String::from_utf8(plain.stdout)?;
    assert!(
        text.contains("pending") && text.contains("Completion is unproven"),
        "{text}"
    );
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{202e}'),
        "{text}"
    );
    let structured = invoke(&["update", "--status", "--json"])?;
    assert!(structured.status.success());
    let value: serde_json::Value = serde_json::from_slice(&structured.stdout)?;
    assert_eq!(value["recorded_update"], receipt);
    assert_eq!(std::fs::read(&path)?, original);
    assert!(!metadata.join("library").exists());
    assert!(!metadata.join("src").exists());
    assert!(!metadata.join("install.lock").exists());

    std::fs::write(&path, vec![b' '; 8193])?;
    let oversized = invoke(&["update", "--status", "--json"])?;
    assert!(!oversized.status.success());
    let error: serde_json::Value = serde_json::from_slice(&oversized.stderr)?;
    assert!(
        error["error"]
            .as_str()
            .is_some_and(|message| message.contains("byte bound"))
    );
    assert!(oversized.stdout.is_empty());
    let conflict = invoke(&["update", "--check", "--status"])?;
    assert!(!conflict.status.success());
    assert!(String::from_utf8(conflict.stderr)?.contains("cannot be used with"));
    Ok(())
}

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
        if arguments == ["--help"] {
            let help = String::from_utf8(output.stdout)?;
            assert!(help.contains("Usage: sigy [OPTIONS] <COMMAND>"), "{help}");
        }
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

const CANARY: &str = "sk-or-v1-canary-0123456789abcdef";
const ROUTE_ADD: [&str; 16] = [
    "provider",
    "route",
    "add",
    "or-es-en",
    "--provider",
    "openrouter",
    "--origin",
    "https://openrouter.ai",
    "--model",
    "vendor/model",
    "--upstream",
    "deepinfra",
    "--secret-env",
    "SIGY_TEST_PROVIDER_KEY",
    "--pair",
    "es:en",
];
const PRICE_ADD: [&str; 14] = [
    "provider",
    "price",
    "add",
    "p1",
    "--route",
    "or-es-en",
    "--retrieved",
    "2026-09-24T00:00:00Z",
    "--prompt",
    "0.00000015",
    "--completion",
    "0.0000006",
    "--note",
    "fixture catalog, not a real price",
];

fn provider_invoke(
    library: &std::path::Path,
    json: bool,
    arguments: &[&str],
) -> std::io::Result<std::process::Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sigy"));
    command
        .env("SIGY_TEST_PROVIDER_KEY", CANARY)
        .arg("--data-dir")
        .arg(library);
    if json {
        command.arg("--json");
    }
    common::output(command.args(arguments))
}

fn contains_canary(bytes: &[u8]) -> bool {
    bytes
        .windows(CANARY.len())
        .any(|window| window == CANARY.as_bytes())
}

#[test]
fn provider_configuration_names_the_secret_and_never_prints_its_value() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("library");
    let mut outputs = Vec::new();
    for (json, arguments) in [
        (true, &["library", "init"][..]),
        (true, &ROUTE_ADD[..]),
        (false, &ROUTE_ADD[..]),
        (true, &PRICE_ADD[..]),
        (true, &["provider", "route", "show", "or-es-en"][..]),
        (false, &["provider", "route", "show", "or-es-en"][..]),
        (true, &["provider", "route", "list"][..]),
        (false, &["provider", "price", "show", "p1"][..]),
        (true, &["library", "status"][..]),
    ] {
        let output = provider_invoke(&library, json, arguments)?;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!contains_canary(&output.stdout) && !contains_canary(&output.stderr));
        outputs.push(output);
    }
    let shown: serde_json::Value =
        serde_json::from_slice(&outputs.get(4).ok_or("route show")?.stdout)?;
    assert_eq!(shown["provider_dispatch_available"], false);
    assert_eq!(shown["budgets"][0]["limit_usd"], "0.000000");
    let route = &shown["provider"]["routes"][0];
    assert_eq!(route["secret_env"], "SIGY_TEST_PROVIDER_KEY");
    assert_eq!(route["allow_fallbacks"], false);
    assert_eq!(route["dispatch_available"], false);
    assert_eq!(route["language_pairs"][0]["validation"], "unvalidated");
    assert_eq!(
        shown["provider"]["prices"][0]["rates"][0]["usd"],
        "0.00000015"
    );
    let human = String::from_utf8(outputs.get(2).ok_or("human add")?.stdout.clone())?;
    assert!(
        human.contains("SIGY_TEST_PROVIDER_KEY (name only"),
        "{human}"
    );
    assert!(human.contains("Unchanged."), "{human}");
    for entry in std::fs::read_dir(&library)? {
        let path = entry?.path();
        if path.is_file() {
            assert!(
                !contains_canary(&std::fs::read(&path)?),
                "{} holds the secret value",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn provider_commands_refuse_key_text_and_offer_no_send() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("library");
    assert!(
        provider_invoke(&library, true, &["library", "init"])?
            .status
            .success()
    );
    let mut pasted = ROUTE_ADD;
    pasted[13] = CANARY;
    let refused = provider_invoke(&library, true, &pasted)?;
    assert!(!refused.status.success());
    let listed = provider_invoke(&library, true, &["provider", "route", "list"])?;
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout)?;
    assert_eq!(listed["provider"]["routes"], serde_json::json!([]));
    let budget = provider_invoke(&library, false, &["budget", "show"])?;
    let budget = String::from_utf8(budget.stdout)?;
    assert!(
        budget.contains("global: lifetime limit $0.000000 (never resets)"),
        "{budget}"
    );
    let help = provider_invoke(&library, false, &["provider", "--help"])?;
    let help = String::from_utf8(help.stdout)?;
    assert!(help.contains("route") && help.contains("price"), "{help}");
    for word in ["send", "test", "dispatch", "call", "run"] {
        assert!(
            !help.lines().any(|line| line.trim_start().starts_with(word)),
            "{help}"
        );
    }
    Ok(())
}
