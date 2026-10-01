//! Durable task scope and observed checkpoints through actual CLI clients and IPC.
//! No decoder, model or network request is needed by these fixtures.

use super::{RunningChild, TestResult, invoke, success};
use std::path::Path;

const GOAL: &str = "تابع المياه / suivre les eaux";

fn prepare(directory: &Path) -> TestResult {
    success(directory, &["library", "init"])?;
    for id in ["one:v1", "two:v1"] {
        success(
            directory,
            &[
                "source",
                "add",
                id,
                "--name",
                id,
                "--url",
                "https://unresolved.invalid/audio?token=private-source",
            ],
        )?;
    }
    monitor_version(directory, false)?;
    Ok(())
}

fn monitor_version(directory: &Path, revise: bool) -> TestResult {
    let mut args = vec!["monitor", if revise { "revise" } else { "create" }, "water"];
    if revise {
        args.extend(["--expected-version", "1"]);
    }
    args.extend([
        "--name",
        if revise { "Water revised" } else { "Water" },
        "--goal",
        GOAL,
        "--term",
        "und:water",
        "--source",
        "one:v1",
        "--source",
        "two:v1",
        "--daily-minutes",
        "1",
        "--total-hours",
        "1",
    ]);
    success(directory, &args)?;
    Ok(())
}

fn create<'a>(id: &'a str, goal: &'a str, actions: &'a str) -> [&'a str; 15] {
    [
        "task",
        "create",
        id,
        "--goal",
        goal,
        "--monitor",
        "water",
        "--monitor-version",
        "1",
        "--monitor-actions",
        actions,
        "--from-ms",
        "0",
        "--to-ms",
        "1000",
    ]
}

fn create_task(
    directory: &Path,
    id: &str,
    goal: &str,
    actions: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    success(directory, &create(id, goal, actions))
}

fn refuse_create(directory: &Path, id: &str, goal: &str) -> TestResult {
    let output = invoke(directory, &create(id, goal, "0"))?;
    assert!(!output.status.success(), "changed scope must conflict");
    Ok(())
}

fn checkpoint(
    directory: &Path,
    id: &str,
    request: &str,
    expected: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let page = success(
        directory,
        &[
            "task",
            "checkpoint",
            id,
            request,
            "--expected-checkpoint",
            expected,
        ],
    )?;
    assert_eq!(page["task"]["kind"], "checkpoint");
    Ok(page["task"]["checkpoint"].clone())
}

fn refuse_checkpoint(directory: &Path, id: &str, request: &str, expected: &str) -> TestResult {
    let output = invoke(
        directory,
        &[
            "task",
            "checkpoint",
            id,
            request,
            "--expected-checkpoint",
            expected,
        ],
    )?;
    assert!(
        !output.status.success(),
        "stale scope or ordinal must conflict"
    );
    Ok(())
}

fn unchanged_effects(before: &serde_json::Value, after: &serde_json::Value) {
    for field in ["budgets", "captures", "provider_dispatch_available"] {
        assert_eq!(
            before[field], after[field],
            "unexpected task effect: {field}"
        );
    }
}

fn action_and_version_fences(directory: &Path, original: &serde_json::Value) -> TestResult {
    success(
        directory,
        &["monitor", "pause", "water", "--action-id", "pause"],
    )?;
    let stale = success(directory, &["task", "show", "watch"])?;
    assert_eq!(stale["task"]["task"]["scope_current"], false);
    refuse_checkpoint(directory, "watch", "after-pause", "2")?;
    assert_eq!(checkpoint(directory, "watch", "first", "0")?, *original);
    assert_eq!(
        create_task(directory, "watch", GOAL, "0")?["task"]["created"],
        false
    );

    create_task(directory, "paused", GOAL, "1")?;
    let paused = checkpoint(directory, "paused", "paused-first", "0")?;
    assert_eq!(paused["monitor_paused"], true);
    monitor_version(directory, true)?;
    let stale = success(directory, &["task", "show", "paused"])?;
    assert_eq!(stale["task"]["task"]["scope_current"], false);
    refuse_checkpoint(directory, "paused", "after-revision", "1")?;
    assert_eq!(
        checkpoint(directory, "paused", "paused-first", "0")?,
        paused
    );
    Ok(())
}

#[test]
fn task_scope_and_checkpoint_replay_survive_abrupt_service_restart() -> TestResult {
    let directory = tempfile::tempdir()?;
    prepare(directory.path())?;
    let mut service = RunningChild::start(directory.path())?;
    let before = success(directory.path(), &["service", "status"])?;
    let processing = success(directory.path(), &["monitor", "show", "water"])?;
    let accepted = create_task(directory.path(), "watch", GOAL, "0")?;
    assert_eq!(accepted["task"]["created"], true);
    assert_eq!(accepted["task"]["task"]["spec"]["goal"], GOAL);
    assert_eq!(accepted["task"]["task"]["checkpoint"], 0);
    assert!(accepted["task"]["task"]["latest_checkpoint"].is_null());
    let first = checkpoint(directory.path(), "watch", "first", "0")?;
    assert_eq!(first["ordinal"], 1);
    assert_eq!(first["window_elapsed"], true);
    assert_eq!(first["transcripts_scanned"], 0);
    assert_eq!(first["citations"], serde_json::json!([]));
    assert_eq!(checkpoint(directory.path(), "watch", "first", "0")?, first);
    refuse_create(directory.path(), "watch", "a different goal")?;
    refuse_checkpoint(directory.path(), "watch", "stale-ordinal", "0")?;
    let second = checkpoint(directory.path(), "watch", "second", "1")?;
    assert_eq!(second["ordinal"], 2);
    unchanged_effects(&before, &success(directory.path(), &["service", "status"])?);
    assert_eq!(
        processing["monitor"]["monitor"]["processing"],
        success(directory.path(), &["monitor", "show", "water"])?["monitor"]["monitor"]["processing"],
    );

    service.0.kill()?;
    service.0.wait()?;
    let mut restarted = RunningChild::start(directory.path())?;
    let replayed = create_task(directory.path(), "watch", GOAL, "0")?;
    assert_eq!(replayed["task"]["created"], false);
    for field in ["spec", "scope_sha256", "monitor_spec_sha256", "created_ms"] {
        assert_eq!(
            replayed["task"]["task"][field],
            accepted["task"]["task"][field]
        );
    }
    assert_eq!(replayed["task"]["task"]["checkpoint"], 2);
    assert_eq!(replayed["task"]["task"]["latest_checkpoint"], second);
    assert_eq!(checkpoint(directory.path(), "watch", "first", "0")?, first);
    assert_eq!(
        checkpoint(directory.path(), "watch", "second", "1")?,
        second
    );
    assert_eq!(
        success(directory.path(), &["task", "checkpoint-show", "watch", "1"])?["task"]["checkpoint"],
        first
    );
    assert_eq!(
        success(directory.path(), &["task", "list", "--limit", "1"])?["task"]["ids"],
        serde_json::json!(["watch"])
    );
    action_and_version_fences(directory.path(), &first)?;
    unchanged_effects(&before, &success(directory.path(), &["service", "status"])?);
    success(directory.path(), &["service", "stop"])?;
    restarted.wait()?;
    Ok(())
}

#[test]
fn task_checkpoint_history_survives_verified_offline_backup_restore() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("source");
    let backup = directory.path().join("backup");
    let restored = directory.path().join("restored");
    prepare(&source)?;
    create_task(&source, "watch", GOAL, "0")?;
    let first = checkpoint(&source, "watch", "first", "0")?;
    let second = checkpoint(&source, "watch", "second", "1")?;
    let before = success(&source, &["task", "show", "watch"])?["task"].clone();
    success(
        &source,
        &["library", "backup", backup.to_str().ok_or("backup path")?],
    )?;
    success(
        &source,
        &[
            "library",
            "verify-backup",
            backup.to_str().ok_or("backup path")?,
        ],
    )?;
    success(
        &source,
        &[
            "library",
            "restore",
            backup.to_str().ok_or("backup path")?,
            "--into",
            restored.to_str().ok_or("restored path")?,
        ],
    )?;
    assert_eq!(
        success(&restored, &["task", "show", "watch"])?["task"],
        before
    );
    assert_eq!(checkpoint(&restored, "watch", "first", "0")?, first);
    assert_eq!(checkpoint(&restored, "watch", "second", "1")?, second);
    let next = checkpoint(&restored, "watch", "third", "2")?;
    assert_eq!(next["ordinal"], 3);
    assert_eq!(
        success(&restored, &["task", "checkpoint-show", "watch", "1"])?["task"]["checkpoint"],
        first
    );
    Ok(())
}
