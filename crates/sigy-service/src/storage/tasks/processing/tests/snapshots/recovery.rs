//! Snapshot transaction faults, bounded lock waits and retained historical observations.

use super::*;
use std::time::{Duration, Instant};

#[test]
fn busy_snapshot_rolls_back_and_restores_connection_for_the_next_request() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, _) = recognized(&path)?;
    let start = clock(&store)?;
    store.connection.busy_timeout(Duration::from_millis(37))?;
    let owner = rusqlite::Connection::open(&path)?;
    owner.execute_batch("BEGIN IMMEDIATE")?;
    let began = Instant::now();
    assert!(
        store
            .freeze_task_evidence("task", "blocked", 0, start + 93_000)
            .is_err()
    );
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(
        count(&store, "SELECT count(*) FROM task_evidence_snapshots")?,
        0
    );
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "busy_timeout", |r| r.get::<_, u32>(0))?,
        37
    );
    owner.execute_batch("ROLLBACK")?;
    let frozen = store.freeze_task_evidence("task", "blocked", 0, start + 93_000)?;
    assert_eq!(frozen.ordinal, 1);
    assert_eq!(store.task_evidence_snapshot("task", 1)?, frozen);
    Ok(())
}

#[test]
fn whole_recording_deletion_changes_live_media_but_not_the_frozen_snapshot() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, recordings) = recognized(&path)?;
    let start = clock(&store)?;
    let frozen = store.freeze_task_evidence("task", "freeze", 0, start + 93_000)?;
    store.begin_delete(&recordings[0], false)?;
    store.finish_delete(&recordings[0])?;
    assert_eq!(store.recording(&recordings[0])?.storage_state, "deleted");
    assert_eq!(store.task_evidence_snapshot("task", 1)?, frozen);
    drop(store);
    assert_eq!(
        Store::open(&path)?.task_evidence_snapshot("task", 1)?,
        frozen
    );
    Ok(())
}

fn downgrade_to_45(store: &Store) -> Result<()> {
    crate::storage::tasks::run::revert_047_for_tests(&store.connection)?;
    crate::storage::tasks::snapshot::remove_snapshot_schema(store)?;
    store.connection.execute_batch("PRAGMA user_version=45")?;
    Ok(())
}

#[test]
fn populated_v45_migration_preserves_processing_withdrawal_and_legacy_observation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, _) = recognized(&path)?;
    let start = clock(&store)?;
    let withdrawal = store.withdraw_task_processing("task", "withdraw", 1, 0, start + 92_000)?;
    let checkpoint = store.checkpoint_task("task", "legacy", 0, start + 93_000)?;
    let processing = store.task_processing("task")?;
    downgrade_to_45(&store)?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert_eq!(reopened.task_withdrawal("task")?, Some(withdrawal));
    assert_eq!(reopened.task_checkpoint("task", 1)?, checkpoint);
    assert_eq!(reopened.task_processing("task")?, processing);
    let frozen = reopened.freeze_task_evidence("task", "new", 0, start + 94_000)?;
    assert_eq!(frozen.ordinal, 1);
    assert_eq!(reopened.task("task")?.checkpoint, 1);
    assert_eq!(reopened.task_evidence_snapshot("task", 1)?, frozen);
    Ok(())
}

#[test]
fn failed_v46_migration_keeps_v45_history_and_retries_cleanly() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (store, _) = recognized(&path)?;
    let start = clock(&store)?;
    let before = store.task_processing("task")?;
    downgrade_to_45(&store)?;
    store
        .connection
        .execute_batch("CREATE TABLE task_evidence_snapshots(conflict INTEGER)")?;
    drop(store);
    assert!(Store::open(&path).is_err());
    let connection = rusqlite::Connection::open(&path)?;
    assert_eq!(
        connection.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))?,
        45
    );
    assert_eq!(
        connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='task_checkpoint_shared_capacity'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    connection.execute_batch("DROP TABLE task_evidence_snapshots")?;
    drop(connection);
    let mut reopened = Store::open(&path)?;
    assert_eq!(reopened.task_processing("task")?, before);
    reopened.freeze_task_evidence("task", "retry", 0, start + 93_000)?;
    Ok(())
}

#[test]
fn partial_segment_release_reduces_live_bytes_without_rewriting_frozen_media() -> TestResult {
    use crate::storage::dvr::{OPEN_SEGMENT_CEILING, SegmentOpen, SegmentSeal};
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    let mut policy = policy(false);
    policy.capture.as_mut().ok_or("capture policy")?.total_bytes = OPEN_SEGMENT_CEILING * 2;
    store.revise_monitor("monitor", 1, &policy, START - 100)?;
    let mut task = store.task("task")?.spec;
    task.monitor_version = 2;
    store.create_task("segmented", &task, START - 99)?;
    store.start_task_collection(
        "segmented",
        "segmented-grant",
        &TaskCollectionSpec {
            captures: vec![TaskCaptureSpec {
                source_revision: "a:v1".into(),
                start_ms: START + 120_000,
                duration_seconds: 30,
                maximum_bytes: OPEN_SEGMENT_CEILING * 2,
            }],
        },
        0,
        START - 98,
    )?;
    let mut launches = store
        .reconcile_schedules_at(START + 120_000, true)?
        .launches;
    assert_eq!(launches.len(), 1);
    let launch = launches.remove(0);
    let recording = launch.job.version.id().to_owned();
    let mut version = store.connect_recording(&launch.job.version)?;
    let bytes = media(0);
    let seal = SegmentSeal {
        bytes: bytes.len() as u64,
        sha256: crate::recognition::sha256_hex(&bytes),
        format: "wav",
        decoded_microseconds: 15_000_000,
    };
    for _ in 0..2 {
        let SegmentOpen::Opened {
            version: opened, ..
        } = store.open_segment(&version)?
        else {
            return Err("segment budget held".into());
        };
        version = store.seal_segment(&opened, &seal)?;
    }
    let mut publication = publication(0);
    publication.bytes *= 2;
    publication.sha256 = crate::recognition::sha256_hex(&bytes.repeat(2));
    publication.segments_sealed = true;
    store.publish_recording(&version, &publication)?;
    let now = Store::clock_ms()?.max(START + 200_000);
    let frozen = store.freeze_task_evidence("segmented", "freeze", 0, now)?;
    let release = store.next_segment_release(true, now)?.ok_or("release")?;
    assert_eq!(release.id, recording);
    // Materialize the checksum fixture, remove its segment, then commit the release receipt.
    let media_path = directory
        .path()
        .join(format!("{}.media", release.object_key));
    std::fs::write(&media_path, &bytes)?;
    std::fs::remove_file(&media_path)?;
    store.mark_segment_released(&release)?;
    let live = store.recording(&recording)?;
    assert!(live.intervals[0].released);
    assert!(!live.intervals[1].released);
    assert_eq!(live.media_bytes, Some(bytes.len() as u64));
    assert_eq!(frozen.media[0].media_bytes, Some(bytes.len() as u64 * 2));
    assert_eq!(store.task_evidence_snapshot("segmented", 1)?, frozen);
    drop(store);
    assert_eq!(
        Store::open(&path)?.task_evidence_snapshot("segmented", 1)?,
        frozen
    );
    Ok(())
}
