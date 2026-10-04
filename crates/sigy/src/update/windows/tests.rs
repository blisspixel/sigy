use super::super::test_child::OwnedChild;
use super::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    directory: tempfile::TempDir,
    source: PathBuf,
    target: PathBuf,
    commit: String,
    config: PathBuf,
    status: PathBuf,
    marker: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Failed helper fixture: {}", self.directory.path().display());
            let diagnostic = self.config.with_file_name("validation-error.txt");
            for path in [&self.status, &self.config, &diagnostic] {
                if let Ok(bytes) = fs::read(path) {
                    eprintln!("{}: {}", path.display(), String::from_utf8_lossy(&bytes));
                }
            }
            for arguments in [
                vec!["rev-parse", "HEAD"],
                vec!["config", "--local", "--get", "remote.origin.url"],
                vec!["status", "--porcelain=v1", "--untracked-files=all"],
                vec!["diff", "--", "."],
            ] {
                if let Ok(output) = Command::new("git")
                    .arg("-C")
                    .arg(&self.source)
                    .args(arguments)
                    .output()
                {
                    eprintln!(
                        "Source audit exit: {} stdout: {}\nstderr: {}",
                        output.status,
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
    }
}

impl Fixture {
    fn new(stub: &str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let base = directory.path().join("space and 'quote [literal]");
        let source = base.join("source");
        fs::create_dir_all(&source)?;
        local_git(&source, &["init"])?;
        fs::write(source.join("input.txt"), "independent source")?;
        local_git(&source, &["add", "."])?;
        local_git(
            &source,
            &[
                "-c",
                "user.name=Nick Seal",
                "-c",
                "user.email=32712898+blisspixel@users.noreply.github.com",
                "commit",
                "-m",
                "Update fixture",
            ],
        )?;
        local_git(
            &source,
            &["remote", "add", "origin", super::super::REPOSITORY],
        )?;
        let commit = local_git(&source, &["rev-parse", "HEAD"])?;
        let target = base.join("custom install/bin/sigy.exe");
        fs::create_dir_all(target.parent().ok_or("fixture target")?)?;
        fs::write(&target, b"previous executable")?;
        let cargo_bin = base.join("custom cargo/bin");
        fs::create_dir_all(&cargo_bin)?;
        fs::write(cargo_bin.join("cargo.cmd"), stub.replace('\n', "\r\n"))?;
        fs::write(
            cargo_bin.join("gh.cmd"),
            "@echo off\r\necho %*>\"%SIGY_FIXTURE_GH_CALLED%\"\r\nexit /b 1\r\n",
        )?;
        fs::write(base.join("empty-gitconfig"), "")?;
        let status = base.join("status.json");
        let marker = base.join("installed-commit");
        fs::write(&marker, "previous commit")?;
        fs::write(
            &status,
            serde_json::to_vec(&serde_json::json!({
                "protocol":1,"operation":"1-2","state":"pending","commit":commit,
                "install_root":base.join("custom install"),"reason":null
            }))?,
        )?;
        let scope = HelperSpec {
            protocol: 1,
            parent_pid: 999_999,
            operation: "1-2",
            commit: &commit,
            source: &source,
            stage: base.join("stage"),
            install_root: target
                .parent()
                .and_then(Path::parent)
                .ok_or("fixture install root")?,
            status: status.clone(),
            marker: marker.clone(),
            lock: base.join("install.lock"),
            cargo_bin,
            cargo_home: base.join("custom cargo"),
        };
        let config = base.join("scope.json");
        fs::write(&config, serde_json::to_vec(&scope)?)?;
        fs::write(base.join("helper.ps1"), include_str!("../windows.ps1"))?;
        Ok(Self {
            directory,
            source,
            target,
            commit,
            config,
            status,
            marker,
        })
    }

    fn command(&self) -> Result<Command> {
        let base = self.config.parent().ok_or("fixture base")?;
        let mut command = Command::new("powershell");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(base.join("helper.ps1"))
            .arg("-Config")
            .arg(&self.config)
            .env(
                "PATH",
                format!(
                    "{};{}",
                    base.join("custom cargo/bin").display(),
                    env::var("PATH")?
                ),
            )
            .env("SIGY_FIXTURE_STAGE", base.join("stage"))
            .env("SIGY_FIXTURE_SOURCE", &self.source)
            .env("SIGY_FIXTURE_CALLED", base.join("called"))
            .env("SIGY_FIXTURE_GH_CALLED", base.join("gh-called"))
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("GIT_CONFIG_GLOBAL", base.join("empty-gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .current_dir(self.directory.path());
        Ok(command)
    }

    fn run(&self) -> Result<std::process::Output> {
        let mut child = OwnedChild::from(
            self.command()?
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
        );
        self.wait(&mut child, "update helper")?;
        let output = child.output()?;
        if !output.status.success() {
            eprintln!(
                "Helper exit {}: stdout={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(output)
    }

    fn wait(
        &self,
        child: &mut std::process::Child,
        phase: &str,
    ) -> Result<std::process::ExitStatus> {
        use std::io::Read as _;
        match wait_bounded(child) {
            Ok(status) => Ok(status),
            Err(error) => {
                let base = self.config.parent().ok_or("fixture base")?;
                for name in [
                    "installer-stdout.txt",
                    "installer-stderr.txt",
                    "validation-error.txt",
                    "gh-called",
                    "direct-called",
                    "scope.json",
                    "home/.sigy/update-status.json",
                ] {
                    if let Ok(file) = fs::File::open(base.join(name)) {
                        let mut bytes = Vec::new();
                        if file.take(8192).read_to_end(&mut bytes).is_ok() {
                            eprintln!("{phase} {name}: {}", String::from_utf8_lossy(&bytes));
                        }
                    }
                }
                Err(format!("{phase}: {error}").into())
            }
        }
    }
    fn outcome(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_slice(&fs::read(&self.status)?)?)
    }
    fn called(&self) -> bool {
        self.config
            .parent()
            .is_some_and(|base| base.join("called").exists())
    }
}

fn wait_bounded(child: &mut std::process::Child) -> Result<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if std::time::Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err("offline update fixture exceeded 15 seconds".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn local_git(source: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(source)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err("fixture Git failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

const SUCCESS: &str = "@echo off\necho called>\"%SIGY_FIXTURE_CALLED%\"\nmkdir \"%SIGY_FIXTURE_STAGE%\\bin\"\necho new executable>\"%SIGY_FIXTURE_STAGE%\\bin\\sigy.exe\"\nexit /b 0\n";

#[test]
fn windows_install_root_preserves_case_insensitive_bin_identity() {
    let root = PathBuf::from("C:/custom root");
    for bin in ["bin", "Bin", "BIN"] {
        assert_eq!(
            root_from_executable(&root.join(bin).join("sigy.exe")),
            Some(root.clone())
        );
    }
    assert_eq!(
        root_from_executable(&root.join("other").join("sigy.exe")),
        None
    );
}

#[test]
fn helper_freezes_absolute_cargo_home_before_changing_source_directory() -> Result<()> {
    let fixture = Fixture::new(&SUCCESS.replace(
        "exit /b 0",
        "echo %CARGO_HOME%>\"%SIGY_FIXTURE_HOME_OBSERVED%\"\nexit /b 0",
    ))?;
    let base = fixture.config.parent().ok_or("fixture base")?;
    let observed = base.join("cargo-home-observed");
    let mut child = OwnedChild::from(
        fixture
            .command()?
            .env("CARGO_HOME", "relative cache")
            .env("SIGY_FIXTURE_HOME_OBSERVED", &observed)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    assert!(wait_bounded(&mut child)?.success());
    assert_eq!(
        fs::read_to_string(observed)?.trim(),
        base.join("custom cargo").to_string_lossy()
    );
    assert!(!fixture.source.join("relative cache").exists());
    Ok(())
}

fn prepare_supersession(fixture: &Fixture) -> Result<(OwnedChild, String, PathBuf)> {
    let base = fixture.config.parent().ok_or("fixture base")?;
    let home = base.join("home");
    let metadata = home.join(".sigy");
    fs::create_dir_all(&metadata)?;
    let status = metadata.join("update-status.json");
    fs::copy(&fixture.status, &status)?;
    let frozen = base.join("frozen");
    local_git(base, &["clone", "--no-local", "source", "frozen"])?;
    local_git(
        &frozen,
        &["remote", "set-url", "origin", super::super::REPOSITORY],
    )?;
    local_git(&fixture.source, &["branch", "-M", "main"])?;
    fs::write(
        fixture.source.join("input.txt"),
        "newer direct installer source",
    )?;
    local_git(&fixture.source, &["add", "."])?;
    local_git(
        &fixture.source,
        &[
            "-c",
            "user.name=Nick Seal",
            "-c",
            "user.email=32712898+blisspixel@users.noreply.github.com",
            "commit",
            "-m",
            "Newer installer fixture",
        ],
    )?;
    let newer = local_git(&fixture.source, &["rev-parse", "HEAD"])?;
    assert_ne!(newer, fixture.commit);
    let parent = OwnedChild::from(
        Command::new("powershell")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut scope: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.config)?)?;
    scope["parent_pid"] = serde_json::json!(parent.id());
    scope["source"] = serde_json::json!(frozen);
    scope["status"] = serde_json::json!(status);
    scope["lock"] = serde_json::json!(metadata.join("install.lock"));
    fs::write(&fixture.config, serde_json::to_vec(&scope)?)?;
    Ok((parent, newer, metadata))
}

fn direct_installer_command(fixture: &Fixture, metadata: &Path) -> Result<Command> {
    let base = fixture.config.parent().ok_or("fixture base")?;
    let home = metadata.parent().ok_or("fixture home")?;
    let gitconfig = base.join("gitconfig");
    fs::write(
        &gitconfig,
        format!(
            "[url \"{}\"]\n insteadOf = {}\n",
            fixture.source.to_string_lossy().replace('\\', "/"),
            super::super::REPOSITORY
        ),
    )?;
    let installer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.ps1");
    let mut direct = Command::new("powershell");
    direct
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(installer)
        .env(
            "PATH",
            format!(
                "{};{}",
                base.join("custom cargo/bin").display(),
                env::var("PATH")?
            ),
        )
        .env("USERPROFILE", home)
        .env("SIGY_SRC", &fixture.source)
        .env("GIT_CONFIG_GLOBAL", gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env("GIT_ALLOW_PROTOCOL", "file")
        .env("CARGO_HOME", base.join("custom cargo"))
        .env(
            "CARGO_INSTALL_ROOT",
            fixture
                .target
                .parent()
                .and_then(Path::parent)
                .ok_or("target root")?,
        )
        .env("SIGY_DIRECT_BINARY", &fixture.target)
        .env("SIGY_FIXTURE_CALLED", base.join("direct-called"))
        .env("SIGY_FIXTURE_GH_CALLED", base.join("gh-called"))
        .env("SIGY_FIXTURE_STAGE", base.join("direct-stage"))
        // Regular fixture files cannot fill a pipe while waiting for child exit.
        .stdout(fs::File::create(base.join("installer-stdout.txt"))?)
        .stderr(fs::File::create(base.join("installer-stderr.txt"))?);
    Ok(direct)
}

#[test]
fn direct_installer_fences_a_real_helper_before_build() -> Result<()> {
    let fixture = Fixture::new(&SUCCESS.replace("exit /b 0", "if defined SIGY_DIRECT_BINARY echo direct newer executable>\"%SIGY_DIRECT_BINARY%\"\nexit /b 0"))?;
    let (mut parent, newer, metadata) = prepare_supersession(&fixture)?;
    let mut helper = OwnedChild::from(
        fixture
            .command()?
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut direct = direct_installer_command(&fixture, &metadata)?;
    let mut installer_child = OwnedChild::from(direct.spawn()?);
    let publication_status = fixture.wait(&mut installer_child, "direct installer")?;
    parent.kill()?;
    parent.wait()?;
    let helper_status = fixture.wait(&mut helper, "superseded helper")?;
    assert!(publication_status.success());
    assert!(!helper_status.success());
    assert!(!fixture.called());
    assert_eq!(
        fs::read_to_string(
            fixture
                .config
                .parent()
                .ok_or("fixture base")?
                .join("gh-called")
        )?
        .trim(),
        "auth status"
    );
    assert_eq!(
        fs::read_to_string(&fixture.target)?.trim(),
        "direct newer executable"
    );
    assert_eq!(
        fs::read_to_string(metadata.join("installed-commit"))?.trim(),
        newer
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(metadata.join("update-status.json"))?)?;
    assert_eq!(receipt["reason"], "installer-superseded");
    assert_eq!(receipt["commit"], fixture.commit);
    Ok(())
}

#[test]
fn prepared_source_change_before_handoff_never_builds_or_publishes() -> Result<()> {
    let fixture = Fixture::new(SUCCESS)?;
    fs::write(fixture.source.join("input.txt"), "changed")?;
    assert!(!fixture.run()?.status.success());
    assert!(!fixture.called());
    assert_eq!(fs::read(&fixture.target)?, b"previous executable");
    assert_eq!(fs::read_to_string(&fixture.marker)?, "previous commit");
    assert_eq!(fixture.outcome()?["reason"], "source-not-clean");
    Ok(())
}

#[test]
fn source_change_during_build_refuses_staged_binary_and_preserves_previous() -> Result<()> {
    let fixture = Fixture::new(&SUCCESS.replace(
        "exit /b 0",
        "echo changed>\"%SIGY_FIXTURE_SOURCE%\\input.txt\"\nexit /b 0",
    ))?;
    assert!(!fixture.run()?.status.success());
    assert!(fixture.called());
    assert_eq!(fs::read(&fixture.target)?, b"previous executable");
    assert_eq!(fs::read_to_string(&fixture.marker)?, "previous commit");
    assert_eq!(fixture.outcome()?["reason"], "source-changed-during-build");
    Ok(())
}

#[test]
fn failed_build_keeps_binary_and_marker_and_records_fixed_failure() -> Result<()> {
    let fixture = Fixture::new("@echo off\necho called>\"%SIGY_FIXTURE_CALLED%\"\nexit /b 37\n")?;
    assert!(!fixture.run()?.status.success());
    assert!(fixture.called());
    assert_eq!(fs::read(&fixture.target)?, b"previous executable");
    assert_eq!(fs::read_to_string(&fixture.marker)?, "previous commit");
    assert_eq!(fixture.outcome()?["reason"], "cargo-build-failed");
    Ok(())
}

#[test]
fn exact_staged_publication_handles_literal_paths_and_updates_marker_only_on_success() -> Result<()>
{
    let fixture = Fixture::new(SUCCESS)?;
    assert!(fixture.run()?.status.success());
    assert!(fixture.called());
    assert_eq!(
        fs::read_to_string(&fixture.target)?.trim(),
        "new executable"
    );
    assert_eq!(fs::read_to_string(&fixture.marker)?.trim(), fixture.commit);
    assert_eq!(fixture.outcome()?["state"], "succeeded");
    assert_exact_hash(&fixture)?;
    Ok(())
}

fn assert_exact_hash(fixture: &Fixture) -> Result<()> {
    use sha2::{Digest, Sha256};
    let expected = Sha256::digest(b"new executable\r\n");
    let published = Sha256::digest(fs::read(&fixture.target)?);
    let stage = fixture
        .config
        .parent()
        .ok_or("fixture base")?
        .join("stage/bin/sigy.exe");
    let staged = Sha256::digest(fs::read(stage)?);
    assert_eq!(published, expected);
    assert_eq!(staged, expected);
    Ok(())
}

#[test]
fn redirected_windows_powershell_publication_handles_incompatible_module_search_path() -> Result<()>
{
    let fixture = Fixture::new(SUCCESS)?;
    let mut child = OwnedChild::from(
        fixture
            .command()?
            .env("PSModulePath", "C:/Program Files/PowerShell/7/Modules")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let status = wait_bounded(&mut child)?;
    let output = child.output()?;
    assert!(
        status.success(),
        "helper stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.outcome()?["state"], "succeeded");
    assert_exact_hash(&fixture)?;
    Ok(())
}

#[test]
fn locked_previous_executable_survives_failed_publication() -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(SUCCESS)?;
    let protection = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&fixture.target)?;
    assert!(!fixture.run()?.status.success());
    drop(protection);
    assert_eq!(fs::read(&fixture.target)?, b"previous executable");
    assert_eq!(fs::read_to_string(&fixture.marker)?, "previous commit");
    assert_eq!(fixture.outcome()?["reason"], "binary-publication-failed");
    Ok(())
}

#[test]
fn waiting_superseded_helper_cannot_overwrite_newer_receipt() -> Result<()> {
    let fixture = Fixture::new(SUCCESS)?;
    let lock_path = fixture
        .config
        .parent()
        .ok_or("fixture base")?
        .join("install.lock");
    let guard = super::super::lock::acquire(&lock_path)?;
    let mut child = OwnedChild::from(
        fixture
            .command()?
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    std::thread::sleep(std::time::Duration::from_millis(250));
    let newer = b"{\"operation\":\"3-4\",\"state\":\"pending\"}";
    fs::write(&fixture.status, newer)?;
    drop(guard);
    assert!(!wait_bounded(&mut child)?.success());
    assert!(!fixture.called());
    assert_eq!(fs::read(&fixture.status)?, newer);
    Ok(())
}
