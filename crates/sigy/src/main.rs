use std::{
    io::{self, IsTerminal, Write},
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
mod claim;
mod copy;
#[cfg(target_os = "linux")]
mod delegation;
mod doctor_view;
mod dvr;
mod exit;
mod explorer;
mod hints;
mod init;
mod languages;
mod listen;
mod mcp;
mod monitor;
mod panel;
mod play;
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
    about = "Practical signals intelligence for everyday people.",
    before_help = init::GET_STARTED,
    help_template = "{about}\n\n{before-help}\n\n{usage-heading} {usage}\n\n{options}"
)]
struct Cli {
    /// Library directory. Linux uses the XDG data directory for a new library.
    /// An existing ~/.sigy/library stays. Windows and macOS use ~/.sigy/library.
    /// MCP requires an explicit directory.
    #[arg(long, global = true, display_order = 900)]
    data_dir: Option<PathBuf>,
    #[arg(skip)]
    explicit_data_dir: bool,
    /// Emit a structured JSON response for automation.
    #[arg(long, global = true, display_order = 901)]
    json: bool,
    /// When human output uses color: `auto`, `always`, or `never`.
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t = InkChoice::Auto,
        display_order = 902
    )]
    color: InkChoice,
    /// Bold emphasis. Failure and live roles keep their color when color is on.
    #[arg(long, global = true, display_order = 903)]
    high_contrast: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum InkChoice {
    Auto,
    Always,
    Never,
}

impl From<InkChoice> for style::ColorChoice {
    fn from(value: InkChoice) -> Self {
        match value {
            InkChoice::Auto => Self::Auto,
            InkChoice::Always => Self::Always,
            InkChoice::Never => Self::Never,
        }
    }
}

fn presentation(cli: &Cli) -> style::Ink {
    if cli.color == InkChoice::Auto && !cli.high_contrast {
        style::Ink::stdout(cli.json)
    } else {
        style::Ink::choose(cli.json, cli.color.into(), cli.high_contrast)
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Pick a random cached station with directory coordinates. Never contacts a stream.
    Explore {
        #[command(flatten)]
        filters: radio::Filters,
        /// Pick only from saved favorites.
        #[arg(long)]
        favorites: bool,
    },
    /// Play one cached station, registering its first audio revision if needed.
    Play {
        /// Station UUID from radio search or the explorer.
        station: String,
        /// `null` discards samples; `system` requests the local output adapter.
        #[arg(long, default_value = "system", value_parser = ["system", "null"])]
        destination: String,
        /// Cancel playback if the launching process exits.
        #[arg(long)]
        parent_pid: Option<u32>,
        /// Cancel playback when stdin closes, for supervised desktop launchers.
        #[arg(long)]
        cancel_on_stdin: bool,
    },
    /// Stop the current play or panel listen. Recordings keep running.
    Stop,
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
    /// Write a claim and link a stored finding. Nothing is fetched and no score is set.
    Claim {
        #[command(subcommand)]
        command: claim::ClaimCommand,
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
    /// Read-only status and one-station playback for desktop panels.
    Panel {
        #[command(subcommand)]
        command: panel::PanelCommand,
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
        /// Inspect the last recorded update outcome without fetching or building.
        #[arg(long, conflicts_with = "check")]
        status: bool,
    },
    /// Open the list explorer. Selection does not start audio, capture, refresh, or a click.
    Tui {
        /// Initial cached station-name search. Use --tag for a genre or --country for a place.
        #[arg(conflicts_with = "name")]
        query: Option<String>,
        #[command(flatten)]
        filters: radio::Filters,
        /// Disable optional globe rotation, including the r toggle.
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
        return Ok(Some(radio::execute(data_dir, command).await?));
    } else if let Command::Explore { filters, favorites } = &cli.command {
        Operation::Radio {
            command: control::DirectoryOperation::Explore {
                filter: filters.filter()?,
                favorites_only: *favorites,
            },
        }
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
    } else if let Command::Claim { command } = &cli.command {
        command.operation()
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
            Box::pin(init::execute(
                library_dir(cli)?,
                options,
                cli.json,
                cli.explicit_data_dir,
            ))
            .await
        }
        _ => Box::pin(run_library(cli)).await,
    }
}

async fn run_library(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if let Command::Play {
        station,
        destination,
        parent_pid,
        cancel_on_stdin,
    } = &cli.command
    {
        return Box::pin(play::execute(
            library_dir(cli)?,
            station,
            destination,
            *parent_pid,
            *cancel_on_stdin,
            cli.json,
        ))
        .await;
    }
    if matches!(cli.command, Command::Stop) {
        return Box::pin(panel::execute(
            library_dir(cli)?,
            &panel::PanelCommand::Stop { id: None },
            cli.json,
            explorer::Modes::resolve(false, false, false),
            false,
        ))
        .await;
    }
    if let Command::Library { command } = &cli.command
        && let Some(result) = backup::execute(cli, command)
    {
        return result;
    }
    if let Command::Listen { command } = &cli.command {
        return Box::pin(listen::execute(library_dir(cli)?, command, cli.json)).await;
    }
    if let Command::Panel { command } = &cli.command {
        return Box::pin(panel::execute(
            library_dir(cli)?,
            command,
            cli.json,
            desk_modes(cli, command),
            cli.explicit_data_dir,
        ))
        .await;
    }
    if matches!(cli.command, Command::Tui { .. }) {
        return Err("the list explorer starts before the service runtime".into());
    }
    let Some(mut view) = Box::pin(execute(cli)).await? else {
        return Ok(());
    };
    if let Command::Doctor { .. } = &cli.command
        && !cli.explicit_data_dir
        && let Some(directory) = cli.data_dir.as_deref()
        && let Some(report) = view.doctor.as_mut()
    {
        doctor_view::note_legacy_library(directory, report);
    }
    let ink = presentation(cli);
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
        return write_recording_path(&mut stdout, cli, &view);
    }
    render_library_snapshot(&mut stdout, cli, &view, ink)?;
    if let Command::Radio { command } = &cli.command {
        radio::write_next(&mut stdout, command, &view, cli)?;
        if matches!(
            command,
            radio::RadioCommand::Search { fetch: true, .. }
                | radio::RadioCommand::Refresh { wait: true, .. }
        ) && view
            .directory_refresh
            .as_ref()
            .is_some_and(|refresh| matches!(refresh.state.as_str(), "failed" | "interrupted"))
        {
            return Err(
                "directory refresh did not complete; the previous cache is preserved".into(),
            );
        }
    }
    if let Command::Explore { filters, favorites } = &cli.command {
        radio::write_explore_next(&mut stdout, filters, *favorites, &view, cli)?;
    }
    if let Command::Doctor { strict } = &cli.command {
        refuse_doctor(&view, *strict, cli.json)?;
    }
    if view.task.as_deref().is_some_and(task::is_partial) {
        return Err(exit::Partial.into());
    }
    Ok(())
}

fn write_recording_path(
    stdout: &mut impl Write,
    cli: &Cli,
    view: &Snapshot,
) -> Result<(), Box<dyn std::error::Error>> {
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
        serde_json::to_writer(&mut *stdout, &serde_json::json!({"path": path}))?;
        writeln!(stdout)?;
    } else {
        writeln!(stdout, "{}", path.display())?;
    }
    Ok(())
}

fn exit_usage(error: &clap::Error) -> ! {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let next = hints::suggestion(&args);
    if args.iter().any(|argument| argument == "--json") {
        let document = exit::error_document(error.to_string().trim(), "usage", next.as_deref());
        let _ = writeln!(io::stderr(), "{document}");
        std::process::exit(2);
    }
    if let Some(next) = next {
        eprintln!("Next: {next}");
    }
    error.exit()
}

fn refuse_doctor(
    view: &Snapshot,
    strict: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = view.doctor.as_ref().ok_or("doctor report is missing")?;
    if let Some(line) = doctor_view::failure_line(report, strict, json) {
        return Err(line.into());
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
        let directory = explicit_directory(cli)?;
        doctor_view::render(stdout, view, report, directory.as_deref(), ink)?;
    } else if let Some(policy) = &view.dvr {
        dvr::render_policy(stdout, policy, ink)?;
    } else if let Some(page) = &view.recording_page {
        dvr::render_records(stdout, page, ink)?;
    } else if let Some(page) = &view.claim {
        claim::render(stdout, page)?;
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
        || view.random_station.is_some()
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

fn explicit_directory(cli: &Cli) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if !cli.explicit_data_dir {
        return Ok(None);
    }
    Ok(Some(library_dir(cli)?.display().to_string()))
}

fn doctor_label(
    state: &sigy_service::control::DoctorState,
    detail: &str,
) -> (&'static str, style::Tone) {
    if detail.to_ascii_lowercase().contains("unmeasured") {
        return ("not measured yet", style::Tone::Muted);
    }
    match state {
        sigy_service::control::DoctorState::Ok => ("ok", style::Tone::Ok),
        sigy_service::control::DoctorState::Attention => ("attention", style::Tone::Attention),
        sigy_service::control::DoctorState::Blocked => ("blocked", style::Tone::Fail),
    }
}

fn render_status(stdout: &mut impl Write, view: &Snapshot, ink: style::Ink) -> io::Result<()> {
    if let Some(service) = &view.service {
        let (state, tone) = if service.stopping {
            ("stopping", style::Tone::Attention)
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
        "Library ready. SQLite {}. {}",
        view.sqlite_version,
        copy::paid_processing(view.provider_dispatch_available)
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
            "{}: lifetime limit {} (never resets), settled {}, reserved {}, available {}{}",
            budget.scope,
            copy::compact_usd(&budget.limit_usd),
            copy::compact_usd(&budget.settled_usd),
            copy::compact_usd(&budget.reserved_usd),
            copy::compact_usd(&budget.available_usd),
            if budget.frozen {
                ink.tint(style::Tone::Attention, " (frozen)")
            } else {
                String::new()
            }
        )?;
    }
    Ok(())
}

fn open_explorer(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let Command::Tui {
        query,
        filters,
        reduced_motion,
        linear,
        monochrome,
        inspect,
    } = &cli.command
    else {
        return Err("the list explorer starts before the service runtime".into());
    };
    let modes = if cli.high_contrast || cli.color != InkChoice::Auto {
        let emphasis = if cli.high_contrast {
            explorer::Emphasis::HighContrast
        } else {
            explorer::Emphasis::Standard
        };
        explorer::Modes::resolve_with(
            *reduced_motion,
            *linear,
            *monochrome,
            emphasis,
            cli.color.into(),
        )
    } else {
        explorer::Modes::resolve(*reduced_motion, *linear, *monochrome)
    };
    let directory = library_dir(cli)?;
    explorer::run(
        directory,
        cli.explicit_data_dir,
        modes,
        inspect.as_deref(),
        filters.search_filter(query.as_deref())?,
    )
}

fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => exit_usage(&error),
    }
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
    let mut cli = parse_cli();
    #[cfg(target_os = "linux")]
    let mut managed_exit = None;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if let Some(refusal) = blocked_explorer(&cli)? {
            return Err(refusal.into());
        }
        if let Some(refusal) = blocked_desk(&cli) {
            return Err(refusal.into());
        }
        init::resolve_directory(&mut cli)?;
        #[cfg(target_os = "linux")]
        if let Some(code) = delegation::enter(&cli)? {
            managed_exit = Some(code);
            return Ok(());
        }
        if let Command::Update { check, status } = cli.command {
            return update::run(check, status, cli.json);
        }
        if matches!(cli.command, Command::Mcp) {
            return mcp::serve(library_dir(&cli)?);
        }
        if matches!(cli.command, Command::Tui { .. }) {
            return open_explorer(&cli);
        }
        #[cfg(unix)]
        service::detach_if_requested(&cli.command)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(4)
            .enable_all()
            .build()?;
        runtime.block_on(async {
            #[cfg(target_os = "linux")]
            {
                tokio::select! {
                    result = Box::pin(run(&cli)) => result,
                    () = delegation::owner_gone() => Err("native launcher exited".into()),
                }
            }
            #[cfg(not(target_os = "linux"))]
            run(&cli).await
        })
    })();
    match result {
        Ok(()) => {
            #[cfg(target_os = "linux")]
            {
                managed_exit.unwrap_or(ExitCode::SUCCESS)
            }
            #[cfg(not(target_os = "linux"))]
            ExitCode::SUCCESS
        }
        Err(error) => report_failure(&cli, error),
    }
}

fn desk_modes(cli: &Cli, command: &panel::PanelCommand) -> explorer::Modes {
    let panel::PanelCommand::Desk {
        reduced_motion,
        linear,
        monochrome,
    } = command
    else {
        return explorer::Modes::resolve(false, false, false);
    };
    let emphasis = if cli.high_contrast {
        explorer::Emphasis::HighContrast
    } else {
        explorer::Emphasis::Standard
    };
    explorer::Modes::resolve_with(
        *reduced_motion,
        *linear,
        *monochrome,
        emphasis,
        cli.color.into(),
    )
}

fn blocked_desk(cli: &Cli) -> Option<panel::DeskRefusal> {
    let Command::Panel {
        command: panel::PanelCommand::Desk { .. },
    } = &cli.command
    else {
        return None;
    };
    let term_dumb = std::env::var_os("TERM").is_some_and(|value| value == "dumb");
    if explorer::interactive_terminal(
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
        term_dumb,
    ) {
        return None;
    }
    let directory = cli.data_dir.as_ref().map(|path| path.display().to_string());
    Some(panel::DeskRefusal::new(directory.as_deref()))
}

fn blocked_explorer(cli: &Cli) -> Result<Option<explorer::Refusal>, Box<dyn std::error::Error>> {
    let Command::Tui { query, filters, .. } = &cli.command else {
        return Ok(None);
    };
    let term_dumb = std::env::var_os("TERM").is_some_and(|value| value == "dumb");
    if explorer::interactive_terminal(
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
        term_dumb,
    ) {
        return Ok(None);
    }
    let filter = filters.search_filter(query.as_deref())?;
    let directory = cli.data_dir.as_ref().map(|path| path.display().to_string());
    Ok(Some(explorer::Refusal::from_search(
        &filter,
        directory.as_deref(),
    )))
}

fn report_failure(cli: &Cli, error: Box<dyn std::error::Error>) -> ExitCode {
    if error.is::<explorer::Interrupted>() {
        return write_classified(cli.json, "interrupted", "interrupted", None, 130);
    }
    if let Some(refusal) = error.downcast_ref::<explorer::Refusal>() {
        return write_refusal(cli.json, refusal);
    }
    if let Some(refusal) = error.downcast_ref::<panel::DeskRefusal>() {
        return write_desk_refusal(cli.json, refusal);
    }
    let class = exit::classify(error.as_ref(), &cli.command);
    let error = init::actionable_error(cli, error);
    if broken_pipe(error.as_ref()) {
        return ExitCode::SUCCESS;
    }
    let message = error.to_string();
    let next = exit::next_command(&message);
    write_classified(cli.json, &message, class.code, next.as_deref(), class.exit)
}

fn broken_pipe(error: &(dyn std::error::Error + 'static)) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|cause| cause.kind() == io::ErrorKind::BrokenPipe)
        || error
            .downcast_ref::<serde_json::Error>()
            .is_some_and(|cause| cause.io_error_kind() == Some(io::ErrorKind::BrokenPipe))
}

fn write_classified(
    json: bool,
    message: &str,
    code: &str,
    next: Option<&str>,
    exit: u8,
) -> ExitCode {
    if !json && code == "interrupted" {
        return ExitCode::from(exit);
    }
    let mut stderr = io::stderr().lock();
    let written = if json {
        writeln!(stderr, "{}", exit::error_document(message, code, next))
    } else {
        writeln!(
            stderr,
            "{} {}",
            style::Ink::stderr().tint(style::Tone::Fail, "sigy:"),
            explorer::text::sanitize(message, 4096)
        )
    };
    if written
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    {
        return ExitCode::SUCCESS;
    }
    ExitCode::from(exit)
}

fn write_refusal(json: bool, refusal: &explorer::Refusal) -> ExitCode {
    write_terminal_refusal(
        json,
        explorer::Refusal::MESSAGE,
        refusal.next(),
        &refusal.to_string(),
    )
}

fn write_desk_refusal(json: bool, refusal: &panel::DeskRefusal) -> ExitCode {
    write_terminal_refusal(
        json,
        panel::DeskRefusal::MESSAGE,
        refusal.next(),
        &refusal.to_string(),
    )
}

fn write_terminal_refusal(json: bool, message: &str, next: &str, human: &str) -> ExitCode {
    let mut stderr = io::stderr().lock();
    let written = if json {
        writeln!(
            stderr,
            "{}",
            exit::error_document(message, "failure", Some(next))
        )
    } else {
        writeln!(stderr, "{human}")
    };
    if written
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    {
        return ExitCode::SUCCESS;
    }
    if written.is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::FAILURE
}

#[cfg(test)]
mod future_frame_tests {
    use clap::{CommandFactory, Parser};

    #[test]
    fn command_routers_keep_large_playback_futures_off_the_poll_stack()
    -> Result<(), Box<dyn std::error::Error>> {
        for command in [
            vec!["listen", "file", "fixture", "--destination", "system"],
            vec!["play", "00000000-0000-0000-0000-000000000001"],
            vec!["radio", "refresh", "--wait"],
            vec!["init", "--radio"],
        ] {
            let mut args = vec!["sigy", "--data-dir", "not-opened"];
            args.extend(command);
            let cli = super::Cli::try_parse_from(args)?;
            let route = super::run(&cli);
            let library = super::run_library(&cli);
            assert!(std::mem::size_of_val(&route) <= 16 * 1024);
            assert!(std::mem::size_of_val(&library) <= 16 * 1024);
        }
        #[cfg(windows)]
        {
            let live = super::audio::play::direct(
                "not-started",
                std::path::Path::new("not-opened"),
                "fixture",
                "wav",
            );
            assert!(std::mem::size_of_val(&live) <= 16 * 1024);
        }
        Ok(())
    }

    #[test]
    fn unmeasured_doctor_checks_are_not_prefixed_ok() {
        let (label, tone) = super::doctor_label(
            &sigy_service::control::DoctorState::Ok,
            "Recognition pace is unmeasured.",
        );
        assert_eq!(label, "not measured yet");
        assert_eq!(tone, crate::style::Tone::Muted);
        let (ok, ok_tone) = super::doctor_label(
            &sigy_service::control::DoctorState::Ok,
            "The configured decoder is present.",
        );
        assert_eq!(ok, "ok");
        assert_eq!(ok_tone, crate::style::Tone::Ok);
        let (blocked, blocked_tone) = super::doctor_label(
            &sigy_service::control::DoctorState::Blocked,
            "Native containment is unavailable.",
        );
        assert_eq!(blocked, "blocked");
        assert_eq!(blocked_tone, crate::style::Tone::Fail);
    }

    #[test]
    fn top_level_help_fits_sixty_columns() -> Result<(), Box<dyn std::error::Error>> {
        for line in crate::init::GET_STARTED.lines() {
            assert!(line.chars().count() <= 60, "{line}");
        }
        let mut command = super::Cli::command()
            .color(clap::ColorChoice::Never)
            .term_width(60);
        let mut buffer = Vec::new();
        command.write_help(&mut buffer)?;
        let text = String::from_utf8(buffer)?;
        assert!(text.contains("Practical signals intelligence for everyday people."));
        assert!(text.contains("sigy init --radio"));
        assert!(text.contains("Usage: sigy [OPTIONS] <COMMAND>"));
        assert!(text.contains("Exit codes"));
        assert!(text.contains("130 interrupted"));
        assert!(!text.contains("sigy radio search\n"));
        for line in text.lines() {
            assert!(visible_width(line) <= 60, "{line}");
        }
        let mut radio = super::Cli::command();
        let radio = radio.find_subcommand_mut("radio").ok_or("radio help")?;
        let mut radio_help = Vec::new();
        radio.write_help(&mut radio_help)?;
        let radio_text = String::from_utf8(radio_help)?;
        assert!(radio_text.contains("search"), "{radio_text}");
        Ok(())
    }

    fn visible_width(line: &str) -> usize {
        let mut count = 0usize;
        let mut chars = line.chars().peekable();
        while let Some(character) = chars.next() {
            if character == char::from(0x1b) {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                continue;
            }
            count += 1;
        }
        count
    }
}
