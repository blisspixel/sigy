//! One catalog path: the open library, or the service when that library is busy.

use std::path::Path;

use sigy_service::{
    Error,
    control::{
        self, DirectoryOperation, DvrOperation, ListenView, Operation, RecordingOperation, Snapshot,
    },
    discovery::StationFilter,
    library::Library,
    storage::dvr::DvrStatus,
};

use crate::explorer::state::{
    BudgetLine, Desk, DirectoryView, Effect, Explorer, Health, Link, PlaybackView, QuotaView,
    RecordingLine, RefreshView, SearchQuery, StationRow,
};
use crate::explorer::text::sanitize;

const PAGE: u32 = Explorer::page_limit();

pub async fn load_desk(
    directory: &Path,
    name: &str,
    favorites_only: bool,
    observed_ms: i64,
) -> Result<Desk, Error> {
    let query = SearchQuery {
        generation: 0,
        name: name.to_owned(),
        favorites_only,
        after: None,
    };
    let (via_status, status) = fetch(directory, Operation::Status {}).await?;
    let (via_radio, radio) = fetch(directory, radio_status()).await?;
    let (via_search, search) = fetch(directory, search_operation(&query)).await?;
    let (via_quota, quota) = fetch(directory, quota_operation()).await?;
    let (via_records, records) = fetch(directory, recording_operation()).await?;
    Ok(assemble(
        via_status || via_radio || via_search || via_quota || via_records,
        observed_ms,
        &status,
        &radio,
        &search,
        &quota,
        &records,
    ))
}

pub async fn perform(directory: &Path, model: &mut Explorer, effect: &Effect) -> Result<(), Error> {
    match effect {
        Effect::None | Effect::Detach => Ok(()),
        Effect::Reload => reload(directory, model).await,
        Effect::Search(query) => {
            let operation = search_operation(query);
            let generation = query.generation;
            apply_fetched(directory, model, operation, |model, snapshot| {
                let Some(page) = snapshot.station_page.as_ref() else {
                    model.note_message("search returned no page");
                    return;
                };
                let rows = rows_from(page);
                let directory_view = directory_from(snapshot);
                if !model.apply_search(generation, rows, directory_view) {
                    model.note_message("stale search ignored");
                }
            })
            .await
        }
        Effect::SetFavorite {
            generation,
            id,
            favorite,
        } => {
            let operation = favorite_operation(id, *favorite);
            let generation = *generation;
            let id = id.clone();
            let favorite = *favorite;
            apply_fetched(directory, model, operation, move |model, snapshot| {
                let directory_view = snapshot.directory.as_ref().map(directory_status);
                if !model.apply_favorite(generation, &id, favorite, directory_view) {
                    model.note_message("stale favorite ignored");
                }
            })
            .await
        }
    }
}

pub fn operations_for(effect: &Effect, model: &Explorer) -> Vec<Operation> {
    match effect {
        Effect::None | Effect::Detach => Vec::new(),
        Effect::Search(query) => vec![search_operation(query)],
        Effect::SetFavorite { id, favorite, .. } => vec![favorite_operation(id, *favorite)],
        Effect::Reload => vec![
            Operation::Status {},
            radio_status(),
            search_operation(&SearchQuery {
                generation: 0,
                name: model.query().to_owned(),
                favorites_only: model.favorites_only(),
                after: None,
            }),
            quota_operation(),
            recording_operation(),
        ],
    }
}

async fn reload(directory: &Path, model: &mut Explorer) -> Result<(), Error> {
    match load_desk(
        directory,
        model.query(),
        model.favorites_only(),
        model.now_ms(),
    )
    .await
    {
        Ok(desk) => model.apply_desk(desk),
        Err(error) => report(model, &error),
    }
    Ok(())
}

async fn apply_fetched<F>(
    directory: &Path,
    model: &mut Explorer,
    operation: Operation,
    apply: F,
) -> Result<(), Error>
where
    F: FnOnce(&mut Explorer, &Snapshot),
{
    match fetch(directory, operation).await {
        Ok((via_service, snapshot)) => {
            model.note_link(link_from(via_service, &snapshot));
            apply(model, &snapshot);
        }
        Err(error) => report(model, &error),
    }
    Ok(())
}

async fn fetch(directory: &Path, operation: Operation) -> Result<(bool, Snapshot), Error> {
    match Library::open(directory, false) {
        Ok(mut library) => {
            let snapshot = control::apply_library(&mut library, operation)?;
            drop(library);
            Ok((false, snapshot))
        }
        Err(Error::LibraryBusy) => Ok((true, control::request(directory, operation).await?)),
        Err(error) => Err(error),
    }
}

fn report(model: &mut Explorer, error: &Error) {
    let message = error.to_string();
    if lost(error) {
        model.note_disconnect(model.now_ms(), &message);
    } else {
        model.note_message(&message);
    }
}

fn lost(error: &Error) -> bool {
    matches!(
        error,
        Error::Io(_) | Error::Timeout | Error::ServiceStopped | Error::Protocol(_) | Error::Json(_)
    )
}

fn assemble(
    via_service: bool,
    observed_ms: i64,
    status: &Snapshot,
    radio: &Snapshot,
    search: &Snapshot,
    quota: &Snapshot,
    records: &Snapshot,
) -> Desk {
    let stations = search
        .station_page
        .as_ref()
        .map(rows_from)
        .unwrap_or_default();
    Desk {
        link: link_from(via_service, records)
            .service()
            .or_else(|| link_from(via_service, search).service())
            .or_else(|| link_from(via_service, status).service())
            .unwrap_or(if via_service {
                Link::Disconnected
            } else {
                Link::LocalCatalog
            }),
        observed_ms,
        schema_version: status.schema_version,
        sqlite_version: status.sqlite_version.clone(),
        provider_dispatch_available: status.provider_dispatch_available,
        budgets: status
            .budgets
            .iter()
            .map(|budget| BudgetLine {
                scope: sanitize(&budget.scope, 64),
                limit_usd: budget.limit_usd.clone(),
                settled_usd: budget.settled_usd.clone(),
                reserved_usd: budget.reserved_usd.clone(),
                available_usd: budget.available_usd.clone(),
                frozen: budget.frozen,
            })
            .collect(),
        captures_active: status.captures.active,
        captures_scheduled: status.captures.scheduled,
        captures_interrupted: status.captures.interrupted,
        captures_terminal: status.captures.terminal,
        dispatch_available: status.captures.dispatch_available,
        directory: radio
            .directory
            .as_ref()
            .map(directory_status)
            .or_else(|| search.directory.as_ref().map(directory_status)),
        stations,
        quota: quota.dvr.as_ref().map(quota_from),
        recordings: records
            .recording_page
            .as_ref()
            .map(|page| page.entries.iter().map(recording_from).collect())
            .unwrap_or_default(),
        playback: records
            .listen
            .as_ref()
            .or(search.listen.as_ref())
            .map(playback_from),
    }
}

fn link_from(via_service: bool, snapshot: &Snapshot) -> Link {
    if let Some(service) = &snapshot.service {
        Link::Service {
            process_id: service.process_id,
            stopping: service.stopping,
        }
    } else if via_service {
        Link::Disconnected
    } else {
        Link::LocalCatalog
    }
}

impl Link {
    fn service(self) -> Option<Self> {
        match self {
            Self::Service { .. } => Some(self),
            Self::LocalCatalog | Self::Disconnected => None,
        }
    }
}

fn directory_from(snapshot: &Snapshot) -> DirectoryView {
    snapshot.directory.as_ref().map_or(
        DirectoryView {
            cached_stations: 0,
            maximum_stations: 0,
            favorite_stations: 0,
            refresh: None,
        },
        directory_status,
    )
}

fn directory_status(status: &sigy_service::storage::discovery::DirectoryStatus) -> DirectoryView {
    let refresh = status.latest_refresh.as_ref().map(|refresh| RefreshView {
        id: sanitize(&refresh.id, 64),
        state: sanitize(&refresh.state, 32),
        accepted: refresh.accepted,
        skipped: refresh.skipped,
        failure: refresh
            .failure
            .as_deref()
            .map(|detail| sanitize(detail, 120)),
    });
    DirectoryView {
        cached_stations: status.cached_stations,
        maximum_stations: status.maximum_stations,
        favorite_stations: status.favorite_stations,
        refresh,
    }
}

fn rows_from(page: &sigy_service::control::StationPage) -> Vec<StationRow> {
    page.entries
        .iter()
        .map(|station| StationRow {
            id: sanitize(&station.id, 64),
            name: sanitize(&station.name, 80),
            favorite: page.favorite_ids.iter().any(|id| id == &station.id),
            directory_health: match station.last_check_ok {
                Some(true) => Health::Succeeded,
                Some(false) => Health::Failed,
                None => Health::Unknown,
            },
            directory_languages: sanitize(&station.languages.join(", "), 80),
            observed_ms: station.observed_ms,
            hls: station.hls,
        })
        .collect()
}

fn quota_from(status: &DvrStatus) -> QuotaView {
    QuotaView {
        quota: status.quota_bytes,
        charged: status.charged_bytes,
        reserved: status.reserved_bytes,
        available: status.available_bytes,
    }
}

fn recording_from(record: &sigy_service::storage::dvr::Recording) -> RecordingLine {
    RecordingLine {
        id: sanitize(&record.id, 64),
        state: sanitize(&record.state, 32),
        storage_state: sanitize(&record.storage_state, 32),
        format: record.format.as_deref().map(|format| sanitize(format, 16)),
    }
}

fn playback_from(listen: &ListenView) -> PlaybackView {
    PlaybackView {
        id: sanitize(&listen.id, 64),
        state: sanitize(&listen.state, 32),
        format: listen.format.as_deref().map(|format| sanitize(format, 16)),
        source_revision: sanitize(&listen.source_revision, 64),
        failure: listen
            .failure
            .as_deref()
            .map(|detail| sanitize(detail, 120)),
    }
}

fn radio_status() -> Operation {
    Operation::Radio {
        command: DirectoryOperation::Status {},
    }
}

fn search_operation(query: &SearchQuery) -> Operation {
    Operation::Radio {
        command: DirectoryOperation::Search {
            filter: StationFilter {
                name: query.name.clone(),
                country: String::new(),
                language: String::new(),
                tag: String::new(),
                healthy_only: false,
            },
            favorites_only: query.favorites_only,
            after: query.after.clone(),
            limit: PAGE,
        },
    }
}

fn favorite_operation(id: &str, favorite: bool) -> Operation {
    Operation::Radio {
        command: DirectoryOperation::SetFavorite {
            id: id.to_owned(),
            favorite,
        },
    }
}

fn quota_operation() -> Operation {
    Operation::Dvr {
        command: DvrOperation::Status {},
    }
}

fn recording_operation() -> Operation {
    Operation::Record {
        command: RecordingOperation::List {
            after: None,
            limit: PAGE,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{favorite_operation, operations_for, search_operation};
    use crate::explorer::state::{Effect, Explorer, Modes, SearchQuery};
    use sigy_service::control::{DirectoryOperation, Operation, RecordingOperation};

    fn model() -> Explorer {
        Explorer::new(
            Modes {
                reduced_motion: true,
                linear: true,
                monochrome: true,
            },
            0,
        )
    }

    fn forbidden(operation: &Operation) -> bool {
        match operation {
            Operation::Stop {}
            | Operation::Listen { .. }
            | Operation::Playback { .. }
            | Operation::Podcast { .. }
            | Operation::Schedule { .. }
            | Operation::Analysis { .. }
            | Operation::Provider { .. } => true,
            Operation::Record { command } => !matches!(
                command,
                RecordingOperation::List { .. } | RecordingOperation::Show { .. }
            ),
            Operation::Radio { command } => matches!(
                command,
                DirectoryOperation::Refresh { .. }
                    | DirectoryOperation::Click { .. }
                    | DirectoryOperation::SetPolicy { .. }
                    | DirectoryOperation::ClearPolicy { .. }
            ),
            Operation::Status {}
            | Operation::SetBudget { .. }
            | Operation::RegisterSource { .. }
            | Operation::ListSources { .. }
            | Operation::ShowSource { .. }
            | Operation::Playlist { .. }
            | Operation::Dvr { .. }
            | Operation::Doctor {} => false,
        }
    }

    #[test]
    fn explorer_effects_use_existing_reads_and_favorite_only() {
        let explorer = model();
        let search = Effect::Search(SearchQuery {
            generation: 2,
            name: "navajo".into(),
            favorites_only: true,
            after: None,
        });
        let favorite = Effect::SetFavorite {
            generation: 1,
            id: "station".into(),
            favorite: true,
        };
        for effect in [
            Effect::None,
            Effect::Detach,
            search,
            favorite,
            Effect::Reload,
        ] {
            for operation in operations_for(&effect, &explorer) {
                assert!(!forbidden(&operation), "{operation:?}");
            }
        }
        assert!(matches!(
            search_operation(&SearchQuery {
                generation: 1,
                name: "navajo".into(),
                favorites_only: false,
                after: None,
            }),
            Operation::Radio {
                command: DirectoryOperation::Search { .. }
            }
        ));
        assert!(matches!(
            favorite_operation("station", false),
            Operation::Radio {
                command: DirectoryOperation::SetFavorite {
                    favorite: false,
                    ..
                }
            }
        ));
    }
}
