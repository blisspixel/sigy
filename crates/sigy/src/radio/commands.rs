use std::{
    path::Path,
    time::{Duration, SystemTime},
};

use sigy_service::{
    control::{self, DirectoryOperation, Operation, Snapshot},
    discovery::RefreshRequest,
    library::Library,
    sources::NetworkScope,
};

use super::RadioCommand;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn request_id(prefix: &str) -> sigy_service::Result<String> {
    let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    Ok(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        time.as_nanos()
    ))
}

pub(crate) async fn execute(directory: &Path, command: &RadioCommand) -> Result<Snapshot> {
    let mut snapshot = apply(directory, command.operation()?).await?;
    match command {
        RadioCommand::Refresh { wait: true, .. } => wait_refresh(directory, snapshot).await,
        RadioCommand::Search {
            fetch: true,
            filters,
            query,
            limit,
            ..
        } => {
            let count = snapshot.station_page.as_ref().map_or_else(
                || {
                    snapshot
                        .ordered_station_page
                        .as_ref()
                        .map_or(0, |page| page.entries.len())
                },
                |page| page.entries.len(),
            );
            if count >= usize::try_from(*limit)? {
                return Ok(snapshot);
            }
            let refreshed = apply(
                directory,
                DirectoryOperation::Refresh {
                    id: request_id("search")?,
                    request: RefreshRequest {
                        filter: filters.search_filter(query.as_deref())?,
                        limit: 100,
                        offset: 0,
                        mirror: None,
                        network: NetworkScope::PublicInternet {},
                    },
                }
                .into(),
            )
            .await?;
            let refreshed = wait_refresh(directory, refreshed).await?;
            snapshot = apply(directory, command.operation()?).await?;
            snapshot.directory_refresh = refreshed.directory_refresh;
            Ok(snapshot)
        }
        _ => Ok(snapshot),
    }
}

pub(crate) async fn apply(directory: &Path, operation: Operation) -> Result<Snapshot> {
    match Library::open(directory, false) {
        Ok(mut library) => Ok(control::apply_library(&mut library, operation)?),
        Err(sigy_service::Error::LibraryBusy) => Ok(control::request(directory, operation).await?),
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn wait_refresh(directory: &Path, mut snapshot: Snapshot) -> Result<Snapshot> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = snapshot
            .directory_refresh
            .as_ref()
            .ok_or("refresh status missing")?;
        if status.state != "running" || tokio::time::Instant::now() >= deadline {
            return Ok(snapshot);
        }
        let operation = DirectoryOperation::RefreshStatus {
            id: status.id.clone(),
        }
        .into();
        tokio::time::sleep_until(
            (tokio::time::Instant::now() + Duration::from_millis(100)).min(deadline),
        )
        .await;
        match tokio::time::timeout_at(deadline, control::request(directory, operation)).await {
            Ok(result) => snapshot = result?,
            Err(_) => return Ok(snapshot),
        }
    }
}

pub(crate) fn write_next(
    writer: &mut impl std::io::Write,
    command: &RadioCommand,
    snapshot: &Snapshot,
    cli: &crate::Cli,
) -> Result<()> {
    if cli.json {
        return Ok(());
    }
    let directory = data_directory(cli)?;
    let mut wrote = false;
    if let Some(hint) = search_command(command, snapshot)? {
        write_command(writer, &hint, directory.as_deref())?;
        wrote = true;
    }
    if let Some(hint) = refresh_command(snapshot) {
        write_command(writer, &hint, directory.as_deref())?;
        wrote = true;
    }
    if !wrote && cache_needs_fetch(snapshot) {
        write_command(
            writer,
            &sigy_service::guidance::DirectoryHint::empty_fetch().command(),
            directory.as_deref(),
        )?;
    }
    Ok(())
}

pub(crate) fn write_explore_next(
    writer: &mut impl std::io::Write,
    filters: &super::Filters,
    favorites: bool,
    snapshot: &Snapshot,
    cli: &crate::Cli,
) -> Result<()> {
    if cli.json {
        return Ok(());
    }
    let Some(result) = snapshot.random_station.as_deref() else {
        return Ok(());
    };
    if result.station.is_some() {
        return Ok(());
    }
    let hint = sigy_service::guidance::DirectoryHint {
        filter: filters.search_filter(None)?,
        favorites,
        order: None,
        limit: None,
    };
    let directory = data_directory(cli)?;
    write_command(writer, &hint.command(), directory.as_deref())
}

fn search_command(command: &RadioCommand, snapshot: &Snapshot) -> Result<Option<String>> {
    let RadioCommand::Search {
        filters,
        query,
        favorites,
        limit,
        order,
        ..
    } = command
    else {
        return Ok(None);
    };
    let empty = snapshot
        .station_page
        .as_ref()
        .is_some_and(|page| page.entries.is_empty())
        || snapshot
            .ordered_station_page
            .as_ref()
            .is_some_and(|page| page.entries.is_empty());
    if !empty {
        return Ok(None);
    }
    let hint = sigy_service::guidance::DirectoryHint {
        filter: filters.search_filter(query.as_deref())?,
        favorites: *favorites,
        order: matches!(order, super::StationOrder::Name)
            .then_some(sigy_service::guidance::SearchOrder::Name),
        limit: (*limit != 16).then_some(*limit),
    };
    Ok(Some(hint.command()))
}

fn refresh_command(snapshot: &Snapshot) -> Option<String> {
    let refresh = snapshot.directory_refresh.as_ref()?;
    if refresh.state == "running" {
        return Some(sigy_service::guidance::refresh_status(&refresh.id));
    }
    if !matches!(refresh.state.as_str(), "failed" | "interrupted") {
        return None;
    }
    let request = &refresh.request;
    let pin = if let NetworkScope::PinnedAddress { address } = &request.network {
        Some(address.to_string())
    } else {
        None
    };
    Some(sigy_service::guidance::refresh_retry(
        &request.filter,
        request.limit,
        request.offset,
        request.mirror.as_deref(),
        pin.as_deref(),
    ))
}

fn cache_needs_fetch(snapshot: &Snapshot) -> bool {
    snapshot
        .directory
        .as_ref()
        .is_some_and(|status| status.cached_stations == 0 || status.stale_stations > 0)
}

fn data_directory(cli: &crate::Cli) -> Result<Option<String>> {
    if !cli.explicit_data_dir {
        return Ok(None);
    }
    Ok(Some(crate::library_dir(cli)?.display().to_string()))
}

fn write_command(
    writer: &mut impl std::io::Write,
    command: &str,
    directory: Option<&str>,
) -> Result<()> {
    writeln!(
        writer,
        "Next: {}",
        sigy_service::guidance::with_data_dir(command, directory)
    )?;
    Ok(())
}

pub(crate) fn quote(value: &str) -> String {
    sigy_service::guidance::shell_quote(&crate::explorer::text::sanitize(value, 4096))
}
