use super::*;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires delegated systemd and both explicit decoder fixtures"]
fn native_clock_conventions_accept_the_pinned_profile_and_refuse_legacy_progress() -> TestResult {
    let selected = std::env::var("SIGY_TEST_FFMPEG")?;
    let legacy = std::env::var("SIGY_TEST_LEGACY_FFMPEG")?;
    if std::env::var_os("SIGY_CLOCK_FIXTURE").is_none() {
        let output = std::process::Command::new("/usr/bin/systemd-run")
            .args([
                "--user",
                "--quiet",
                "--wait",
                "--pipe",
                "--collect",
                "--service-type=exec",
                "--expand-environment=no",
                "--property=Delegate=cpu memory pids",
                "--property=DelegateSubgroup=sigy-control",
                "--property=KillMode=control-group",
                "--property=SendSIGKILL=yes",
                "--property=TimeoutStopSec=5s",
                "--property=RuntimeMaxSec=60s",
                "--setenv=SIGY_CLOCK_FIXTURE=1",
            ])
            .arg(format!(
                "--unit=sigy-clock-fixture-{}.service",
                std::process::id()
            ))
            .arg(format!("--setenv=SIGY_TEST_FFMPEG={selected}"))
            .arg(format!("--setenv=SIGY_TEST_LEGACY_FFMPEG={legacy}"))
            .arg("--")
            .arg(std::env::current_exe()?)
            .args([
                "native_clock_conventions",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .stdin(Stdio::null())
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return Ok(());
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let accepted = NativeAudioGroup::for_decoder()?;
            check(&selected, &accepted).await?;
            accepted.finish().await?;
            let refused = NativeAudioGroup::for_decoder()?;
            let error = check(&legacy, &refused)
                .await
                .err()
                .ok_or("legacy progress was accepted")?;
            assert!(
                error.to_string().contains("incompatible decoder progress"),
                "{error}"
            );
            refused.finish().await?;
            println!(
                "pinned clock accepted, legacy clock refused, both native groups joined empty"
            );
            Ok::<_, Box<dyn std::error::Error>>(())
        })
}
