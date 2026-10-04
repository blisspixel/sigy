use super::*;
use crate::{
    discovery::{RefreshRequest, StationFilter, radio_browser},
    sources::{NetworkScope, RedirectPolicy},
    storage::dvr::{Publication, Retention},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const ID: &str = "00000000-0000-0000-0000-000000000001";

fn refresh(store: &mut Store, request_id: &str, name: &str, endpoint: &str) -> Result<()> {
    let request = RefreshRequest {
        filter: StationFilter::default(),
        limit: 10,
        offset: 0,
        mirror: None,
        network: NetworkScope::PublicInternet {},
    };
    let last: Option<i64> = store.connection.query_row(
        "SELECT max(started_ms) FROM directory_refreshes",
        [],
        |row| row.get(0),
    )?;
    let now = last
        .unwrap_or(0)
        .checked_add(3000)
        .ok_or(Error::StorageIntegrity)?;
    store.begin_refresh_at(request_id, &request, now)?;
    let body = serde_json::to_vec(&serde_json::json!([
        {"stationuuid":ID,"name":name,"url":endpoint},
        {"stationuuid":"00000000-0000-0000-0000-000000000002","name":name,"url":endpoint}
    ]))?;
    let batch = radio_browser::parse(&body, 10, "https://directory.example".into())?;
    store.finish_refresh(request_id, batch)
}

fn record(store: &mut Store, id: &str, revision: &str) -> Result<()> {
    let job = store
        .admit_recording(id, revision, 1, 100, Retention::Temporary, false)?
        .ok_or(Error::RequestState)?;
    store.publish_recording(
        &job.version,
        &Publication {
            bytes: 20,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![crate::sources::HttpHop {
                origin: "https://radio.example".into(),
                peer: ([8, 8, 8, 8], 443).into(),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
            gap: None,
        },
    )?;
    Ok(())
}

#[test]
fn linked_context_uses_exact_identity_and_preserves_historical_metadata_and_expired_media()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable")?,
    )?;
    refresh(&mut store, "initial", "東京", "https://radio.example/one")?;
    store.add_station_source(ID, "radio:v1", RedirectPolicy::Deny)?;
    let before = store.directory_catalog()?;
    record(&mut store, "old-recording", "radio:v1")?;
    store.begin_delete("old-recording", false)?;
    store.finish_delete("old-recording")?;
    refresh(&mut store, "changed", "اسم", "https://radio.example/two")?;
    store.add_station_source(ID, "radio:v2", RedirectPolicy::Deny)?;
    let page = store.linked_station_context(ID, None)?;
    assert_eq!(page.sources.len(), 2);
    assert_eq!(page.sources[0].registered_station.name, "東京");
    assert_eq!(page.sources[1].registered_station.name, "اسم");
    assert_eq!(page.sources[0].recordings[0].storage_state, "deleted");
    assert!(!page.sources[0].recordings[0].retained_segments);
    assert_eq!(page.sources[0].recordings[0].source_revision, "radio:v1");
    assert!(page.sources[1].recordings.is_empty());
    let duplicate = store.linked_station_context("00000000-0000-0000-0000-000000000002", None)?;
    assert!(duplicate.sources.is_empty());
    assert!(store.linked_station_context(ID, Some(&before)).is_err());
    assert_eq!(
        store
            .linked_station_context(ID, Some(&page.catalog))?
            .sources
            .len(),
        2
    );
    Ok(())
}

#[test]
fn linked_context_has_independent_real_lookahead_bounds_without_total_count_claims() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable")?,
    )?;
    refresh(&mut store, "one", "Station", "https://radio.example/one")?;
    for index in 0..5 {
        store.add_station_source(ID, &format!("radio:{index}"), RedirectPolicy::Deny)?;
    }
    for index in 0..5 {
        record(&mut store, &format!("recording:{index}"), "radio:0")?;
    }
    let page = store.linked_station_context(ID, None)?;
    assert_eq!(page.sources.len(), 4);
    assert!(page.more_sources);
    assert_eq!(page.sources[0].recordings.len(), 4);
    assert!(page.sources[0].more_recordings);
    assert!(!page.sources[1].more_recordings);
    assert_eq!(page.sources[0].recordings[3].id, "recording:3");
    let mut plan = store.connection.prepare("EXPLAIN QUERY PLAN SELECT source_revision FROM source_directory_links WHERE provider='radio_browser' AND station_id=?1 ORDER BY source_revision LIMIT 5")?;
    let descriptions = plan
        .query_map([ID], |row| row.get::<_, String>(3))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(
        descriptions
            .iter()
            .any(|line| line.contains("station_source_links"))
    );
    let mut plan = store.connection.prepare("EXPLAIN QUERY PLAN SELECT r.id FROM capture_jobs c JOIN recordings r ON r.id=c.id WHERE c.source_revision=?1 ORDER BY c.id LIMIT 5")?;
    let descriptions = plan
        .query_map(["radio:0"], |row| row.get::<_, String>(3))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(
        descriptions
            .iter()
            .any(|line| line.contains("captures_by_source_revision"))
    );
    assert!(
        descriptions
            .iter()
            .all(|line| !line.contains("TEMP B-TREE"))
    );
    Ok(())
}

#[test]
fn linked_gap_presence_does_not_claim_continuous_retained_audio() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable")?,
    )?;
    refresh(&mut store, "one", "Station", "https://radio.example/one")?;
    store.add_station_source(ID, "radio:v1", RedirectPolicy::Deny)?;
    let job = store
        .admit_recording("with-gap", "radio:v1", 60, 100, Retention::Temporary, false)?
        .ok_or(Error::RequestState)?;
    store.publish_recording(
        &job.version,
        &Publication {
            bytes: 20,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "stream_gap",
            http_route: vec![crate::sources::HttpHop {
                origin: "https://radio.example".into(),
                peer: ([8, 8, 8, 8], 443).into(),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
            gap: Some(crate::storage::dvr::GapCause::Disconnect),
        },
    )?;
    let page = store.linked_station_context(ID, None)?;
    let recording = &page.sources[0].recordings[0];
    assert!(recording.has_gaps && recording.retained_segments && !recording.released_segments);
    assert_eq!(recording.decoded_microseconds, Some(1_000_000));
    Ok(())
}

#[test]
fn scoped_low_vm_budget_interrupts_real_projection_and_following_full_guard_read_recovers()
-> TestResult {
    use crate::storage::query_work::{Limits, QueryWork};
    use std::time::Duration;
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable")?,
    )?;
    refresh(&mut store, "one", "Station", "https://radio.example/one")?;
    for source in 0..4 {
        let revision = format!("radio:{source}");
        store.add_station_source(ID, &revision, RedirectPolicy::Deny)?;
        for recording in 0..4 {
            record(
                &mut store,
                &format!("recording:{source}:{recording}"),
                &revision,
            )?;
        }
    }
    store.connection.busy_timeout(Duration::from_millis(37))?;
    // Open the read transaction before installing the injected one-op checkpoint,
    // so refusal comes from the actual projection rather than fixture BEGIN.
    let tx = store.connection.unchecked_transaction()?;
    let work = QueryWork::start_for_test(
        &store.connection,
        Limits {
            wall: Duration::from_millis(100),
            vm_ops: 1,
            lock_wait: Duration::from_millis(10),
        },
        1,
    )?;
    let refused = store.linked_station_context_in_work(ID, None);
    assert!(refused.is_err() && work.exhausted());
    assert!(work.checkpoint_ops() >= 1);
    work.finish()?;
    drop(tx);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))?,
        37
    );
    let healthy = store.linked_station_context(ID, None)?;
    assert_eq!(healthy.sources.len(), 4);
    assert!(
        healthy
            .sources
            .iter()
            .all(|source| source.recordings.len() == 4 && !source.more_recordings)
    );
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))?,
        37
    );
    Ok(())
}

#[test]
fn absent_cache_is_explicit_without_inferred_registration_and_removed_observation_preserves_links()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    let empty = store.linked_station_context(ID, None)?;
    assert!(!empty.cached && empty.sources.is_empty() && !empty.more_sources);
    refresh(&mut store, "one", "Station", "https://radio.example/one")?;
    store.add_station_source(ID, "radio:v1", RedirectPolicy::Deny)?;
    // Controlled cache-removal resilience fixture. Current refresh merges, never deletes rows.
    store
        .connection
        .execute("DELETE FROM directory_stations WHERE id=?1", [ID])?;
    let page = store.linked_station_context(ID, None)?;
    assert!(!page.cached);
    assert_eq!(page.sources[0].source.revision_id, "radio:v1");
    assert_eq!(page.sources[0].registered_station.id, ID);
    Ok(())
}

#[test]
fn oversized_registration_refuses_before_copy_and_following_valid_query_recovers() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    refresh(&mut store, "one", "Station", "https://radio.example/one")?;
    store.add_station_source(ID, "radio:v1", RedirectPolicy::Deny)?;
    store
        .connection
        .execute_batch("DROP TRIGGER source_directory_link_immutable;")?;
    let original: String = store.connection.query_row(
        "SELECT metadata_json FROM source_directory_links",
        [],
        |row| row.get(0),
    )?;
    // SQL character cap permits this multibyte corruption; the reader enforces UTF-8 bytes.
    store.connection.execute(
        "UPDATE source_directory_links SET metadata_json=?1",
        ["界".repeat(3000)],
    )?;
    assert!(store.linked_station_context(ID, None).is_err());
    store.connection.execute(
        "UPDATE source_directory_links SET metadata_json=?1",
        [original],
    )?;
    assert_eq!(store.linked_station_context(ID, None)?.sources.len(), 1);
    Ok(())
}

#[test]
fn populated_schema49_migration_preserves_source_links_and_recordings_on_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = Store::open(&path)?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable")?,
    )?;
    refresh(&mut store, "one", "古いラジオ", "https://radio.example/one")?;
    store.add_station_source(ID, "radio:v1", RedirectPolicy::Deny)?;
    record(&mut store, "migration-recording", "radio:v1")?;
    let catalog = store.directory_catalog()?;
    let original: String = store.connection.query_row(
        "SELECT metadata_json FROM source_directory_links",
        [],
        |row| row.get(0),
    )?;
    revert_050_for_tests(&store.connection)?;
    store.connection.pragma_update(None, "user_version", 49)?;
    drop(store);
    let store = Store::open(&path)?;
    let page = store.linked_station_context(ID, Some(&catalog))?;
    assert_eq!(page.sources[0].registered_station.name, "古いラジオ");
    assert_eq!(page.sources[0].recordings[0].id, "migration-recording");
    let retained: String = store.connection.query_row(
        "SELECT metadata_json FROM source_directory_links",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(retained, original);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        crate::storage::SCHEMA_VERSION
    );
    drop(store);
    assert_eq!(
        Store::open(&path)?
            .linked_station_context(ID, None)?
            .sources
            .len(),
        1
    );
    Ok(())
}
