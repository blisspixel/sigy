//! Encoder stress uses cloned legal observations, never fabricated catalog lineage.

use super::*;
use crate::task::snapshot::MAX_SNAPSHOT_BYTES;

#[test]
fn oversized_required_metadata_is_refused_without_replacing_history() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, _) = recognized(&directory.path().join("catalog"))?;
    let start = clock(&store)?;
    let frozen = store.freeze_task_evidence("task", "freeze", 0, start + 93_000)?;
    let mut oversized = frozen.clone();
    oversized.scope.goal = "\"".repeat(MAX_SNAPSHOT_BYTES);
    assert!(matches!(
        crate::storage::tasks::snapshot::fit_for_test(&mut oversized),
        Err(Error::InvalidInput("task snapshot mandatory size"))
    ));
    assert_eq!(store.task_evidence_snapshot("task", 1)?, frozen);
    assert_eq!(
        count(&store, "SELECT count(*) FROM task_evidence_snapshots")?,
        1
    );
    Ok(())
}

#[test]
fn byte_truncation_makes_pending_partial_and_preserves_required_coverage() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, recordings) = recognized(&directory.path().join("catalog"))?;
    let start = clock(&store)?;
    let request = translation(&store, &recordings[0], 1)?;
    store.enqueue_task_translation(&scope(0, &recordings[0], 0), &request, start + 92_000)?;
    let frozen = store.freeze_task_evidence("task", "freeze", 0, start + 93_000)?;
    assert_eq!(frozen.evidence.outcome, TaskOutcome::Pending);
    let mut oversized = frozen.clone();
    oversized.evidence.citations[0].source = "x".repeat(MAX_SNAPSHOT_BYTES);
    let json = crate::storage::tasks::snapshot::fit_for_test(&mut oversized)?;
    assert!(json.len() <= MAX_SNAPSHOT_BYTES);
    assert_eq!(oversized.evidence.outcome, TaskOutcome::Partial);
    assert!(oversized.evidence.more);
    assert!(oversized.evidence.citations.is_empty());
    assert!(
        oversized
            .evidence
            .reasons
            .iter()
            .any(|r| r == "snapshot-citations-truncated")
    );
    assert_eq!(oversized.evidence.entries, frozen.evidence.entries);
    assert_eq!(
        oversized.evidence.recognized_us,
        frozen.evidence.recognized_us
    );
    assert_eq!(oversized.jobs, frozen.jobs);
    assert_eq!(oversized.media, frozen.media);
    assert_eq!(store.task_evidence_snapshot("task", 1)?, frozen);
    Ok(())
}

#[test]
fn waiting_collection_freezes_missing_coverage_without_fabricating_recordings() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let frozen = store.freeze_task_evidence("task", "waiting", 0, START - 500)?;
    assert_eq!(frozen.evidence.outcome, TaskOutcome::Pending);
    assert_eq!(frozen.evidence.planned_us, 60_000_000);
    assert_eq!(frozen.evidence.recorded_us, 0);
    assert_eq!(frozen.evidence.recognized_us, 0);
    assert_eq!(frozen.evidence.uncovered_us, 60_000_000);
    assert_eq!(frozen.evidence.entries.len(), 2);
    assert!(
        frozen
            .evidence
            .entries
            .iter()
            .all(|e| e.recording_id.is_none())
    );
    assert!(frozen.jobs.is_empty());
    assert!(frozen.media.is_empty());
    assert!(frozen.evidence.citations.is_empty());
    assert_eq!(store.task_evidence_snapshot("task", 1)?, frozen);
    collect(&mut store)?;
    assert_eq!(store.task_evidence_snapshot("task", 1)?, frozen);
    Ok(())
}

#[test]
fn mandatory_snapshot_exactly_at_byte_limit_fits_but_one_more_byte_is_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, _) = recognized(&directory.path().join("catalog"))?;
    let start = clock(&store)?;
    let mut frozen = store.freeze_task_evidence("task", "freeze", 0, start + 93_000)?;
    // Encoder-only stress: discard optional citations and pad an ASCII metadata field.
    // The modified clone is never inserted or presented as valid task scope.
    frozen.evidence.citations.clear();
    frozen.scope.goal.clear();
    let overhead = serde_json::to_vec(&frozen)?.len();
    frozen.scope.goal = "x".repeat(MAX_SNAPSHOT_BYTES.checked_sub(overhead).ok_or("overhead")?);
    assert_eq!(
        crate::storage::tasks::snapshot::fit_for_test(&mut frozen)?.len(),
        MAX_SNAPSHOT_BYTES
    );
    frozen.scope.goal.push('x');
    assert!(matches!(
        crate::storage::tasks::snapshot::fit_for_test(&mut frozen),
        Err(Error::InvalidInput("task snapshot mandatory size"))
    ));
    Ok(())
}

#[test]
fn rehashed_zero_or_impossible_frozen_media_bytes_fail_lineage_audit() -> TestResult {
    for bytes in [0, u64::MAX] {
        let directory = tempfile::tempdir()?;
        let (mut store, _) = recognized(&directory.path().join("catalog"))?;
        let start = clock(&store)?;
        let mut frozen = store.freeze_task_evidence("task", "freeze", 0, start + 93_000)?;
        frozen.media[0].media_bytes = Some(bytes);
        let json = serde_json::to_string(&frozen)?;
        let digest = crate::recognition::sha256_hex(
            format!("[\"sigy-task-evidence-snapshot-v1\",{json}]").as_bytes(),
        );
        store
            .connection
            .execute_batch("DROP TRIGGER task_evidence_snapshot_no_update")?;
        store.connection.execute("UPDATE task_evidence_snapshots SET payload_json=?1,payload_sha256=?2 WHERE task_id='task' AND ordinal=1", rusqlite::params![json,digest])?;
        assert!(matches!(
            store.task_evidence_snapshot("task", 1),
            Err(Error::StorageIntegrity)
        ));
    }
    Ok(())
}

#[test]
fn production_query_budget_interrupts_sql_and_restores_the_connection() -> TestResult {
    use crate::storage::query_work::{Limits, QueryWork};
    let connection = rusqlite::Connection::open_in_memory()?;
    connection.busy_timeout(std::time::Duration::from_millis(37))?;
    let work = QueryWork::start(&connection, Limits::TASK_EVIDENCE)?;
    let result = connection.query_row(
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n",
        [], |row| row.get::<_, i64>(0),
    );
    assert!(
        matches!(result, Err(rusqlite::Error::SqliteFailure(ref error, _)) if error.code == rusqlite::ErrorCode::OperationInterrupted)
    );
    assert!(work.exhausted());
    assert!(work.checkpoint_ops() <= Limits::TASK_EVIDENCE.vm_ops);
    work.finish()?;
    assert_eq!(
        connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))?,
        37
    );
    assert_eq!(
        connection.query_row("SELECT 7", [], |row| row.get::<_, i64>(0))?,
        7
    );
    Ok(())
}
