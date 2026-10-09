use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Serialize)]
struct HelperSpec<'a> {
    protocol: u32,
    parent_pid: u32,
    operation: &'a str,
    commit: &'a str,
    source: &'a Path,
    stage: PathBuf,
    install_root: &'a Path,
    status: PathBuf,
    marker: PathBuf,
    lock: PathBuf,
    cargo_bin: PathBuf,
    cargo_home: PathBuf,
}

pub(super) fn install(source: &Path, commit: &str, operation: &str) -> Result<(), String> {
    let work = source.parent().ok_or("update workspace missing")?;
    let install_root = install_root()?;
    let spec = HelperSpec {
        protocol: 1,
        parent_pid: std::process::id(),
        operation,
        commit,
        source,
        stage: work.join("stage"),
        install_root: &install_root,
        status: super::receipt::path()?,
        marker: super::record_path().map_err(str::to_owned)?,
        lock: super::home_dir()
            .map_err(str::to_owned)?
            .join(".sigy/install.lock"),
        cargo_bin: cargo_home()?.join("bin"),
        cargo_home: cargo_home()?,
    };
    let bytes = serde_json::to_vec(&spec).map_err(|_| "cannot encode update helper scope")?;
    if bytes.len() > 8192 {
        return Err("update helper scope exceeds byte bound".into());
    }
    let config = work.join("scope.json");
    fs::write(&config, bytes).map_err(|_| "cannot write update helper scope")?;
    let helper = work.join("install.ps1");
    fs::write(&helper, include_str!("windows.ps1")).map_err(|_| "cannot write update helper")?;
    super::receipt::write(&super::receipt::Receipt {
        protocol: 1,
        operation: operation.into(),
        state: "pending".into(),
        commit: commit.into(),
        install_root: install_root.clone(),
        reason: None,
    })?;
    let ps = powershell_path()?;
    let mut command = Command::new(ps);
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&helper)
        .arg("-Config")
        .arg(&config)
        .env("CARGO_HOME", cargo_home()?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    if command.spawn().is_err() {
        super::receipt::write(&super::receipt::Receipt {
            protocol: 1,
            operation: operation.into(),
            state: "failed".into(),
            commit: commit.into(),
            install_root: spec.install_root.to_path_buf(),
            reason: Some("helper-start-failed".into()),
        })?;
        return Err("cannot start the hidden update helper".into());
    }
    Ok(())
}

pub(super) fn cargo_home() -> Result<PathBuf, String> {
    env::var_os("CARGO_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .map_or_else(
            || Ok(super::home_dir().map_err(str::to_owned)?.join(".cargo")),
            absolute,
        )
}

fn install_root() -> Result<PathBuf, String> {
    if let Some(root) = env::var_os("CARGO_INSTALL_ROOT").filter(|path| !path.is_empty()) {
        return absolute(PathBuf::from(root));
    }
    let executable = env::current_exe().map_err(|_| "cannot resolve the running binary")?;
    if let Some(root) = root_from_executable(&executable) {
        return Ok(root);
    }
    absolute(cargo_home()?)
}

fn root_from_executable(executable: &Path) -> Option<PathBuf> {
    let bin = executable.parent()?;
    let name = bin.file_name()?.to_str()?;
    let is_bin = if cfg!(windows) {
        name.eq_ignore_ascii_case("bin")
    } else {
        name == "bin"
    };
    is_bin
        .then(|| bin.parent().map(Path::to_path_buf))
        .flatten()
}

fn powershell_path() -> Result<PathBuf, String> {
    if let Some(root) = env::var_os("SystemRoot") {
        let system_ps = PathBuf::from(root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if super::is_safe_executable(&system_ps) {
            return Ok(system_ps);
        }
    }
    super::find_tool("powershell")
}

fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path)
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|_| "cannot resolve install root".into())
    }
}

#[cfg(all(test, windows))]
mod tests;
