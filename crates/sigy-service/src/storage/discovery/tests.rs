use super::*;
use crate::discovery::radio_browser;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn request() -> RefreshRequest {
    RefreshRequest {
        filter: StationFilter::default(),
        limit: 10,
        offset: 0,
        mirror: None,
        network: NetworkScope::PublicInternet {},
    }
}

fn batch(name: &str, url: &str) -> Result<RefreshBatch> {
    let body = serde_json::json!([{"stationuuid":"12345678-1234-1234-1234-123456789abc", "name":name, "url":url, "countrycode":"CA", "language":"french,english", "tags":"news,économie", "lastcheckok":1}]);
    radio_browser::parse(
        &serde_json::to_vec(&body)?,
        10,
        "https://directory.example".into(),
    )
}

#[test]
fn refresh_is_atomic_searchable_and_does_not_rewrite_registered_sources() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    assert!(store.begin_refresh_at("one", &request(), 1)?);
    store.finish_refresh("one", batch("Radio Québec", "https://radio.example/one")?)?;
    assert!(!store.begin_refresh_at("one", &request(), 3000)?);
    let query = StationFilter {
        name: "QUÉBEC".into(),
        language: "FRENCH".into(),
        tag: "ÉCONOMIE".into(),
        healthy_only: true,
        ..StationFilter::default()
    };
    let stations = store.search_stations(&query, false, None, 16)?;
    assert_eq!(stations.len(), 1);
    let id = &stations[0].id;
    store.add_station_source(id, "radio:v1", RedirectPolicy::Deny)?;
    assert!(store.begin_refresh_at("two", &request(), 3000)?);
    store.finish_refresh("two", batch("Radio Québec", "https://radio.example/two")?)?;
    assert_eq!(
        store
            .source("radio:v1")?
            .ok_or("source missing")?
            .source
            .endpoint(),
        "https://radio.example/one"
    );
    assert!(matches!(
        store.add_station_source(id, "radio:v1", RedirectPolicy::Deny),
        Err(Error::IdempotencyConflict)
    ));
    assert!(
        store
            .add_station_source(id, "radio:v2", RedirectPolicy::Public)?
            .newly_created
    );
    assert_eq!(store.directory_status()?.cached_stations, 1);
    Ok(())
}

#[test]
fn failure_restart_and_replay_preserve_cache_without_redispatch() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.begin_refresh_at("one", &request(), 1)?;
    store.finish_refresh("one", batch("Station", "https://radio.example/audio")?)?;
    store.begin_refresh_at("two", &request(), 3000)?;
    store.fail_refresh("two", &Error::Acquisition("fixture failure"))?;
    assert_eq!(store.directory_status()?.cached_stations, 1);
    store.begin_refresh_at("three", &request(), 6000)?;
    drop(store);
    let mut store = Store::open(&path)?;
    store.recover_directory_refreshes()?;
    assert_eq!(store.directory_refresh("three")?.state, "interrupted");
    assert!(!store.begin_refresh_at("three", &request(), 9000)?);
    assert_eq!(store.directory_status()?.cached_stations, 1);
    let changed = RefreshRequest {
        offset: 1,
        ..request()
    };
    assert!(matches!(
        store.begin_refresh_at("three", &changed, 9000),
        Err(Error::IdempotencyConflict)
    ));
    Ok(())
}

#[test]
fn malformed_candidates_are_counted_and_duplicate_identity_is_rejected() -> TestResult {
    assert_eq!(batch("bad\u{1b}[2J", "https://radio.example/")?.skipped, 1);
    assert_eq!(batch("Local", "http://127.0.0.1/")?.skipped, 1);
    let value = serde_json::json!({"stationuuid":"12345678-1234-1234-1234-123456789abc", "name":"Good", "url":"https://radio.example/"});
    for field in ["language", "languagecodes", "tags"] {
        for (labels, rejected) in [
            (vec!["a"; 32].join(","), 0),
            (vec!["a"; 33].join(","), 1),
            ("a".repeat(129), 1),
        ] {
            let mut row = value.clone();
            row[field] = labels.into();
            let parsed = radio_browser::parse(
                &serde_json::to_vec(&vec![row])?,
                1,
                "https://directory.example".into(),
            )?;
            assert_eq!(parsed.skipped, rejected);
            assert_eq!(parsed.candidates.len(), usize::try_from(1 - rejected)?);
        }
    }
    let body = serde_json::to_vec(&vec![value.clone(), value])?;
    assert!(radio_browser::parse(&body, 2, "https://directory.example".into()).is_err());
    assert!(radio_browser::parse(&body, 1, "https://directory.example".into()).is_err());
    Ok(())
}

#[test]
fn page_write_failure_rolls_back_station_updates_and_completion() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.begin_refresh_at("one", &request(), 1)?;
    store.finish_refresh("one", batch("Original", "https://radio.example/one")?)?;
    store.begin_refresh_at("two", &request(), 3000)?;
    store.connection.execute_batch("CREATE TRIGGER inject_directory_failure AFTER UPDATE ON directory_stations BEGIN SELECT RAISE(ABORT, 'fixture'); END;")?;
    assert!(
        store
            .finish_refresh("two", batch("Changed", "https://radio.example/two")?)
            .is_err()
    );
    assert_eq!(store.directory_refresh("two")?.state, "running");
    assert_eq!(
        store.search_stations(&StationFilter::default(), false, None, 16)?[0].name,
        "Original"
    );
    store.fail_refresh("two", &Error::SourceIntegrity)?;
    assert_eq!(store.directory_refresh("two")?.state, "failed");
    Ok(())
}

#[test]
fn discovery_migration_preserves_v4_and_rolls_back_conflicts() -> TestResult {
    let directory = tempfile::tempdir()?;
    for fail in [false, true] {
        let path = directory
            .path()
            .join(if fail { "bad.sqlite3" } else { "good.sqlite3" });
        let connection = rusqlite::Connection::open(&path)?;
        for migration in [
            include_str!("../001-foundation.sql"),
            include_str!("../002-captures.sql"),
            include_str!("../003-sources.sql"),
            include_str!("../004-dvr.sql"),
        ] {
            connection.execute_batch(migration)?;
        }
        connection.execute("UPDATE budgets SET limit_micros = 7654321", [])?;
        if fail {
            connection.execute("CREATE TABLE directory_stations(existing TEXT)", [])?;
        }
        let result = Store::open(&path);
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if fail {
            assert!(result.is_err());
            assert_eq!(version, 4);
            let tables: u32 = connection.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'directory_refreshes'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(tables, 0);
        } else {
            let store = result?;
            assert_eq!(version, super::super::SCHEMA_VERSION);
            assert_eq!(store.dvr_status()?.quota_bytes, 50_000_000_000);
            assert_eq!(store.budget("global")?.limit().to_string(), "7.654321");
        }
    }
    Ok(())
}

#[test]
fn favorites_survive_refresh_and_reopen_without_changing_source_authority() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    let id = "12345678-1234-1234-1234-123456789abc";
    store.begin_refresh_at("one", &request(), 1)?;
    store.finish_refresh("one", batch("Original", "https://radio.example/one")?)?;
    store.add_station_source(id, "saved:v1", RedirectPolicy::Deny)?;
    store.set_station_favorite(id, true)?;
    store.set_station_favorite(id, true)?;
    assert_eq!(store.directory_status()?.favorite_stations, 1);
    assert!(
        store
            .connection
            .execute("DELETE FROM directory_stations WHERE id = ?1", [id])
            .is_err()
    );

    store.begin_refresh_at("two", &request(), 3000)?;
    store.finish_refresh("two", batch("Changed", "https://radio.example/two")?)?;
    store.begin_refresh_at("partial", &request(), 6000)?;
    let mut partial = batch("Other", "https://other.example/audio")?;
    partial.candidates[0].station.id = "12345678-1234-1234-1234-123456789abd".into();
    store.finish_refresh("partial", partial)?;
    drop(store);

    let mut store = Store::open(&path)?;
    let saved = store.search_stations(&StationFilter::default(), true, None, 16)?;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, id);
    assert_eq!(saved[0].name, "Changed");
    assert_eq!(
        store
            .source("saved:v1")?
            .ok_or("source missing")?
            .source
            .endpoint(),
        "https://radio.example/one"
    );
    store.set_station_favorite(id, false)?;
    store.set_station_favorite(id, false)?;
    assert_eq!(store.directory_status()?.favorite_stations, 0);
    assert_eq!(store.directory_status()?.cached_stations, 2);
    assert!(store.source("saved:v1")?.is_some());
    assert!(matches!(
        store.set_station_favorite("12345678-1234-1234-1234-123456789abe", true),
        Err(Error::NotFound)
    ));
    assert!(store.set_station_favorite("bad-id", true).is_err());
    Ok(())
}

#[test]
fn favorite_pages_filter_before_limiting_and_report_only_matching_cursors() -> TestResult {
    use crate::control::{DirectoryOperation, apply};
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    let mut page = batch("Radio Québec", "https://radio.example/audio")?;
    for suffix in ["abd", "abe"] {
        let mut next = batch("Other", "https://other.example/audio")?;
        next.candidates[0].station.id = format!("12345678-1234-1234-1234-123456789{suffix}");
        page.candidates.extend(next.candidates);
    }
    store.begin_refresh_at("one", &request(), 1)?;
    store.finish_refresh("one", page)?;
    let first = "12345678-1234-1234-1234-123456789abc";
    let last = "12345678-1234-1234-1234-123456789abe";
    store.set_station_favorite(first, true)?;
    store.set_station_favorite(last, true)?;
    let search = |after| {
        DirectoryOperation::Search {
            filter: StationFilter::default(),
            favorites_only: true,
            after,
            limit: 1,
        }
        .into()
    };
    let one = apply(&mut store, search(None))?
        .station_page
        .ok_or("page missing")?;
    assert_eq!(one.entries.len(), 1);
    assert_eq!(one.entries[0].id, first);
    assert_eq!(one.favorite_ids, [first]);
    assert_eq!(one.next_after.as_deref(), Some(first));
    let two = apply(&mut store, search(one.next_after))?
        .station_page
        .ok_or("page missing")?;
    assert_eq!(two.entries.len(), 1);
    assert_eq!(two.entries[0].id, last);
    assert_eq!(two.favorite_ids, [last]);
    assert!(two.next_after.is_none());
    let filter = StationFilter {
        name: "QUÉBEC".into(),
        ..StationFilter::default()
    };
    assert_eq!(store.search_stations(&filter, true, None, 16)?.len(), 1);
    assert_eq!(
        store
            .search_stations(&StationFilter::default(), false, None, 16)?
            .len(),
        3
    );

    store.connection.execute_batch("CREATE TRIGGER favorite_failure AFTER INSERT ON station_favorites BEGIN SELECT RAISE(ABORT, 'fixture'); END;")?;
    let middle = "12345678-1234-1234-1234-123456789abd";
    assert!(store.set_station_favorite(middle, true).is_err());
    assert!(!store.is_station_favorite(middle)?);
    assert_eq!(store.directory_status()?.favorite_stations, 2);
    Ok(())
}

#[test]
fn favorites_migration_preserves_v6_or_rolls_back_on_conflict() -> TestResult {
    let directory = tempfile::tempdir()?;
    for fail in [false, true] {
        let path = directory
            .path()
            .join(if fail { "bad.sqlite3" } else { "good.sqlite3" });
        let connection = rusqlite::Connection::open(&path)?;
        for migration in [
            include_str!("../001-foundation.sql"),
            include_str!("../002-captures.sql"),
            include_str!("../003-sources.sql"),
            include_str!("../004-dvr.sql"),
            include_str!("../005-discovery.sql"),
            include_str!("../006-redirects.sql"),
        ] {
            connection.execute_batch(migration)?;
        }
        connection.execute("UPDATE budgets SET limit_micros = 7654321", [])?;
        if fail {
            connection.execute("CREATE TABLE station_favorites(existing TEXT)", [])?;
        }
        let result = Store::open(&path);
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if fail {
            assert!(result.is_err());
            assert_eq!(version, 6);
            let columns: u32 = connection.query_row("SELECT count(*) FROM pragma_table_info('station_favorites') WHERE name = 'existing'", [], |r| r.get(0))?;
            assert_eq!(columns, 1);
        } else {
            let store = result?;
            assert_eq!(version, super::super::SCHEMA_VERSION);
            assert_eq!(store.directory_status()?.favorite_stations, 0);
            assert_eq!(store.budget("global")?.limit().to_string(), "7.654321");
            assert_eq!(store.dvr_status()?.quota_bytes, 50_000_000_000);
        }
    }
    Ok(())
}
