use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use sigy_service::{
    control::{self, Operation, Snapshot},
    domain::money::Usd,
    library::Library,
};

mod analysis;
mod dvr;
mod explorer;
mod listen;
mod mcp;
mod podcast;
mod radio;
mod schedule;
mod service;
mod sources;
mod style;
mod update;

#[derive(Debug, Parser)]
#[command(version, about = "Local-first signals discovery and analysis")]
struct Cli {
    /// Library directory. Required except for `update`, help, and version.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Emit a structured JSON response for automation.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Discover internet stations and search the local directory cache.
    Radio {
        #[command(subcommand)]
        command: radio::RadioCommand,
    },
    /// Record direct audio streams and manage retained media.
    Record {
        #[command(subcommand)]
        command: dvr::RecordCommand,
    },
    /// Schedule one source by civil time. No analysis profile can be attached.
    Schedule {
        #[command(subcommand)]
        command: schedule::ScheduleCommand,
    },
    /// Pin a published recording for analysis. No source URL is attached.
    Analysis {
        #[command(subcommand)]
        command: analysis::AnalysisCommand,
    },
    /// Configure rolling retention, storage quota and media validation.
    Dvr {
        #[command(subcommand)]
        command: dvr::DvrCommand,
    },
    /// Register and inspect immutable source configurations.
    Source {
        #[command(subcommand)]
        command: sources::SourceCommand,
    },
    /// Store a podcast subscription, refresh one RSS document, or download one enclosure.
    Podcast {
        #[command(subcommand)]
        command: podcast::PodcastCommand,
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
    /// Play retained audio in this client. Capture continues in the service.
    Listen {
        #[command(subcommand)]
        command: listen::ListenCommand,
    },
    /// Speak MCP 2026-07-28 on stdin and stdout for one configured library.
    Mcp,
    /// Check the catalog, decoder, quota, and cache age. This does not use the network.
    Doctor {
        /// Exit with an error when any check needs attention.
        #[arg(long)]
        strict: bool,
    },
    /// Check or install the latest main commit from GitHub. This does not use a library.
    Update {
        /// Report whether a newer commit exists and do not install it.
        #[arg(long)]
        check: bool,
    },
    /// Open the list explorer. Selection does not start audio, capture, refresh, or a click.
    Tui {
        /// Freeze decorative motion. No animation is drawn in this explorer.
        #[arg(long)]
        reduced_motion: bool,
        /// Use one reading order with explicit text labels.
        #[arg(long)]
        linear: bool,
        /// Draw without color. `NO_COLOR` also selects this.
        #[arg(long)]
        monochrome: bool,
        /// Draw one frame, write a JSON size report, and exit. Does not stop the service.
        #[arg(long)]
        inspect: Option<PathBuf>,
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

fn library_dir(cli: &Cli) -> Result<&Path, Box<dyn std::error::Error>> {
    cli.data_dir
        .as_deref()
        .ok_or("--data-dir is required".into())
}

async fn execute(cli: &Cli) -> Result<Option<Snapshot>, Box<dyn std::error::Error>> {
    let data_dir = library_dir(cli)?;
    if let Command::Service { command } = &cli.command {
        return service::execute(data_dir, command).await;
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
    } else if let Command::Podcast { command } = &cli.command {
        command.operation()
    } else if let Command::Radio { command } = &cli.command {
        command.operation()
    } else if let Command::Record { command } = &cli.command {
        command.operation()?
    } else if let Command::Schedule { command } = &cli.command {
        command.operation()?
    } else if let Command::Analysis { command } = &cli.command {
        command.operation()
    } else if let Command::Dvr { command } = &cli.command {
        command.operation()?
    } else if matches!(cli.command, Command::Doctor { .. }) {
        Operation::Doctor {}
    } else {
        Operation::Status {}
    };
    match Library::open(data_dir, create) {
        Ok(mut library) => Ok(Some(control::apply_library(&mut library, operation)?)),
        Err(sigy_service::Error::LibraryBusy) if !create => {
            Ok(Some(control::request(data_dir, operation).await?))
        }
        Err(error) => Err(error.into()),
    }
}

async fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if let Command::Listen { command } = &cli.command {
        return listen::execute(library_dir(cli)?, command, cli.json).await;
    }
    if matches!(cli.command, Command::Tui { .. }) {
        return Err("the list explorer starts before the service runtime".into());
    }
    let Some(view) = execute(cli).await? else {
        return Ok(());
    };
    let ink = style::Ink::stdout(cli.json);
    let mut stdout = io::stdout().lock();
    if let Some(metadata) = &view.recording_metadata {
        serde_json::to_writer_pretty(&mut stdout, metadata)?;
        writeln!(stdout)?;
        return Ok(());
    }
    if matches!(
        cli.command,
        Command::Record {
            command: dvr::RecordCommand::Path { .. }
        }
    ) {
        let record = view
            .recording_page
            .as_ref()
            .and_then(|page| page.entries.first())
            .ok_or("recording not found")?;
        if record.storage_state != "retained" {
            return Err("recording has no verified retained media".into());
        }
        let path = sigy_service::recordings::media_path(library_dir(cli)?, &record.object_key)?;
        if !path.is_file() {
            return Err("retained media is missing".into());
        }
        if cli.json {
            serde_json::to_writer(&mut stdout, &serde_json::json!({"path": path}))?;
            writeln!(stdout)?;
        } else {
            writeln!(stdout, "{}", path.display())?;
        }
        return Ok(());
    }
    if cli.json {
        serde_json::to_writer(&mut stdout, &view)?;
        writeln!(stdout)?;
    } else if let Some(report) = &view.doctor {
        render_doctor(&mut stdout, report, ink)?;
    } else if let Some(policy) = view.dvr {
        dvr::render_policy(&mut stdout, &policy, ink)?;
    } else if let Some(page) = view.recording_page {
        dvr::render_records(&mut stdout, &page, ink)?;
    } else if let Some(page) = view.schedule {
        schedule::render(&mut stdout, &page)?;
    } else if let Some(page) = &view.analysis {
        analysis::render(&mut stdout, page, ink)?;
    } else if view.playlist.is_some() || view.source_page.is_some() {
        if let Some(playlist) = &view.playlist {
            sources::render_playlist(&mut stdout, playlist)?;
        }
        if let Some(page) = &view.source_page {
            sources::render(&mut stdout, page)?;
        }
    } else if view.podcast_page.is_some() || view.podcast_feed.is_some() {
        if let Some(page) = &view.podcast_page {
            podcast::render(&mut stdout, page)?;
        }
        if let Some(feed) = &view.podcast_feed {
            podcast::render_feed(&mut stdout, feed)?;
        }
    } else if let Some(text) = &view.publisher_text {
        podcast::render_text(&mut stdout, text)?;
    } else if view.directory.is_some() {
        radio::render(&mut stdout, &view, ink)?;
    } else {
        render_status(&mut stdout, &view, ink)?;
    }
    if let Command::Doctor { strict } = &cli.command {
        let report = view.doctor.as_ref().ok_or("doctor report is missing")?;
        if report.blocked() || (*strict && report.attention()) {
            return Err("doctor found a check that needs action".into());
        }
    }
    Ok(())
}

fn render_doctor(
    stdout: &mut impl Write,
    report: &sigy_service::control::DoctorReport,
    ink: style::Ink,
) -> io::Result<()> {
    let blocked = report
        .checks
        .iter()
        .filter(|check| check.state == sigy_service::control::DoctorState::Blocked)
        .count();
    let attention = report
        .checks
        .iter()
        .filter(|check| check.state == sigy_service::control::DoctorState::Attention)
        .count();
    writeln!(
        stdout,
        "Doctor: {attention} attention, {blocked} blocked. No network request was made."
    )?;
    for check in &report.checks {
        let (state, tone) = match check.state {
            sigy_service::control::DoctorState::Ok => ("ok", style::Tone::Ok),
            sigy_service::control::DoctorState::Attention => ("attention", style::Tone::Warn),
            sigy_service::control::DoctorState::Blocked => ("blocked", style::Tone::Fail),
        };
        writeln!(
            stdout,
            "{} {}: {}",
            ink.tint(tone, state),
            check.name,
            check.detail
        )?;
        if let Some(command) = &check.command {
            writeln!(stdout, "  Next: {command}")?;
        }
    }
    Ok(())
}

fn render_status(stdout: &mut impl Write, view: &Snapshot, ink: style::Ink) -> io::Result<()> {
    if let Some(service) = &view.service {
        let (state, tone) = if service.stopping {
            ("stopping", style::Tone::Warn)
        } else {
            ("running", style::Tone::Ok)
        };
        writeln!(
            stdout,
            "Service {}: {}, uptime {} seconds.",
            service.process_id,
            ink.tint(tone, state),
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
        "Capture jobs: {} scheduled, {} active, {} interrupted, {} terminal. Recording dispatch: {}.",
        view.captures.scheduled,
        view.captures.active,
        view.captures.interrupted,
        view.captures.terminal,
        if view.captures.dispatch_available {
            "available"
        } else {
            "requires running service and configured DVR"
        }
    )?;
    for budget in &view.budgets {
        writeln!(
            stdout,
            "{}: limit ${}, settled ${}, reserved ${}, available ${}{}",
            budget.scope,
            budget.limit_usd,
            budget.settled_usd,
            budget.reserved_usd,
            budget.available_usd,
            if budget.frozen {
                ink.tint(style::Tone::Warn, " (frozen)")
            } else {
                String::new()
            }
        )?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if let Command::Update { check } = cli.command {
            return update::run(check, cli.json);
        }
        if matches!(cli.command, Command::Mcp) {
            return mcp::serve(library_dir(&cli)?);
        }
        if let Command::Tui {
            reduced_motion,
            linear,
            monochrome,
            inspect,
        } = &cli.command
        {
            return explorer::run(
                library_dir(&cli)?,
                explorer::Modes::resolve(*reduced_motion, *linear, *monochrome),
                inspect.as_deref(),
            );
        }
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
                writeln!(
                    stderr,
                    "{} {error}",
                    style::Ink::stderr().tint(style::Tone::Fail, "sigy:")
                )
            };
            if written.is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}
