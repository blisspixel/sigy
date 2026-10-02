//! Migration, rollback, restore and hostile catalog edits around v44 processing records.

use super::*;
use crate::storage::monitors::MonitorJobScope;

fn monitor_scope(recording: &str, audio_us: u64) -> MonitorJobScope<'_> {
    MonitorJobScope {
        monitor_id: "monitor",
        policy_version: 1,
        action_count: 0,
        recording_id: recording,
        audio_us,
    }
}

/// Direct, monitor and shared jobs as a v43 service would leave them.
fn populate_history(store: &mut Store) -> Result<Vec<String>> {
    let recordings = collect(store)?;
    let now = START + 90_000;
    let first = recognition(store, &recordings[0], now)?;
    store.enqueue_monitor_recognition(&monitor_scope(&recordings[0], AUDIO[0]), &first, now)?;
    let second = recognition(store, &recordings[1], now)?;
    store.enqueue_local_asr(&second, now)?;
    store.enqueue_monitor_recognition(
        &monitor_scope(&recordings[1], AUDIO[1]),
        &second,
        now + 1,
    )?;
    let direct = LocalAsrRequest {
        id: "direct-only".into(),
        ..second.clone()
    };
    store.enqueue_local_asr(&direct, now + 2)?;
    hear(store, &first.id, "Informe sobre el agua", now + 3)?;
    let mt = translation(store, &recordings[0], 1)?;
    store.enqueue_monitor_translation(&monitor_scope(&recordings[0], 0), &mt, now + 4)?;
    let direct_mt = TranslationRequest {
        id: "direct-mt".into(),
        ..mt
    };
    store.enqueue_translation(&direct_mt, now + 5)?;
    Ok(recordings)
}

fn interests(store: &Store) -> Result<Vec<String>> {
    let mut statement = store.connection.prepare("SELECT family || '|' || job_id || '|' || authority || '|' || owner_id || '|' || origin || '|' || created_ms FROM job_interests ORDER BY family, job_id, authority, owner_id")?;
    Ok(statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

#[test]
fn populated_v43_catalog_migrates_with_labeled_derived_interests() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, true)?;
    let recordings = populate_history(&mut store)?;
    let admitted = interests(&store)?;
    assert_eq!(admitted.len(), 6);
    assert!(admitted.iter().all(|row| row.contains("|admitted|")));
    let jobs = effects(&store)?;
    store.connection.execute_batch(REVERT_044_FOR_TESTS)?;
    store.connection.execute_batch("PRAGMA user_version = 43")?;
    drop(store);
    let reopened = Store::open(&path)?;
    let migrated = interests(&reopened)?;
    let profile = recognition_profile()?.profile_sha256;
    let shared = pipeline::recognition_job_id(&recordings[1], &profile);
    // The pre-v44 direct receipt of a job a monitor later attached to was never stored.
    assert_eq!(
        migrated,
        admitted
            .iter()
            .filter(|row| !row.starts_with(&format!("recognition|{shared}|direct|")))
            .map(|row| row.replace("|admitted|", "|migrated|"))
            .collect::<Vec<_>>()
    );
    assert_eq!(migrated.len(), 5);
    let mut rows = effects(&reopened)?;
    rows.retain(|row| !row.contains("|migrated|"));
    let mut before = jobs;
    before.retain(|row| !row.contains("|admitted|"));
    assert_eq!(rows, before);
    Ok(())
}

#[test]
fn interrupted_v44_migration_leaves_the_v43_catalog_unchanged() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, true)?;
    populate_history(&mut store)?;
    store.connection.execute_batch(REVERT_044_FOR_TESTS)?;
    store.connection.execute_batch(
        "CREATE TABLE task_processing(conflict INTEGER); PRAGMA user_version = 43",
    )?;
    drop(store);
    assert!(Store::open(&path).is_err());
    let connection = rusqlite::Connection::open(&path)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let interests: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'job_interests'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!((version, interests), (43, 0));
    connection.execute_batch("DROP TABLE task_processing")?;
    drop(connection);
    let reopened = Store::open(&path)?;
    assert_eq!(crate::storage::SCHEMA_VERSION, 44);
    assert_eq!(count(&reopened, "SELECT count(*) FROM job_interests")?, 5);
    Ok(())
}

#[test]
fn interests_need_their_job_and_receipt_and_no_job_is_left_without_one() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, true)?;
    let recordings = populate_history(&mut store)?;
    for sql in [
        "INSERT INTO job_interests VALUES ('recognition', 'absent', 'direct', '', 'admitted', 1)",
        "INSERT INTO job_interests VALUES ('translation', 'direct-only', 'direct', '', 'admitted', 1)",
        "INSERT INTO job_interests VALUES ('recognition', 'direct-only', 'monitor', 'monitor', 'admitted', 1)",
        "INSERT INTO job_interests VALUES ('recognition', 'direct-only', 'task', 'task', 'admitted', 1)",
        "INSERT INTO job_interests VALUES ('recognition', 'direct-only', 'direct', 'monitor', 'admitted', 1)",
        "INSERT INTO job_interests VALUES ('recognition', 'direct-only', 'monitor', '', 'admitted', 1)",
    ] {
        assert!(store.connection.execute(sql, []).is_err(), "{sql}");
    }
    let orphan = LocalAsrRequest {
        id: "orphan".into(),
        ..recognition(&mut store, &recordings[1], START + 99_000)?
    };
    let prepared = store.prepare_local_asr_job(&orphan, START + 99_000)?;
    Store::enqueue_local_asr_in(
        &store.connection,
        &orphan,
        prepared.as_ref(),
        START + 99_000,
    )?;
    assert!(matches!(
        store.audit_job_interests(),
        Err(Error::StorageIntegrity)
    ));
    drop(store);
    assert!(matches!(Store::open(&path), Err(Error::StorageIntegrity)));
    Ok(())
}

#[test]
fn processing_and_interest_history_is_immutable_and_hostile_edits_fail_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let now = START + 90_000;
    let request = recognition(&mut store, &recordings[0], now)?;
    store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now)?;
    store.cancel_task_processing("task", "stop", 1, now + 1)?;
    for sql in [
        "UPDATE task_processing SET created_ms = 0",
        "DELETE FROM task_processing",
        "UPDATE task_processing_steps SET audio_us = 1",
        "DELETE FROM task_processing_steps",
        "UPDATE task_processing_cancellations SET created_ms = 0",
        "DELETE FROM task_processing_cancellations",
        "UPDATE job_interests SET authority = 'direct', owner_id = ''",
        "DELETE FROM job_interests",
    ] {
        assert!(store.connection.execute(sql, []).is_err(), "{sql}");
    }
    drop(store);
    for tamper in [
        "DROP TRIGGER job_interests_no_delete; DELETE FROM job_interests;",
        "DROP TRIGGER job_interests_no_update; UPDATE job_interests SET created_ms = created_ms + 1;",
        "DROP TRIGGER task_processing_steps_no_update; UPDATE task_processing_steps SET audio_us = 1000000;",
        "DROP TRIGGER task_processing_no_update; UPDATE task_processing SET maximum_audio_us = 1800000000;",
        "DROP TRIGGER task_processing_cancellations_no_update; UPDATE task_processing_cancellations SET step_mask = 0;",
    ] {
        let copy = directory.path().join("tampered");
        std::fs::copy(&path, &copy)?;
        let connection = rusqlite::Connection::open(&copy)?;
        connection.execute_batch(tamper)?;
        drop(connection);
        assert!(Store::open(&copy).is_err(), "{tamper}");
        std::fs::remove_file(&copy)?;
    }
    Store::open(&path)?;
    Ok(())
}

#[test]
fn backup_and_restore_reproduce_receipts_interests_charges_and_media() -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let mut library = crate::library::Library::open(&root, true)?;
    populate(library.store_mut(), true)?;
    let store = library.store_mut();
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(store)?;
    let now = START + 90_000;
    let request = recognition(store, &recordings[0], now)?;
    store.enqueue_monitor_recognition(&monitor_scope(&recordings[0], AUDIO[0]), &request, now)?;
    store.enqueue_task_recognition(&scope(0, &recordings[0], AUDIO[0]), &request, now + 1)?;
    crate::recordings::checked_directory(library.directory(), true)?;
    for (ordinal, recording) in recordings.iter().enumerate() {
        let saved = library.store().recording(recording)?;
        std::fs::write(
            crate::recordings::media_path(library.directory(), &saved.intervals[0].object_key)?,
            media(ordinal),
        )?;
    }
    let view = library.store().task_processing("task")?;
    let before = effects(library.store())?;
    let backup = directory.path().join("backup");
    let manifest = crate::backup::backup(&library, &backup)?;
    assert_eq!(manifest.schema_version, 44);
    assert_eq!(manifest.media.len(), 2);
    drop(library);
    let restored = directory.path().join("restored");
    crate::backup::restore(&backup, &restored)?;
    let restored = crate::library::Library::open(&restored, false)?;
    assert_eq!(restored.store().task_processing("task")?, view);
    assert_eq!(effects(restored.store())?, before);
    Ok(())
}
