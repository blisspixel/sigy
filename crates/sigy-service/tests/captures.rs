use std::{
    sync::{Arc, Barrier},
    thread,
};

use sigy_service::{
    Error,
    domain::capture::{CaptureEvent as E, CaptureState as S},
    storage::{
        Store,
        captures::{CapturePlan, MAX_PENDING_CAPTURES},
    },
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn plan() -> sigy_service::Result<CapturePlan> {
    CapturePlan::new("fixture:radio:v1", 0, i64::MAX, 1_048_576)
}

#[test]
fn intent_is_finite_and_idempotency_cannot_restart_a_terminal_job() -> TestResult {
    for (start, end, bytes) in [(-1, 2, 3), (1, 1, 3), (2, 1, 3), (0, 1, 0), (0, 1, -1)] {
        assert!(CapturePlan::new("fixture:v1", start, end, bytes).is_err());
    }
    assert!(CapturePlan::new("https://example.invalid/secret", 0, 1, 1).is_err());
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    let first = store.create_capture("capture-a", &plan()?)?;
    assert!(first.newly_created);
    assert!(!store.create_capture("capture-a", &plan()?)?.newly_created);
    let changed = CapturePlan::new("fixture:radio:v2", 0, i64::MAX, 1_048_576)?;
    assert!(matches!(
        store.create_capture("capture-a", &changed),
        Err(Error::IdempotencyConflict)
    ));
    let cancelled = store.transition_capture(&first.job.version, E::Cancel, "user_cancelled")?;
    assert_eq!(cancelled.state, S::Cancelled);
    let replay = store.create_capture("capture-a", &plan()?)?;
    assert!(!replay.newly_created);
    assert_eq!(replay.job, cancelled);
    assert!(
        store
            .transition_capture(&cancelled.version, E::Start, "restart")
            .is_err()
    );
    assert_eq!(store.capture_history("capture-a", None, 64)?.len(), 2);
    store.audit_captures()?;
    Ok(())
}

#[test]
fn restart_interrupts_active_work_without_replaying_or_completing_it() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let old_worker = {
        let mut store = Store::open(&path)?;
        let job = store.create_capture("active", &plan()?)?.job;
        store.create_capture("waiting", &plan()?)?;
        let starting = store.transition_capture(&job.version, E::Start, "admitted")?;
        store
            .transition_capture(&starting.version, E::Connected, "source_open")?
            .version
    };
    let mut store = Store::open(&path)?;
    assert_eq!(store.recover_captures()?, 1);
    assert_eq!(store.recover_captures()?, 0);
    let recovered = store.capture("active")?.ok_or("missing capture")?;
    assert_eq!(recovered.state, S::Interrupted);
    assert!(recovered.version.generation() > old_worker.generation());
    assert_eq!(
        store.capture("waiting")?.ok_or("missing capture")?.state,
        S::Scheduled
    );
    assert!(matches!(
        store.transition_capture(&old_worker, E::Retry, "late_worker"),
        Err(Error::StaleCapture)
    ));
    let restarted = store.transition_capture(&recovered.version, E::Start, "explicit_restart")?;
    assert!(restarted.version.generation() > recovered.version.generation());
    let stopping = store.transition_capture(&restarted.version, E::Stop, "user_stop")?;
    assert!(
        store
            .transition_capture(&stopping.version, E::Finalized, "worker_exit")
            .is_err()
    );
    assert_eq!(
        store.capture("active")?.ok_or("missing capture")?.state,
        S::Stopping
    );
    assert_eq!(store.recover_captures()?, 1);
    let interrupted = store.capture("active")?.ok_or("missing capture")?;
    let cancelled = store.transition_capture(&interrupted.version, E::Cancel, "user_cancelled")?;
    assert_eq!(cancelled.state, S::Cancelled);
    let history = store.capture_history("active", None, 64)?;
    assert_eq!(history.len(), 8);
    assert_eq!(history[3].reason, "service_recovery");
    assert!(history.iter().all(|event| event.state != S::Completed));
    store.audit_captures()?;
    Ok(())
}

#[test]
fn active_admission_is_atomic_across_competing_connections() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    let active = store.create_capture("active", &plan()?)?.job;
    store.transition_capture(&active.version, E::Start, "admitted")?;
    let mut contenders = Vec::new();
    for id in ["left", "right"] {
        let job = store.create_capture(id, &plan()?)?.job;
        contenders.push((Store::open(&path)?, job.version));
    }
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = contenders
        .into_iter()
        .map(|(mut store, version)| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.transition_capture(&version, E::Start, "admitted")
            })
        })
        .collect();
    let mut admitted = 0;
    for worker in workers {
        match worker.join().map_err(|_| "worker panicked")? {
            Ok(_) => admitted += 1,
            Err(Error::CaptureCapacity) => (),
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!(admitted, 1);
    assert_eq!(store.capture_counts()?.active, 2);
    assert_eq!(store.capture_counts()?.scheduled, 1);
    store.audit_captures()?;
    Ok(())
}

#[test]
fn replayed_start_and_stale_acknowledgments_cannot_dispatch_twice() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    let scheduled = store.create_capture("job", &plan()?)?.job;
    let first = store.transition_capture(&scheduled.version, E::Start, "admitted")?;
    assert!(matches!(
        store.transition_capture(&scheduled.version, E::Start, "replay"),
        Err(Error::StaleCapture)
    ));
    let retrying = store.transition_capture(&first.version, E::Retry, "connection_failed")?;
    let second = store.transition_capture(&retrying.version, E::Start, "retry_admitted")?;
    assert!(second.version.generation() > first.version.generation());
    assert!(matches!(
        store.transition_capture(&first.version, E::Connected, "late_connection"),
        Err(Error::StaleCapture)
    ));
    store.transition_capture(&second.version, E::Connected, "source_open")?;
    store.audit_captures()?;
    Ok(())
}

#[test]
fn journal_failure_rolls_back_creation_transition_and_recovery() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    let fault = rusqlite::Connection::open(&path)?;
    fault.execute_batch("CREATE TRIGGER injected_creation_failure BEFORE INSERT ON capture_events WHEN NEW.revision = 0 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")?;
    assert!(store.create_capture("absent", &plan()?).is_err());
    assert!(store.capture("absent")?.is_none());
    fault.execute_batch("DROP TRIGGER injected_creation_failure;")?;
    let mut started = Vec::new();
    for id in ["a", "b"] {
        let scheduled = store.create_capture(id, &plan()?)?.job;
        started.push(store.transition_capture(&scheduled.version, E::Start, "admitted")?);
    }
    fault.execute_batch("CREATE TRIGGER injected_transition_failure BEFORE INSERT ON capture_events WHEN NEW.job_id = 'b' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")?;
    assert!(
        store
            .transition_capture(&started[1].version, E::Connected, "source_open")
            .is_err()
    );
    assert_eq!(store.capture("b")?, Some(started[1].clone()));
    assert!(store.recover_captures().is_err());
    assert_eq!(store.capture("a")?, Some(started[0].clone()));
    assert_eq!(store.capture("b")?, Some(started[1].clone()));
    fault.execute_batch("DROP TRIGGER injected_transition_failure;")?;
    assert_eq!(store.recover_captures()?, 2);
    store.audit_captures()?;
    Ok(())
}

#[test]
fn windows_and_queue_limits_fail_closed_with_bounded_pagination() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    let expired = CapturePlan::new("fixture:v1", 0, 1, 1)?;
    let job = store.create_capture("expired", &expired)?.job;
    assert!(
        store
            .transition_capture(&job.version, E::Start, "admitted")
            .is_err()
    );
    store.transition_capture(&job.version, E::Fail, "missed_window")?;
    let future = CapturePlan::new("fixture:v1", i64::MAX - 1, i64::MAX, 1)?;
    let job = store.create_capture("future", &future)?.job;
    assert!(
        store
            .transition_capture(&job.version, E::Start, "admitted")
            .is_err()
    );
    for index in 1..MAX_PENDING_CAPTURES {
        store.create_capture(&format!("job-{index:04}"), &plan()?)?;
    }
    assert!(matches!(
        store.create_capture("overflow", &plan()?),
        Err(Error::CaptureCapacity)
    ));
    assert!(!store.create_capture("future", &future)?.newly_created);
    assert!(store.captures(None, 0).is_err());
    assert!(store.captures(None, 65).is_err());
    assert!(store.capture_history("future", Some(-1), 1).is_err());
    let first = store.captures(None, 2)?;
    let next = store.captures(Some(first[1].version.id()), 2)?;
    assert_eq!(first.len(), 2);
    assert_eq!(next.len(), 2);
    assert!(first[1].version.id() < next[0].version.id());
    assert_eq!(store.capture_history("expired", None, 1)?[0].revision, 0);
    assert_eq!(store.capture_history("expired", Some(0), 1)?[0].revision, 1);
    assert_eq!(
        store.capture_counts()?.scheduled,
        u64::from(MAX_PENDING_CAPTURES)
    );
    store.audit_captures()?;
    Ok(())
}

#[test]
fn immutable_intent_and_journal_are_enforced_and_projection_damage_is_detected() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.create_capture("job", &plan()?)?;
    let connection = rusqlite::Connection::open(&path)?;
    for sql in [
        "UPDATE capture_jobs SET maximum_bytes = 2",
        "UPDATE capture_jobs SET source_revision = 'changed'",
        "UPDATE capture_events SET reason = 'rewritten'",
        "DELETE FROM capture_events",
    ] {
        assert!(connection.execute(sql, []).is_err());
    }
    connection.execute("UPDATE capture_jobs SET generation = generation + 1", [])?;
    assert!(matches!(
        store.audit_captures(),
        Err(Error::CaptureIntegrity)
    ));
    drop(store);
    assert!(matches!(Store::open(&path), Err(Error::CaptureIntegrity)));
    Ok(())
}

#[test]
fn journal_audit_rejects_a_forged_but_contiguous_transition() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    let job = store.create_capture("job", &plan()?)?.job;
    store.transition_capture(&job.version, E::Cancel, "user_cancelled")?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("DROP TRIGGER capture_events_no_update; UPDATE capture_events SET event = 'connected', state = 'running' WHERE revision = 1; UPDATE capture_jobs SET state = 'running';")?;
    assert!(matches!(
        store.audit_captures(),
        Err(Error::CaptureIntegrity)
    ));
    Ok(())
}

#[test]
fn migration_preserves_existing_liabilities_and_rolls_back_on_failure() -> TestResult {
    let directory = tempfile::tempdir()?;
    for fail in [false, true] {
        let path = directory.path().join(if fail {
            "failure.sqlite3"
        } else {
            "old.sqlite3"
        });
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(include_str!("../src/storage/001-foundation.sql"))?;
        connection.execute_batch("UPDATE budgets SET limit_micros = 1000000, reserved_micros = 600000; INSERT INTO requests(id, context, maximum_micros, state, created_ms) VALUES ('pending', 'provider:v1', 600000, 'submitted', 1); INSERT INTO request_budgets(request_id, budget_id) VALUES ('pending', 'global');")?;
        if fail {
            connection.execute("CREATE TABLE capture_jobs(conflict TEXT)", [])?;
        }
        let migrated = Store::open(&path);
        if fail {
            assert!(migrated.is_err());
            assert_eq!(
                connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
                1
            );
            assert_eq!(
                connection.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = 'capture_events'",
                    [],
                    |row| row.get::<_, u32>(0)
                )?,
                0
            );
        } else {
            let migrated = migrated?;
            assert_eq!(
                migrated.budget("global")?.reserved().to_string(),
                "0.600000"
            );
            assert_eq!(
                connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
                sigy_service::storage::SCHEMA_VERSION
            );
            assert_eq!(migrated.capture_counts()?.scheduled, 0);
            migrated.audit_captures()?;
        }
    }
    Ok(())
}
