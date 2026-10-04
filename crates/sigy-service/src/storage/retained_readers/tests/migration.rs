use super::*;

#[test]
fn utf16_catalogs_migrate_and_reopen_maximum_ascii_reader_identity() -> TestResult {
    for encoding in ["UTF-16le", "UTF-16be"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog.sqlite3");
        let connection = rusqlite::Connection::open(&path)?;
        connection.pragma_update(None, "encoding", encoding)?;
        connection.execute_batch("CREATE TABLE encoding_fixture(value TEXT); INSERT INTO encoding_fixture VALUES('persist encoding'); DROP TABLE encoding_fixture;")?;
        drop(connection);
        let mut store = Store::open(&path)?;
        configure(&mut store)?;
        published(&mut store, "recording")?;
        revert_049_for_tests(&store.connection)?;
        store.connection.pragma_update(None, "user_version", 48)?;
        drop(store);
        let mut store = Store::open(&path)?;
        let id = "r".repeat(128);
        let reason = "h".repeat(128);
        let (spec, _) = store.admit_retained_reader(&id, "recording", 0, 10)?;
        let view = store.hold_retained_reader(&id, 1, &reason, 11)?;
        assert_eq!(view.spec, spec);
        assert_eq!(view.recovery_reason.as_deref(), Some(reason.as_str()));
        drop(store);
        let store = Store::open(&path)?;
        let actual: String = store
            .connection
            .pragma_query_value(None, "encoding", |row| row.get(0))?;
        assert_eq!(actual.to_lowercase(), encoding.to_lowercase());
        assert_eq!(store.retained_reader(&id)?, view);
        assert!(
            store
                .connection
                .execute(
                    "UPDATE retained_readers SET recovery_reason='untrusted 雨' WHERE id=?1",
                    [&id]
                )
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn populated_48_migration_preserves_recording_and_failure_rolls_back() -> TestResult {
    for conflict in [false, true] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog.sqlite3");
        let mut store = Store::open(&path)?;
        configure(&mut store)?;
        published(&mut store, "recording")?;
        let before = store.recording("recording")?;
        revert_049_for_tests(&store.connection)?;
        store.connection.pragma_update(None, "user_version", 48)?;
        if conflict {
            store
                .connection
                .execute_batch("CREATE TABLE retained_readers(conflict TEXT);")?;
        }
        drop(store);
        if conflict {
            assert!(Store::open(&path).is_err());
            let connection = rusqlite::Connection::open(&path)?;
            let version: i64 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            assert_eq!(version, 48);
            let guards: i64 = connection.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='retained_reader_delete_guard'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(guards, 0);
        } else {
            let mut store = Store::open(&path)?;
            assert_eq!(
                serde_json::to_value(store.recording("recording")?)?,
                serde_json::to_value(before)?
            );
            store.admit_retained_reader("reader", "recording", 0, 10)?;
            assert!(revert_049_for_tests(&store.connection).is_err());
            assert_eq!(store.retained_reader("reader")?.state, "running");
        }
    }
    Ok(())
}

#[tokio::test]
async fn populated_50_migration_preserves_legacy_json_hashes_and_unresolved_protection()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let mut library = crate::library::Library::open(directory.path(), true)?;
    configure(library.store_mut())?;
    published(library.store_mut(), "recording")?;
    for id in ["running", "cancelling", "held", "closed"] {
        library
            .store_mut()
            .admit_retained_reader(id, "recording", 250_000, 10)?;
    }
    library
        .store_mut()
        .cancel_retained_reader("cancelling", 1, 11)?;
    library
        .store_mut()
        .hold_retained_reader("held", 1, "fixture-held", 12)?;
    let closed = library.store().retained_reader("closed")?.spec;
    let (_stop, signal) = tokio::sync::watch::channel(true);
    let receipt = crate::recordings::retained::stream_retained(
        directory.path().to_owned(),
        closed,
        library.hold_ownership(),
        signal,
        |_| async { Ok(()) },
    )
    .await?;
    library.store_mut().finish_retained_reader(&receipt, 13)?;
    let before = serde_json::to_string(&library.store().retained_readers()?)?;
    assert!(!before.contains("excerpt"));
    revert_051_for_tests(&library.store().connection)?;
    library
        .store()
        .connection
        .pragma_update(None, "user_version", 50)?;
    drop(library);
    let mut library = crate::library::Library::open(directory.path(), false)?;
    assert_eq!(
        serde_json::to_string(&library.store().retained_readers()?)?,
        before
    );
    assert!(
        library
            .store_mut()
            .begin_delete("recording", false)
            .is_err()
    );
    library
        .store_mut()
        .admit_retained_range("fourth", "recording", 125_000, 375_000, 14)?;
    assert!(
        library
            .store_mut()
            .admit_retained_range("fifth", "recording", 125_000, 375_000, 15)
            .is_err()
    );
    assert!(
        library
            .store()
            .retained_reader("closed")?
            .spec
            .excerpt
            .is_none()
    );
    library.store().audit_retained_readers()?;
    Ok(())
}

#[test]
fn failed_51_migration_rolls_back_added_columns_and_preserves_legacy_guards() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    configure(&mut store)?;
    published(&mut store, "recording")?;
    let (spec, _) = store.admit_retained_reader("legacy", "recording", 0, 10)?;
    revert_051_for_tests(&store.connection)?;
    store.connection.execute_batch(
        "ALTER TABLE retained_readers ADD COLUMN citation_transcript TEXT; PRAGMA user_version=50;",
    )?;
    drop(store);
    assert!(Store::open(&path).is_err());
    let connection = rusqlite::Connection::open(&path)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 50);
    let added: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('retained_readers') WHERE name IN ('excerpt_end_us','citation_monitor','citation_finding','excerpt_version'))", [], |row|row.get(0))?;
    assert!(!added);
    let hash: String = connection.query_row(
        "SELECT spec_sha256 FROM retained_readers WHERE id='legacy'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(hash, spec.spec_sha256);
    assert!(
        connection
            .execute(
                "UPDATE retained_readers SET seek_us=1 WHERE id='legacy'",
                []
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE recordings SET storage_state='deleted' WHERE id='recording'",
                []
            )
            .is_err()
    );
    Ok(())
}
