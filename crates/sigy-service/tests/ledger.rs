use std::{
    sync::{Arc, Barrier},
    thread,
};

use sigy_service::{
    Error,
    domain::{budget::BudgetError, money::Usd},
    storage::{
        Store,
        ledger::{Change, RequestState},
    },
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn paid_processing_is_disabled_in_a_new_library() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    assert_eq!(store.budget("global")?.limit(), Usd::ZERO);
    assert!(matches!(
        store.reserve("r1", "model-v1", "0.1".parse()?, &[]),
        Err(Error::Budget(BudgetError::Disabled))
    ));
    assert!(store.reservation("r1")?.is_none());
    store.audit_ledger()?;
    Ok(())
}

#[test]
fn scope_failure_rolls_back_every_reservation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    store.set_budget_limit("global", "10".parse()?)?;
    store.set_budget_limit("monitor:news", "0.2".parse()?)?;
    assert!(matches!(
        store.reserve("r1", "model-v1", "0.3".parse()?, &["monitor:news"]),
        Err(Error::Budget(BudgetError::InsufficientFunds))
    ));
    assert_eq!(store.budget("global")?.reserved(), Usd::ZERO);
    assert_eq!(store.budget("monitor:news")?.reserved(), Usd::ZERO);
    assert!(store.reservation("r1")?.is_none());
    assert!(matches!(
        store.reserve("r2", "model-v1", "0.1".parse()?, &["unknown"]),
        Err(Error::NotFound)
    ));
    assert_eq!(store.budget("global")?.reserved(), Usd::ZERO);
    store.audit_ledger()?;
    Ok(())
}

#[test]
fn concurrent_requests_cannot_spend_the_same_remaining_balance() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.set_budget_limit("global", "1".parse()?)?;
    let connections: Vec<Store> = (0..8)
        .map(|_| Store::open(&path))
        .collect::<Result<_, _>>()?;
    let barrier = Arc::new(Barrier::new(connections.len()));
    let mut workers = Vec::new();
    for (index, mut connection) in connections.into_iter().enumerate() {
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            barrier.wait();
            connection.reserve(
                &format!("r{index}"),
                "model-v1",
                Usd::from_micros(600_000)?,
                &[],
            )
        }));
    }
    let mut accepted = 0;
    for worker in workers {
        match worker
            .join()
            .map_err(|_| std::io::Error::other("worker panicked"))?
        {
            Ok(admission) => {
                assert!(admission.newly_reserved);
                accepted += 1;
            }
            Err(Error::Budget(BudgetError::InsufficientFunds)) => (),
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!(accepted, 1);
    assert_eq!(store.budget("global")?.reserved(), "0.6".parse()?);
    assert_eq!(store.budget("global")?.available(), "0.4".parse()?);
    store.audit_ledger()?;
    Ok(())
}

#[test]
fn idempotency_requires_identical_operation_amount_and_scopes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    store.set_budget_limit("global", "5".parse()?)?;
    store.set_budget_limit("provider:a", "5".parse()?)?;
    let first = store.reserve("r1", "model-v1", "0.5".parse()?, &["provider:a"])?;
    assert!(first.newly_reserved);
    let again = store.reserve(
        "r1",
        "model-v1",
        "0.5".parse()?,
        &["global", "provider:a", "provider:a"],
    )?;
    assert!(!again.newly_reserved);
    assert_eq!(first.reservation, again.reservation);
    assert!(matches!(
        store.reserve("r1", "model-v2", "0.5".parse()?, &["provider:a"]),
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        store.reserve("r1", "model-v1", "0.6".parse()?, &["provider:a"]),
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        store.reserve("r1", "model-v1", "0.5".parse()?, &[]),
        Err(Error::IdempotencyConflict)
    ));
    assert_eq!(store.budget("global")?.reserved(), "0.5".parse()?);
    store.audit_ledger()?;
    Ok(())
}

#[test]
fn restart_retains_unknown_liability_and_prevents_resubmission() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    {
        let mut store = Store::open(&path)?;
        store.set_budget_limit("global", "1".parse()?)?;
        store.reserve("r1", "model-v1", "1".parse()?, &[])?;
        assert_eq!(store.mark_submitted("r1")?, Change::Applied);
        assert_eq!(store.mark_submitted("r1")?, Change::Unchanged);
    }
    let mut recovered = Store::open(&path)?;
    assert_eq!(recovered.recover_submitted()?, 1);
    assert_eq!(recovered.recover_submitted()?, 0);
    assert_eq!(
        recovered.reservation("r1")?.ok_or("missing request")?.state,
        RequestState::Uncertain
    );
    assert_eq!(recovered.budget("global")?.reserved(), "1".parse()?);
    assert!(matches!(
        recovered.release_unsubmitted("r1"),
        Err(Error::RequestState)
    ));
    assert!(matches!(
        recovered.mark_submitted("r1"),
        Err(Error::RequestState)
    ));
    assert!(matches!(
        recovered.set_budget_limit("global", Usd::ZERO),
        Err(Error::Budget(BudgetError::InsufficientFunds))
    ));
    assert_eq!(
        recovered.settle("r1", "0.3".parse()?, "receipt:1")?,
        Change::Applied
    );
    assert_eq!(
        recovered.settle("r1", "0.3".parse()?, "receipt:1")?,
        Change::Unchanged
    );
    assert!(matches!(
        recovered.settle("r1", "0.4".parse()?, "receipt:1"),
        Err(Error::IdempotencyConflict)
    ));
    assert_eq!(recovered.budget("global")?.available(), "0.7".parse()?);
    recovered.audit_ledger()?;
    Ok(())
}

#[test]
fn cancelling_an_unsubmitted_request_cannot_make_its_key_reusable() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    store.set_budget_limit("global", "1".parse()?)?;
    store.reserve("r1", "model-v1", "0.5".parse()?, &[])?;
    assert_eq!(store.release_unsubmitted("r1")?, Change::Applied);
    assert_eq!(store.release_unsubmitted("r1")?, Change::Unchanged);
    let replay = store.reserve("r1", "model-v1", "0.5".parse()?, &[])?;
    assert!(!replay.newly_reserved);
    assert_eq!(replay.reservation.state, RequestState::Released);
    assert!(store.mark_submitted("r1").is_err());
    assert_eq!(store.budget("global")?.available(), "1".parse()?);
    store.audit_ledger()?;
    Ok(())
}

#[test]
fn an_overcharge_is_recorded_and_freezes_every_affected_scope() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.set_budget_limit("global", "1".parse()?)?;
    store.set_budget_limit("provider:a", "2".parse()?)?;
    store.reserve("r1", "model-v1", "0.5".parse()?, &["provider:a"])?;
    store.mark_submitted("r1")?;
    store.settle("r1", "1.5".parse()?, "receipt:overcharge")?;
    drop(store);
    let mut store = Store::open(&path)?;
    for scope in ["global", "provider:a"] {
        assert_eq!(store.budget(scope)?.settled(), "1.5".parse()?);
        assert!(store.budget(scope)?.frozen());
        assert_eq!(store.budget(scope)?.reserved(), Usd::ZERO);
    }
    store.set_budget_limit("global", "10".parse()?)?;
    assert!(matches!(
        store.reserve("r2", "model-v1", "0.1".parse()?, &[]),
        Err(Error::Budget(BudgetError::Frozen))
    ));
    store.audit_ledger()?;
    Ok(())
}

#[test]
fn event_write_failure_rolls_back_state_and_all_accounting() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.set_budget_limit("global", "1".parse()?)?;
    store.reserve("r1", "model-v1", "0.5".parse()?, &[])?;
    store.mark_submitted("r1")?;
    let fault = rusqlite::Connection::open(&path)?;
    fault.execute_batch("CREATE TRIGGER injected_failure BEFORE INSERT ON ledger_events WHEN NEW.kind = 'settled' BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END;")?;
    assert!(store.settle("r1", "0.2".parse()?, "receipt:1").is_err());
    assert_eq!(store.budget("global")?.settled(), Usd::ZERO);
    assert_eq!(store.budget("global")?.reserved(), "0.5".parse()?);
    assert_eq!(
        store.reservation("r1")?.ok_or("missing request")?.state,
        RequestState::Submitted
    );
    store.audit_ledger()?;
    fault.execute_batch("DROP TRIGGER injected_failure;")?;
    store.settle("r1", "0.2".parse()?, "receipt:1")?;
    assert_eq!(store.budget("global")?.available(), "0.8".parse()?);
    Ok(())
}

#[test]
fn ledger_events_cannot_be_rewritten_or_deleted() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.set_budget_limit("global", "1".parse()?)?;
    let connection = rusqlite::Connection::open(&path)?;
    assert!(
        connection
            .execute("UPDATE ledger_events SET amount_micros = 0", [])
            .is_err()
    );
    assert!(connection.execute("DELETE FROM ledger_events", []).is_err());
    Ok(())
}

#[test]
fn inconsistent_projection_and_future_schema_fail_closed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.set_budget_limit("global", "1".parse()?)?;
    store.reserve("r1", "model-v1", "0.5".parse()?, &[])?;
    drop(store);
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute(
        "UPDATE budgets SET reserved_micros = 0 WHERE id = 'global'",
        [],
    )?;
    assert!(matches!(Store::open(&path), Err(Error::LedgerIntegrity)));
    connection.pragma_update(None, "user_version", 99)?;
    assert!(matches!(
        Store::open(&path),
        Err(Error::FutureSchema { found: 99, .. })
    ));
    Ok(())
}
