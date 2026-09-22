use processkit::{ProcessGroup, ProcessGroupOptions};
use std::{error::Error, io::Write, process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    time::{Instant, sleep, timeout},
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn child_mode(mode: &str) -> Result<()> {
    match mode {
        "hold" => std::thread::sleep(Duration::from_secs(20)),
        "spawn-grandchild" => {
            let exe = std::env::current_exe()?;
            let _child = std::process::Command::new(exe)
                .arg("hold")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            std::thread::sleep(Duration::from_millis(500));
        }
        "allocate" => {
            let mut blocks: Vec<Vec<u8>> = Vec::new();
            for _ in 0..128 {
                let mut block = Vec::new();
                if block.try_reserve_exact(1024 * 1024).is_err() {
                    std::process::exit(42);
                }
                block.resize(1024 * 1024, 0);
                block.fill(1);
                blocks.push(block);
            }
            std::hint::black_box(&blocks);
        }
        "spin" => {
            let end = std::time::Instant::now() + Duration::from_secs(2);
            let mut n = 0_u64;
            while std::time::Instant::now() < end {
                n = n.wrapping_add(1);
                std::hint::black_box(n);
            }
        }
        "flood" => {
            std::io::stdout().write_all(&vec![b'x'; 1024 * 1024])?;
            std::thread::sleep(Duration::from_secs(20));
        }
        _ => return Err("unknown child mode".into()),
    }
    Ok(())
}

fn command(mode: &str) -> Result<Command> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg(mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(cmd)
}

async fn confirm_empty(group: &ProcessGroup, duration: Duration) -> Result<()> {
    let end = Instant::now() + duration;
    loop {
        let stats = group.stats()?;
        if stats.active_process_count == 0 {
            println!(
                "empty=true peak_commit={:?} total_cpu={:?}",
                stats.peak_memory_bytes, stats.total_cpu_time
            );
            return Ok(());
        }
        if Instant::now() >= end {
            return Err(format!(
                "group still has {} active members",
                stats.active_process_count
            )
            .into());
        }
        sleep(Duration::from_millis(20)).await;
    }
}

async fn run() -> Result<()> {
    let invalid = ProcessGroup::with_options(ProcessGroupOptions::default().max_processes(0));
    println!("invalid_limit_refused={}", invalid.is_err());
    if invalid.is_ok() {
        return Err("invalid process limit accepted".into());
    }

    let group = ProcessGroup::with_options(
        ProcessGroupOptions::default()
            .max_memory(64 * 1024 * 1024)
            .max_processes(1)
            .cpu_quota(0.5),
    )?;
    println!("mechanism={:?}", group.mechanism());
    let mut first = group.spawn(command("hold")?)?;
    let second = group.spawn(command("hold")?);
    println!("second_process_refused={}", second.is_err());
    if second.is_ok() {
        return Err("process-count cap did not refuse second child".into());
    }
    group.kill_all()?;
    timeout(Duration::from_secs(2), first.wait()).await??;
    confirm_empty(&group, Duration::from_secs(2)).await?;

    let descendants = ProcessGroup::with_options(
        ProcessGroupOptions::default()
            .max_memory(64 * 1024 * 1024)
            .max_processes(2),
    )?;
    let mut root = descendants.spawn(command("spawn-grandchild")?)?;
    let root_status = timeout(Duration::from_secs(2), root.wait()).await??;
    if !root_status.success() {
        return Err("grandchild launcher failed".into());
    }
    let before = descendants.stats()?;
    println!(
        "descendants_after_root_exit={}",
        before.active_process_count
    );
    if before.active_process_count != 1 {
        return Err("grandchild was not retained in the job".into());
    }
    descendants.kill_all()?;
    confirm_empty(&descendants, Duration::from_secs(2)).await?;

    let memory = ProcessGroup::with_options(
        ProcessGroupOptions::default()
            .max_memory(32 * 1024 * 1024)
            .max_processes(1),
    )?;
    let mut allocation = memory.spawn(command("allocate")?)?;
    let allocation_status = timeout(Duration::from_secs(3), allocation.wait()).await??;
    let memory_stats = memory.stats()?;
    println!(
        "allocation_exit={:?} peak_commit={:?}",
        allocation_status.code(),
        memory_stats.peak_memory_bytes
    );
    if allocation_status.code() != Some(42) {
        return Err("expected an explicit allocation refusal".into());
    }
    confirm_empty(&memory, Duration::from_secs(2)).await?;

    let cpu = ProcessGroup::with_options(
        ProcessGroupOptions::default()
            .max_processes(1)
            .cpu_quota(0.25),
    )?;
    let mut spinner = cpu.spawn(command("spin")?)?;
    let spinner_status = timeout(Duration::from_secs(4), spinner.wait()).await??;
    if !spinner_status.success() {
        return Err("CPU quota child failed".into());
    }
    let cpu_stats = cpu.stats()?;
    let capped_cpu = cpu_stats
        .total_cpu_time
        .ok_or("capped job has no CPU counter")?;
    println!("two_second_spin_cpu={:?}", cpu_stats.total_cpu_time);
    confirm_empty(&cpu, Duration::from_secs(2)).await?;

    let baseline = ProcessGroup::with_options(ProcessGroupOptions::default().max_processes(1))?;
    let mut baseline_spinner = baseline.spawn(command("spin")?)?;
    let baseline_status = timeout(Duration::from_secs(4), baseline_spinner.wait()).await??;
    if !baseline_status.success() {
        return Err("uncapped CPU child failed".into());
    }
    let baseline_stats = baseline.stats()?;
    let baseline_cpu = baseline_stats
        .total_cpu_time
        .ok_or("uncapped job has no CPU counter")?;
    println!(
        "two_second_spin_baseline_cpu={:?}",
        baseline_stats.total_cpu_time
    );
    if capped_cpu >= baseline_cpu {
        return Err("quota did not reduce observed CPU use".into());
    }
    confirm_empty(&baseline, Duration::from_secs(2)).await?;

    let flood = ProcessGroup::with_options(
        ProcessGroupOptions::default()
            .max_memory(32 * 1024 * 1024)
            .max_processes(1),
    )?;
    let mut flood_command = command("flood")?;
    flood_command.stdout(Stdio::piped());
    let mut flood_child = flood.spawn(flood_command)?;
    let mut stdout = flood_child.stdout.take().ok_or("stdout pipe unavailable")?;
    let mut buffer = [0_u8; 4096];
    let mut received = 0_usize;
    let output_limit = 64 * 1024;
    while received <= output_limit {
        let remaining = output_limit + 1 - received;
        let chunk_len = remaining.min(buffer.len());
        let n = timeout(
            Duration::from_secs(2),
            stdout.read(&mut buffer[..chunk_len]),
        )
        .await??;
        if n == 0 {
            return Err("flood child ended before bound".into());
        }
        received += n;
    }
    println!("no_newline_flood_stopped_at={received}");
    flood.kill_all()?;
    timeout(Duration::from_secs(2), flood_child.wait()).await??;
    confirm_empty(&flood, Duration::from_secs(2)).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(mode) = std::env::args().nth(1) {
        return child_mode(&mode);
    }
    run().await
}
