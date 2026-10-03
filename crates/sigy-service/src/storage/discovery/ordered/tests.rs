use super::*;
use crate::discovery::{RefreshRequest, radio_browser};
mod capacity;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn id(index: usize) -> String {
    format!("00000000-0000-0000-0000-{:012x}", 100 - index)
}

fn publish(store: &mut Store, request_id: &str, names: &[&str], now: i64) -> Result<()> {
    let request = RefreshRequest {
        filter: StationFilter::default(),
        limit: 500,
        offset: 0,
        mirror: None,
        network: NetworkScope::PublicInternet {},
    };
    store.begin_refresh_at(request_id, &request, now)?;
    let body=names.iter().enumerate().map(|(index,name)|serde_json::json!({"stationuuid":id(index),"name":name,"url":"https://station.example/audio","countrycode":"CA","language":"french,english","tags":"news","lastcheckok":1})).collect::<Vec<_>>();
    let batch = radio_browser::parse(
        &serde_json::to_vec(&body)?,
        500,
        "https://directory.example".into(),
    )?;
    store.finish_refresh(request_id, batch)
}

#[test]
fn independent_name_keys_page_thirty_seven_native_script_stations() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = Store::open(&path)?;
    // Explicit expected keys are independent of the production normalizer.
    let pairs = [
        ("Zulu", "zulu"),
        ("Alpha", "alpha"),
        ("ALPHA", "alpha"),
        ("Straße", "strasse"),
        ("STRASSE", "strasse"),
        ("Écho", "écho"),
        ("E\u{301}cho", "écho"),
        ("Αθήνα", "αθήνα"),
        ("Москва", "москва"),
        ("القاهرة", "القاهرة"),
        ("दिल्ली", "दिल्ली"),
        ("東京", "東京"),
        ("北京", "北京"),
        ("서울", "서울"),
        ("Łódź", "łódź"),
        ("İzmir", "i\u{307}zmir"),
        ("Beta", "beta"),
        ("Charlie", "charlie"),
        ("Delta", "delta"),
        ("Echo", "echo"),
        ("Foxtrot", "foxtrot"),
        ("Golf", "golf"),
        ("Hotel", "hotel"),
        ("India", "india"),
        ("Juliet", "juliet"),
        ("Kilo", "kilo"),
        ("Lima", "lima"),
        ("Mike", "mike"),
        ("November", "november"),
        ("Oscar", "oscar"),
        ("Papa", "papa"),
        ("Quebec", "quebec"),
        ("Romeo", "romeo"),
        ("Sierra", "sierra"),
        ("Tango", "tango"),
        ("Uniform", "uniform"),
        ("Victor", "victor"),
    ];
    let names = pairs.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    publish(&mut store, "initial", &names, 1)?;
    let mut expected = pairs
        .iter()
        .enumerate()
        .map(|(index, (_, key))| (*key, id(index)))
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| {
        left.0
            .as_bytes()
            .cmp(right.0.as_bytes())
            .then(left.1.cmp(&right.1))
    });
    let mut actual = Vec::new();
    let mut after = None;
    let catalog = store.directory_catalog()?;
    for length in [16, 16, 5] {
        let page = store.search_stations_ordered(
            &StationFilter::default(),
            false,
            after.as_deref(),
            16,
        )?;
        assert_eq!(page.entries.len(), length);
        assert_eq!(page.catalog, catalog);
        actual.extend(page.entries.iter().map(|station| station.id.clone()));
        after = page.next_after;
    }
    assert!(after.is_none());
    assert_eq!(
        actual,
        expected.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    );
    assert_eq!(store.station(&id(6))?.name, "E\u{301}cho");
    let legacy = store.search_stations(&StationFilter::default(), false, None, 16)?;
    assert!(legacy.windows(2).all(|pair| pair[0].id < pair[1].id));
    drop(store);
    assert_eq!(Store::open(&path)?.directory_catalog()?, catalog);
    Ok(())
}

#[test]
fn cursor_binds_effective_filters_page_policy_and_catalog_drift() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    publish(&mut store, "initial", &["Alpha", "Beta", "Charlie"], 1)?;
    let filter = StationFilter {
        language: "FRENCH".into(),
        tag: "NEWS".into(),
        ..StationFilter::default()
    };
    let page = store.search_stations_ordered(&filter, false, None, 1)?;
    let after = page.next_after.ok_or("cursor")?;
    assert!(after.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let lower = StationFilter {
        language: "french".into(),
        tag: "news".into(),
        ..StationFilter::default()
    };
    assert_eq!(
        store
            .search_stations_ordered(&lower, false, Some(&after), 1)?
            .entries[0]
            .name,
        "Beta"
    );
    for (filter, favorite, limit) in [
        (&lower, true, 1),
        (&lower, false, 2),
        (&StationFilter::default(), false, 1),
    ] {
        assert!(
            store
                .search_stations_ordered(filter, favorite, Some(&after), limit)
                .is_err()
        );
    }
    let initial = store.directory_catalog()?;
    store.set_station_favorite(&id(0), false)?;
    assert_eq!(store.directory_catalog()?, initial);
    store.set_station_favorite(&id(0), true)?;
    assert_eq!(store.directory_catalog()?.revision, initial.revision + 1);
    assert!(
        store
            .search_stations_ordered(&lower, false, Some(&after), 1)
            .is_err()
    );
    let changed = store.directory_catalog()?;
    store.set_station_favorite(&id(0), true)?;
    assert_eq!(store.directory_catalog()?, changed);
    let favorite = store.search_stations_ordered(&lower, true, None, 16)?;
    assert_eq!(favorite.favorite_ids, vec![id(0)]);
    let current = store
        .search_stations_ordered(&lower, false, None, 1)?
        .next_after
        .ok_or("cursor")?;
    publish(&mut store, "renamed", &["Zulu", "Beta", "Charlie"], 3000)?;
    assert!(
        store
            .search_stations_ordered(&lower, false, Some(&current), 1)
            .is_err()
    );
    Ok(())
}

#[test]
fn failed_publish_and_catalog_overflow_rollback_favorites_and_keys() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    publish(&mut store, "initial", &["Alpha", "Beta"], 1)?;
    let catalog = store.directory_catalog()?;
    store.connection.execute_batch("CREATE TRIGGER reject_catalog BEFORE UPDATE ON directory_catalog BEGIN SELECT RAISE(ABORT,'fixture'); END")?;
    assert!(store.set_station_favorite(&id(0), true).is_err());
    assert!(!store.is_station_favorite(&id(0))?);
    assert!(publish(&mut store, "fault", &["Changed", "Beta"], 3000).is_err());
    assert_eq!(store.station(&id(0))?.name, "Alpha");
    assert_eq!(store.directory_catalog()?, catalog);
    store.connection.execute_batch(
        "DROP TRIGGER reject_catalog; UPDATE directory_catalog SET revision=9223372036854775807",
    )?;
    assert!(store.set_station_favorite(&id(0), true).is_err());
    assert!(!store.is_station_favorite(&id(0))?);
    Ok(())
}

#[test]
fn cursor_cannot_substitute_an_existing_out_of_filter_station() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    publish(&mut store, "initial", &["Alpha", "Alpine", "Zulu"], 1)?;
    let filter = StationFilter {
        name: "Al".into(),
        ..StationFilter::default()
    };
    let page = store.search_stations_ordered(&filter, false, None, 1)?;
    let after = page.next_after.ok_or("cursor")?;
    let mut forged: Cursor = serde_json::from_slice(&decode(&after)?)?;
    forged.id = id(2);
    forged.key = "zulu".into();
    let forged = crate::storage::dvr::hex(&encode(&forged, MAX_CURSOR_BYTES / 2)?);
    assert!(
        store
            .search_stations_ordered(&filter, false, Some(&forged), 1)
            .is_err()
    );
    assert_eq!(
        store
            .search_stations_ordered(&filter, false, Some(&after), 1)?
            .entries[0]
            .name,
        "Alpine"
    );
    Ok(())
}

#[test]
fn malformed_metadata_cursor_and_work_failure_restore_guard_state() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    publish(&mut store, "initial", &["Alpha", "Beta"], 1)?;
    store
        .connection
        .busy_timeout(std::time::Duration::from_millis(37))?;
    for after in [
        "bad".to_owned(),
        "a".repeat(MAX_CURSOR_BYTES + 1),
        "7b7d".into(),
    ] {
        assert!(
            store
                .search_stations_ordered(&StationFilter::default(), false, Some(&after), 16)
                .is_err()
        );
    }
    let original: String = store.connection.query_row(
        "SELECT metadata_json FROM directory_stations WHERE id=?1",
        [id(0)],
        |row| row.get(0),
    )?;
    store.connection.execute(
        "UPDATE directory_stations SET metadata_json=?1 WHERE id=?2",
        params![format!("\"{}\"", "水".repeat(3000)), id(0)],
    )?;
    assert!(
        store
            .search_stations_ordered(&StationFilter::default(), false, None, 16)
            .is_err()
    );
    store.connection.execute(
        "UPDATE directory_stations SET metadata_json=?1 WHERE id=?2",
        params![original, id(0)],
    )?;
    let expensive=store.guarded_directory_read(|store| {
        store.connection.query_row("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<10000000) SELECT sum(x) FROM n",[],|row|row.get::<_,i64>(0)).map_err(Into::into)
    });
    assert!(expensive.is_err());
    let wait: u32 = store
        .connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    assert_eq!(wait, 37);
    let none = StationFilter {
        name: "does not match".into(),
        ..StationFilter::default()
    };
    assert!(
        store
            .search_stations_ordered(&none, false, None, 16)?
            .entries
            .is_empty()
    );
    assert_eq!(
        store
            .search_stations_ordered(&StationFilter::default(), false, None, 16)?
            .entries
            .len(),
        2
    );
    Ok(())
}

#[test]
fn populated_v47_migration_and_ddl_failure_are_atomic() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = Store::open(&path)?;
    publish(&mut store, "initial", &["Straße", "東京"], 1)?;
    store.set_station_favorite(&id(0), true)?;
    let original = store.station(&id(0))?;
    revert_048_for_tests(&store.connection)?;
    store
        .connection
        .execute_batch("CREATE TABLE directory_catalog(conflict INTEGER)")?;
    drop(store);
    assert!(Store::open(&path).is_err());
    let connection = Connection::open(&path)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 47);
    let columns: u32 = connection.query_row(
        "SELECT count(*) FROM pragma_table_info('directory_stations') WHERE name='name_ordered'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(columns, 0);
    connection.execute_batch("DROP TABLE directory_catalog")?;
    drop(connection);
    let store = Store::open(&path)?;
    assert_eq!(store.station(&id(0))?.name, original.name);
    assert!(store.is_station_favorite(&id(0))?);
    assert_eq!(
        store
            .search_stations_ordered(&StationFilter::default(), false, None, 16)?
            .entries
            .len(),
        2
    );
    Ok(())
}

#[test]
fn encoding_refuses_before_copying_or_growing_beyond_the_cap() -> TestResult {
    let mut writer = Capped {
        bytes: Vec::new(),
        maximum: 64,
    };
    writer.write_all(&[b'a'; 63])?;
    writer.write_all(b"b")?;
    assert_eq!(writer.bytes.len(), 64);
    let capacity = writer.bytes.capacity();
    assert!(capacity <= 64);
    assert!(writer.write_all(b"c").is_err());
    assert_eq!(writer.bytes.len(), 64);
    assert_eq!(writer.bytes.capacity(), capacity);
    Ok(())
}

#[test]
fn restored_library_rotates_only_disposable_cursor_scope() -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let mut library = crate::library::Library::open(&root, true)?;
    publish(library.store_mut(), "initial", &["Alpha", "Beta"], 1)?;
    library.store_mut().set_station_favorite(&id(0), true)?;
    let page =
        library
            .store()
            .search_stations_ordered(&StationFilter::default(), false, None, 1)?;
    let cursor = page.next_after.ok_or("cursor")?;
    let backup = directory.path().join("backup");
    crate::backup::backup(&library, &backup)?;
    let restored = directory.path().join("restored");
    crate::backup::restore(&backup, &restored)?;
    let restored = crate::library::Library::open(&restored, false)?;
    let catalog = restored.store().directory_catalog()?;
    assert_ne!(catalog.namespace, page.catalog.namespace);
    assert_eq!(catalog.revision, 0);
    assert!(
        restored
            .store()
            .search_stations_ordered(&StationFilter::default(), false, Some(&cursor), 1)
            .is_err()
    );
    assert_eq!(restored.store().station(&id(0))?.name, "Alpha");
    assert!(restored.store().is_station_favorite(&id(0))?);
    assert_eq!(library.store().directory_catalog()?, page.catalog);
    Ok(())
}

#[test]
fn utf16_catalogs_publish_and_migrate_utf8_blob_keysets() -> TestResult {
    for encoding in ["UTF-16le", "UTF-16be"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "encoding", encoding)?;
        connection.execute_batch("CREATE TABLE encoding_fixture(value TEXT); INSERT INTO encoding_fixture VALUES('persist encoding'); DROP TABLE encoding_fixture")?;
        drop(connection);
        let mut store = Store::open(&path)?;
        let actual: String = store
            .connection
            .pragma_query_value(None, "encoding", |row| row.get(0))?;
        assert_eq!(actual.to_lowercase(), encoding.to_lowercase());
        let names = [
            "東京",
            "Москва",
            "Ārī",
            "Zulu",
            "Alpha",
            "\u{10400}",
            "\u{e000}",
        ];
        publish(&mut store, "initial", &names, 1)?;
        store.set_station_favorite(&id(0), true)?;
        let expected = [
            "Alpha",
            "Zulu",
            "Ārī",
            "Москва",
            "東京",
            "\u{e000}",
            "\u{10400}",
        ];
        let original: Vec<String> = store
            .connection
            .prepare("SELECT metadata_json FROM directory_stations ORDER BY id")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        // LE differs already for ASCII versus U+0101. BE places the Deseret
        // surrogate before U+E000 as TEXT, opposite to declared UTF-8 byte order.
        for migrated in [false, true] {
            let mut after = None;
            let mut actual = Vec::new();
            loop {
                let page = store.search_stations_ordered(
                    &StationFilter::default(),
                    false,
                    after.as_deref(),
                    2,
                )?;
                actual.extend(page.entries.into_iter().map(|station| station.name));
                after = page.next_after;
                if after.is_none() {
                    break;
                }
            }
            assert_eq!(actual, expected);
            let non_blobs: u32 = store.connection.query_row(
                "SELECT count(*) FROM directory_stations WHERE typeof(name_ordered)!='blob'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(non_blobs, 0);
            assert!(store.is_station_favorite(&id(0))?);
            assert_eq!(store.station(&id(0))?.name, "東京");
            let retained: Vec<String> = store
                .connection
                .prepare("SELECT metadata_json FROM directory_stations ORDER BY id")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(retained, original);
            if !migrated {
                revert_048_for_tests(&store.connection)?;
                drop(store);
                store = Store::open(&path)?;
                let actual: String =
                    store
                        .connection
                        .pragma_query_value(None, "encoding", |row| row.get(0))?;
                assert_eq!(actual.to_lowercase(), encoding.to_lowercase());
            }
        }
    }
    Ok(())
}

#[test]
fn malformed_blob_keys_refuse_without_rewriting_metadata() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = Store::open(&path)?;
    publish(&mut store, "initial", &["Alpha"], 1)?;
    for value in [
        rusqlite::types::Value::Text("alpha".into()),
        rusqlite::types::Value::Blob(vec![0xff]),
        rusqlite::types::Value::Blob(vec![b'a'; 3073]),
    ] {
        assert!(
            store
                .connection
                .query_row("SELECT ?1", [value], |row| blob_key(row, 0))
                .is_err()
        );
    }
    // The STRICT table and byte constraint reject ordinary invalid writes.
    for value in [
        rusqlite::types::Value::Text("alpha".into()),
        rusqlite::types::Value::Blob(vec![b'a'; 3073]),
    ] {
        assert!(
            store
                .connection
                .execute("UPDATE directory_stations SET name_ordered=?1", [value])
                .is_err()
        );
    }
    let original: String =
        store
            .connection
            .query_row("SELECT metadata_json FROM directory_stations", [], |row| {
                row.get(0)
            })?;
    for key in [b"wrong".as_slice(), &[0xff]] {
        store
            .connection
            .execute("UPDATE directory_stations SET name_ordered=?1", [key])?;
        assert!(
            store
                .search_stations_ordered(&StationFilter::default(), false, None, 16)
                .is_err()
        );
        drop(store);
        assert!(Store::open(&path).is_err());
        let connection = Connection::open(&path)?;
        connection.execute(
            "UPDATE directory_stations SET name_ordered=?1",
            [b"alpha".as_slice()],
        )?;
        drop(connection);
        store = Store::open(&path)?;
        let retained: String = store.connection.query_row(
            "SELECT metadata_json FROM directory_stations",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(retained, original);
        assert_eq!(
            store
                .search_stations_ordered(&StationFilter::default(), false, None, 16)?
                .entries[0]
                .name,
            "Alpha"
        );
    }
    Ok(())
}
