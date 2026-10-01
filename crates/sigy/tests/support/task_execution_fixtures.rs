//! Actual CLI/IPC publication delegation, cancellation and restart without native work.

use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

use super::{RunningChild, TestResult, invoke, success};

fn prepare(directory: &Path) -> TestResult {
    success(directory, &["library", "init"])?;
    success(
        directory,
        &[
            "source",
            "add",
            "station",
            "--name",
            "Station",
            "--url",
            "https://unresolved.invalid/audio?private=canary",
        ],
    )?;
    success(
        directory,
        &[
            "monitor",
            "create",
            "water",
            "--name",
            "Water",
            "--goal",
            "Follow water",
            "--term",
            "und:water",
            "--source",
            "station",
            "--daily-minutes",
            "1",
            "--total-hours",
            "1",
        ],
    )?;
    success(
        directory,
        &[
            "schedule",
            "create",
            "independent",
            "--source",
            "station",
            "--zone",
            "UTC",
            "--once",
            "2099-01-01T00:00:00",
            "--seconds",
            "60",
            "--max-mib",
            "1",
        ],
    )?;
    Ok(())
}

fn create(directory: &Path, id: &str) -> TestResult {
    success(
        directory,
        &[
            "task",
            "create",
            id,
            "--goal",
            "تابع المياه",
            "--monitor",
            "water",
            "--monitor-version",
            "1",
            "--monitor-actions",
            "0",
            "--from-ms",
            "0",
            "--to-ms",
            "1000",
        ],
    )?;
    success(
        directory,
        &[
            "task",
            "checkpoint",
            id,
            "frozen",
            "--expected-checkpoint",
            "0",
        ],
    )?;
    Ok(())
}

fn execute(
    directory: &Path,
    id: &str,
    request: &str,
    maximum: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(success(
        directory,
        &[
            "task",
            "execute",
            id,
            request,
            "--checkpoint",
            "1",
            "--max-findings",
            maximum,
            "--expected-generation",
            "0",
        ],
    )?["task"]["run"]
        .clone())
}

fn execution(directory: &Path, id: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(success(directory, &["task", "execution", id])?["task"]["run"].clone())
}

fn unchanged(before: &serde_json::Value, after: &serde_json::Value) {
    for field in ["budgets", "captures", "provider_dispatch_available"] {
        assert_eq!(
            before[field], after[field],
            "unexpected workflow effect: {field}"
        );
    }
}

#[test]
fn offline_delegation_and_cancellation_are_exact_and_do_not_change_independent_work() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path();
    prepare(path)?;
    create(path, "cancelled")?;
    assert!(execution(path, "cancelled")?.is_null());
    let before = success(path, &["library", "status"])?;
    let schedule = success(path, &["schedule", "show", "independent"])?["schedule"].clone();
    let processing =
        success(path, &["monitor", "show", "water"])?["monitor"]["monitor"]["processing"].clone();
    let accepted = execute(path, "cancelled", "delegate", "4")?;
    assert_eq!(accepted["state"], "running");
    assert_eq!(accepted["generation"], 1);
    assert_eq!(accepted["planned_findings"], 0);
    assert_eq!(accepted["steps"], serde_json::json!([]));
    assert_eq!(execute(path, "cancelled", "delegate", "4")?, accepted);
    assert!(
        !invoke(
            path,
            &[
                "task",
                "execute",
                "cancelled",
                "delegate",
                "--checkpoint",
                "1",
                "--max-findings",
                "5",
                "--expected-generation",
                "0"
            ]
        )?
        .status
        .success()
    );
    assert!(
        !invoke(
            path,
            &[
                "task",
                "cancel",
                "cancelled",
                "stop",
                "--expected-generation",
                "2"
            ]
        )?
        .status
        .success()
    );
    let cancelled = success(
        path,
        &[
            "task",
            "cancel",
            "cancelled",
            "stop",
            "--expected-generation",
            "1",
        ],
    )?["task"]["run"]
        .clone();
    assert_eq!(cancelled["state"], "cancelled");
    assert_eq!(cancelled["generation"], 2);
    assert_eq!(
        success(
            path,
            &[
                "task",
                "cancel",
                "cancelled",
                "stop",
                "--expected-generation",
                "1"
            ]
        )?["task"]["run"],
        cancelled
    );
    let mut service = RunningChild::start(path)?;
    assert_eq!(execution(path, "cancelled")?, cancelled);
    assert_eq!(
        success(path, &["monitor", "show", "water"])?["monitor"]["monitor"]["processing"],
        processing
    );
    assert_eq!(
        success(path, &["schedule", "show", "independent"])?["schedule"],
        schedule
    );
    unchanged(&before, &success(path, &["service", "status"])?);
    success(path, &["service", "stop"])?;
    service.wait()?;
    assert_eq!(execution(path, "cancelled")?, cancelled);
    Ok(())
}

fn wait_terminal(
    directory: &Path,
    id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let run = execution(directory, id)?;
        if run["state"] != "running" {
            return Ok(run);
        }
        assert!(Instant::now() < deadline, "task reconciliation deadline");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn admitted_publication_survives_restart_and_empty_evidence_stays_partial() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path();
    prepare(path)?;
    create(path, "resume")?;
    create(path, "stale")?;
    let accepted = execute(path, "resume", "delegate", "4")?;
    let before = success(path, &["library", "status"])?;
    let mut service = RunningChild::start(path)?;
    service.0.kill()?;
    service.0.wait()?;
    let mut restarted = RunningChild::start(path)?;
    let finished = wait_terminal(path, "resume")?;
    assert_eq!(finished["state"], "partial");
    assert_eq!(finished["created_ms"], accepted["created_ms"]);
    assert_eq!(finished["published_findings"], 0);
    assert!(finished["briefing_id"].is_string());
    assert_eq!(execute(path, "resume", "delegate", "4")?, finished);
    let briefing = finished["briefing_id"]
        .as_str()
        .ok_or("briefing identity")?;
    let report = success(path, &["monitor", "briefing", "water", briefing, "show"])?;
    assert!(report["monitor"]["briefing"].is_object());
    success(path, &["monitor", "pause", "water", "--action-id", "pause"])?;
    assert!(
        !invoke(
            path,
            &[
                "task",
                "execute",
                "stale",
                "delegate",
                "--checkpoint",
                "1",
                "--max-findings",
                "4",
                "--expected-generation",
                "0"
            ]
        )?
        .status
        .success()
    );
    assert!(execution(path, "stale")?.is_null());
    assert_eq!(execute(path, "resume", "delegate", "4")?, finished);
    unchanged(&before, &success(path, &["service", "status"])?);
    success(path, &["service", "stop"])?;
    restarted.wait()?;
    assert_eq!(execution(path, "resume")?, finished);
    Ok(())
}
