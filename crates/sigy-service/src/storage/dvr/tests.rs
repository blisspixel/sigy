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
    }
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
        .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary)?
        .ok_or("not admitted")?;
    assert!(
        store
            .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary)?
            .is_none()
    );
    assert!(matches!(
        store.admit_recording("two", "radio:v1", 60, 600, Retention::Temporary),
        Err(Error::StorageQuota)
    ));
    assert!(store.capture("two")?.is_none());
    assert!(matches!(
        store.admit_recording("one", "radio:v1", 61, 600, Retention::Temporary),
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
            .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary)?
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
            .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary)?
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
            contender.admit_recording(id, "radio:v1", 60, 600, Retention::Temporary)
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
        .admit_recording("one", "radio:v1", 60, 600, Retention::Kept)?
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
            .admit_recording(id, "radio:v1", 60, 600, retention)?
            .ok_or("not admitted")?;
        store.publish_recording(&job.version, &publication(100))?;
    }
    store.admit_recording("active", "radio:v1", 60, 600, Retention::Temporary)?;
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
