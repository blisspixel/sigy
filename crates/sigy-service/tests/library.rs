use sigy_service::{Error, library::Library, storage::Store};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn library_has_one_owner_and_can_reopen_after_owner_exits() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    assert!(Library::open(directory.path(), false).is_err());
    assert_eq!(library.store().sqlite_version()?, "3.53.4");
    drop(library);
    let reopened = Library::open(directory.path(), false)?;
    assert_eq!(reopened.store().budgets()?.len(), 1);
    Ok(())
}

#[test]
fn opening_missing_library_does_not_initialize_it() -> TestResult {
    let directory = tempfile::tempdir()?;
    assert!(Library::open(&directory.path().join("absent"), false).is_err());
    assert!(!directory.path().join("absent").exists());
    assert!(Library::open(directory.path(), false).is_err());
    assert!(!directory.path().join("catalog.sqlite3").exists());
    Ok(())
}

#[test]
fn unrelated_databases_are_not_migrated() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("unrelated.sqlite3");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute("CREATE TABLE unrelated(value TEXT)", [])?;
    assert!(matches!(Store::open(&path), Err(Error::ForeignCatalog)));
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'budgets'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn truncated_catalog_is_preserved_instead_of_reinitialized() -> TestResult {
    let directory = tempfile::tempdir()?;
    drop(Library::open(directory.path(), true)?);
    let catalog = directory.path().join("catalog.sqlite3");
    for damaged in [&[][..], b"SQLite format 3\0partial"] {
        std::fs::write(&catalog, damaged)?;
        assert!(matches!(
            Library::open(directory.path(), false),
            Err(Error::CatalogIntegrity)
        ));
        assert!(matches!(
            Library::open(directory.path(), true),
            Err(Error::CatalogIntegrity)
        ));
        assert_eq!(std::fs::read(&catalog)?, damaged);
    }
    Ok(())
}
