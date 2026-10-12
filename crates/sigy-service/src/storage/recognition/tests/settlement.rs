//! SQLite hooks inject response-read and commit faults in disposable catalogs.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use rusqlite::hooks::{AuthContext, Authorization};

use super::*;

pub(super) fn deny_reads_after_commit(connection: &Connection) -> Result<Arc<AtomicBool>> {
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    connection.commit_hook(Some(move || {
        observed.store(true, Ordering::SeqCst);
        false
    }))?;
    deny_reads_when(connection, Arc::clone(&committed))?;
    Ok(committed)
}

pub(super) fn deny_reads_after_update(connection: &Connection, table: &'static str) -> Result<()> {
    let updated = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&updated);
    connection.update_hook(Some(move |_, _: &str, changed: &str, _| {
        if changed == table {
            observed.store(true, Ordering::SeqCst);
        }
    }))?;
    deny_reads_when(connection, updated)
}

fn deny_reads_when(connection: &Connection, signal: Arc<AtomicBool>) -> Result<()> {
    connection.authorizer(Some(move |context: AuthContext<'_>| {
        if signal.load(Ordering::SeqCst)
            && matches!(context.action, rusqlite::hooks::AuthAction::Read { .. })
        {
            Authorization::Deny
        } else {
            Authorization::Allow
        }
    }))?;
    Ok(())
}

pub(super) fn clear_hooks(connection: &Connection) -> Result<()> {
    connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?;
    connection.update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>)?;
    connection.commit_hook(None::<fn() -> bool>)?;
    Ok(())
}

#[test]
fn recognition_claim_and_completion_need_no_read_after_commit() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    store.enqueue_local_asr(&request("asr", 0), 20)?;
    let committed = deny_reads_after_commit(&store.connection)?;
    let work = store
        .claim_local_asr("asr", super::super::super::analysis_jobs::TEST_OWNER, 21)?
        .ok_or("claim")?;
    assert!(committed.load(Ordering::SeqCst));
    assert!(store.local_asr_job("asr").is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.local_asr_job("asr")?, work.job);
    let committed = deny_reads_after_commit(&store.connection)?;
    let job = store.finish_local_asr(&work, &proof(&work, "speech"), 22)?;
    assert!(committed.load(Ordering::SeqCst));
    assert!(store.local_asr_job("asr").is_err());
    clear_hooks(&store.connection)?;
    assert_eq!(store.local_asr_job("asr")?, job);
    assert_eq!(counts(&store)?, (1, 1, 1, 1));
    Ok(())
}

#[test]
fn recognition_response_read_failure_rolls_back_claim_and_publication() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    store.enqueue_local_asr(&request("asr", 0), 20)?;
    deny_reads_after_update(&store.connection, "analysis_jobs")?;
    assert!(
        store
            .claim_local_asr("asr", super::super::super::analysis_jobs::TEST_OWNER, 21)
            .is_err()
    );
    clear_hooks(&store.connection)?;
    assert_eq!(store.local_asr_job("asr")?.state, "queued");
    let work = store
        .claim_local_asr("asr", super::super::super::analysis_jobs::TEST_OWNER, 21)?
        .ok_or("claim")?;
    deny_reads_after_update(&store.connection, "analysis_jobs")?;
    assert!(
        store
            .finish_local_asr(&work, &proof(&work, "speech"), 22)
            .is_err()
    );
    clear_hooks(&store.connection)?;
    assert_eq!(store.local_asr_job("asr")?, work.job);
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    assert!(store.begin_delete("one", false).is_err());
    assert_eq!(
        store
            .finish_local_asr(&work, &proof(&work, "speech"), 23)?
            .state,
        "succeeded"
    );
    Ok(())
}

#[test]
fn recognition_commit_refusal_returns_no_claim_or_partial_result() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    store.enqueue_local_asr(&request("asr", 0), 20)?;
    store.connection.commit_hook(Some(|| true))?;
    assert!(
        store
            .claim_local_asr("asr", super::super::super::analysis_jobs::TEST_OWNER, 21)
            .is_err()
    );
    clear_hooks(&store.connection)?;
    assert_eq!(store.local_asr_job("asr")?.state, "queued");
    let work = store
        .claim_local_asr("asr", super::super::super::analysis_jobs::TEST_OWNER, 21)?
        .ok_or("claim")?;
    store.connection.commit_hook(Some(|| true))?;
    assert!(
        store
            .finish_local_asr(&work, &proof(&work, "speech"), 22)
            .is_err()
    );
    clear_hooks(&store.connection)?;
    assert_eq!(store.local_asr_job("asr")?, work.job);
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    assert!(store.begin_delete("one", false).is_err());
    Ok(())
}
