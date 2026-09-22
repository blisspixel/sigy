//! Read-only preflight. It does not refresh, delete, or contact a network.

use serde::{Deserialize, Serialize};

use super::Snapshot;
use crate::{
    Result,
    storage::{
        Store,
        discovery::{DIRECTORY_FRESH_MS, DirectoryStatus},
        dvr::DvrStatus,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorState {
    Ok,
    Attention,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DoctorCheck {
    pub name: String,
    pub state: DoctorState,
    pub detail: String,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    #[must_use]
    pub fn blocked(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.state == DoctorState::Blocked)
    }

    #[must_use]
    pub fn attention(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.state == DoctorState::Attention)
    }
}

pub(crate) fn decoder_is_file(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4_096
        && !path.chars().any(char::is_control)
        && std::path::Path::new(path).is_file()
}

pub(crate) fn apply(store: &Store) -> Result<Snapshot> {
    let mut view = super::snapshot(store)?;
    let directory = store.directory_status()?;
    let dvr = store.dvr_status()?;
    view.doctor = Some(inspect(store, &view, &directory, &dvr)?);
    view.directory = Some(directory);
    view.dvr = Some(dvr);
    Ok(view)
}

fn inspect(
    store: &Store,
    view: &Snapshot,
    directory: &DirectoryStatus,
    dvr: &DvrStatus,
) -> Result<DoctorReport> {
    let checks = vec![
        catalog(view, store.catalog_quick_check()?),
        provider(view.provider_dispatch_available),
        budgets(&view.budgets),
        decoder(dvr.decoder.as_ref()),
        quota(dvr),
        directory_cache(directory),
        directory_refresh(directory),
        podcasts(store)?,
        publisher_text(store)?,
        captures(view.captures.interrupted),
    ];
    Ok(DoctorReport { checks })
}

fn catalog(view: &Snapshot, intact: bool) -> DoctorCheck {
    if intact {
        check(
            "catalog",
            DoctorState::Ok,
            format!(
                "schema {} and SQLite {}",
                view.schema_version, view.sqlite_version
            ),
            None,
        )
    } else {
        check(
            "catalog",
            DoctorState::Blocked,
            "the catalog failed SQLite quick_check".into(),
            None,
        )
    }
}

fn provider(available: bool) -> DoctorCheck {
    if available {
        check(
            "provider",
            DoctorState::Blocked,
            "paid dispatch is available without a qualified provider".into(),
            None,
        )
    } else {
        check(
            "provider",
            DoctorState::Ok,
            "paid dispatch is off".into(),
            None,
        )
    }
}

fn budgets(budgets: &[super::BudgetView]) -> DoctorCheck {
    let Some(global) = budgets.iter().find(|budget| budget.scope == "global") else {
        return check(
            "budget",
            DoctorState::Blocked,
            "the global budget is missing".into(),
            Some("budget show".into()),
        );
    };
    if global.frozen {
        return check(
            "budget",
            DoctorState::Attention,
            "the global budget is frozen".into(),
            Some("budget show".into()),
        );
    }
    check(
        "budget",
        DoctorState::Ok,
        format!("global limit ${}", global.limit_usd),
        None,
    )
}

fn decoder(path: Option<&String>) -> DoctorCheck {
    let Some(path) = path else {
        return check(
            "decoder",
            DoctorState::Attention,
            "no FFmpeg decoder is configured. Recording and playback need one.".into(),
            Some("dvr configure --decoder PATH_TO_FFMPEG".into()),
        );
    };
    if decoder_is_file(path) {
        check(
            "decoder",
            DoctorState::Ok,
            "the configured decoder is a file".into(),
            None,
        )
    } else {
        check(
            "decoder",
            DoctorState::Blocked,
            "the configured decoder is not a file".into(),
            Some("dvr configure --decoder PATH_TO_FFMPEG".into()),
        )
    }
}

fn quota(dvr: &DvrStatus) -> DoctorCheck {
    if dvr.available_bytes == 0 {
        check(
            "quota",
            DoctorState::Attention,
            format!(
                "no quota remains. Retention is {} days. Kept and archived media stay.",
                dvr.retention_days
            ),
            Some("dvr status".into()),
        )
    } else {
        check(
            "quota",
            DoctorState::Ok,
            format!(
                "{} bytes available. Retention is {} days.",
                dvr.available_bytes, dvr.retention_days
            ),
            None,
        )
    }
}

fn directory_cache(directory: &DirectoryStatus) -> DoctorCheck {
    if directory.cached_stations == 0 {
        return check(
            "directory",
            DoctorState::Attention,
            "the station cache is empty. Favorites are kept when a refresh merges a page.".into(),
            Some("radio refresh NEW_ID --limit 100".into()),
        );
    }
    if directory.stale_stations > 0 {
        return check(
            "directory",
            DoctorState::Attention,
            format!(
                "{} of {} cached stations are older than 24 hours. They stay searchable. A refresh does not delete unseen stations or favorites.",
                directory.stale_stations, directory.cached_stations
            ),
            Some("radio refresh NEW_ID --limit 100".into()),
        );
    }
    check(
        "directory",
        DoctorState::Ok,
        format!(
            "{} cached stations are within 24 hours. This is a partial cache, not the world catalog. Favorites: {}.",
            directory.cached_stations, directory.favorite_stations
        ),
        None,
    )
}

fn directory_refresh(directory: &DirectoryStatus) -> DoctorCheck {
    let Some(refresh) = &directory.latest_refresh else {
        return check(
            "directory_refresh",
            DoctorState::Ok,
            "no directory refresh has run".into(),
            None,
        );
    };
    match refresh.state.as_str() {
        "running" => check(
            "directory_refresh",
            DoctorState::Attention,
            format!("refresh {} is already running", refresh.id),
            Some(format!("radio refresh-status {}", refresh.id)),
        ),
        "failed" | "interrupted" => check(
            "directory_refresh",
            DoctorState::Attention,
            format!(
                "refresh {} is {}. The previous cache was kept.",
                refresh.id, refresh.state
            ),
            Some("radio refresh NEW_ID --limit 100".into()),
        ),
        "completed" => check(
            "directory_refresh",
            DoctorState::Ok,
            format!(
                "refresh {} completed. Accepted {} and skipped {}.",
                refresh.id, refresh.accepted, refresh.skipped
            ),
            None,
        ),
        _ => check(
            "directory_refresh",
            DoctorState::Blocked,
            format!("refresh {} has state {}", refresh.id, refresh.state),
            None,
        ),
    }
}

fn podcasts(store: &Store) -> Result<DoctorCheck> {
    let health = store.podcast_cache_health(DIRECTORY_FRESH_MS)?;
    if health.subscriptions == 0 {
        return Ok(check(
            "podcasts",
            DoctorState::Ok,
            "no podcast subscriptions".into(),
            None,
        ));
    }
    if health.missing_snapshots > 0 || health.stale_snapshots > 0 {
        return Ok(check(
            "podcasts",
            DoctorState::Attention,
            format!(
                "{} subscriptions, {} without a snapshot, {} snapshots older than 24 hours. A refresh does not download enclosures.",
                health.subscriptions, health.missing_snapshots, health.stale_snapshots
            ),
            Some("podcast refresh SUBSCRIPTION --id NEW_ID".into()),
        ));
    }
    Ok(check(
        "podcasts",
        DoctorState::Ok,
        format!(
            "{} podcast snapshots are within 24 hours",
            health.subscriptions
        ),
        None,
    ))
}

fn publisher_text(store: &Store) -> Result<DoctorCheck> {
    let running = store.running_publisher_fetches()?;
    if running == 0 {
        Ok(check(
            "publisher_text",
            DoctorState::Ok,
            "no publisher text fetch is running".into(),
            None,
        ))
    } else {
        Ok(check(
            "publisher_text",
            DoctorState::Attention,
            "a publisher text fetch is already running. One fetch runs at a time.".into(),
            None,
        ))
    }
}

fn captures(interrupted: u64) -> DoctorCheck {
    if interrupted == 0 {
        check(
            "captures",
            DoctorState::Ok,
            "no interrupted capture is holding a reservation".into(),
            None,
        )
    } else {
        check(
            "captures",
            DoctorState::Attention,
            format!(
                "{interrupted} interrupted captures keep their reservation until the attempt is deleted"
            ),
            Some("record list".into()),
        )
    }
}

fn check(name: &str, state: DoctorState, detail: String, command: Option<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.to_owned(),
        state,
        detail,
        command,
    }
}

#[cfg(test)]
mod tests {
    use super::DoctorState;
    use crate::storage::Store;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn fresh_library_reports_attention_without_a_network_call() -> TestResult {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        let report = super::apply(&store)?.doctor.ok_or("missing doctor")?;
        if report.blocked() {
            return Err(format!("fresh library was blocked: {report:?}").into());
        }
        let names = report
            .checks
            .iter()
            .filter(|check| check.state == DoctorState::Attention)
            .map(|check| check.name.as_str())
            .collect::<Vec<_>>();
        if !names.contains(&"decoder") || !names.contains(&"directory") {
            return Err(format!("missing attention: {names:?}").into());
        }
        let directory = report
            .checks
            .iter()
            .find(|check| check.name == "directory")
            .ok_or("directory")?;
        if directory.command.as_deref() != Some("radio refresh NEW_ID --limit 100") {
            return Err("empty cache did not suggest a refresh".into());
        }
        Ok(())
    }

    #[test]
    fn stale_station_stays_cached_and_a_missing_decoder_blocks() -> TestResult {
        let root = tempfile::tempdir()?;
        let mut store = Store::open(&root.path().join("catalog.sqlite3"))?;
        store.configure_dvr(
            50_000_000_000,
            256 * 1024 * 1024,
            14,
            &std::env::current_exe()?.display().to_string(),
        )?;
        store.begin_refresh(
            "refresh:v1",
            &crate::discovery::RefreshRequest {
                filter: crate::discovery::StationFilter::default(),
                limit: 10,
                offset: 0,
                mirror: None,
                network: crate::sources::NetworkScope::PublicInternet {},
            },
        )?;
        let body = serde_json::json!([{
            "stationuuid": "12345678-1234-1234-1234-123456789abc",
            "name": "Local",
            "url": "https://radio.example/one",
            "countrycode": "CA",
            "language": "french",
            "tags": "news",
            "lastcheckok": 1
        }]);
        let batch = crate::discovery::radio_browser::parse(
            &serde_json::to_vec(&body)?,
            10,
            "https://directory.example".into(),
        )?;
        store.finish_refresh("refresh:v1", batch)?;
        store.set_cached_observation_age(1)?;
        let stale = super::apply(&store)?.doctor.ok_or("missing doctor")?;
        let directory = stale
            .checks
            .iter()
            .find(|check| check.name == "directory")
            .ok_or("directory")?;
        if directory.state != DoctorState::Attention || stale.blocked() {
            return Err(format!("stale cache was not attention: {directory:?}").into());
        }
        store.clear_cached_observation_time()?;
        let missing_time = super::apply(&store)?.doctor.ok_or("missing doctor")?;
        let missing_time_check = missing_time
            .checks
            .iter()
            .find(|check| check.name == "directory")
            .ok_or("directory")?;
        if missing_time_check.state != DoctorState::Attention || missing_time.blocked() {
            return Err(format!("undated cache was not attention: {missing_time_check:?}").into());
        }
        let missing = root.path().join("missing-ffmpeg");
        store.set_decoder_path(&missing.display().to_string())?;
        let blocked = super::apply(&store)?.doctor.ok_or("missing doctor")?;
        if !blocked.blocked() {
            return Err("a missing decoder file was not blocked".into());
        }
        Ok(())
    }
}
