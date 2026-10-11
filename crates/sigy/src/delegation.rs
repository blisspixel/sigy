//! Fixed systemd user units own Linux native work and abrupt-death cleanup.

use sha2::{Digest, Sha256};
use sigy_service::control::{self, Operation, Snapshot};
use std::{
    path::Path,
    process::{Command, ExitCode, Stdio},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const LAUNCHED: &str = "SIGY_DELEGATED_LAUNCH";

pub(super) fn available() -> bool {
    use std::os::unix::fs::FileTypeExt;
    let uid = rustix::process::geteuid().as_raw();
    Path::new("/usr/bin/systemd-run").is_file()
        && std::fs::metadata(format!("/run/user/{uid}/bus"))
            .is_ok_and(|metadata| metadata.file_type().is_socket())
}

fn native(command: &super::Command) -> bool {
    matches!(
        command,
        super::Command::Play { .. }
            | super::Command::Listen {
                command: super::listen::ListenCommand::File { .. }
                    | super::listen::ListenCommand::Finding { .. }
                    | super::listen::ListenCommand::Source { .. }
                    | super::listen::ListenCommand::Play { .. }
            }
            | super::Command::Panel {
                command: super::panel::PanelCommand::Play { .. }
            }
            | super::Command::Service {
                command: super::service::ServiceCommand::Run { .. }
            }
    )
}

/// Enter a managed native client before constructing its async runtime.
pub(super) fn enter(cli: &super::Cli) -> Result<Option<ExitCode>> {
    if !native(&cli.command) || sigy_service::native::delegated_parent().is_ok() {
        return Ok(None);
    }
    if std::env::var_os(LAUNCHED).is_some() {
        return Err(sigy_service::native::delegated_parent()
            .err()
            .ok_or("delegation state changed")?
            .into());
    }
    if !available() {
        if matches!(cli.command, super::Command::Service { .. }) {
            return Ok(None);
        }
        return Err("bounded native containment unavailable: Linux requires a systemd user manager with cpu, memory and pids delegation; inspect `sigy doctor`".into());
    }
    let id = crate::radio::request_id("native")?;
    let mut command = launcher(&format!("sigy-{id}.service"))?;
    command.args(["--wait", "--pipe"]);
    if !matches!(cli.command, super::Command::Service { .. }) {
        command.arg("--property=RuntimeMaxSec=31min");
    }
    command.arg(format!(
        "--setenv=SIGY_NATIVE_OWNER_PID={}",
        std::process::id()
    ));
    forward_audio_environment(&mut command)?;
    command
        .arg("--")
        .arg(std::env::current_exe()?)
        .args(std::env::args_os().skip(1));
    let status = command.status()?;
    Ok(Some(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }))
}

fn launcher(unit: &str) -> Result<Command> {
    let uid = rustix::process::geteuid().as_raw();
    let mut command = Command::new("/usr/bin/systemd-run");
    command
        .args([
            "--user",
            "--quiet",
            "--collect",
            "--service-type=exec",
            "--expand-environment=no",
            "--unit",
            unit,
            "--property=Delegate=cpu memory pids",
            "--property=DelegateSubgroup=sigy-control",
            "--property=KillMode=control-group",
            "--property=SendSIGKILL=yes",
            "--property=TimeoutStopSec=5s",
            "--property=UMask=0077",
            "--setenv=SIGY_DELEGATED_LAUNCH=1",
        ])
        .arg(format!("--setenv=XDG_RUNTIME_DIR=/run/user/{uid}"))
        .arg(format!(
            "--working-directory={}",
            std::env::current_dir()?.display()
        ))
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path=/run/user/{uid}/bus"),
        )
        .env("SYSTEMD_BUS_TIMEOUT", "2s");
    Ok(command)
}

fn forward_audio_environment(command: &mut Command) -> Result<()> {
    // Explicit local desktop settings are passed as argv, never evaluated by a shell.
    for key in [
        "PULSE_SERVER",
        "PIPEWIRE_REMOTE",
        "DISPLAY",
        "WAYLAND_DISPLAY",
    ] {
        if let Ok(value) = std::env::var(key) {
            if value.len() > 4096 || value.contains(['\0', '\n', '\r']) {
                return Err("invalid desktop audio environment".into());
            }
            command.arg(format!("--setenv={key}={value}"));
        }
    }
    Ok(())
}

pub(super) async fn start(directory: &Path) -> Result<Snapshot> {
    let unit = library_unit(directory);
    let mut command = launcher(&unit)?;
    command.stdin(Stdio::null()).stdout(Stdio::null());
    command
        .arg("--")
        .arg(std::env::current_exe()?)
        .arg("--data-dir")
        .arg(directory)
        .args(["service", "run"]);
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let started = tokio::time::timeout(Duration::from_secs(5), command.status())
        .await??
        .success();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(snapshot)) =
            tokio::time::timeout_at(deadline, control::request(directory, Operation::Status {}))
                .await
        {
            return Ok(snapshot);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if started {
        let mut stop = tokio::process::Command::new("/usr/bin/systemctl");
        stop.args(["--user", "stop", &unit])
            .kill_on_drop(true)
            .env("SYSTEMD_BUS_TIMEOUT", "2s")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!(
                    "unix:path=/run/user/{}/bus",
                    rustix::process::geteuid().as_raw()
                ),
            );
        let _ = tokio::time::timeout(Duration::from_secs(7), stop.status()).await;
    }
    Err(
        format!("managed service did not become ready; inspect `journalctl --user -u {unit}`")
            .into(),
    )
}

fn library_unit(directory: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let hash = Sha256::digest(directory.as_os_str().as_bytes());
    let hex = hash
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            const DIGITS: &[u8] = b"0123456789abcdef";
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 15)]));
            text
        });
    format!("sigy-library-{hex}.service")
}

pub(super) async fn owner_gone() {
    let Some(pid) = std::env::var("SIGY_NATIVE_OWNER_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 1)
    else {
        return std::future::pending().await;
    };
    loop {
        if !crate::panel::is_process_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn only_explicit_native_commands_enter_managed_clients() -> Result<()> {
        for args in [
            vec!["play", "uuid"],
            vec!["listen", "source", "id", "--revision", "revision"],
            vec!["service", "run"],
        ] {
            let mut argv = vec!["sigy"];
            argv.extend(args);
            assert!(native(&super::super::Cli::try_parse_from(argv)?.command));
        }
        for args in [
            vec!["doctor"],
            vec!["radio", "search"],
            vec!["tui"],
            vec!["service", "start"],
            vec!["stop"],
        ] {
            let mut argv = vec!["sigy"];
            argv.extend(args);
            assert!(!native(&super::super::Cli::try_parse_from(argv)?.command));
        }
        assert_eq!(library_unit(Path::new("/a")), library_unit(Path::new("/a")));
        assert_ne!(library_unit(Path::new("/a")), library_unit(Path::new("/b")));
        Ok(())
    }
}
