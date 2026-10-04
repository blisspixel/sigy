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
mod audio;
mod backup;
mod dvr;
mod explorer;
mod init;
mod languages;
mod listen;
mod mcp;
mod monitor;
mod podcast;
mod provider;
mod radio;
mod schedule;
mod service;
mod sources;
mod style;
mod task;
mod update;

#[derive(Debug, Parser)]
#[command(
    name = "sigy",
    bin_name = "sigy",
    version,
    about = "Local-first signals discovery and analysis",
    after_help = init::GET_STARTED
)]
struct Cli {
    /// Library directory. Defaults to ~/.sigy/library; MCP requires an explicit directory.
    #[arg(long, global = true, display_order = 900)]
    data_dir: Option<PathBuf>,
    #[arg(skip)]
    explicit_data_dir: bool,
    /// Emit a structured JSON response for automation.
    #[arg(long, global = true, display_order = 901)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Prepare your private library and start its background service. No directory fetch by default.
    Init {
        #[command(flatten)]
        options: init::InitOptions,
    },
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
    /// Record one source on a once, daily, or weekly civil-time schedule.
    Schedule {
        #[command(subcommand)]
        command: schedule::ScheduleCommand,
    },
    /// Inspect retained inputs and run configured local recognition and translation.
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
    /// Follow a topic across sources within limits you set. Versions and every action are kept.
    Monitor {
        #[command(subcommand)]
        command: monitor::MonitorCommand,
    },
    /// Keep bounded task progress and explicitly delegate cited publications.
    Task {
        #[command(subcommand)]
        command: task::TaskCommand,
    },
    /// Store provider routes and dated prices. Nothing is sent to a provider.
    Provider {
        #[command(subcommand)]
        command: provider::ProviderCommand,
    },
    /// Inspect or explicitly configure lifetime spending allowances. They never reset.
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
    /// Copy the catalog and every retained recording into a new directory, with hashes.
    /// Stop the service first; the backup holds the library lock while it runs.
    Backup {
        /// A directory that does not exist yet.
        destination: std::path::PathBuf,
    },
    /// Check every file in a backup against its manifest. Reads nothing else.
    VerifyBackup { backup: std::path::PathBuf },
    /// Restore a verified backup into a new library directory.
    Restore {
        backup: std::path::PathBuf,
        /// The new library directory. It must not exist yet.
        #[arg(long)]
        into: std::path::PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum BudgetCommand {
    /// Show settled, reserved, and remaining amounts in USD.
    Show,
    /// Set a finite lifetime allowance. It never resets or refills; only this command changes it.
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
        command.operation()?
    } else if let Command::Record { command } = &cli.command {
        command.operation()?
    } else if let Command::Schedule { command } = &cli.command {
        command.operation()?
    } else if let Command::Analysis { command } = &cli.command {
        command.operation()?
    } else if let Command::Dvr { command } = &cli.command {
        command.operation()?
    } else if let Command::Monitor { command } = &cli.command {
        command.operation()?
    } else if let Command::Task { command } = &cli.command {
        command.operation()?
    } else if let Command::Provider { command } = &cli.command {
        command.operation()
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
    if let Command::Radio { command } = &cli.command
        && radio::offline(command, cli.json)?
    {
        return Ok(());
    }
    match &cli.command {
        Command::Init { options } => {
            init::execute(library_dir(cli)?, options, cli.json, cli.explicit_data_dir).await
        }
        _ => run_library(cli).await,
    }
}

async fn run_library(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if let Command::Library { command } = &cli.command
        && let Some(result) = backup::execute(cli, command)
    {
        return result;
    }
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
    if write_document(&mut stdout, &view)? {
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
    render_library_snapshot(&mut stdout, cli, &view, ink)?;
    if let Command::Doctor { strict } = &cli.command {
        let report = view.doctor.as_ref().ok_or("doctor report is missing")?;
        if report.blocked() || (*strict && report.attention()) {
            return Err("doctor found a check that needs action".into());
        }
    }
    Ok(())
}

fn render_library_snapshot(
    stdout: &mut impl Write,
    cli: &Cli,
    view: &Snapshot,
    ink: style::Ink,
) -> Result<(), Box<dyn std::error::Error>> {
    if cli.json {
        serde_json::to_writer(&mut *stdout, view)?;
        writeln!(stdout)?;
    } else if let Some(report) = &view.doctor {
        render_doctor(stdout, view, report, cli.explicit_data_dir, ink)?;
    } else if let Some(policy) = &view.dvr {
        dvr::render_policy(stdout, policy, ink)?;
    } else if let Some(page) = &view.recording_page {
        dvr::render_records(stdout, page, ink)?;
    } else if let Some(page) = &view.monitor {
        monitor::render(stdout, page)?;
    } else if let Some(page) = &view.task {
        task::render(stdout, page)?;
    } else if let Some(page) = &view.provider {
        provider::render(stdout, page)?;
    } else if let Some(page) = &view.schedule {
        schedule::render(stdout, page)?;
    } else if let Some(recognition) = &view.recognition {
        analysis::render_recognition(stdout, recognition)?;
    } else if let Some(job) = &view.analysis_job {
        analysis::render_job(stdout, job)?;
    } else if let Some(page) = &view.analysis {
        analysis::render(stdout, page, ink)?;
    } else if view.playlist.is_some() || view.source_page.is_some() {
        if let Some(playlist) = &view.playlist {
            sources::render_playlist(stdout, playlist)?;
        }
        if let Some(page) = &view.source_page {
            sources::render(stdout, page)?;
        }
    } else if view.podcast_page.is_some() || view.podcast_feed.is_some() {
        if let Some(page) = &view.podcast_page {
            podcast::render(stdout, page)?;
        }
        if let Some(feed) = &view.podcast_feed {
            podcast::render_feed(stdout, feed)?;
        }
    } else if let Some(text) = &view.publisher_text {
        podcast::render_text(stdout, text)?;
    } else if view.directory.is_some()
        || view.ordered_station_page.is_some()
        || view.linked_station_context.is_some()
    {
        radio::render(stdout, view, ink)?;
    } else {
        render_status(stdout, view, ink)?;
    }
    Ok(())
}

fn write_document(
    stdout: &mut impl Write,
    view: &Snapshot,
) -> Result<bool, Box<dyn std::error::Error>> {
    if let Some(metadata) = &view.recording_metadata {
        serde_json::to_writer_pretty(&mut *stdout, metadata)?;
        writeln!(stdout)?;
        return Ok(true);
    }
    if let Some(export) = &view.briefing_export {
        monitor::write_export(stdout, export)?;
        return Ok(true);
    }
    Ok(false)
}

fn render_doctor(
    stdout: &mut impl Write,
    view: &Snapshot,
    report: &sigy_service::control::DoctorReport,
    explicit_data_dir: bool,
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
    let same = if explicit_data_dir {
        " with the same --data-dir"
    } else {
        ""
    };
    match &view.service {
        Some(service) => writeln!(
            stdout,
            "Service: process {} is {}.",
            service.process_id,
            if service.stopping {
                ink.tint(style::Tone::Warn, "stopping")
            } else {
                ink.tint(style::Tone::Ok, "running")
            }
        )?,
        None => writeln!(
            stdout,
            "Service: {}. Refresh, recording and processing need `sigy service start`{same}.",
            ink.tint(style::Tone::Warn, "not running")
        )?,
    }
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
            writeln!(stdout, "  Next: sigy {command}{same}")?;
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
            "{}: lifetime limit ${} (never resets), settled ${}, reserved ${}, available ${}{}",
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
    if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new("--internal-audio-output-v1"))
    {
        if std::env::args_os().count() != 2 || audio::run_helper().is_err() {
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
    let mut cli = Cli::parse();
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        init::resolve_directory(&mut cli)?;
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
            let error = init::actionable_error(&cli, error);
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
