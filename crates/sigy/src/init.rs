//! Explicit first-use setup over existing library ownership and service operations.

use std::{
    ffi::OsString,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::Args;
use serde::Serialize;
use sigy_service::{
    control::{self, DirectoryOperation, Operation, Snapshot},
    discovery::{RefreshRequest, StationFilter},
    library::Library,
    sources::NetworkScope,
    storage::discovery::RefreshStatus,
};

use crate::{Cli, Command, LibraryCommand, service};

mod guidance;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The first steps shown after the top-level command list.
pub const GET_STARTED: &str = "Get started:
  sigy init --radio  Create your library, start its service and load one station page
  sigy tui           Open the explorer. Quitting leaves the service running
  sigy doctor        Check local setup without using the network

Commands use ~/.sigy/library unless you pass --data-dir PATH.
Guide: https://github.com/blisspixel/sigy/blob/main/docs/usage.md";
const RADIO_REQUEST: &str = "sigy-init-radio-v1";
const RADIO_WAIT: Duration = Duration::from_secs(15);

#[derive(Debug, Args)]
pub struct InitOptions {
    /// Explicitly fetch one page of up to 100 stations. Repeating init reuses that request.
    #[arg(long, conflicts_with = "no_start")]
    radio: bool,
    /// Seed the requested radio page with a country name or two-letter code.
    #[arg(
        long,
        requires = "radio",
        default_value = "",
        hide_default_value = true
    )]
    country: String,
    /// Seed the requested radio page with one whole directory tag, such as jazz.
    #[arg(
        long,
        requires = "radio",
        default_value = "",
        hide_default_value = true
    )]
    tag: String,
    /// Prepare or inspect storage without starting a service or fetching a directory page.
    #[arg(long)]
    no_start: bool,
}

#[derive(Serialize)]
struct InitReport {
    library: PathBuf,
    service_ready: bool,
    radio: Option<RefreshStatus>,
    snapshot: Snapshot,
}

pub fn resolve_directory(cli: &mut Cli) -> Result<()> {
    cli.explicit_data_dir = cli.data_dir.is_some();
    if !needs_library(&cli.command) {
        return Ok(());
    }
    if cli.data_dir.is_none() {
        if matches!(cli.command, Command::Mcp) {
            return Err("--data-dir is required for mcp".into());
        }
        #[cfg(windows)]
        let home = std::env::var_os("USERPROFILE");
        #[cfg(not(windows))]
        let home = std::env::var_os("HOME");
        cli.data_dir = Some(default_directory(home)?);
    }
    Ok(())
}

fn needs_library(command: &Command) -> bool {
    !matches!(
        command,
        Command::Update { .. }
            | Command::Radio {
                command: crate::radio::RadioCommand::Countries { .. }
            }
            | Command::Library {
                command: LibraryCommand::VerifyBackup { .. } | LibraryCommand::Restore { .. }
            }
    )
}

pub fn actionable_error(
    cli: &Cli,
    error: Box<dyn std::error::Error>,
) -> Box<dyn std::error::Error> {
    if !needs_library(&cli.command) {
        return error;
    }
    let missing_catalog = matches!(
        error.downcast_ref::<sigy_service::Error>(),
        Some(sigy_service::Error::InvalidInput(
            "library has not been initialized"
        ))
    );
    let missing_io = error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
        || matches!(
            error.downcast_ref::<sigy_service::Error>(),
            Some(sigy_service::Error::Io(error)) if error.kind() == io::ErrorKind::NotFound
        );
    let missing_directory = missing_io
        && cli.data_dir.as_deref().is_some_and(|directory| {
            matches!(std::fs::symlink_metadata(directory), Err(error) if error.kind() == io::ErrorKind::NotFound)
        });
    if missing_catalog || missing_directory {
        return "library has not been initialized; run `sigy init` with the same --data-dir if supplied".into();
    }
    guidance::explain(cli, error)
}

fn default_directory(home: Option<OsString>) -> Result<PathBuf> {
    let home = home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.parent().is_some());
    let home = home.ok_or(
        "a nonempty absolute user home directory is required; supply --data-dir PATH explicitly",
    )?;
    Ok(home.join(".sigy").join("library"))
}

pub async fn execute(
    directory: &Path,
    options: &InitOptions,
    json: bool,
    explicit: bool,
) -> Result<()> {
    if options.radio {
        radio_request(options)?;
    }
    let mut snapshot = match Library::open(directory, true) {
        Ok(mut library) => control::apply_library(&mut library, Operation::Status {})?,
        Err(sigy_service::Error::LibraryBusy) => {
            control::request(directory, Operation::Status {}).await?
        }
        Err(error) => return Err(error.into()),
    };
    if !options.no_start {
        snapshot = service::execute(directory, &service::ServiceCommand::Start)
            .await?
            .ok_or("service startup returned no status")?;
        if snapshot.service.as_ref().is_none_or(|view| view.stopping) {
            return Err(
                "service is stopping; wait for it to exit, then run `sigy init` again".into(),
            );
        }
    }
    if options.radio {
        snapshot = refresh_radio(directory, options).await?;
    }
    let report = InitReport {
        library: directory.canonicalize()?,
        service_ready: snapshot.service.as_ref().is_some_and(|view| !view.stopping),
        radio: if options.radio {
            snapshot.directory_refresh.clone()
        } else {
            None
        },
        snapshot,
    };
    render(&mut io::stdout().lock(), &report, options, json, explicit)?;
    radio_result(&report)
}

fn radio_result(report: &InitReport) -> Result<()> {
    if let Some(refresh) = &report.radio
        && matches!(refresh.state.as_str(), "failed" | "interrupted")
    {
        return Err(format!("library is ready, but the directory refresh did not complete; inspect `sigy radio refresh-status {}` with the same --data-dir if supplied", refresh.id).into());
    }
    Ok(())
}

fn radio_request(options: &InitOptions) -> Result<(String, StationFilter)> {
    use sha2::Digest;
    let filter = StationFilter {
        country: sigy_service::discovery::countries::resolve(&options.country)?,
        tag: options.tag.clone(),
        ..StationFilter::default()
    };
    filter.validate()?;
    let id = if filter == StationFilter::default() {
        RADIO_REQUEST.to_owned()
    } else {
        let hash = sha2::Sha256::digest(serde_json::to_vec(&filter)?);
        let mut digest = String::new();
        for byte in hash {
            use std::fmt::Write;
            write!(digest, "{byte:02x}")?;
        }
        format!("sigy-init-radio-v2-{digest}")
    };
    Ok((id, filter))
}

async fn refresh_radio(directory: &Path, options: &InitOptions) -> Result<Snapshot> {
    let (id, filter) = radio_request(options)?;
    let deadline = tokio::time::Instant::now() + RADIO_WAIT;
    let request = control::request(
        directory,
        DirectoryOperation::Refresh {
            id: id.clone(),
            request: RefreshRequest {
                filter,
                limit: 100,
                offset: 0,
                mirror: None,
                network: NetworkScope::PublicInternet {},
            },
        }
        .into(),
    );
    let mut snapshot = tokio::time::timeout_at(deadline, request)
        .await
        .map_err(|_| {
            format!(
                "directory request outcome is unknown; inspect `sigy radio refresh-status {id}`"
            )
        })??;
    loop {
        let refresh = snapshot
            .directory_refresh
            .as_ref()
            .ok_or("directory refresh returned no status")?;
        if refresh.state != "running" {
            return Ok(snapshot);
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(snapshot);
        }
        tokio::time::sleep_until(
            (tokio::time::Instant::now() + Duration::from_millis(100)).min(deadline),
        )
        .await;
        let request = control::request(
            directory,
            DirectoryOperation::RefreshStatus { id: id.clone() }.into(),
        );
        match tokio::time::timeout_at(deadline, request).await {
            Ok(result) => snapshot = result?,
            Err(_) => return Ok(snapshot),
        }
    }
}

/// A canonical Windows path carries a verbatim prefix that people do not type.
fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}");
    }
    match text.strip_prefix(r"\\?\") {
        Some(local) => local.to_owned(),
        None => text,
    }
}

fn render(
    stdout: &mut impl Write,
    report: &InitReport,
    options: &InitOptions,
    json: bool,
    explicit: bool,
) -> Result<()> {
    if json {
        let document = serde_json::to_vec(report)?;
        stdout.write_all(&document)?;
        writeln!(stdout)?;
        return Ok(());
    }
    writeln!(
        stdout,
        "Library ready: {}",
        crate::explorer::text::sanitize(&display_path(&report.library), 1024)
    )?;
    let command = if explicit {
        writeln!(
            stdout,
            "Replace PATH_TO_LIBRARY below with the library path above."
        )?;
        "sigy --data-dir PATH_TO_LIBRARY"
    } else {
        "sigy"
    };
    if let Some(service) = &report.snapshot.service {
        writeln!(
            stdout,
            "Service {}: {}.",
            service.process_id,
            if service.stopping {
                "stopping"
            } else {
                "running"
            }
        )?;
    } else {
        writeln!(
            stdout,
            "Service not started. Run `{command} service start` when ready."
        )?;
    }
    if let Some(budget) = report
        .snapshot
        .budgets
        .iter()
        .find(|budget| budget.scope == "global")
    {
        writeln!(
            stdout,
            "Lifetime paid allowance: ${}. Existing settings are preserved.",
            crate::explorer::text::sanitize(&budget.limit_usd, 64)
        )?;
    }
    if let Some(refresh) = &report.radio {
        writeln!(
            stdout,
            "Directory refresh: {}, {} stations accepted.",
            crate::explorer::text::sanitize(&refresh.state, 32),
            refresh.accepted
        )?;
        if refresh.state != "completed" {
            writeln!(
                stdout,
                "Check progress: {command} radio refresh-status {}",
                refresh.id
            )?;
        }
    } else if !options.no_start {
        writeln!(stdout, "Get a first station page: {command} init --radio")?;
    }
    if report.service_ready {
        writeln!(stdout, "Open the explorer: {command} tui")?;
    }
    if options.country.is_empty()
        && let Some(country) = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .find(|locale| !locale.is_empty())
            .and_then(|locale| locale_country(&locale))
    {
        writeln!(
            stdout,
            "Locale suggests {country}. Fetch only if you choose: {command} radio search --country {country} --fetch"
        )?;
    }
    writeln!(
        stdout,
        "Check local setup: {command} doctor. Recording and playback need your own FFmpeg."
    )?;
    Ok(())
}

fn locale_country(locale: &str) -> Option<String> {
    if locale.len() > 128 {
        return None;
    }
    let (_, territory) = locale.split_once('_')?;
    let territory = territory.split(['.', '@']).next()?;
    if territory.len() != 2 || !territory.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    sigy_service::discovery::countries::page(territory, "en", None)
        .ok()?
        .entries
        .into_iter()
        .find(|country| country.listed)
        .map(|country| country.code)
}

#[cfg(test)]
mod tests {
    use super::{
        InitOptions, InitReport, RADIO_REQUEST, default_directory, radio_result, refresh_radio,
        render, resolve_directory,
    };
    use crate::Cli;
    use clap::Parser;
    use sigy_service::{
        control::{self, Operation, Snapshot},
        discovery::{RefreshRequest, StationFilter},
        library::Library,
        sources::NetworkScope,
    };
    use std::{ffi::OsString, path::PathBuf, time::Duration};

    type Result = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn locale_country_is_only_a_valid_reference_suggestion() {
        assert_eq!(super::locale_country("en_US.UTF-8"), Some("US".into()));
        assert_eq!(super::locale_country("fr_CA@euro"), Some("CA".into()));
        for locale in [
            "C",
            "POSIX",
            "en",
            "en_ZZ",
            "en_US;touch",
            "en_123",
            "en_Canada",
        ] {
            assert_eq!(super::locale_country(locale), None, "{locale}");
        }
        assert_eq!(super::locale_country(&"x".repeat(129)), None);
    }

    #[tokio::test]
    async fn setup_seeds_bind_resolved_filters_and_reject_invalid_input_before_effects() -> Result {
        let mut options = InitOptions {
            radio: true,
            no_start: false,
            country: "Canada".into(),
            tag: "jazz".into(),
        };
        let first = super::radio_request(&options)?;
        options.country = "CA".into();
        assert_eq!(first, super::radio_request(&options)?);
        options.tag = "news".into();
        assert_ne!(first.0, super::radio_request(&options)?.0);
        options.country = "Congo".into();
        let temporary = tempfile::tempdir()?;
        let directory = temporary.path().join("not-created");
        assert!(
            super::execute(&directory, &options, true, true)
                .await
                .is_err()
        );
        assert!(!directory.exists());
        Ok(())
    }

    #[test]
    fn untrusted_default_bases_never_select_a_relative_or_root_library() {
        for home in [None, Some(OsString::new()), Some("relative home".into())] {
            assert!(default_directory(home).is_err());
        }
        #[cfg(windows)]
        let root = r"C:\";
        #[cfg(not(windows))]
        let root = "/";
        assert!(default_directory(Some(root.into())).is_err());
    }

    #[test]
    fn user_home_keeps_unicode_and_spaces() -> Result {
        let temporary = tempfile::tempdir()?;
        let home = temporary.path().join("private home français");
        assert_eq!(
            default_directory(Some(home.as_os_str().to_owned()))?,
            home.join(".sigy/library")
        );
        Ok(())
    }

    #[test]
    fn setup_flags_refuse_conflicts_and_allow_explicit_path_after_command() -> Result {
        assert!(Cli::try_parse_from(["sigy", "init", "--radio", "--no-start"]).is_err());
        let mut cli = Cli::try_parse_from(["sigy", "init", "--data-dir", "chosen", "--no-start"])?;
        resolve_directory(&mut cli)?;
        assert_eq!(cli.data_dir, Some(PathBuf::from("chosen")));
        let mut repeated =
            Cli::try_parse_from(["sigy", "--data-dir", "a", "init", "--data-dir", "b"])?;
        resolve_directory(&mut repeated)?;
        assert_eq!(repeated.data_dir, Some(PathBuf::from("b")));
        Ok(())
    }

    #[test]
    fn displayed_library_paths_omit_the_windows_verbatim_prefix() {
        for (canonical, shown) in [
            (
                r"\\?\C:\Users\me\.sigy\library",
                r"C:\Users\me\.sigy\library",
            ),
            (r"\\?\UNC\server\share\library", r"\\server\share\library"),
            ("/home/me/.sigy/library", "/home/me/.sigy/library"),
        ] {
            assert_eq!(super::display_path(&PathBuf::from(canonical)), shown);
        }
    }

    #[test]
    fn mcp_never_selects_the_user_default() -> Result {
        let mut cli = Cli::try_parse_from(["sigy", "mcp"])?;
        assert!(resolve_directory(&mut cli).is_err());
        Ok(())
    }

    fn seeded_library(
        state: &str,
    ) -> std::result::Result<(tempfile::TempDir, Library, Snapshot), Box<dyn std::error::Error>>
    {
        let temporary = tempfile::tempdir()?;
        let mut library = Library::open(temporary.path(), true)?;
        let request = RefreshRequest {
            filter: StationFilter::default(),
            limit: 100,
            offset: 0,
            mirror: None,
            network: NetworkScope::PublicInternet {},
        };
        let connection = rusqlite::Connection::open(temporary.path().join("catalog.sqlite3"))?;
        connection.execute(
            "INSERT INTO directory_refreshes (id, request_json, state, started_ms, completed_ms, accepted, skipped, failure) VALUES (?1, ?2, ?3, 1, ?4, 0, 0, ?5)",
            rusqlite::params![RADIO_REQUEST, serde_json::to_string(&request)?, state, if state == "running" { None } else { Some(2) }, if state == "failed" { Some("preserved fixture failure") } else { None }],
        )?;
        drop(connection);
        let snapshot = control::apply_library(&mut library, Operation::Status {})?;
        // Read the historical fixture through the canonical typed reader.
        assert_eq!(
            library.store().directory_refresh(RADIO_REQUEST)?.state,
            state
        );
        Ok((temporary, library, snapshot))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminal_first_page_requests_replay_without_new_admission() -> Result {
        for state in ["completed", "failed", "interrupted"] {
            Box::pin(replay_terminal(state)).await?;
        }
        Ok(())
    }

    async fn replay_terminal(state: &str) -> Result {
        let (temporary, library, _) = seeded_library(state)?;
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(control::run(library, async {
            let _ = stopped.await;
        }));
        let outcome = async {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if control::request(temporary.path(), Operation::Status {})
                        .await
                        .is_ok()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await?;
            let options = InitOptions {
                radio: true,
                no_start: false,
                country: String::new(),
                tag: String::new(),
            };
            let first = refresh_radio(temporary.path(), &options).await?;
            let second = refresh_radio(temporary.path(), &options).await?;
            Ok::<_, Box<dyn std::error::Error>>((first, second))
        }
        .await;
        let _ = shutdown.send(());
        tokio::time::timeout(Duration::from_secs(6), server).await???;
        let (first, second) = outcome?;
        assert_eq!(
            first
                .directory_refresh
                .as_ref()
                .ok_or("missing first refresh")?
                .state,
            state
        );
        assert_eq!(
            serde_json::to_value(&first.directory_refresh)?,
            serde_json::to_value(&second.directory_refresh)?
        );
        let connection = rusqlite::Connection::open(temporary.path().join("catalog.sqlite3"))?;
        let rows: i64 =
            connection.query_row("SELECT count(*) FROM directory_refreshes", [], |row| {
                row.get(0)
            })?;
        assert_eq!(rows, 1);
        assert_eq!(
            first
                .directory
                .as_ref()
                .ok_or("missing directory")?
                .cached_stations,
            0
        );
        Ok(())
    }

    #[test]
    fn radio_states_and_custom_commands_are_truthful() -> Result {
        let options = InitOptions {
            radio: true,
            no_start: false,
            country: String::new(),
            tag: String::new(),
        };
        for state in ["running", "completed", "failed", "interrupted"] {
            let (temporary, library, snapshot) = seeded_library(state)?;
            let report = InitReport {
                library: temporary.path().to_owned(),
                service_ready: false,
                radio: Some(library.store().directory_refresh(RADIO_REQUEST)?),
                snapshot,
            };
            let mut output = Vec::new();
            render(&mut output, &report, &options, false, true)?;
            let text = String::from_utf8(output)?;
            assert!(text.contains("sigy --data-dir PATH_TO_LIBRARY service start"));
            assert!(text.contains(&format!("Directory refresh: {state}, 0 stations accepted.")));
            assert_eq!(text.contains("Check progress:"), state != "completed");
            assert_eq!(
                radio_result(&report).is_err(),
                matches!(state, "failed" | "interrupted")
            );
            let mut document = Vec::new();
            render(&mut document, &report, &options, true, true)?;
            let parsed: serde_json::Value = serde_json::from_slice(&document)?;
            assert_eq!(parsed["radio"]["state"], state);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_json_paths_write_no_partial_document() -> Result {
        use std::os::unix::ffi::OsStringExt;
        let (_temporary, _library, snapshot) = seeded_library("completed")?;
        let report = InitReport {
            library: PathBuf::from(OsString::from_vec(b"/private/\xff".to_vec())),
            service_ready: false,
            radio: None,
            snapshot,
        };
        let mut output = Vec::new();
        assert!(
            render(
                &mut output,
                &report,
                &InitOptions {
                    radio: false,
                    no_start: true,
                    country: String::new(),
                    tag: String::new(),
                },
                true,
                false
            )
            .is_err()
        );
        assert_eq!(output, [] as [u8; 0]);
        Ok(())
    }
}
