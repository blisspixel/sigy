//! Real delegated-host checks. These do not qualify a speaker or hostile-code sandbox.
#![cfg(target_os = "linux")]

use processkit::{ProcessGroup, ProcessGroupOptions};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn manager(unit: &str) -> Command {
    let mut command = Command::new("/usr/bin/systemd-run");
    command
        .args([
            "--user",
            "--quiet",
            "--wait",
            "--pipe",
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
            "--property=RuntimeMaxSec=60s",
        ])
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!(
                "unix:path=/run/user/{}/bus",
                rustix::process::geteuid().as_raw()
            ),
        );
    command
}

#[test]
#[ignore = "requires an actual systemd user manager with cgroup v2 delegation"]
fn delegated_native_contracts() -> TestResult {
    if std::env::var_os("SIGY_TEST_INSIDE_UNIT").is_none() {
        let output = manager(&format!("sigy-fixture-{}.service", std::process::id()))
            .arg("--setenv=SIGY_TEST_INSIDE_UNIT=1")
            .arg("--")
            .arg(std::env::current_exe()?)
            .args([
                "--exact",
                "delegated_native_contracts",
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
        return owner_death();
    }
    let parent = sigy_service::native::delegated_parent()?;
    assert_eq!(
        std::fs::read_to_string(parent.join("cgroup.procs"))?.trim(),
        ""
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        for role in ["threads", "task-limit", "cpu", "memory"] {
            let group = bounded(&parent)?;
            let mut worker = group.spawn(payload(role, None)?)?;
            let status = tokio::time::timeout(Duration::from_secs(12), worker.wait()).await??;
            assert_eq!(status.success(), role != "memory", "{role}: {status}");
            drained(&group).await?;
            let evidence = group.limit_evidence();
            let verdict = match role {
                "cpu" => evidence.cpu(),
                "memory" => evidence.memory(),
                "task-limit" => evidence.processes(),
                _ => processkit::LimitVerdict::NotTripped,
            };
            assert_eq!(
                verdict,
                if role == "threads" {
                    processkit::LimitVerdict::NotTripped
                } else {
                    processkit::LimitVerdict::Tripped
                }
            );
            println!("{role}: native completion and empty group observed");
        }
        let scratch = tempfile::tempdir()?;
        let receipt = scratch.path().join("ready");
        let group = bounded(&parent)?;
        let mut worker = group.spawn(payload("hang", Some(&receipt))?)?;
        wait_file(&receipt)?;
        let mut second = tokio::process::Command::new("/bin/true");
        second
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        assert!(
            group.spawn(second).is_err(),
            "second process exceeded the explicit cap"
        );
        group.kill_all()?;
        tokio::time::timeout(Duration::from_secs(5), worker.wait()).await??;
        drained(&group).await?;
        println!("process admission and cancellation: refused second process, joined empty group");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

fn bounded(parent: &Path) -> Result<ProcessGroup, Box<dyn std::error::Error>> {
    Ok(ProcessGroup::with_delegated_options(
        ProcessGroupOptions::default()
            .max_processes(1)
            .max_memory(128 * 1024 * 1024)
            .cpu_quota(0.1),
        parent,
        16,
    )?)
}

fn payload(role: &str, receipt: Option<&Path>) -> Result<tokio::process::Command, std::io::Error> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            "linux_payload",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("SIGY_TEST_NATIVE_ROLE", role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if let Some(path) = receipt {
        command.env("SIGY_TEST_NATIVE_RECEIPT", path);
    }
    Ok(command)
}

async fn drained(group: &ProcessGroup) -> TestResult {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if group.stats()?.active_process_count == 0 {
                break Ok::<_, processkit::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

#[test]
#[ignore = "private child fixture for delegated_native_contracts"]
fn linux_payload() -> TestResult {
    let Ok(role) = std::env::var("SIGY_TEST_NATIVE_ROLE") else {
        return Ok(());
    };
    let membership = std::fs::read_to_string("/proc/self/cgroup")?;
    let path = membership
        .trim()
        .strip_prefix("0::")
        .ok_or("missing unified membership")?;
    let leaf = Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/'));
    let group = leaf.parent().ok_or("missing native parent")?;
    assert!(
        group
            .file_name()
            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"processkit-"))
    );
    assert_eq!(
        std::fs::read_to_string(group.join("memory.max"))?.trim(),
        "134217728"
    );
    assert_eq!(
        std::fs::read_to_string(group.join("memory.swap.max"))?.trim(),
        "0"
    );
    assert_eq!(
        std::fs::read_to_string(group.join("pids.max"))?.trim(),
        "16"
    );
    assert_eq!(
        std::fs::read_to_string(group.join("cpu.max"))?.trim(),
        "10000 100000"
    );
    match role.as_str() {
        "threads" => {
            let threads: Vec<_> = (0..4)
                .map(|_| std::thread::Builder::new().spawn(|| 42))
                .collect::<Result<_, _>>()?;
            for thread in threads {
                assert_eq!(thread.join().map_err(|_| "thread failed")?, 42);
            }
            let error = Command::new("/bin/true")
                .spawn()
                .err()
                .ok_or("native process creation was allowed")?;
            assert_eq!(error.raw_os_error(), Some(1));
        }
        "task-limit" => thread_limit()?,
        "cpu" => {
            let until = Instant::now() + Duration::from_secs(2);
            while Instant::now() < until {
                std::hint::black_box(42_u64.wrapping_mul(765_431));
            }
            let counters = std::fs::read_to_string(group.join("cpu.stat"))?;
            let throttled = counters
                .lines()
                .find_map(|line| line.strip_prefix("nr_throttled "))
                .ok_or("CPU counter missing")?
                .parse::<u64>()?;
            assert!(throttled > 0, "CPU quota never throttled");
        }
        "memory" => {
            let mut blocks = Vec::new();
            for _ in 0..256 {
                let mut block = vec![0_u8; 1024 * 1024];
                for page in block.chunks_mut(4096) {
                    page[0] = 1;
                }
                blocks.push(block);
                std::hint::black_box(&blocks);
            }
            return Err("memory cap did not stop the worker".into());
        }
        "hang" => {
            let receipt = std::env::var_os("SIGY_TEST_NATIVE_RECEIPT").ok_or("receipt missing")?;
            let path = Path::new(&receipt);
            let partial = path.with_extension("part");
            let mut file = std::fs::File::create(&partial)?;
            writeln!(file, "{}\n{}", std::process::id(), group.display())?;
            file.sync_all()?;
            std::fs::rename(partial, path)?;
            std::thread::sleep(Duration::from_secs(45));
            return Err("waiting worker was not cancelled".into());
        }
        _ => return Err("unknown private fixture role".into()),
    }
    Ok(())
}

fn thread_limit() -> TestResult {
    let release = Arc::new(AtomicBool::new(false));
    let mut threads = Vec::new();
    let mut refused = false;
    for _ in 0..32 {
        let release = Arc::clone(&release);
        match std::thread::Builder::new().spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !release.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }) {
            Ok(thread) => threads.push(thread),
            Err(error) => {
                assert_eq!(error.raw_os_error(), Some(11));
                refused = true;
                break;
            }
        }
    }
    release.store(true, Ordering::Release);
    assert!(refused && !threads.is_empty() && threads.len() < 16);
    for thread in threads {
        thread.join().map_err(|_| "thread join failed")?;
    }
    Ok(())
}

fn wait_file(path: &Path) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        if Instant::now() >= deadline {
            return Err("native readiness deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[test]
#[ignore = "private owner-death fixture for delegated_native_contracts"]
fn owner_fixture() -> TestResult {
    let Some(receipt) = std::env::var_os("SIGY_TEST_OWNER_RECEIPT") else {
        return Ok(());
    };
    let parent = sigy_service::native::delegated_parent()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let group = bounded(&parent)?;
        let worker = group.spawn(payload("hang", Some(Path::new(&receipt)))?)?;
        // SIGKILL must exercise manager cleanup, with no Rust destructor assistance.
        std::mem::forget(worker);
        std::mem::forget(group);
        std::future::pending::<TestResult>().await
    })
}

fn owner_death() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let receipt = scratch.path().join("ready");
    let unit = format!("sigy-owner-fixture-{}.service", std::process::id());
    let mut launch = manager(&unit)
        .arg(format!(
            "--setenv=SIGY_TEST_OWNER_RECEIPT={}",
            receipt.display()
        ))
        .arg("--")
        .arg(std::env::current_exe()?)
        .args([
            "--exact",
            "owner_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    wait_file(&receipt)?;
    let receipt = std::fs::read_to_string(&receipt)?;
    let group = Path::new(receipt.lines().nth(1).ok_or("group missing")?);
    let parent = group.parent().ok_or("unit missing")?;
    let owner = std::fs::read_to_string(parent.join("sigy-control/cgroup.procs"))?
        .trim()
        .parse::<i32>()?;
    let pid = rustix::process::Pid::from_raw(owner).ok_or("invalid owner PID")?;
    rustix::process::kill_process(pid, rustix::process::Signal::KILL)?;
    let deadline = Instant::now() + Duration::from_secs(9);
    while parent.exists() || launch.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            return Err("manager did not remove killed owner's native subtree".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("owner SIGKILL: manager removed the complete native subtree");
    Ok(())
}
