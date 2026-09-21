use std::net::IpAddr;

use sigy_service::{
    Error,
    sources::{HttpSource, NetworkScope},
    storage::{SCHEMA_VERSION, Store},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const PUBLIC: NetworkScope = NetworkScope::PublicInternet {};

#[test]
fn source_policy_messages_reject_implicit_or_ambiguous_permissions() {
    for value in [
        serde_json::json!({"kind": "public_internet", "address": "127.0.0.1"}),
        serde_json::json!({"kind": "pinned_address"}),
        serde_json::json!({"kind": "pinned_address", "address": "127.0.0.1", "allow_all": true}),
        serde_json::json!({"kind": "allow_all"}),
    ] {
        assert!(serde_json::from_value::<NetworkScope>(value).is_err());
    }
}

#[test]
fn source_boundary_preserves_scripts_and_rejects_unsafe_authorities() -> TestResult {
    for name in [
        "Radio Québec",
        "Diné Bizaad",
        "tlhIngan Hol",
        "راديو",
        "世界广播",
    ] {
        let source = HttpSource::new(name, "https://radio.example/music?private=value", PUBLIC)?;
        assert_eq!(source.name(), name);
        assert_eq!(source.origin(), "https://radio.example");
        let debug = format!("{source:?}");
        assert!(!debug.contains("music"));
        assert!(!debug.contains("private=value"));
    }
    for name in [
        "",
        " ",
        "station\x1b[2J",
        "station\nspoof",
        "station\u{202e}spoof",
        "station\u{2066}spoof",
    ] {
        assert!(HttpSource::new(name, "https://radio.example/", PUBLIC).is_err());
    }
    for endpoint in [
        "file:///etc/passwd",
        "ftp://radio.example/audio",
        "data:audio/mp3,abc",
        "https://user:pass@radio.example/",
        "https://radio.example/#part",
        "https://radio.example:0/",
        "https://radio.example/\nstuff",
        "https://radio.example/a b",
        "https://radio.example\\@127.0.0.1/",
        "http://127.1/",
        "http://2130706433/",
        "http://0x7f000001/",
        "http://0177.0.0.1/",
        "http://[::ffff:127.0.0.1]/",
        "http://[64:ff9b::7f00:1]/",
        "http://[fe80::1%25eth0]/",
    ] {
        assert!(
            HttpSource::new("station", endpoint, PUBLIC).is_err(),
            "accepted {endpoint}"
        );
    }
    Ok(())
}

#[test]
fn address_policy_distinguishes_public_and_exact_local_grants() -> TestResult {
    for address in [
        "0.1.2.3",
        "10.0.0.1",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "168.63.129.16",
        "172.16.0.1",
        "192.0.0.9",
        "192.0.2.1",
        "192.168.1.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.1.2.3",
        "255.255.255.255",
        "::",
        "::1",
        "fc00::1",
        "fe80::1",
        "ff02::1",
        "2001:db8::1",
        "2001::1",
        "2002:7f00:1::1",
        "3fff::1",
        "5f00::1",
    ] {
        let ip: IpAddr = address.parse()?;
        let authority = match ip {
            IpAddr::V4(_) => address.into(),
            IpAddr::V6(_) => format!("[{address}]"),
        };
        assert!(
            matches!(
                HttpSource::new("station", &format!("http://{authority}/"), PUBLIC),
                Err(Error::DestinationDenied)
            ),
            "accepted {address}"
        );
    }
    for address in [
        "8.8.8.8",
        "1.1.1.1",
        "172.15.255.255",
        "172.32.0.1",
        "2001:4860:4860::8888",
        "2606:4700:4700::1111",
    ] {
        let ip: IpAddr = address.parse()?;
        let authority = match ip {
            IpAddr::V4(_) => address.into(),
            IpAddr::V6(_) => format!("[{address}]"),
        };
        assert!(HttpSource::new("station", &format!("http://{authority}/"), PUBLIC).is_ok());
    }
    for address in ["127.0.0.1", "10.1.2.3", "::1", "fd12::1"] {
        let network = NetworkScope::PinnedAddress {
            address: address.parse()?,
        };
        HttpSource::new("local", "http://radio.example:8123/audio", network)?;
    }
    for address in [
        "0.0.0.0",
        "169.254.169.254",
        "224.0.0.1",
        "::ffff:127.0.0.1",
        "fe80::1",
    ] {
        let network = NetworkScope::PinnedAddress {
            address: address.parse()?,
        };
        assert!(HttpSource::new("local", "http://radio.example/audio", network).is_err());
    }
    assert!(
        HttpSource::new(
            "local",
            "http://127.0.0.2/audio",
            NetworkScope::PinnedAddress {
                address: "127.0.0.1".parse()?
            }
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn immutable_revisions_replay_exactly_and_paginate_without_connecting() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    let source = HttpSource::new("Radio Québec", "https://unresolved.invalid/audio", PUBLIC)?;
    let first = store.register_source("station:v1", &source)?;
    assert!(first.newly_created);
    assert!(!store.register_source("station:v1", &source)?.newly_created);
    let changed = HttpSource::new("Changed", "https://unresolved.invalid/audio", PUBLIC)?;
    assert!(matches!(
        store.register_source("station:v1", &changed),
        Err(Error::IdempotencyConflict)
    ));
    let changed = HttpSource::new("Radio Québec", "https://unresolved.invalid/other", PUBLIC)?;
    assert!(matches!(
        store.register_source("station:v1", &changed),
        Err(Error::IdempotencyConflict)
    ));
    let changed = HttpSource::new(
        "Radio Québec",
        "https://unresolved.invalid/audio",
        NetworkScope::PinnedAddress {
            address: "127.0.0.1".parse()?,
        },
    )?;
    assert!(matches!(
        store.register_source("station:v1", &changed),
        Err(Error::IdempotencyConflict)
    ));
    store.register_source("station:v2", &changed)?;
    assert_eq!(store.sources(None, 1)?[0], first.revision);
    assert_eq!(store.sources(Some("station:v1"), 1)?[0].id, "station:v2");
    assert!(store.sources(Some("station:v2"), 32)?.is_empty());
    assert!(store.sources(None, 0).is_err());
    assert!(store.sources(None, 33).is_err());
    let connection = rusqlite::Connection::open(&path)?;
    assert!(
        connection
            .execute("UPDATE source_revisions SET name = 'forged'", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM source_revisions", [])
            .is_err()
    );
    drop(store);
    assert_eq!(
        Store::open(&path)?.source("station:v1")?,
        Some(first.revision)
    );
    Ok(())
}

#[test]
fn source_write_failure_is_atomic_and_corrupt_authority_fails_on_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    let connection = rusqlite::Connection::open(&path)?;
    let source = HttpSource::new("Station", "https://radio.example/audio", PUBLIC)?;
    connection.execute_batch("CREATE TRIGGER inject_failure AFTER INSERT ON source_revisions BEGIN SELECT RAISE(ABORT, 'fixture'); END;")?;
    assert!(store.register_source("station:v1", &source).is_err());
    assert!(store.sources(None, 32)?.is_empty());
    connection.execute("DROP TRIGGER inject_failure", [])?;
    store.register_source("station:v1", &source)?;
    connection.execute_batch("DROP TRIGGER source_revisions_no_update; UPDATE source_revisions SET endpoint = 'http://127.0.0.1/';")?;
    assert!(matches!(
        store.source("station:v1"),
        Err(Error::SourceIntegrity)
    ));
    drop(store);
    assert!(matches!(Store::open(&path), Err(Error::SourceIntegrity)));
    Ok(())
}

#[test]
fn source_migration_preserves_prior_catalog_and_rolls_back_conflicts() -> TestResult {
    let directory = tempfile::tempdir()?;
    for fail in [false, true] {
        let path = directory.path().join(if fail {
            "failure.sqlite3"
        } else {
            "success.sqlite3"
        });
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(include_str!("../src/storage/001-foundation.sql"))?;
        connection.execute_batch(include_str!("../src/storage/002-captures.sql"))?;
        connection.execute("UPDATE budgets SET limit_micros = 7654321", [])?;
        if fail {
            connection.execute("CREATE TABLE source_revisions(existing TEXT)", [])?;
        }
        let result = Store::open(&path);
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if fail {
            assert!(result.is_err());
            assert_eq!(version, 2);
            let triggers: u32 = connection.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'source_revisions_no_delete'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(triggers, 0);
        } else {
            let store = result?;
            assert_eq!(version, SCHEMA_VERSION);
            assert_eq!(store.budget("global")?.limit().to_string(), "7.654321");
            store.audit_captures()?;
        }
    }
    Ok(())
}

#[test]
fn competing_registrations_cannot_overfill_the_source_catalog() -> TestResult {
    use sigy_service::storage::sources::MAX_SOURCE_REVISIONS;
    use std::{
        sync::{Arc, Barrier},
        thread,
    };
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    drop(Store::open(&path)?);
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute("WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM ids WHERE n < ?1) INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms) SELECT 'fixture:' || n, 'http_audio', 'Fixture', 'https://radio.example/audio', 'public_internet', 1 FROM ids", [MAX_SOURCE_REVISIONS - 1])?;
    let barrier = Arc::new(Barrier::new(2));
    let workers = ["new:a", "new:b"].map(|id| {
        let barrier = barrier.clone();
        let path = path.clone();
        thread::spawn(move || {
            // Reach the barrier even if opening fails so a fixture error cannot
            // leave the competing thread waiting indefinitely.
            let store = Store::open(&path);
            barrier.wait();
            let mut store = store?;
            let source = HttpSource::new("Station", "https://radio.example/audio", PUBLIC)?;
            store.register_source(id, &source)
        })
    });
    let mut accepted = 0;
    let mut denied = 0;
    for worker in workers {
        match worker.join().map_err(|_| "registration worker panicked")? {
            Ok(_) => accepted += 1,
            Err(Error::SourceCapacity) => denied += 1,
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!((accepted, denied), (1, 1));
    let count: u32 = connection.query_row("SELECT count(*) FROM source_revisions", [], |row| {
        row.get(0)
    })?;
    assert_eq!(count, MAX_SOURCE_REVISIONS);
    Ok(())
}
