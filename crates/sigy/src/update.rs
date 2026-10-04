//! Install or report the latest `main` commit from the GitHub repository.
//! This is not `cargo verify` and it does not download a release binary.

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const REPOSITORY: &str = "https://github.com/blisspixel/sigy.git";
const BUILT_FROM: Option<&str> = option_env!("SIGY_GIT_COMMIT");

mod lock;
mod receipt;
mod windows;

#[cfg(all(test, windows))]
mod test_child;

fn is_full_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdatePlan {
    Current,
    Available,
    Install,
}

fn plan(installed: Option<&str>, latest: &str, check_only: bool) -> UpdatePlan {
    if installed == Some(latest) {
        UpdatePlan::Current
    } else if check_only {
        UpdatePlan::Available
    } else {
        UpdatePlan::Install
    }
}

fn is_commit(value: &str) -> bool {
    let bytes = value.as_bytes();
    (7..=40).contains(&bytes.len()) && bytes.iter().all(u8::is_ascii_hexdigit)
}

fn home_dir() -> Result<PathBuf, &'static str> {
    #[cfg(windows)]
    let value = env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let value = env::var_os("HOME");
    value
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or("the home directory is not set")
}

fn source_dir() -> Result<PathBuf, &'static str> {
    if let Some(path) = source_override(env::var_os("SIGY_SRC")) {
        return Ok(path);
    }
    Ok(home_dir()?.join(".sigy").join("src"))
}

fn source_override(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.filter(|path| !path.is_empty()).map(PathBuf::from)
}

fn record_path() -> Result<PathBuf, &'static str> {
    Ok(home_dir()?.join(".sigy").join("installed-commit"))
}

fn installed_commit() -> Option<String> {
    let recorded = record_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok());
    select_installed_commit(BUILT_FROM, recorded.as_deref())
}

fn select_installed_commit(built_from: Option<&str>, recorded: Option<&str>) -> Option<String> {
    built_from
        .filter(|value| is_commit(value))
        .or_else(|| recorded.map(str::trim).filter(|value| is_commit(value)))
        .map(str::to_owned)
}

fn git_prefix(use_gh: bool) -> Vec<&'static str> {
    let mut prefix = vec!["-c", "core.abbrev=40"];
    if use_gh {
        // Reset helpers, then use the GitHub CLI login for this private repository.
        // The empty helper does not change the user's saved Git configuration.
        prefix.extend([
            "-c",
            "credential.helper=",
            "-c",
            "credential.helper=!gh auth git-credential",
        ]);
    }
    prefix
}

fn github_cli_authenticated() -> bool {
    let Ok(status) = Command::new("gh")
        .args(["auth", "status"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    else {
        return false;
    };
    status.success()
}

fn git(directory: Option<&Path>, args: &[&str], use_gh: bool) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(git_prefix(use_gh));
    command.env("GIT_TERMINAL_PROMPT", "0");
    if let Some(directory) = directory {
        command.arg("-C").arg(directory);
    }
    command.args(args);
    let output = command.output().map_err(|_| {
        "git is not available. Install Git and authenticate to https://github.com/blisspixel/sigy."
            .to_owned()
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(if detail.is_empty() {
            "git failed".to_owned()
        } else {
            format!("git failed: {detail}")
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn ensure_checkout(directory: &Path, use_gh: bool) -> Result<(), String> {
    if directory.join(".git").is_dir() {
        validate_managed_checkout(directory, use_gh)?;
        git(
            Some(directory),
            &["fetch", "--depth", "1", REPOSITORY, "main"],
            use_gh,
        )?;
        git(
            Some(directory),
            &["checkout", "--detach", "FETCH_HEAD"],
            use_gh,
        )?;
        validate_managed_checkout(directory, use_gh)?;
        return Ok(());
    }
    if directory.exists() {
        return Err(format!(
            "{} exists and is not a sigy checkout",
            directory.display()
        ));
    }
    if let Some(parent) = directory.parent() {
        fs::create_dir_all(parent).map_err(|_| "cannot create the sigy source directory")?;
    }
    git(
        None,
        &[
            "clone",
            "--depth",
            "1",
            "--branch",
            "main",
            REPOSITORY,
            &directory.display().to_string(),
        ],
        use_gh,
    )?;
    validate_managed_checkout(directory, use_gh)?;
    Ok(())
}

fn validate_managed_checkout(directory: &Path, use_gh: bool) -> Result<(), String> {
    let origin = git(
        Some(directory),
        &["config", "--local", "--get", "remote.origin.url"],
        use_gh,
    )?;
    if origin != REPOSITORY {
        return Err("managed source origin differs from the Sigy repository".into());
    }
    let changes = git(
        Some(directory),
        &["status", "--porcelain=v1", "--untracked-files=all"],
        use_gh,
    )?;
    if !changes.is_empty() {
        return Err(
            "managed source has local changes; preserve it and use a clean directory".into(),
        );
    }
    Ok(())
}

fn latest_commit(directory: &Path, use_gh: bool) -> Result<String, String> {
    let commit = git(Some(directory), &["rev-parse", "HEAD"], use_gh)?;
    if is_commit(&commit) {
        Ok(commit)
    } else {
        Err("the fetched commit is not a git SHA".into())
    }
}

fn cargo_install(directory: &Path, commit: &str, operation: &str) -> Result<(), String> {
    if cfg!(windows) {
        return windows::install(directory, commit, operation);
    }
    let output = cargo_command(directory, commit)?.output().map_err(|_| {
        "cargo is not available. Install Rust 1.98.1, then run sigy update.".to_owned()
    })?;
    if output.status.success() {
        validate_managed_checkout(directory, false)?;
        if latest_commit(directory, false)? != commit {
            return Err(
                "update source changed during build; installed identity is unproven".into(),
            );
        }
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    Err(format!("cargo install failed: {}", detail.trim()))
}

fn cargo_command(directory: &Path, commit: &str) -> Result<Command, String> {
    let mut command = Command::new("cargo");
    if let Some(path) = cargo_path() {
        command.env("PATH", path);
    }
    command.env("CARGO_HOME", windows::cargo_home()?);
    if let Some(root) = env::var_os("CARGO_INSTALL_ROOT").filter(|root| !root.is_empty()) {
        let root = PathBuf::from(root);
        let absolute = if root.is_absolute() {
            root
        } else {
            env::current_dir()
                .map_err(|_| "cannot resolve install root")?
                .join(root)
        };
        command.env("CARGO_INSTALL_ROOT", absolute);
    }
    command
        .current_dir(directory)
        .env("SIGY_GIT_COMMIT", commit)
        .args(["install", "--path", "crates/sigy", "--locked", "--force"]);
    Ok(command)
}

fn cargo_path() -> Option<std::ffi::OsString> {
    let cargo_bin = windows::cargo_home().ok()?.join("bin");
    let current = env::var_os("PATH")?;
    let mut path = std::ffi::OsString::from(cargo_bin);
    path.push(if cfg!(windows) { ";" } else { ":" });
    path.push(current);
    Some(path)
}

fn remember(commit: &str) -> Result<(), String> {
    let path = record_path().map_err(str::to_owned)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "cannot record the installed commit")?;
    }
    fs::write(path, format!("{commit}\n")).map_err(|_| "cannot record the installed commit")?;
    Ok(())
}

fn report(json: bool, installed: Option<&str>, latest: &str, changed: bool) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    if json {
        writeln!(
            stdout,
            "{{\"installed\":{},\"latest\":\"{latest}\",\"changed\":{changed}}}",
            installed.map_or("null".to_owned(), |commit| format!("\"{commit}\""))
        )?;
        return Ok(());
    }
    match installed {
        Some(commit) if commit == latest && !changed => {
            writeln!(stdout, "sigy is current at {latest}.")
        }
        Some(commit) if changed => {
            writeln!(stdout, "Updated sigy from {commit} to {latest}.")
        }
        Some(commit) => writeln!(
            stdout,
            "Installed sigy is {commit}. Latest main is {latest}. Run sigy update to install it."
        ),
        None if changed => writeln!(stdout, "Installed sigy at {latest}."),
        None => writeln!(
            stdout,
            "No installed commit is recorded. Latest main is {latest}. Run sigy update to install it."
        ),
    }
}

pub(crate) fn run(
    check_only: bool,
    status_only: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if status_only {
        return receipt::status(json);
    }
    let metadata = home_dir().map_err(str::to_owned)?.join(".sigy");
    fs::create_dir_all(&metadata)?;
    let _lock = lock::acquire(&metadata.join("install.lock"))?;
    let directory = source_dir().map_err(str::to_owned)?;
    let use_gh = github_cli_authenticated();
    ensure_checkout(&directory, use_gh)?;
    let latest = latest_commit(&directory, use_gh)?;
    let installed = installed_commit();
    match plan(installed.as_deref(), &latest, check_only) {
        UpdatePlan::Current => report(json, installed.as_deref(), &latest, false)?,
        UpdatePlan::Available => {
            report(json, installed.as_deref(), &latest, false)?;
            let message = if installed.is_none() {
                "no sigy install is recorded"
            } else {
                "a newer sigy commit is available"
            };
            return Err(message.into());
        }
        UpdatePlan::Install => {
            let (prepared, operation) = prepare(&directory, &latest, &metadata, use_gh)?;
            cargo_install(&prepared, &latest, &operation)?;
            if cfg!(windows) {
                let mut stdout = io::stdout().lock();
                if json {
                    serde_json::to_writer(
                        &mut stdout,
                        &serde_json::json!({
                            "installed": installed, "latest": latest, "changed": false,
                            "state": "scheduled", "operation": operation,
                            "next": "sigy update --status"
                        }),
                    )?;
                    writeln!(stdout)?;
                } else {
                    writeln!(
                        stdout,
                        "Fetched {latest}. Hidden installation is scheduled after this process exits."
                    )?;
                    writeln!(
                        stdout,
                        "Inspect its recorded outcome with: sigy update --status"
                    )?;
                }
            } else {
                remember(&latest)?;
                report(json, installed.as_deref(), &latest, true)?;
            }
        }
    }
    Ok(())
}

fn prepare(
    directory: &Path,
    commit: &str,
    metadata: &Path,
    use_gh: bool,
) -> Result<(PathBuf, String), String> {
    if !is_full_commit(commit) {
        return Err("the fetched commit is not a full Git SHA".into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "update clock unavailable")?
        .as_nanos();
    let operation = format!("{}-{stamp}", std::process::id());
    let work_root = metadata.join("update-work");
    fs::create_dir_all(&work_root).map_err(|_| "cannot create update workspace root")?;
    let entries = fs::read_dir(&work_root).map_err(|_| "cannot inspect update workspaces")?;
    if entries.take(16).count() >= 16 {
        return Err(format!(
            "update workspace limit reached; inspect sigy update --status and preserve or remove completed work under {} before retrying",
            work_root.display()
        ));
    }
    let work = work_root.join(&operation);
    fs::create_dir_all(&work).map_err(|_| "cannot create update workspace")?;
    let source = work.join("source");
    git(
        None,
        &[
            "clone",
            "--no-local",
            "--no-hardlinks",
            "--no-checkout",
            &directory.display().to_string(),
            &source.display().to_string(),
        ],
        false,
    )?;
    git(
        Some(&source),
        &["remote", "set-url", "origin", REPOSITORY],
        false,
    )?;
    git(Some(&source), &["checkout", "--detach", commit], false)?;
    validate_managed_checkout(&source, use_gh)?;
    if latest_commit(&source, false)? != commit {
        return Err("prepared update source differs from requested commit".into());
    }
    Ok((source, operation))
}

#[cfg(test)]
mod tests {
    use super::{UpdatePlan, is_commit, plan};

    #[test]
    fn empty_source_override_has_the_same_unset_meaning_as_installers() {
        assert_eq!(super::source_override(None), None);
        assert_eq!(
            super::source_override(Some(std::ffi::OsString::new())),
            None
        );
        assert_eq!(
            super::source_override(Some("relative source".into())),
            Some(std::path::PathBuf::from("relative source"))
        );
    }

    #[test]
    fn status_is_a_read_only_parser_mode_and_conflicts_with_check() {
        use clap::Parser;
        assert!(crate::Cli::try_parse_from(["sigy", "update", "--status", "--json"]).is_ok());
        assert!(crate::Cli::try_parse_from(["sigy", "update", "--status", "--check"]).is_err());
    }

    #[test]
    fn current_commit_does_not_reinstall() {
        let sha = "a".repeat(40);
        assert_eq!(plan(Some(&sha), &sha, false), UpdatePlan::Current);
        assert_eq!(plan(Some(&sha), &sha, true), UpdatePlan::Current);
    }

    #[test]
    fn check_reports_a_newer_commit_without_installing() {
        let old = "a".repeat(40);
        let new = "b".repeat(40);
        assert_eq!(plan(Some(&old), &new, true), UpdatePlan::Available);
        assert_eq!(plan(None, &new, true), UpdatePlan::Available);
        assert_eq!(plan(Some(&old), &new, false), UpdatePlan::Install);
        assert!(is_commit(&old));
        assert!(!is_commit("not a commit"));
        assert!(!is_commit("gg"));
    }

    #[test]
    fn running_binary_commit_outweighs_a_global_install_marker() {
        let built = "a".repeat(40);
        let recorded = "b".repeat(40);
        assert_eq!(
            super::select_installed_commit(Some(&built), Some(&recorded)),
            Some(built)
        );
        assert_eq!(
            super::select_installed_commit(None, Some(&recorded)),
            Some(recorded)
        );
    }

    #[test]
    fn managed_checkout_requires_the_fixed_origin_and_clean_tree()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::{fs, process::Command};

        let temp = tempfile::tempdir()?;
        let source = temp.path().join("managed");
        let init = Command::new("git").arg("init").arg(&source).output()?;
        assert!(init.status.success());
        let add_origin = Command::new("git")
            .arg("-C")
            .arg(&source)
            .args(["remote", "add", "origin", super::REPOSITORY])
            .output()?;
        assert!(add_origin.status.success());
        super::validate_managed_checkout(&source, false)?;

        fs::write(source.join("local-edit.txt"), "untracked")?;
        assert!(super::validate_managed_checkout(&source, false).is_err());
        fs::remove_file(source.join("local-edit.txt"))?;
        let wrong_origin = Command::new("git")
            .arg("-C")
            .arg(&source)
            .args([
                "remote",
                "set-url",
                "origin",
                "https://example.org/other.git",
            ])
            .output()?;
        assert!(wrong_origin.status.success());
        assert!(super::validate_managed_checkout(&source, false).is_err());
        Ok(())
    }

    #[test]
    fn prepared_checkout_stays_frozen_after_managed_head_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let managed = temporary.path().join("managed");
        let metadata = temporary.path().join("metadata");
        super::git(None, &["init", &managed.display().to_string()], false)?;
        super::git(
            Some(&managed),
            &["remote", "add", "origin", super::REPOSITORY],
            false,
        )?;
        std::fs::write(managed.join("witness.txt"), "first independent artifact")?;
        let commit = |source: &std::path::Path| -> Result<String, String> {
            super::git(Some(source), &["add", "."], false)?;
            super::git(
                Some(source),
                &[
                    "-c",
                    "user.name=Nick Seal",
                    "-c",
                    "user.email=32712898+blisspixel@users.noreply.github.com",
                    "commit",
                    "-m",
                    "Installer fixture",
                ],
                false,
            )?;
            super::latest_commit(source, false)
        };
        let first = commit(&managed)?;
        let (prepared, _) = super::prepare(&managed, &first, &metadata, false)?;
        std::fs::write(managed.join("witness.txt"), "second independent artifact")?;
        assert_ne!(commit(&managed)?, first);
        assert_eq!(super::latest_commit(&prepared, false)?, first);
        assert_eq!(
            std::fs::read_to_string(prepared.join("witness.txt"))?,
            "first independent artifact"
        );
        super::validate_managed_checkout(&prepared, false)?;
        for index in 0..15 {
            std::fs::create_dir(
                metadata
                    .join("update-work")
                    .join(format!("fixture-{index}")),
            )?;
        }
        assert!(super::prepare(&managed, &first, &metadata, false).is_err());
        Ok(())
    }

    #[test]
    fn github_login_is_a_one_shot_git_credential_helper() {
        assert_eq!(
            super::git_prefix(true),
            [
                "-c",
                "core.abbrev=40",
                "-c",
                "credential.helper=",
                "-c",
                "credential.helper=!gh auth git-credential",
            ]
        );
        assert_eq!(super::git_prefix(false), ["-c", "core.abbrev=40"]);
    }
}
