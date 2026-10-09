use std::{fs, path::Path};

pub(super) fn acquire(path: &Path) -> Result<fs::File, String> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|_| "another Sigy installation is active, or its lock cannot be opened")?;
    #[cfg(not(windows))]
    file.try_lock()
        .map_err(|_| "another Sigy installation is active, or its lock cannot be acquired")?;
    Ok(file)
}

#[cfg(all(test, windows))]
mod tests {
    use super::super::test_child::OwnedChild;
    #[test]
    fn operating_system_lock_releases_after_owner_process_death()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("installation.lock");
        let signal = directory.path().join("acquired");
        let mut child = OwnedChild::from(std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$f=[IO.File]::Open($env:SIGY_LOCK_PATH,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); [IO.File]::WriteAllText($env:SIGY_LOCK_SIGNAL,'acquired'); Start-Sleep -Seconds 30"])
            .env("SIGY_LOCK_PATH", &path)
            .env("SIGY_LOCK_SIGNAL", &signal)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !signal.exists() {
            if std::time::Instant::now() >= deadline || child.try_wait()?.is_some() {
                let _ = child.kill();
                child.wait()?;
                return Err("lock fixture did not acquire its OS handle".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(super::acquire(&path).is_err());
        child.kill()?;
        child.wait()?;
        let recovered = super::acquire(&path)?;
        drop(recovered);
        assert!(path.exists());
        Ok(())
    }
}
