use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use sigy_service::{
    control::{self, Operation, Snapshot},
    domain::money::Usd,
    library::Library,
};

mod service;
mod sources;

#[derive(Debug, Parser)]
#[command(version, about = "Local-first signals discovery and analysis")]
struct Cli {
    /// Explicit library directory. No source or model work starts implicitly.
    #[arg(long)]
    data_dir: PathBuf,
    /// Emit a structured JSON response for automation.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Register and inspect immutable source configurations.
    Source {
        #[command(subcommand)]
        command: sources::SourceCommand,
    },
    /// Start, inspect, and stop the persistent local controller.
    Service {
        #[command(subcommand)]
        command: service::ServiceCommand,
    },
    /// Initialize and inspect local durable storage.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Inspect or explicitly configure lifetime spending limits.
    Budget {
        #[command(subcommand)]
        command: BudgetCommand,
    },
}

#[derive(Debug, Subcommand)]
enum LibraryCommand {
    /// Create the catalog with paid processing disabled.
    Init,
    /// Validate the catalog and report its current state.
    Status,
}

#[derive(Debug, Subcommand)]
enum BudgetCommand {
    /// Show settled, reserved, and remaining amounts in USD.
    Show,
    /// Set a finite limit. This does not configure or invoke a provider.
    Set {
        /// Global or a named provider/task budget scope.
        #[arg(default_value = "global")]
        scope: String,
        #[arg(long)]
        usd: Usd,
    },
}

async fn execute(cli: &Cli) -> Result<Option<Snapshot>, Box<dyn std::error::Error>> {
    if let Command::Service { command } = &cli.command {
        return service::execute(&cli.data_dir, command).await;
    }
    let create = matches!(
        cli.command,
        Command::Library {
            command: LibraryCommand::Init
        }
    );
    let operation = if let Command::Budget {
        command: BudgetCommand::Set { scope, usd },
    } = &cli.command
    {
        Operation::SetBudget {
            scope: scope.clone(),
            limit_usd: usd.to_string(),
        }
    } else if let Command::Source { command } = &cli.command {
        command.operation()
    } else {
        Operation::Status {}
    };
    match Library::open(&cli.data_dir, create) {
        Ok(mut library) => Ok(Some(control::apply(library.store_mut(), operation)?)),
        Err(sigy_service::Error::LibraryBusy) if !create => {
            Ok(Some(control::request(&cli.data_dir, operation).await?))
        }
        Err(error) => Err(error.into()),
    }
}

async fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let Some(view) = execute(cli).await? else {
        return Ok(());
    };
    let mut stdout = io::stdout().lock();
    if cli.json {
        serde_json::to_writer(&mut stdout, &view)?;
        writeln!(stdout)?;
    } else if let Some(page) = view.source_page {
        sources::render(&mut stdout, page)?;
    } else {
        if let Some(service) = view.service {
            writeln!(
                stdout,
                "Service {}: {}, uptime {} seconds.",
                service.process_id,
                if service.stopping {
                    "stopping"
                } else {
                    "running"
                },
                service.uptime_seconds
            )?;
        }
        writeln!(
            stdout,
            "Library ready. SQLite {}. Provider dispatch is not implemented.",
            view.sqlite_version
        )?;
        writeln!(
            stdout,
            "Capture jobs: {} scheduled, {} active, {} interrupted, {} terminal. Capture dispatch is not implemented.",
            view.captures.scheduled,
            view.captures.active,
            view.captures.interrupted,
            view.captures.terminal
        )?;
        for budget in view.budgets {
            writeln!(
                stdout,
                "{}: limit ${}, settled ${}, reserved ${}, available ${}{}",
                budget.scope,
                budget.limit_usd,
                budget.settled_usd,
                budget.reserved_usd,
                budget.available_usd,
                if budget.frozen { " (frozen)" } else { "" }
            )?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(unix)]
        service::detach_if_requested(&cli.command)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(4)
            .enable_all()
            .build()?;
        runtime.block_on(run(&cli))
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
                || error
                    .downcast_ref::<serde_json::Error>()
                    .is_some_and(|e| e.io_error_kind() == Some(io::ErrorKind::BrokenPipe))
            {
                return ExitCode::SUCCESS;
            }
            let mut stderr = io::stderr().lock();
            let written = if cli.json {
                writeln!(
                    stderr,
                    "{}",
                    serde_json::json!({ "error": error.to_string() })
                )
            } else {
                writeln!(stderr, "sigy: {error}")
            };
            if written.is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}
