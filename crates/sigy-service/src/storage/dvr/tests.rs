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
    let mut hidden = publication(20);
    hidden
        .observations
        .push(crate::sources::icy::IcyObservation {
            audio_offset: 20,
            text: title.to_owned(),
        });
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
    let mut unsafe_text = publication(20);
    unsafe_text
        .observations
        .push(crate::sources::icy::IcyObservation {
            audio_offset: 20,
            text: "bad\nname".to_owned(),
        });
    assert!(store.publish_recording(&job.version, &unsafe_text).is_err());
    let mut beyond = publication(20);
    beyond
        .observations
        .push(crate::sources::icy::IcyObservation {
            audio_offset: 21,
            text: title.to_owned(),
        });
    assert!(store.publish_recording(&job.version, &beyond).is_err());
    assert_eq!(store.recording("titled")?.storage_state, "reserved");
    let mut published = publication(20);
    published.sha256 = "b".repeat(64);
    published
        .observations
        .push(crate::sources::icy::IcyObservation {
            audio_offset: 20,
            text: title.to_owned(),
        });
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
