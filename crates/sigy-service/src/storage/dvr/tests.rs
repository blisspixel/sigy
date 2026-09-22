use super::*;
use crate::sources::{HttpSource, NetworkScope};
type TestResult = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn setup(path: &std::path::Path, quota: u64) -> Result<Store> {
    let mut store = Store::open(path)?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        quota,
        64 * 1024 * 1024,
        14,
        executable
            .to_str()
            .ok_or(Error::InvalidInput("test executable"))?,
    )?;
    store.register_source(
        "radio:v1",
        &HttpSource::new(
            "Test radio",
            "https://example.com/audio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    Ok(store)
}

fn titled(bytes: u64, offset: u64, text: &str) -> Publication {
    let mut item = publication(bytes);
    item.observations.push(crate::sources::icy::IcyObservation {
        audio_offset: offset,
        text: text.to_owned(),
    });
    item
}

fn publication(bytes: u64) -> Publication {
    Publication {
        bytes,
        sha256: "a".repeat(64),
        format: "wav",
        decoded_microseconds: 1_000_000,
        end_reason: "end_of_body",
        http_route: vec![crate::sources::HttpHop {
            origin: "https://example.com".into(),
            peer: std::net::SocketAddr::from(([8, 8, 8, 8], 443)),
            status: 200,
        }],
        observations: Vec::new(),
        segments_sealed: false,
    }
}

#[test]
fn redirect_migration_keeps_v5_sources_denied_and_rolls_back_conflicts() -> TestResult {
    let directory = tempfile::tempdir()?;
    for fail in [false, true] {
        let path = directory.path().join(if fail {
            "conflict.sqlite3"
        } else {
            "legacy.sqlite3"
        });
        let connection = rusqlite::Connection::open(&path)?;
        for sql in [
            include_str!("../001-foundation.sql"),
            include_str!("../002-captures.sql"),
            include_str!("../003-sources.sql"),
            include_str!("../004-dvr.sql"),
            include_str!("../005-discovery.sql"),
        ] {
            connection.execute_batch(sql)?;
        }
        connection.execute("INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms) VALUES ('legacy:v1', 'http_audio', 'Legacy', 'https://example.com/audio', 'public_internet', 1)", [])?;
        connection.execute("UPDATE budgets SET limit_micros = 1234567", [])?;
        if fail {
            connection.execute(
                "ALTER TABLE source_revisions ADD COLUMN redirect_policy TEXT",
                [],
            )?;
        }
        let opened = Store::open(&path);
        if fail {
            assert!(opened.is_err());
            assert_eq!(
                connection.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))?,
                5
            );
            let count: u32 = connection.query_row("SELECT count(*) FROM pragma_table_info('recordings') WHERE name = 'http_route_json'", [], |r| r.get(0))?;
            assert_eq!(count, 0);
        } else {
            let mut store = opened?;
            let source = store
                .source("legacy:v1")?
                .ok_or("missing legacy source")?
                .source;
            assert_eq!(source.redirects(), crate::sources::RedirectPolicy::Deny);
            assert_eq!(store.budget("global")?.limit().to_string(), "1.234567");
            let changed = source.with_redirects(crate::sources::RedirectPolicy::Public)?;
            assert!(matches!(
                store.register_source("legacy:v1", &changed),
                Err(Error::IdempotencyConflict)
            ));
            store.register_source("legacy:v2", &changed)?;
            drop(store);
            assert_eq!(
                Store::open(&path)?
                    .source("legacy:v2")?
                    .ok_or("missing new source")?
                    .source
                    .redirects(),
                crate::sources::RedirectPolicy::Public
            );
        }
    }
    Ok(())
}

#[test]
fn radio_ceilings_stay_at_fifteen_minutes_and_256_mib() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 512 * 1024 * 1024)?;
    assert!(matches!(
        store.admit_recording("long", "radio:v1", 901, 1024, Retention::Temporary, false),
        Err(Error::InvalidInput("recording duration or byte ceiling"))
    ));
    assert!(matches!(
        store.admit_recording(
            "big",
            "radio:v1",
            60,
            256 * 1024 * 1024 + 1,
            Retention::Temporary,
            false
        ),
        Err(Error::InvalidInput("recording duration or byte ceiling"))
    ));
    assert!(store.capture("long")?.is_none());
    assert!(store.capture("big")?.is_none());
    Ok(())
}

#[test]
fn route_publication_is_atomic_immutable_and_validated_on_export() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 1000)?;
    let job = store
        .admit_recording("route", "radio:v1", 1, 100, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    assert!(store.recording_route("route")?.is_none());
    let mut invalid = publication(20);
    invalid.http_route[0].peer = std::net::SocketAddr::from(([127, 0, 0, 1], 443));
    assert!(store.publish_recording(&job.version, &invalid).is_err());
    assert_eq!(store.recording("route")?.storage_state, "reserved");
    assert!(store.recording_route("route")?.is_none());
    store.publish_recording(&job.version, &publication(20))?;
    assert_eq!(
        store.recording_route("route")?.ok_or("missing route")?[0].status,
        200
    );
    assert!(
        store
            .connection
            .execute(
                "UPDATE recordings SET http_route_json = '[]' WHERE id = 'route'",
                []
            )
            .is_err()
    );
    store.connection.execute_batch("DROP TRIGGER recording_route_immutable; UPDATE recordings SET http_route_json = '[]' WHERE id = 'route';")?;
    assert!(store.recording_route("route").is_err());
    Ok(())
}

#[test]
fn migration_from_v3_preserves_budget_and_rolls_back_conflicts() -> TestResult {
    let directory = tempfile::tempdir()?;
    for fail in [false, true] {
        let path = directory
            .path()
            .join(if fail { "bad.sqlite3" } else { "good.sqlite3" });
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(include_str!("../001-foundation.sql"))?;
        connection.execute_batch(include_str!("../002-captures.sql"))?;
        connection.execute_batch(include_str!("../003-sources.sql"))?;
        connection.execute("UPDATE budgets SET limit_micros = 7654321", [])?;
        if fail {
            connection.execute("CREATE TABLE recordings(existing TEXT)", [])?;
        }
        let result = Store::open(&path);
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if fail {
            assert!(result.is_err());
            assert_eq!(version, 3);
            let policy_tables: u32 = connection.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'dvr_policy'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(policy_tables, 0);
        } else {
            let store = result?;
            assert_eq!(version, super::super::SCHEMA_VERSION);
            assert_eq!(store.budget("global")?.limit().to_string(), "7.654321");
            assert_eq!(store.dvr_status()?.charged_bytes, 0);
        }
    }
    Ok(())
}

#[test]
fn defaults_and_reservations_survive_restart_without_replay() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let initial = Store::open(&path)?.dvr_status()?;
    assert_eq!(initial.quota_bytes, 50_000_000_000);
    assert_eq!(initial.retention_days, 14);
    let mut store = setup(&path, 1000)?;
    let job = store
        .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    assert!(
        store
            .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary, false)?
            .is_none()
    );
    assert!(matches!(
        store.admit_recording("two", "radio:v1", 60, 600, Retention::Temporary, false),
        Err(Error::StorageQuota)
    ));
    assert!(store.capture("two")?.is_none());
    assert!(matches!(
        store.admit_recording("one", "radio:v1", 61, 600, Retention::Temporary, false),
        Err(Error::IdempotencyConflict)
    ));
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.recover_captures()?, 1);
    assert_eq!(store.dvr_status()?.reserved_bytes, 600);
    assert!(matches!(
        store.publish_recording(&job.version, &publication(100)),
        Err(Error::StaleCapture)
    ));
    assert!(
        store
            .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary, false)?
            .is_none()
    );
    assert_eq!(store.recording("one")?.state, "interrupted");
    store.begin_delete("one", false)?;
    store.finish_delete("one")?;
    assert_eq!(store.capture_counts()?.interrupted, 0);
    assert_eq!(store.recording("one")?.state, "cancelled");
    assert_eq!(store.dvr_status()?.charged_bytes, 0);
    assert!(
        store
            .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary, false)?
            .is_none()
    );
    store.audit_captures()?;
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn concurrent_admission_cannot_overbook_storage() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let store = setup(&path, 1000)?;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut workers = Vec::new();
    for id in ["left", "right"] {
        let mut contender = Store::open(&path)?;
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            contender.admit_recording(id, "radio:v1", 60, 600, Retention::Temporary, false)
        }));
    }
    let mut admissions = 0;
    for worker in workers {
        match worker.join().map_err(|_| "worker panicked")? {
            Ok(Some(_)) => admissions += 1,
            Err(Error::StorageQuota) => (),
            other => return Err(format!("unexpected result: {other:?}").into()),
        }
    }
    assert_eq!(admissions, 1);
    assert_eq!(store.dvr_status()?.charged_bytes, 600);
    assert_eq!(store.capture_counts()?.active, 1);
    Ok(())
}

#[test]
fn publication_and_deletion_keep_exact_accounting_and_history() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = setup(&path, 1000)?;
    let job = store
        .admit_recording("one", "radio:v1", 60, 600, Retention::Kept, false)?
        .ok_or("not admitted")?;
    assert!(store.begin_delete("one", false).is_err());
    assert!(
        store
            .publish_recording(&job.version, &publication(601))
            .is_err()
    );
    assert_eq!(store.recording("one")?.state, "starting");
    store.publish_recording(&job.version, &publication(100))?;
    assert_eq!(store.dvr_status()?.charged_bytes, 100);
    assert_eq!(store.dvr_status()?.reserved_bytes, 0);
    assert!(
        store
            .publish_recording(&job.version, &publication(100))
            .is_err()
    );
    assert!(store.begin_delete("one", true).is_err());
    store.begin_delete("one", false)?;
    assert_eq!(store.dvr_status()?.charged_bytes, 100);
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.recording("one")?.storage_state, "deleting");
    store.finish_delete("one")?;
    assert_eq!(store.dvr_status()?.charged_bytes, 0);
    assert_eq!(store.recording("one")?.sha256, Some("a".repeat(64)));
    assert_eq!(store.recording("one")?.state, "completed");
    store.audit_captures()?;
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn age_pressure_and_processing_never_evict_protected_or_active_media() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 10_000)?;
    for (id, retention) in [
        ("a", Retention::Temporary),
        ("b", Retention::Kept),
        ("c", Retention::Archived),
        ("d", Retention::Temporary),
    ] {
        let job = store
            .admit_recording(id, "radio:v1", 60, 600, retention, false)?
            .ok_or("not admitted")?;
        store.publish_recording(&job.version, &publication(100))?;
    }
    store.admit_recording("active", "radio:v1", 60, 600, Retention::Temporary, false)?;
    let now = now_ms()?;
    assert!(store.prune_candidates_at(false, now)?.is_empty());
    assert_eq!(
        store.prune_candidates_at(false, now + 15 * 86_400_000)?,
        vec!["a", "d"]
    );
    assert_eq!(store.prune_candidates_at(true, now)?, vec!["a", "d"]);
    store.acknowledge_processing("d", "analysis:receipt1")?;
    assert_eq!(store.prune_candidates_at(false, now)?, vec!["d"]);
    store.retain_recording("d", Retention::Kept)?;
    assert!(store.prune_candidates_at(false, now)?.is_empty());
    assert!(store.acknowledge_processing("active", "premature").is_err());
    Ok(())
}

#[test]
fn icy_observations_do_not_change_the_source_or_the_audio_hash() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 2_000)?;
    let title = "StreamTitle='Owned Station';StreamUrl='http://127.0.0.1/secret-stream';";
    let denied = store
        .admit_recording("plain", "radio:v1", 60, 600, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    let hidden = titled(20, 20, title);
    assert!(store.publish_recording(&denied.version, &hidden).is_err());
    assert_eq!(store.recording("plain")?.storage_state, "reserved");
    assert!(store.recording_observations("plain")?.is_empty());
    assert!(matches!(
        store.admit_recording("plain", "radio:v1", 60, 600, Retention::Temporary, true),
        Err(Error::IdempotencyConflict)
    ));
    let job = store
        .admit_recording("titled", "radio:v1", 60, 600, Retention::Temporary, true)?
        .ok_or("not admitted")?;
    assert!(
        store
            .admit_recording("titled", "radio:v1", 60, 600, Retention::Temporary, true)?
            .is_none()
    );
    assert!(
        store
            .publish_recording(&job.version, &titled(20, 20, "bad\nname"))
            .is_err()
    );
    assert!(
        store
            .publish_recording(&job.version, &titled(20, 21, title))
            .is_err()
    );
    assert_eq!(store.recording("titled")?.storage_state, "reserved");
    let mut published = titled(20, 20, title);
    published.sha256 = "b".repeat(64);
    store.publish_recording(&job.version, &published)?;
    assert_eq!(
        store.recording_observations("titled")?,
        vec![(20_u64, title.to_owned())]
    );
    assert_eq!(
        store
            .source("radio:v1")?
            .ok_or("missing source")?
            .source
            .name(),
        "Test radio"
    );
    let recording = store.recording("titled")?;
    let envelope = crate::recordings::metadata::export(&store, &recording)?;
    assert_eq!(envelope.schema_version, 3);
    assert_eq!(envelope.capture.icy_observations.len(), 1);
    assert_eq!(envelope.capture.icy_observations[0].text, title);
    assert_eq!(envelope.capture.icy_observations[0].audio_offset, 20);
    assert_eq!(
        envelope.storage.sha256.as_deref(),
        Some(published.sha256.as_str())
    );
    assert_eq!(envelope.source.name, "Test radio");
    store.audit_dvr()?;
    let plain = crate::recordings::metadata::export(&store, &store.recording("plain")?)?;
    assert_eq!(plain.schema_version, 2);
    assert!(plain.capture.icy_observations.is_empty());
    Ok(())
}

#[test]
fn icy_migration_preserves_v10_and_rolls_back_conflicts() -> TestResult {
    let directory = tempfile::tempdir()?;
    for fail in [false, true] {
        let path = directory
            .path()
            .join(if fail { "bad.sqlite3" } else { "good.sqlite3" });
        let connection = rusqlite::Connection::open(&path)?;
        for sql in [
            include_str!("../001-foundation.sql"),
            include_str!("../002-captures.sql"),
            include_str!("../003-sources.sql"),
            include_str!("../004-dvr.sql"),
            include_str!("../005-discovery.sql"),
            include_str!("../006-redirects.sql"),
            include_str!("../007-favorites.sql"),
            include_str!("../008-playlists.sql"),
            include_str!("../009-clicks.sql"),
            include_str!("../010-listens.sql"),
        ] {
            connection.execute_batch(sql)?;
        }
        connection.execute("UPDATE budgets SET limit_micros = 4242", [])?;
        connection.execute(
            "INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms, redirect_policy) VALUES ('radio:v1', 'http_audio', 'Radio', 'https://example.com/audio', 'public_internet', 1, 'deny')",
            [],
        )?;
        if fail {
            connection.execute("CREATE TABLE recording_observations(existing TEXT)", [])?;
        }
        let opened = Store::open(&path);
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if fail {
            assert!(opened.is_err());
            assert_eq!(version, 10);
            let column: u32 = connection.query_row(
                "SELECT count(*) FROM pragma_table_info('recordings') WHERE name = 'metadata_requested'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(column, 0);
        } else {
            let store = opened?;
            assert_eq!(version, super::super::SCHEMA_VERSION);
            assert_eq!(store.budget("global")?.limit().micros(), 4242);
            let column: i64 = store.connection.query_row(
                "SELECT count(*) FROM pragma_table_info('recordings') WHERE name = 'metadata_requested'",
                [],
                |row| row.get(0),
            )?;
            let table: i64 = store.connection.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'recording_observations'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!((column, table), (1, 1));
        }
    }
    Ok(())
}

#[test]
fn published_file_is_one_measured_interval_and_old_rows_project() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = setup(&path, 10_000)?;
    let published = store
        .admit_recording("done", "radio:v1", 60, 600, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    store.publish_recording(&published.version, &publication(100))?;
    store.admit_recording("open", "radio:v1", 60, 600, Retention::Temporary, false)?;
    assert_measured_interval(&store, "done", 1_000_000, 100)?;
    assert_no_interval(&store, "open")?;
    if store
        .connection
        .execute(
            "INSERT INTO recording_intervals(recording_id, ordinal, decoded_start_us, decoded_end_us, byte_start, byte_end) VALUES ('open', 0, 0, 60000000, 0, 600)",
            [],
        )
        .is_ok()
    {
        return Err("an unpublished reservation became an interval".into());
    }
    drop(store);
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("DROP TABLE IF EXISTS recording_gaps; DROP TABLE recording_intervals; DROP INDEX IF EXISTS recording_open_object_keys; ALTER TABLE recordings DROP COLUMN lease_renewals; ALTER TABLE recordings DROP COLUMN lease_expires_ms; ALTER TABLE recordings DROP COLUMN open_object_key; ALTER TABLE recordings DROP COLUMN open_ceiling; ALTER TABLE recordings DROP COLUMN escrow_bytes; PRAGMA user_version = 15;")?;
    drop(connection);
    let store = Store::open(&path)?;
    assert_measured_interval(&store, "done", 1_000_000, 100)?;
    assert_no_interval(&store, "open")?;
    let planned: i64 = store.connection.query_row(
        "SELECT duration_seconds FROM recordings WHERE id = 'done'",
        [],
        |row| row.get(0),
    )?;
    if planned != 60 {
        return Err("the planned window was rewritten".into());
    }
    store.audit_dvr()?;
    Ok(())
}

fn assert_measured_interval(store: &Store, id: &str, end_us: i64, end_byte: i64) -> TestResult {
    let (start_us, measured_us, start_byte, measured_byte): (i64, i64, i64, i64) = store
        .connection
        .query_row(
            "SELECT decoded_start_us, decoded_end_us, byte_start, byte_end FROM recording_intervals WHERE recording_id = ?1 AND ordinal = 0",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    if (start_us, measured_us, start_byte, measured_byte) != (0, end_us, 0, end_byte) {
        return Err("interval bounds are not the measured publication".into());
    }
    Ok(())
}

fn assert_no_interval(store: &Store, id: &str) -> TestResult {
    let count: i64 = store.connection.query_row(
        "SELECT count(*) FROM recording_intervals WHERE recording_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    if count != 0 {
        return Err("unpublished bytes were projected as airtime".into());
    }
    Ok(())
}

#[test]
fn sealed_segments_keep_one_reservation_and_reject_a_stale_token() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let budget = OPEN_SEGMENT_CEILING * 2;
    let mut store = setup(&path, budget + 1_000)?;
    let admitted = store
        .admit_recording("job", "radio:v1", 60, budget, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    let stale = admitted.version.clone();
    let running = store.connect_recording(&admitted.version)?;
    assert!(matches!(
        store.open_segment(&stale),
        Err(Error::StaleCapture)
    ));
    let SegmentOpen::Opened {
        version,
        ceiling,
        ordinal,
        ..
    } = store.open_segment(&running)?
    else {
        return Err("first segment did not open".into());
    };
    assert_eq!((ceiling, ordinal), (OPEN_SEGMENT_CEILING, 0));
    store.seal_segment(&version, &seal(1_000, 1_000_000, "ab"))?;
    assert!(matches!(
        store.seal_segment(&stale, &seal(1, 1, "ab")),
        Err(Error::StaleCapture)
    ));
    let record = store.recording("job")?;
    assert_eq!(record.state, "running");
    assert_eq!(record.escrow_bytes, budget - 1_000);
    assert_eq!(record.open_ceiling, 0);
    assert_eq!(record.charged_bytes, budget);
    assert_eq!(record.intervals.len(), 1);
    store.renew_segment_lease(&version)?;
    assert_eq!(store.recording("job")?.lease_renewals, 1);
    assert_eq!(
        store
            .capture("job")?
            .ok_or("missing job")?
            .version
            .generation(),
        running.generation()
    );
    let SegmentOpen::Opened { version, .. } = store.open_segment(&version)? else {
        return Err("second segment did not open".into());
    };
    store.seal_segment(&version, &seal(2_000, 1_500_000, "cd"))?;
    let SegmentOpen::Opened {
        version, ceiling, ..
    } = store.open_segment(&version)?
    else {
        return Err("third segment did not open".into());
    };
    assert_eq!(ceiling, OPEN_SEGMENT_CEILING);
    let record = store.recording("job")?;
    assert_eq!(record.intervals.len(), 2);
    assert_eq!(
        (
            record.intervals[0].byte_end,
            record.intervals[1].byte_start,
            record.intervals[1].byte_end
        ),
        (1_000, 1_000, 3_000)
    );
    assert_eq!(record.state, "running");
    assert_eq!(record.open_ceiling, OPEN_SEGMENT_CEILING);
    assert_eq!(record.charged_bytes, budget);
    assert_eq!(store.dvr_status()?.reserved_bytes, budget);
    let reserved: i64 = store.connection.query_row(
        "SELECT count(*) FROM recordings WHERE storage_state = 'reserved'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(reserved, 1);
    store.audit_dvr()?;
    store.release_open_segment(&version)?;
    let mut done = publication(3_000);
    done.segments_sealed = true;
    done.decoded_microseconds = 2_500_000;
    done.sha256 = "ef".repeat(32);
    store.publish_recording(&version, &done)?;
    let record = store.recording("job")?;
    assert_eq!(record.state, "completed");
    assert_eq!(record.media_bytes, Some(3_000));
    assert_eq!(record.charged_bytes, 3_000);
    assert_eq!(record.escrow_bytes, 0);
    assert_eq!(record.intervals.len(), 2);
    assert_eq!(store.dvr_status()?.reserved_bytes, 0);
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn next_segment_opens_only_when_a_full_ceiling_remains() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let budget = OPEN_SEGMENT_CEILING * 2;
    let mut store = setup(&path, budget)?;
    let admitted = store
        .admit_recording("full", "radio:v1", 60, budget, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    let running = store.connect_recording(&admitted.version)?;
    let SegmentOpen::Opened { version, .. } = store.open_segment(&running)? else {
        return Err("first full segment did not open".into());
    };
    store.seal_segment(&version, &seal(OPEN_SEGMENT_CEILING, 1_000_000, "aa"))?;
    let SegmentOpen::Opened { version, .. } = store.open_segment(&version)? else {
        return Err("second full segment did not open".into());
    };
    store.seal_segment(&version, &seal(OPEN_SEGMENT_CEILING, 1_000_000, "bb"))?;
    assert!(matches!(
        store.open_segment(&version),
        Ok(SegmentOpen::BudgetHeld)
    ));
    let record = store.recording("full")?;
    assert_eq!(record.intervals.len(), 2);
    assert_eq!(record.open_ceiling, 0);
    assert_eq!(record.escrow_bytes, 0);
    assert_eq!(record.charged_bytes, budget);
    assert_eq!(record.state, "running");
    store.audit_dvr()?;
    let stale = version.clone();
    store.transition_capture(
        &version,
        sigy_core::capture::CaptureEvent::Lost,
        "worker_lost",
    )?;
    assert!(matches!(
        store.seal_segment(&stale, &seal(1, 1, "aa")),
        Err(Error::StaleCapture)
    ));
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn recovery_returns_an_open_ceiling_to_the_escrow() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let budget = OPEN_SEGMENT_CEILING * 2;
    let admitted = {
        let mut store = setup(&path, budget)?;
        let admitted = store
            .admit_recording("open", "radio:v1", 60, budget, Retention::Temporary, false)?
            .ok_or("not admitted")?;
        let running = store.connect_recording(&admitted.version)?;
        let SegmentOpen::Opened { ceiling, .. } = store.open_segment(&running)? else {
            return Err("segment did not open".into());
        };
        assert_eq!(ceiling, OPEN_SEGMENT_CEILING);
        admitted.version
    };
    let mut store = Store::open(&path)?;
    assert_eq!(store.recover_captures()?, 1);
    let record = store.recording("open")?;
    assert_eq!(record.state, "interrupted");
    assert_eq!(record.open_ceiling, 0);
    assert_eq!(record.escrow_bytes, budget);
    assert_eq!(record.charged_bytes, budget);
    assert!(record.intervals.is_empty());
    assert_eq!(record.media_bytes, None);
    assert_hole(&record, GapCause::Recovery, 0)?;
    assert!(matches!(
        store.seal_segment(&admitted, &seal(1, 1, "aa")),
        Err(Error::StaleCapture)
    ));
    store.audit_dvr()?;
    Ok(())
}

fn running(
    store: &mut Store,
    id: &str,
    bytes: u64,
) -> std::result::Result<CaptureVersion, Box<dyn std::error::Error + Send + Sync>> {
    let admitted = store
        .admit_recording(id, "radio:v1", 60, bytes, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    Ok(store.connect_recording(&admitted.version)?)
}

fn assert_hole(record: &Recording, cause: GapCause, start_us: u64) -> TestResult {
    let gap = record.gaps.first().ok_or("missing gap")?;
    if record.gaps.len() != 1
        || gap.cause != cause
        || gap.start_us != start_us
        || gap.end_us != 60_000_000
        || blocking_gap(&record.gaps, start_us).is_none()
        || blocking_gap(&record.gaps, gap.end_us).is_some()
    {
        return Err(format!("gap did not block its range: {gap:?}").into());
    }
    Ok(())
}

#[test]
fn disconnect_journals_a_gap_without_a_silence_file() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 10_000)?;
    let dropped = running(&mut store, "drop", 1024)?;
    store.fail_recording(
        &dropped,
        &Error::Acquisition("body interrupted; partial body is unverified"),
    )?;
    let record = store.recording("drop")?;
    assert_eq!(record.state, "failed");
    assert_eq!(record.media_bytes, None);
    assert!(record.intervals.is_empty());
    assert_hole(&record, GapCause::Disconnect, 0)?;
    let other = running(&mut store, "other", 1024)?;
    store.fail_recording(&other, &Error::Acquisition("deadline reached"))?;
    assert!(store.recording("other")?.gaps.is_empty());
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn codec_change_journals_a_gap_instead_of_another_file() -> TestResult {
    let directory = tempfile::tempdir()?;
    let budget = OPEN_SEGMENT_CEILING * 2;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), budget + 1_000)?;
    let version = running(&mut store, "codec", budget)?;
    let SegmentOpen::Opened { version, .. } = store.open_segment(&version)? else {
        return Err("segment did not open".into());
    };
    store.seal_segment(&version, &seal(1_000, 1_000_000, "ab"))?;
    let SegmentOpen::Opened { version, .. } = store.open_segment(&version)? else {
        return Err("second segment did not open".into());
    };
    let rejected = SegmentSeal {
        format: "mp3",
        ..seal(1_000, 1_000_000, "cd")
    };
    assert!(matches!(
        store.seal_segment(&version, &rejected),
        Err(Error::InvalidInput("codec change is a gap"))
    ));
    let record = store.recording("codec")?;
    assert_eq!(record.state, "failed");
    assert_eq!(record.intervals.len(), 1);
    assert_eq!(record.media_bytes, None);
    assert_hole(&record, GapCause::CodecChange, 1_000_000)?;
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn expired_lease_refuses_renewal_and_journals_a_gap() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 10_000)?;
    let version = running(&mut store, "lease", 1024)?;
    store.connection.execute(
        "UPDATE recordings SET lease_expires_ms = 1 WHERE id = 'lease'",
        [],
    )?;
    assert!(matches!(
        store.renew_segment_lease(&version),
        Err(Error::InvalidInput("segment lease renewal was refused"))
    ));
    let record = store.recording("lease")?;
    assert_eq!(record.state, "failed");
    assert_eq!(record.lease_renewals, 0);
    assert_eq!(record.media_bytes, None);
    assert_hole(&record, GapCause::RefusedRenewal, 0)?;
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn capture_pause_journals_a_gap_and_interrupts() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 10_000)?;
    let version = running(&mut store, "pause", 1024)?;
    store.pause_capture("pause")?;
    let record = store.recording("pause")?;
    assert_eq!(record.state, "interrupted");
    assert_eq!(record.media_bytes, None);
    assert!(record.intervals.is_empty());
    assert_hole(&record, GapCause::CapturePause, 0)?;
    assert!(matches!(
        store.seal_segment(&version, &seal(1, 1, "aa")),
        Err(Error::StaleCapture)
    ));
    store.audit_dvr()?;
    Ok(())
}

#[test]
fn backward_clock_journals_a_gap_without_recording_the_earlier_time() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"), 10_000)?;
    running(&mut store, "clock", 1024)?;
    assert!(matches!(
        store.note_clock("clock", 0),
        Err(Error::InvalidInput("clock moved backward"))
    ));
    let record = store.recording("clock")?;
    assert_eq!(record.state, "failed");
    assert_eq!(record.media_bytes, None);
    assert_hole(&record, GapCause::BackwardClock, 0)?;
    let updated: i64 = store.connection.query_row(
        "SELECT updated_ms FROM capture_jobs WHERE id = 'clock'",
        [],
        |row| row.get(0),
    )?;
    if updated == 0 {
        return Err("backward timestamp was stored".into());
    }
    store.audit_dvr()?;
    Ok(())
}

fn seal(bytes: u64, decoded: u64, prefix: &str) -> SegmentSeal {
    SegmentSeal {
        bytes,
        sha256: prefix.repeat(32),
        format: "wav",
        decoded_microseconds: decoded,
    }
}
