//! Delegated Linux execution context. Resource containment is not a hostile-code sandbox.

use std::{
    io,
    path::{Component, Path, PathBuf},
};

const LEAF: &str = "sigy-control";

/// Resolve the manager-owned unit containing this control process.
///
/// # Errors
/// Refuses missing delegation, foreign membership, symlinks and incomplete lifecycle bounds.
pub fn delegated_parent() -> io::Result<PathBuf> {
    static CONTEXT: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    CONTEXT
        .get_or_init(inspect)
        .clone()
        .map_err(io::Error::other)
}

fn refuse(reason: &str) -> String {
    format!("Linux native execution needs a delegated systemd user unit: {reason}")
}

fn membership(text: &str) -> Result<PathBuf, String> {
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| refuse("unified cgroup membership missing"))?;
    if path.len() > 4096
        || !path.starts_with('/')
        || path == "/"
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(refuse("invalid cgroup membership"));
    }
    let relative = Path::new(path)
        .strip_prefix("/")
        .map_err(|_| refuse("invalid membership root"))?;
    if relative.file_name().is_none_or(|name| name != LEAF) {
        return Err(refuse("control process is outside sigy-control"));
    }
    let parent = relative.parent().ok_or_else(|| refuse("unit missing"))?;
    let unit = parent
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| refuse("unit missing"))?;
    if !unit.starts_with("sigy-")
        || !unit.ends_with(".service")
        || !unit
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err(refuse("foreign unit"));
    }
    Ok(Path::new("/sys/fs/cgroup").join(parent))
}

fn inspect() -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    let parent = membership(
        &std::fs::read_to_string("/proc/self/cgroup")
            .map_err(|error| refuse(&error.to_string()))?,
    )?;
    let canonical = parent
        .canonicalize()
        .map_err(|error| refuse(&error.to_string()))?;
    if canonical != parent || !parent.starts_with("/sys/fs/cgroup") {
        return Err(refuse("symlinked delegation"));
    }
    let uid = rustix::process::geteuid().as_raw();
    if std::fs::metadata(&parent)
        .map_err(|error| refuse(&error.to_string()))?
        .uid()
        != uid
    {
        return Err(refuse("delegation owner differs"));
    }
    let read = |file: &str| {
        std::fs::read_to_string(parent.join(file)).map_err(|error| refuse(&error.to_string()))
    };
    if !read("cgroup.procs")?.trim().is_empty() || read("cgroup.type")?.trim() != "domain" {
        return Err(refuse("delegated parent is populated or not a domain"));
    }
    let controllers = read("cgroup.controllers")?;
    if ["cpu", "memory", "pids"].iter().any(|required| {
        !controllers
            .split_whitespace()
            .any(|value| value == *required)
    }) {
        return Err(refuse("cpu, memory and pids controllers are required"));
    }
    if !read("sigy-control/cgroup.procs")?
        .lines()
        .any(|pid| pid == std::process::id().to_string())
    {
        return Err(refuse("control membership changed"));
    }
    let unit = parent.file_name().ok_or_else(|| refuse("unit missing"))?;
    let output = std::process::Command::new("/usr/bin/systemctl")
        .args(["--user", "show", "--no-pager", "--property=MainPID,ControlGroup,Delegate,DelegateSubgroup,KillMode,SendSIGKILL,TimeoutStopUSec"])
        .arg(unit).env("SYSTEMD_BUS_TIMEOUT", "2s")
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{uid}/bus"))
        .stdin(std::process::Stdio::null()).output().map_err(|error| refuse(&error.to_string()))?;
    if !output.status.success() || output.stdout.len() > 8192 {
        return Err(refuse("cannot verify user unit properties"));
    }
    let properties =
        String::from_utf8(output.stdout).map_err(|_| refuse("invalid unit properties"))?;
    let relative_group = parent
        .strip_prefix("/sys/fs/cgroup")
        .map_err(|_| refuse("invalid unit path"))?;
    let expected_group = format!("/{}", relative_group.display());
    verify_properties(&properties, &expected_group, std::process::id())?;
    Ok(parent)
}

fn verify_properties(properties: &str, group: &str, pid: u32) -> Result<(), String> {
    let get = |key: &str| {
        properties.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|value| value.strip_prefix('='))
        })
    };
    if get("MainPID") != Some(pid.to_string().as_str())
        || get("ControlGroup") != Some(group)
        || get("Delegate") != Some("yes")
        || get("DelegateSubgroup") != Some(LEAF)
        || get("KillMode") != Some("control-group")
        || get("SendSIGKILL") != Some("yes")
        || get("TimeoutStopUSec") != Some("5s")
    {
        return Err(refuse("unit lifecycle or delegation properties differ"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_and_ambiguous_memberships_cannot_select_a_parent() {
        for text in [
            "0::/",
            "0::/other.service/sigy-control",
            "0::/sigy-a.service",
            "0::/sigy-a.service/../sigy-control",
            "0::relative",
            "1:cpu:/sigy-a.service/sigy-control",
        ] {
            assert!(membership(text).is_err(), "{text}");
        }
        assert_eq!(
            membership("0::/user.slice/sigy-library-a.service/sigy-control").ok(),
            Some(PathBuf::from(
                "/sys/fs/cgroup/user.slice/sigy-library-a.service"
            ))
        );
    }

    #[test]
    fn manager_properties_bind_lifecycle_group_and_main_process() {
        let properties = "MainPID=42\nControlGroup=/user.slice/sigy-a.service\nDelegate=yes\nDelegateSubgroup=sigy-control\nKillMode=control-group\nSendSIGKILL=yes\nTimeoutStopUSec=5s\n";
        assert!(verify_properties(properties, "/user.slice/sigy-a.service", 42).is_ok());
        for replacement in [
            properties.replace("MainPID=42", "MainPID=43"),
            properties.replace("KillMode=control-group", "KillMode=process"),
            properties.replace("SendSIGKILL=yes", "SendSIGKILL=no"),
            properties.replace("TimeoutStopUSec=5s", "TimeoutStopUSec=infinity"),
            properties.replace("Delegate=yes", "Delegate=no"),
        ] {
            assert!(verify_properties(&replacement, "/user.slice/sigy-a.service", 42).is_err());
        }
    }
}
