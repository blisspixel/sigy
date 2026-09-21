use std::{path::Path, time::Duration};

mod child;
use child::ServiceChild;

use clap::Subcommand;
use sigy_service::{
    control::{self, Operation, Snapshot},
    library::Library,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// Run in the foreground until stopped through the CLI or by a signal.
    Run {
        #[arg(long, hide = true)]
        background: bool,
    },
    /// Start a detached per-user process and wait for a successful connection.
    Start,
    /// Inspect a running service. Does not create or start one.
    Status,
    /// Request orderly shutdown; admitted changes finish before exit.
    Stop,
}

#[cfg(unix)]
pub fn detach_if_requested(command: &super::Command) -> Result<()> {
    if matches!(
        command,
        super::Command::Service {
            command: ServiceCommand::Run { background: true }
        }
    ) {
        rustix::process::setsid()?;
    }
    Ok(())
}

pub async fn execute(directory: &Path, command: &ServiceCommand) -> Result<Option<Snapshot>> {
    match command {
        ServiceCommand::Run { .. } => {
            let library = Library::open(directory, false)?;
            control::run(library, shutdown_signal()).await?;
            Ok(None)
        }
        ServiceCommand::Start => start(directory).await.map(Some),
        ServiceCommand::Status => Ok(Some(
            control::request(directory, Operation::Status {}).await?,
        )),
        ServiceCommand::Stop => Ok(Some(control::request(directory, Operation::Stop {}).await?)),
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => (),
                _ = terminate.recv() => (),
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn start(directory: &Path) -> Result<Snapshot> {
    // Start is idempotent only after successfully verifying the current endpoint.
    if let Ok(snapshot) = control::request(directory, Operation::Status {}).await {
        return Ok(snapshot);
    }
    // Validate before spawning. The child acquires ownership again, closing the
    // race with another simultaneous starter without permitting two writers.
    drop(Library::open(directory, false)?);
    let directory = directory.canonicalize()?;
    let mut child = StartingChild(Some(ServiceChild::spawn(&directory)?));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if let Some(process) = child.0.as_mut()
            && let Some(status) = process.exited()?
        {
            return Err(format!(
                "service exited during startup ({status}); run `service run` to inspect the error"
            )
            .into());
        }
        if let Ok(Ok(snapshot)) =
            tokio::time::timeout_at(deadline, control::request(&directory, Operation::Status {}))
                .await
        {
            // Ownership has transferred to the running service. Dropping Child
            // releases the parent handle without terminating that process.
            if snapshot.service.as_ref().map(|service| service.process_id)
                == child.0.as_ref().map(ServiceChild::id)
            {
                child.0.take();
            }
            return Ok(snapshot);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("service did not become ready; run `service run` to inspect the error".into())
}

/// A failed starter must not leave behind a child whose readiness is unknown.
struct StartingChild(Option<ServiceChild>);

impl Drop for StartingChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            child.terminate();
        }
    }
}
