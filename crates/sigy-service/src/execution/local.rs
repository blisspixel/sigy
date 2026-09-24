//! The local process executor. Each native process runs in its own contained group
//! with a process-count limit, a committed-memory limit, a CPU rate limit, kill-on-close
//! and a wall deadline. An envelope is constructed only after every started group
//! reports no active member. This is not an operating-system network sandbox.

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use processkit::{ProcessGroup, ProcessGroupOptions};
use tokio::sync::watch;

use super::{Executor, ResultEnvelope, TaskKind, TaskSpec, stage::LocalStage, stage::SCRATCH};
use crate::{Error, Result};

const DRAIN_DEADLINE: Duration = Duration::from_secs(5);

/// Runs recognition and translation specs as contained local processes.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LocalProcessExecutor;

impl Executor for LocalProcessExecutor {
    type Stage = LocalStage;

    async fn execute(
        &self,
        spec: TaskSpec,
        stage: LocalStage,
        signal: watch::Receiver<bool>,
    ) -> Result<ResultEnvelope> {
        match spec.kind {
            TaskKind::Recognition => super::asr::run(spec, stage, signal).await,
            TaskKind::Translation => super::translate::run(spec, stage, signal).await,
        }
    }
}

/// Remove scratch directories left by an earlier service process.
pub(crate) fn clear_scratch(directory: &Path) -> Result<()> {
    let scratch = directory.join(SCRATCH);
    match std::fs::symlink_metadata(&scratch) {
        Ok(metadata) if metadata.is_dir() => Ok(std::fs::remove_dir_all(&scratch)?),
        Ok(_) => Err(Error::StorageIntegrity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn prepare_scratch(scratch: &Path) -> Result<()> {
    if let Some(parent) = scratch.parent() {
        std::fs::create_dir_all(parent)?;
        super::files::plain_directory(parent)?;
    }
    if std::fs::symlink_metadata(scratch).is_ok() {
        std::fs::remove_dir_all(scratch)?;
    }
    std::fs::create_dir(scratch)?;
    Ok(())
}

pub(super) fn stopped(signal: &watch::Receiver<bool>) -> bool {
    *signal.borrow()
}

/// Run file hashing off the async runtime. A stop request sets the flag the hashing
/// loop checks between reads, then waits for the read to actually return.
pub(super) async fn blocking<T: Send + 'static>(
    stop: &Arc<AtomicBool>,
    signal: &mut watch::Receiver<bool>,
    work: impl FnOnce(&AtomicBool) -> T + Send + 'static,
) -> Result<T> {
    let flag = Arc::clone(stop);
    let mut handle = tokio::task::spawn_blocking(move || work(&flag));
    let joined = tokio::select! {
        joined = &mut handle => joined,
        _ = signal.changed() => {
            stop.store(true, Ordering::Release);
            handle.await
        }
    };
    joined.map_err(|_| Error::Analysis("worker-panicked"))
}

pub(super) fn contained(options: ProcessGroupOptions) -> Option<ProcessGroup> {
    ProcessGroup::with_options(options).ok()
}

/// Wait for a started group to report no active member.
pub(super) async fn drain(group: &ProcessGroup) -> Result<()> {
    let end = tokio::time::Instant::now() + DRAIN_DEADLINE;
    loop {
        let active = group
            .stats()
            .map_err(|_| Error::Analysis("native-cleanup-unproven"))?
            .active_process_count;
        if active == 0 {
            return Ok(());
        }
        if tokio::time::Instant::now() >= end {
            let _ = group.kill_all();
            return Err(Error::Analysis("native-cleanup-unproven"));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(super) fn runtime_environment(command: &mut tokio::process::Command, runtime: &Path) {
    // Idle OpenMP threads must sleep rather than spin. Under a job CPU rate limit and a
    // busy host, spinning measured about 17 times slower per generated token (2026-09-24).
    command
        .env("PATH", runtime)
        .env("OMP_WAIT_POLICY", "PASSIVE");
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let mut path = runtime.as_os_str().to_owned();
        path.push(";");
        path.push(Path::new(&root).join("System32"));
        command.env("SystemRoot", root).env("PATH", path);
    }
    #[cfg(unix)]
    command
        .env("LD_LIBRARY_PATH", runtime)
        .env("DYLD_LIBRARY_PATH", runtime);
}
