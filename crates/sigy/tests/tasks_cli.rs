use std::{io, path::Path, process::Command};

mod common;

fn invoke(directory: &Path, arguments: &[&str]) -> std::io::Result<std::process::Output> {
    let context = match arguments.get(1).copied() {
        Some("--help") => "task help",
        Some("create") => "task create",
        Some("list") => "task list",
        Some("checkpoint") => "task checkpoint",
        Some("checkpoint-show") => "task checkpoint-show",
        Some("execute") => "task execute",
        Some("cancel") => "task cancel",
        _ => "task command",
    };
    common::output(
        Command::new(env!("CARGO_BIN_EXE_sigy"))
            .arg("--data-dir")
            .arg(directory)
            .args(arguments),
    )
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{context}: bounded CLI invocation failed: {error}"),
        )
    })
}

#[test]
fn hostile_task_scope_is_refused_before_opening_a_library() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let directory = root.path().join("never-opened");
    let private_goal = "private-task-canary\u{1b}[2J";
    let output = invoke(
        &directory,
        &[
            "task",
            "create",
            "water",
            "--goal",
            private_goal,
            "--monitor",
            "monitor",
            "--monitor-version",
            "1",
            "--monitor-actions",
            "0",
            "--from-ms",
            "0",
            "--to-ms",
            "60000",
        ],
    )?;
    assert!(!output.status.success());
    assert!(!directory.exists());
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(!stdout.contains("private-task-canary"));
    assert!(!stderr.contains("private-task-canary"));
    assert!(!stderr.contains('\u{1b}'));
    assert!(stderr.contains("task scope"));
    Ok(())
}

#[test]
fn task_help_and_parser_expose_bounded_observation_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("never-opened");
    let output = invoke(&directory, &["task", "--help"])?;
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout)?;
    for command in [
        "create",
        "show",
        "list",
        "checkpoint",
        "checkpoint-show",
        "execute",
        "execution",
        "cancel",
    ] {
        assert!(help.contains(command));
    }
    assert!(help.contains("Creates no worker jobs"));
    for arguments in [
        vec!["task", "list", "--limit", "17"],
        vec!["task", "list", "--limit", "0"],
        vec!["task", "checkpoint", "water", "request"],
        vec!["task", "checkpoint-show", "water", "0"],
        vec!["task", "checkpoint-show", "water", "129"],
        vec![
            "task",
            "execute",
            "water",
            "request",
            "--checkpoint",
            "1",
            "--max-findings",
            "65",
            "--expected-generation",
            "0",
        ],
        vec![
            "task",
            "execute",
            "water",
            "request",
            "--checkpoint",
            "0",
            "--max-findings",
            "1",
            "--expected-generation",
            "0",
        ],
        vec![
            "task",
            "execute",
            "water",
            "request",
            "--checkpoint",
            "1",
            "--max-findings",
            "1",
        ],
        vec![
            "task",
            "cancel",
            "water",
            "cancel",
            "--expected-generation",
            "0",
        ],
    ] {
        let refused = invoke(&directory, &arguments)?;
        assert!(!refused.status.success());
    }
    assert!(!directory.exists());
    Ok(())
}
