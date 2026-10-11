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
    use std::io::Read;

    #[test]
    #[ignore = "child-only fixture launched by the owner-death test"]
    fn lock_owner_fixture() -> Result<(), Box<dyn std::error::Error>> {
        let Some(path) = std::env::var_os("SIGY_LOCK_PATH") else {
            return Ok(());
        };
        let signal = std::env::var_os("SIGY_LOCK_SIGNAL").ok_or("missing readiness path")?;
        let _owner = super::acquire(std::path::Path::new(&path))?;
        std::fs::write(signal, b"acquired")?;
        // The parent owns stdin and kills this process while the OS handle is live.
        let _ = std::io::stdin().read(&mut [0_u8; 1])?;
        Ok(())
    }

    #[test]
    fn operating_system_lock_releases_after_owner_process_death()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("installation.lock");
        let signal = directory.path().join("acquired");
        let mut child = OwnedChild::from(
            std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "update::lock::tests::lock_owner_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SIGY_LOCK_PATH", &path)
                .env("SIGY_LOCK_SIGNAL", &signal)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()?,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !signal.exists() {
            if std::time::Instant::now() >= deadline || child.try_wait()?.is_some() {
                let _ = child.kill();
                let status = child.wait()?;
                let mut diagnostic = Vec::new();
                if let Some(stderr) = child.stderr.take() {
                    stderr.take(4096).read_to_end(&mut diagnostic)?;
                }
                return Err(format!(
                    "lock fixture did not acquire its OS handle ({status}): {}",
                    String::from_utf8_lossy(&diagnostic)
                )
                .into());
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
