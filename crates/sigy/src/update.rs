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
    if let Some(path) = env::var_os("SIGY_SRC") {
        return Ok(PathBuf::from(path));
    }
    Ok(home_dir()?.join(".sigy").join("src"))
}

fn record_path() -> Result<PathBuf, &'static str> {
    Ok(home_dir()?.join(".sigy").join("installed-commit"))
}

fn installed_commit() -> Option<String> {
    let recorded = record_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok());
    let recorded = recorded
        .as_deref()
        .map(str::trim)
        .filter(|value| is_commit(value));
    recorded.map(str::to_owned).or_else(|| {
        BUILT_FROM
            .filter(|value| is_commit(value))
            .map(str::to_owned)
    })
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
        git(
            Some(directory),
            &["fetch", "--depth", "1", "origin", "main"],
            use_gh,
        )?;
        git(
            Some(directory),
            &["checkout", "--detach", "FETCH_HEAD"],
            use_gh,
        )?;
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

fn cargo_install(directory: &Path, commit: &str) -> Result<(), String> {
    if cfg!(windows) {
        return schedule_windows_install(directory, commit);
    }
    let output = cargo_command(directory, commit).output().map_err(|_| {
        "cargo is not available. Install Rust 1.98.1, then run sigy update.".to_owned()
    })?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    Err(format!("cargo install failed: {}", detail.trim()))
}

fn cargo_command(directory: &Path, commit: &str) -> Command {
    let mut command = Command::new("cargo");
    if let Some(path) = cargo_path() {
        command.env("PATH", path);
    }
    command
        .current_dir(directory)
        .env("SIGY_GIT_COMMIT", commit)
        .args(["install", "--path", "crates/sigy", "--locked", "--force"]);
    command
}

fn schedule_windows_install(directory: &Path, commit: &str) -> Result<(), String> {
    let record = record_path().map_err(str::to_owned)?;
    let script = windows_installer(std::process::id(), directory, &record, commit);
    let path = env::temp_dir().join(format!("sigy-update-{}.ps1", std::process::id()));
    fs::write(&path, script).map_err(|_| "cannot write the update helper")?;
    spawn_windows_helper(&path)
}

fn spawn_windows_helper(path: &Path) -> Result<(), String> {
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Own console so the helper survives after this process exits.
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    command
        .spawn()
        .map_err(|_| "cannot start the update helper".to_owned())?;
    Ok(())
}

fn windows_installer(pid: u32, directory: &Path, record: &Path, commit: &str) -> String {
    format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue\n$env:SIGY_GIT_COMMIT = '{commit}'\n$env:Path = \"$env:USERPROFILE\\.cargo\\bin;$env:Path\"\nSet-Location -LiteralPath '{directory}'\n& cargo install --path crates/sigy --locked --force\nif ($LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}\nSet-Content -LiteralPath '{record}' -Value \"{commit}`n\"\n",
        directory = powershell_quote(directory),
        record = powershell_quote(record),
    )
}

fn powershell_quote(path: &Path) -> String {
    path.display().to_string().replace('\'', "''")
}

fn cargo_path() -> Option<std::ffi::OsString> {
    let home = home_dir().ok()?;
    let cargo_bin = home.join(".cargo").join("bin");
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

pub(crate) fn run(check_only: bool, json: bool) -> Result<(), Box<dyn std::error::Error>> {
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
            cargo_install(&directory, &latest)?;
            if cfg!(windows) {
                let mut stdout = io::stdout().lock();
                writeln!(
                    stdout,
                    "Fetched {latest}. Installation continues after this process exits."
                )?;
            } else {
                remember(&latest)?;
                report(json, installed.as_deref(), &latest, true)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{UpdatePlan, is_commit, plan};
    use std::path::Path;

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
    fn windows_helper_waits_for_the_running_process() {
        let script = super::windows_installer(
            42,
            Path::new("C:\\sigy"),
            Path::new("C:\\Users\\me\\.sigy\\installed-commit"),
            "abcdef1",
        );
        assert!(script.contains("Wait-Process -Id 42"));
        assert!(script.contains("SIGY_GIT_COMMIT = 'abcdef1'"));
        assert!(script.contains("cargo install --path crates/sigy --locked --force"));
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
