//! Last-file cleanup uses durable whole-recording deletion, with unchanged publication history.

use super::*;

#[test]
fn final_segment_refuses_direct_release_and_recovers_failed_whole_file_cleanup() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = setup(&path, 1000)?;
    let job = store
        .admit_recording("last", "radio:v1", 60, 600, Retention::Temporary, false)?
        .ok_or("not admitted")?;
    store.publish_recording(&job.version, &publication(100))?;
    let before = store.recording("last")?;
    let release = store
        .next_segment_release(true, Store::clock_ms()?)?
        .ok_or("release")?;
    assert!(matches!(
        store.mark_segment_released(&release),
        Err(Error::RequestState)
    ));
    let receipts: i64 =
        store
            .connection
            .query_row("SELECT count(*) FROM recording_releases", [], |row| {
                row.get(0)
            })?;
    assert_eq!(receipts, 0);
    assert_eq!(store.recording("last")?.charged_bytes, 100);
    store.audit_dvr()?;
    drop(store);
    let media = crate::recordings::checked_directory(directory.path(), true)?;
    let blocked_file = media.join(format!("{}.part", before.object_key));
    std::fs::create_dir(&blocked_file)?;
    let audio_file = media.join(format!("{}.media", before.object_key));
    std::fs::write(&audio_file, b"private fixture audio")?;
    let mut library = crate::library::Library::open(directory.path(), false)?;
    assert!(
        crate::recordings::release_segments_at(&mut library, Store::clock_ms()?, true).is_err()
    );
    let pending = library.store().recording("last")?;
    assert_eq!(pending.storage_state, "deleting");
    assert_eq!(pending.charged_bytes, 100);
    assert_eq!(pending.media_bytes, Some(100));
    assert!(audio_file.exists());
    drop(library);
    std::fs::remove_dir(&blocked_file)?;
    let mut library = crate::library::Library::open(directory.path(), false)?;
    crate::recordings::recover_deletions(&mut library)?;
    let after = library.store().recording("last")?;
    assert_eq!(after.storage_state, "deleted");
    assert_eq!(after.charged_bytes, 0);
    assert_eq!(after.media_bytes, before.media_bytes);
    assert_eq!(after.sha256, before.sha256);
    assert_eq!(after.decoded_microseconds, before.decoded_microseconds);
    assert_eq!(after.intervals.len(), 1);
    assert!(!after.intervals[0].released);
    assert!(!audio_file.exists());
    assert!(library.store().pending_deletions()?.is_empty());
    assert!(!crate::recordings::release_segments_at(
        &mut library,
        Store::clock_ms()?,
        true
    )?);
    assert_eq!(library.store().dvr_status()?.available_bytes, 1000);
    library.store().audit_dvr()?;
    drop(library);
    let store = Store::open(&path)?;
    assert_eq!(store.recording("last")?.media_bytes, Some(100));
    assert_eq!(store.dvr_status()?.charged_bytes, 0);
    store.audit_dvr()?;
    Ok(())
}
