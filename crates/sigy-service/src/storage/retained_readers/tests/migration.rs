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
