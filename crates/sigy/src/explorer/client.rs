//! One catalog path: the open library, or the service when that library is busy.

use std::path::Path;

use sigy_service::{
    Error,
    control::{
        self, DirectoryOperation, DvrOperation, ListenView, MonitorOperation, MonitorPage,
        Operation, RecordingOperation, Snapshot,
    },
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
    query: &SearchQuery,
    observed_ms: i64,
) -> Result<Desk, Error> {
    let (via_status, status) = fetch(directory, Operation::Status {}).await?;
    let (via_radio, radio) = fetch(directory, radio_status()).await?;
    let (via_search, search) = fetch(directory, search_operation(query)).await?;
    let page = search
        .ordered_station_page
        .as_ref()
        .ok_or(Error::Protocol("ordered station page missing"))?;
    if search.directory_catalog.as_ref() != Some(&page.catalog) {
        return Err(Error::Protocol("ordered station catalog mismatch"));
    }
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
        Effect::LinkedContext {
            generation,
            id,
            catalog,
        } => {
            let operation = Operation::Radio {
                command: DirectoryOperation::Linked {
                    id: id.clone(),
                    catalog: Some(catalog.clone()),
                },
            };
            match fetch(directory, operation).await {
                Ok((_, snapshot)) => {
                    if snapshot.directory_catalog.as_ref() == Some(catalog)
                        && let Some(page) = snapshot.linked_station_context
                    {
                        model.apply_linked_context(*generation, id, catalog, page);
                    } else {
                        model.fail_linked_context(
                            *generation,
                            "missing or mismatched catalog response",
                        );
                    }
                }
                Err(error) => model.fail_linked_context(*generation, &error.to_string()),
            }
            Ok(())
        }
        Effect::Reload => Box::pin(reload(directory, model)).await,
        Effect::MonitorList => {
            apply_fetched(
                directory,
                model,
                monitor_operation(MonitorOperation::List {}),
                |model, snapshot| {
                    if let Some(MonitorPage::List { ids }) = snapshot.monitor.as_deref() {
                        model.monitors.load_ids(ids.clone());
                    }
                },
            )
            .await
        }
        Effect::MonitorDetail { id } => read_monitor(directory, model, id).await,
        Effect::Finding { monitor, finding } => {
            apply_fetched(
                directory,
                model,
                finding_operation(monitor, finding),
                |model, snapshot| {
                    if let Some(MonitorPage::Finding { finding: page }) =
                        snapshot.monitor.as_deref()
                        && model.findings.load(monitor, finding, page)
                    {
                        model.note_message(
                        "Stored citation read. Arrows scroll; o reads original recording metadata.",
                    );
                    } else {
                        model.note_message("Unexpected finding page; previous citation kept.");
                    }
                },
            )
            .await
        }
        Effect::FindingOriginal { recording } => {
            apply_fetched(
                directory,
                model,
                original_operation(recording),
                |model, snapshot| {
                    if let Some(page) = &snapshot.recording_page
                        && page.entries.len() == 1
                        && page.entries[0].id == *recording
                        && model.findings.load_original(&page.entries[0])
                    {
                        model.open_finding_recording(recording_from(&page.entries[0]));
                    } else {
                        model.note_message("Unexpected recording page; previous citation kept.");
                    }
                },
            )
            .await
        }
        Effect::Search(query) => Box::pin(search(directory, model, query)).await,
        Effect::SetFavorite {
            generation,
            id,
            favorite,
        } => set_favorite(directory, model, *generation, id, *favorite).await,
    }
}

pub fn operations_for(effect: &Effect, model: &Explorer) -> Vec<Operation> {
    match effect {
        Effect::None | Effect::Detach => Vec::new(),
        Effect::LinkedContext { id, catalog, .. } => vec![Operation::Radio {
            command: DirectoryOperation::Linked {
                id: id.clone(),
                catalog: Some(catalog.clone()),
            },
        }],
        Effect::Search(query) => vec![search_operation(query)],
        Effect::SetFavorite { id, favorite, .. } => vec![favorite_operation(id, *favorite)],
        Effect::MonitorList => vec![monitor_operation(MonitorOperation::List {})],
        Effect::MonitorDetail { id } => monitor_reads(id, model.now_ms()),
        Effect::Finding { monitor, finding } => vec![finding_operation(monitor, finding)],
        Effect::FindingOriginal { recording } => vec![original_operation(recording)],
        Effect::Reload => vec![
            Operation::Status {},
            radio_status(),
            search_operation(&model.current_search()),
            quota_operation(),
            recording_operation(),
        ],
    }
}

fn monitor_operation(command: MonitorOperation) -> Operation {
    Operation::Monitor { command }
}

fn finding_operation(monitor: &str, finding: &str) -> Operation {
    monitor_operation(MonitorOperation::ShowFinding {
        monitor: monitor.into(),
        finding: finding.into(),
    })
}

fn original_operation(recording: &str) -> Operation {
    Operation::Record {
        command: RecordingOperation::Show {
            id: recording.into(),
        },
    }
}

fn monitor_reads(id: &str, now: i64) -> Vec<Operation> {
    let to_ms = now.max(1);
    let from_ms = to_ms.saturating_sub(86_400_000).max(0);
    vec![
        monitor_operation(MonitorOperation::Show {
            id: id.to_owned(),
            version: None,
        }),
        monitor_operation(MonitorOperation::Coverage {
            id: id.to_owned(),
            from_ms,
            to_ms,
        }),
        monitor_operation(MonitorOperation::Matches {
            id: id.to_owned(),
            from_ms,
            to_ms,
        }),
    ]
}

async fn read_monitor(directory: &Path, model: &mut Explorer, id: &str) -> Result<(), Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::InvalidInput("system clock"))?;
    let now = i64::try_from(now.as_millis()).map_err(|_| Error::InvalidInput("system clock"))?;
    let mut monitor = None;
    let mut coverage = None;
    let mut matches = None;
    for operation in monitor_reads(id, now) {
        let (via_service, snapshot) = match fetch(directory, operation).await {
            Ok(result) => result,
            Err(error) => {
                report(model, &error);
                return Ok(());
            }
        };
        model.note_link(link_from(via_service, &snapshot));
        match snapshot.monitor.map(|page| *page) {
            Some(MonitorPage::Monitor { monitor: page, .. }) => monitor = Some(page),
            Some(MonitorPage::Coverage { coverage: page }) => coverage = Some(page),
            Some(MonitorPage::Matches { matches: page }) => matches = Some(page),
            _ => {
                model.note_message(
                    "The monitor read returned an unexpected page; previous snapshot kept.",
                );
                return Ok(());
            }
        }
    }
    if let (Some(monitor), Some(coverage), Some(matches)) = (monitor, coverage, matches) {
        if model.monitors.load_detail(&monitor, &coverage, &matches) {
            model.note_message(
                "Monitor snapshot loaded. Arrows scroll; Esc returns. No work was admitted.",
            );
        } else {
            model.note_message(
                "The monitor version changed during the read; previous snapshot kept. Enter retries.",
            );
        }
    }
    Ok(())
}

async fn reload(directory: &Path, model: &mut Explorer) -> Result<(), Error> {
    if let Ok(now) = super::now_ms() {
        model.tick_clock(now);
    }
    match Box::pin(load_desk(
        directory,
        &model.current_search(),
        model.now_ms(),
    ))
    .await
    {
        Ok(desk) => model.apply_desk(desk),
        Err(error) => report(model, &error),
    }
    Ok(())
}

async fn set_favorite(
    directory: &Path,
    model: &mut Explorer,
    generation: u64,
    id: &str,
    saved: bool,
) -> Result<(), Error> {
    match fetch(directory, favorite_operation(id, saved)).await {
        Ok((via_service, snapshot)) => {
            let Some(catalog) = &snapshot.directory_catalog else {
                let _ = model.fail_favorite(generation, "Favorite reply omitted catalog identity; previous rows kept. g rereads cached page 1.");
                return Ok(());
            };
            if model.apply_ordered_favorite(
                generation,
                id,
                saved,
                snapshot.directory.as_ref().map(directory_status),
                catalog,
            ) {
                model.note_link(link_from(via_service, &snapshot));
            }
        }
        Err(error) => {
            if model.fail_favorite(generation, &error.to_string()) {
                report(model, &error);
            }
        }
    }
    Ok(())
}

async fn search(directory: &Path, model: &mut Explorer, query: &SearchQuery) -> Result<(), Error> {
    match fetch(directory, search_operation(query)).await {
        Ok((via_service, snapshot)) => {
            let Some(page) = snapshot.ordered_station_page.as_ref() else {
                let _ = model.fail_search(
                    query.generation,
                    "The search returned no page; the previous list is kept.",
                );
                return Ok(());
            };
            if snapshot.directory_catalog.as_ref() != Some(&page.catalog) {
                let _ = model.fail_search(
                    query.generation,
                    "Catalog metadata disagrees; the previous list is kept.",
                );
                return Ok(());
            }
            if model.apply_ordered_search(
                query.generation,
                rows_from(page),
                directory_from(&snapshot, model),
                page.next_after.clone(),
                &page.catalog,
            ) {
                model.note_link(link_from(via_service, &snapshot));
            }
        }
        Err(error) => {
            if model.fail_search(query.generation, &error.to_string()) {
                report(model, &error);
            }
        }
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
    if message.contains("ordered station cursor scope changed") {
        model.note_cursor_changed();
        return;
    }
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
        .ordered_station_page
        .as_ref()
        .map(rows_from)
        .unwrap_or_default();
    let next_after = search
        .ordered_station_page
        .as_ref()
        .and_then(|page| page.next_after.clone());
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
        directory: search
            .directory
            .as_ref()
            .map(directory_status)
            .or_else(|| radio.directory.as_ref().map(directory_status)),
        stations,
        next_after,
        catalog: search
            .ordered_station_page
            .as_ref()
            .map(|page| page.catalog.clone()),
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

fn directory_from(snapshot: &Snapshot, model: &Explorer) -> DirectoryView {
    snapshot.directory.as_ref().map_or(
        model
            .directory()
            .filter(|_| {
                model
                    .directory_catalog()
                    .zip(snapshot.directory_catalog.as_ref())
                    .is_some_and(|(known, incoming)| {
                        known.namespace == incoming.namespace
                            && known.comparison == incoming.comparison
                    })
            })
            .cloned()
            .unwrap_or(DirectoryView {
                cached_stations: 0,
                maximum_stations: 0,
                favorite_stations: 0,
                refresh: None,
            }),
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

fn rows_from(page: &sigy_service::control::OrderedStationPage) -> Vec<StationRow> {
    page.entries
        .iter()
        .map(|station| StationRow {
            id: station.id.clone(),
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
            coordinates: match (station.latitude, station.longitude) {
                (Some(latitude), Some(longitude)) => Some(super::state::Coordinates {
                    latitude,
                    longitude,
                }),
                _ => None,
            },
            metadata: Some(station.clone()),
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
        id: record.id.clone(),
        state: sanitize(&record.state, 32),
        storage_state: sanitize(&record.storage_state, 32),
        format: record.format.as_deref().map(|format| sanitize(format, 16)),
        timeline: super::timeline::Timeline::from_recording(record),
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
        command: DirectoryOperation::SearchOrdered {
            filter: query.filter.clone(),
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
    use sigy_service::control::{
        DirectoryOperation, DvrOperation, MonitorOperation, Operation, RecordingOperation,
    };
    use sigy_service::discovery::StationFilter;

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
            | Operation::Retained { .. }
            | Operation::Playback { .. }
            | Operation::Podcast { .. }
            | Operation::Schedule { .. }
            | Operation::Analysis { .. }
            | Operation::Provider { .. }
            | Operation::SetBudget { .. }
            | Operation::RegisterSource { .. }
            | Operation::Playlist { .. }
            | Operation::Task { .. } => true,
            Operation::Monitor { command } => !matches!(
                command,
                MonitorOperation::List {}
                    | MonitorOperation::Show { .. }
                    | MonitorOperation::Coverage { .. }
                    | MonitorOperation::Matches { .. }
                    | MonitorOperation::ShowFinding { .. }
            ),
            Operation::Record { command } => !matches!(
                command,
                RecordingOperation::List { .. } | RecordingOperation::Show { .. }
            ),
            Operation::Radio { command } => !matches!(
                command,
                DirectoryOperation::Status {}
                    | DirectoryOperation::Linked { .. }
                    | DirectoryOperation::SearchOrdered { .. }
                    | DirectoryOperation::SetFavorite { .. }
            ),
            Operation::Dvr { command } => !matches!(command, DvrOperation::Status {}),
            Operation::Status {}
            | Operation::ListSources { .. }
            | Operation::ShowSource { .. }
            | Operation::Doctor {} => false,
        }
    }

    #[test]
    fn explorer_effects_use_existing_reads_and_favorite_only() {
        let explorer = model();
        let search = Effect::Search(SearchQuery {
            generation: 2,
            filter: StationFilter {
                name: "navajo".into(),
                ..StationFilter::default()
            },
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
            Effect::MonitorList,
            Effect::MonitorDetail { id: "news".into() },
            Effect::Finding {
                monitor: "news".into(),
                finding: "one".into(),
            },
            Effect::FindingOriginal {
                recording: "rec".into(),
            },
            Effect::LinkedContext {
                generation: 1,
                id: "00000000-0000-0000-0000-000000000001".into(),
                catalog: sigy_service::control::DirectoryCatalog {
                    namespace: "a".repeat(32),
                    revision: 1,
                    comparison: "fixture".into(),
                },
            },
        ] {
            for operation in operations_for(&effect, &explorer) {
                assert!(!forbidden(&operation), "{operation:?}");
            }
        }
        assert!(matches!(
            search_operation(&SearchQuery {
                generation: 1,
                filter: StationFilter {
                    name: "navajo".into(),
                    ..StationFilter::default()
                },
                favorites_only: false,
                after: None,
            }),
            Operation::Radio {
                command: DirectoryOperation::SearchOrdered { .. }
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

    #[test]
    fn reload_reads_the_page_on_screen_with_the_existing_cursor() {
        use crate::explorer::state::{DirectoryView, Key, Link};
        let mut explorer = model();
        explorer.note_link(Link::LocalCatalog);
        let directory = DirectoryView {
            cached_stations: 40,
            maximum_stations: 10_000,
            favorite_stations: 0,
            refresh: None,
        };
        for after in [None, Some("cursor-1")] {
            let Effect::Search(query) = explorer.handle(if after.is_none() {
                Key::Char('v')
            } else {
                Key::Char('n')
            }) else {
                panic!("a page request is a search");
            };
            assert!(explorer.apply_search(
                query.generation,
                Vec::new(),
                directory.clone(),
                Some("cursor-1".into())
            ));
        }
        let reload = operations_for(&Effect::Reload, &explorer);
        assert!(reload.iter().any(|operation| matches!(
            operation,
            Operation::Radio {
                command: DirectoryOperation::SearchOrdered { after: Some(after), favorites_only: true, .. }
            } if after == "cursor-1"
        )));
        assert!(reload.iter().all(|operation| !forbidden(operation)));
    }

    #[tokio::test]
    async fn replacement_catalog_cannot_inherit_previous_directory_statistics()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        drop(sigy_service::library::Library::open(
            directory.path(),
            true,
        )?);
        let mut explorer = model();
        explorer.apply_desk(
            Box::pin(super::load_desk(
                directory.path(),
                &explorer.current_search(),
                0,
            ))
            .await?,
        );
        let (_, mut snapshot) = super::fetch(
            directory.path(),
            super::search_operation(&explorer.current_search()),
        )
        .await?;
        assert!(snapshot.directory.is_none());
        assert_eq!(
            super::directory_from(&snapshot, &explorer).maximum_stations,
            10_000
        );
        let Some(catalog) = &mut snapshot.directory_catalog else {
            panic!("catalog");
        };
        catalog.namespace = if catalog.namespace == "a".repeat(32) {
            "b".repeat(32)
        } else {
            "a".repeat(32)
        };
        assert_eq!(
            super::directory_from(&snapshot, &explorer).maximum_stations,
            0
        );
        snapshot.directory_catalog = None;
        assert_eq!(
            super::directory_from(&snapshot, &explorer).maximum_stations,
            0
        );
        Ok(())
    }

    #[tokio::test]
    async fn actual_cache_reads_apply_filters_and_validation_failure_ends_pending_state()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::explorer::state::{Key, Link};
        let directory = tempfile::tempdir()?;
        drop(sigy_service::library::Library::open(
            directory.path(),
            true,
        )?);
        let mut explorer = model();
        explorer.note_link(Link::LocalCatalog);
        let desk = Box::pin(super::load_desk(
            directory.path(),
            &explorer.current_search(),
            0,
        ))
        .await?;
        explorer.apply_desk(desk);
        let previous_directory = explorer.directory().cloned();
        explorer.handle(Key::Char('F'));
        explorer.handle(Key::Tab);
        explorer.handle(Key::Paste("ca".into()));
        let Effect::Search(query) = explorer.handle(Key::Enter) else {
            panic!("valid filters read cache");
        };
        super::perform(directory.path(), &mut explorer, &Effect::Search(query)).await?;
        assert_eq!(explorer.current_search().filter.country, "CA");
        assert_eq!(explorer.directory().cloned(), previous_directory);
        assert!(!explorer.search.pending());
        explorer.handle(Key::ClearInput);
        explorer.handle(Key::Paste("fr".into()));
        let Effect::Search(mut invalid) = explorer.handle(Key::Enter) else {
            panic!("next valid draft reads");
        };
        invalid.filter.country = "France".into();
        super::perform(directory.path(), &mut explorer, &Effect::Search(invalid)).await?;
        assert_eq!(explorer.current_search().filter.country, "CA");
        assert_eq!(explorer.search.draft.filter.country, "fr");
        assert!(!explorer.search.pending());
        assert!(explorer.search.failed);
        assert_eq!(explorer.link(), &Link::LocalCatalog);
        let (_, status) = super::fetch(directory.path(), Operation::Status {}).await?;
        assert_eq!(status.captures.active, 0);
        assert_eq!(status.captures.scheduled, 0);
        assert!(explorer.playback().is_none());
        Ok(())
    }

    #[tokio::test]
    async fn failed_finding_reads_preserve_citation_and_admit_nothing()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        drop(sigy_service::library::Library::open(
            directory.path(),
            true,
        )?);
        let mut explorer = model();
        let finding = crate::explorer::finding::fixture();
        assert!(explorer.findings.load("world-news", "one", &finding));
        for effect in [
            Effect::Finding {
                monitor: "missing".into(),
                finding: "missing".into(),
            },
            Effect::FindingOriginal {
                recording: "recording-one".into(),
            },
        ] {
            super::perform(directory.path(), &mut explorer, &effect).await?;
            assert!(
                explorer
                    .findings
                    .lines(20, 132, 0)
                    .join("\n")
                    .contains("world-news / one")
            );
        }
        let (_, status) = super::fetch(directory.path(), Operation::Status {}).await?;
        assert_eq!(status.captures.active, 0);
        assert_eq!(status.captures.scheduled, 0);
        assert!(explorer.playback().is_none());
        assert!(
            explorer
                .findings
                .lines(20, 132, 0)
                .join("\n")
                .contains("Current media unread")
        );
        Ok(())
    }
}
