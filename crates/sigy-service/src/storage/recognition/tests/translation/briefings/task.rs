//! A task briefing binds exact owned findings and the selected coverage observation.

use super::*;
use crate::storage::briefings::write_task_briefing;

#[test]
fn task_briefing_keeps_exact_membership_and_frozen_coverage_after_policy_changes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    store.publish_finding("fair", "owned", &cite(1), 40)?;
    store.publish_finding("fair", "unrelated", &cite(1), 41)?;
    let (from_ms, to_ms) = window(&store)?;
    let coverage = store.monitor_coverage("fair", from_ms, to_ms)?;
    let before = activity(&store)?;
    let ids = vec!["owned".into()];
    {
        let tx = store.connection.transaction()?;
        write_task_briefing(&tx, "fair", "task-report", &coverage, &ids, 50)?;
        tx.commit()?;
    }
    let first = store.briefing("fair", "task-report")?;
    assert_eq!(first.members.len(), 1);
    assert_eq!(first.members[0].finding_id, "owned");
    assert_eq!(first.coverage, coverage);
    let mut policy = super::super::super::follow("radio:v1");
    policy.daily_audio_seconds = 7200;
    policy.total_audio_seconds = 7200;
    store.revise_monitor("fair", 1, &policy, 60)?;
    write_task_briefing(
        &store.connection,
        "fair",
        "task-report",
        &coverage,
        &ids,
        70,
    )?;
    assert_eq!(store.briefing("fair", "task-report")?, first);
    let current = store.monitor_coverage("fair", from_ms, to_ms)?;
    assert!(matches!(
        write_task_briefing(&store.connection, "fair", "task-report", &current, &ids, 71),
        Err(Error::Analysis("briefing-conflict"))
    ));
    assert!(matches!(
        write_task_briefing(
            &store.connection,
            "fair",
            "task-report",
            &coverage,
            &["unrelated".into()],
            71,
        ),
        Err(Error::Analysis("briefing-conflict"))
    ));
    assert_eq!(activity(&store)?, before);
    drop(store);
    let store = Store::open(&path)?;
    assert_eq!(store.briefing("fair", "task-report")?, first);
    Ok(())
}

#[test]
fn task_briefing_refuses_duplicate_oversized_and_cross_monitor_membership() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "owned", &cite(1), 40)?;
    store.create_monitor("other", &super::super::super::follow("radio:v1"), 41)?;
    store.publish_finding("other", "foreign", &cite(1), 42)?;
    let (from_ms, to_ms) = window(&store)?;
    let coverage = store.monitor_coverage("fair", from_ms, to_ms)?;
    assert!(matches!(
        write_task_briefing(
            &store.connection,
            "fair",
            "bad",
            &coverage,
            &["owned".into(), "owned".into()],
            50
        ),
        Err(Error::Analysis("task-briefing-duplicate"))
    ));
    let large = (0..65).map(|n| format!("finding-{n}")).collect::<Vec<_>>();
    assert!(matches!(
        write_task_briefing(&store.connection, "fair", "bad", &coverage, &large, 50),
        Err(Error::Analysis("task-briefing-limit"))
    ));
    assert!(matches!(
        write_task_briefing(
            &store.connection,
            "fair",
            "bad",
            &coverage,
            &["foreign".into()],
            50
        ),
        Err(Error::NotFound)
    ));
    assert_eq!(briefings(&store)?, 0);
    Ok(())
}

#[test]
fn task_briefing_respects_capacity_and_replays_when_the_store_is_full() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    let (from_ms, to_ms) = window(&store)?;
    let coverage = store.monitor_coverage("fair", from_ms, to_ms)?;
    {
        let tx = store.connection.transaction()?;
        for ordinal in 0..1024 {
            write_task_briefing(&tx, "fair", &format!("full-{ordinal}"), &coverage, &[], 50)?;
        }
        tx.commit()?;
    }
    write_task_briefing(&store.connection, "fair", "full-0", &coverage, &[], 51)?;
    assert!(matches!(
        write_task_briefing(&store.connection, "fair", "overflow", &coverage, &[], 51),
        Err(Error::Analysis("briefing-limit"))
    ));
    assert_eq!(briefings(&store)?, 1024);
    store.audit_briefings()?;
    Ok(())
}

#[test]
fn a_failed_task_snapshot_rolls_back_its_header_and_members_with_the_caller() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "owned", &cite(1), 40)?;
    let (from_ms, to_ms) = window(&store)?;
    let coverage = store.monitor_coverage("fair", from_ms, to_ms)?;
    let ids = vec!["owned".into()];
    store.connection.execute_batch(
        "CREATE TRIGGER task_snapshot_fault BEFORE INSERT ON monitor_briefing_sources BEGIN SELECT RAISE(ABORT, 'fixture-snapshot-fault'); END;",
    )?;
    {
        let tx = store.connection.transaction()?;
        assert!(write_task_briefing(&tx, "fair", "report", &coverage, &ids, 50).is_err());
    }
    assert_eq!(briefings(&store)?, 0);
    let members: i64 =
        store
            .connection
            .query_row("SELECT count(*) FROM monitor_briefing_members", [], |row| {
                row.get(0)
            })?;
    assert_eq!(members, 0);
    store
        .connection
        .execute_batch("DROP TRIGGER task_snapshot_fault;")?;
    {
        let tx = store.connection.transaction()?;
        write_task_briefing(&tx, "fair", "report", &coverage, &ids, 51)?;
        tx.commit()?;
    }
    assert_eq!(store.briefing("fair", "report")?.generation, 1);
    assert_eq!(store.briefing("fair", "report")?.members.len(), 1);
    Ok(())
}
