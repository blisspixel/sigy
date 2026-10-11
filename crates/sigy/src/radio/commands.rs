use std::{
    fmt::Write as _,
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
    let base = if cli.explicit_data_dir {
        format!(
            "sigy --data-dir {}",
            quote(&crate::library_dir(cli)?.display().to_string())
        )
    } else {
        "sigy".to_owned()
    };
    if let RadioCommand::Search {
        filters,
        query,
        favorites,
        limit,
        order,
        ..
    } = command
    {
        let empty = snapshot
            .station_page
            .as_ref()
            .is_some_and(|page| page.entries.is_empty())
            || snapshot
                .ordered_station_page
                .as_ref()
                .is_some_and(|page| page.entries.is_empty());
        if empty {
            let filter = filters.search_filter(query.as_deref())?;
            let suffix = filter_arguments(&filter);
            if *favorites {
                writeln!(
                    writer,
                    "Fetch a directory page for these filters: {base} radio refresh{suffix} --wait"
                )?;
            } else {
                let order = match order {
                    super::StationOrder::Id => "id",
                    super::StationOrder::Name => "name",
                };
                writeln!(
                    writer,
                    "Fetch matches: {base} radio search{suffix} --limit {limit} --order {order} --fetch"
                )?;
            }
        }
    }
    if let Some(refresh) = &snapshot.directory_refresh {
        if refresh.state == "running" {
            writeln!(
                writer,
                "Refresh continues in the service. Inspect: {base} radio refresh-status {}",
                quote(&refresh.id)
            )?;
        } else if matches!(refresh.state.as_str(), "failed" | "interrupted") {
            let request = &refresh.request;
            let mut suffix = filter_arguments(&request.filter);
            if let Some(mirror) = &request.mirror {
                write!(suffix, " --mirror {}", quote(mirror))?;
            }
            if let NetworkScope::PinnedAddress { address } = request.network {
                write!(suffix, " --pin-address {address}")?;
            }
            writeln!(
                writer,
                "Retry with a new automatic ID: {base} radio refresh{suffix} --limit {} --offset {} --wait",
                request.limit, request.offset
            )?;
        }
    }
    Ok(())
}

fn filter_arguments(filter: &sigy_service::discovery::StationFilter) -> String {
    let mut arguments = String::new();
    for (flag, value) in [
        ("name", &filter.name),
        ("country", &filter.country),
        ("language", &filter.language),
        ("tag", &filter.tag),
    ] {
        if !value.is_empty() {
            let _ = write!(arguments, " --{flag} {}", quote(value));
        }
    }
    if filter.healthy_only {
        arguments.push_str(" --healthy");
    }
    arguments
}

pub(crate) fn quote(value: &str) -> String {
    let value = crate::explorer::text::sanitize(value, 4096);
    #[cfg(windows)]
    let value = value.replace('\'', "''");
    #[cfg(not(windows))]
    let value = value.replace('\'', "'\"'\"'");
    format!("'{value}'")
}
