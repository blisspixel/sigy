use std::path::Path;

use rusqlite::{Connection, TransactionBehavior, types::Value};

use super::{SCHEMA_VERSION, Store, widen};
use crate::{
    Error, Result,
    sources::{HttpHop, HttpSource, NetworkScope},
    storage::dvr::{GapCause, Publication, Retention},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

const V29: [&str; 30] = [
    include_str!("001-foundation.sql"),
    include_str!("002-captures.sql"),
    include_str!("003-sources.sql"),
    include_str!("004-dvr.sql"),
    include_str!("005-discovery.sql"),
    include_str!("006-redirects.sql"),
    include_str!("007-favorites.sql"),
    include_str!("008-playlists.sql"),
    include_str!("009-clicks.sql"),
    include_str!("010-listens.sql"),
    include_str!("011-icy.sql"),
    include_str!("012-podcasts.sql"),
    include_str!("013-podcast-feeds.sql"),
    include_str!("014-enclosures.sql"),
    include_str!("015-publisher-text.sql"),
    include_str!("016-recording-intervals.sql"),
    include_str!("017-segment-seals.sql"),
    include_str!("018-recording-gaps.sql"),
    include_str!("019-segment-retention.sql"),
    include_str!("020-schedules.sql"),
    include_str!("021-directory-policy.sql"),
    include_str!("022-analysis-inputs.sql"),
    include_str!("023-transcripts.sql"),
    include_str!("024-language-evidence.sql"),
    include_str!("025-analysis-jobs.sql"),
    include_str!("026-transcript-revisions.sql"),
    include_str!("026-transcript-invariants.sql"),
    include_str!("027-recognition-profiles.sql"),
    include_str!("028-provider-routes.sql"),
    include_str!("029-translations.sql"),
];

fn publication(
    format: &'static str,
    end_reason: &'static str,
    gap: Option<GapCause>,
) -> Publication {
    Publication {
        bytes: 100,
        sha256: "a".repeat(64),
        format,
        decoded_microseconds: 2_000_000,
        end_reason,
        http_route: vec![HttpHop {
            origin: "https://example.com".into(),
            peer: ([8, 8, 8, 8], 443).into(),
            status: 200,
        }],
        observations: Vec::new(),
        segments_sealed: false,
        gap,
    }
}

/// A genuine v29 catalog with a published recording, an analysis pin, a paused
/// capture with a gap, and one resolved playlist entry.
fn v29_catalog(path: &Path) -> Result<()> {
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    for sql in V29 {
        connection.execute_batch(sql)?;
    }
    let mut store = Store { connection };
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        10_000,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    store.register_source(
        "radio:v1",
        &HttpSource::new(
            "Fixture",
            "https://example.com/audio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    let job = store
        .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary, false)?
        .ok_or(Error::StorageIntegrity)?;
    store.publish_recording(&job.version, &publication("wav", "end_of_body", None))?;
    store.admit_analysis("pin", "one", false, 10)?;
    store.publish_analysis("pin", 1)?;
    let paused = store
        .admit_recording("two", "radio:v1", 60, 600, Retention::Kept, false)?
        .ok_or(Error::StorageIntegrity)?;
    store.connect_recording(&paused.version)?;
    store.pause_capture("two")?;
    store.connection.execute_batch(
        "INSERT INTO playlist_resolves(id, parent_revision, state, started_ms, completed_ms, document_sha256, final_origin, entry_count) VALUES ('list', 'radio:v1', 'completed', 1, 2, 'abababababababababababababababababababababababababababababababab', 'https://example.com', 1);
         INSERT INTO playlist_entries(resolve_id, entry_index, endpoint, origin) VALUES ('list', 0, 'https://example.com/entry', 'https://example.com');",
    )?;
    Ok(())
}

fn rows(connection: &Connection, table: &str) -> Result<Vec<Vec<Value>>> {
    let mut statement =
        connection.prepare(&format!("SELECT rowid, * FROM {table} ORDER BY rowid"))?;
    let width = statement.column_count();
    let rows = statement
        .query_map([], |row| {
            (0..width)
                .map(|index| row.get::<_, Value>(index))
                .collect::<rusqlite::Result<Vec<_>>>()
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn dependents(connection: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut statement = connection.prepare(
        "SELECT type, name, sql FROM sqlite_schema WHERE tbl_name IN ('recordings', 'recording_intervals', 'recording_gaps') AND type IN ('index', 'trigger') AND sql IS NOT NULL ORDER BY name",
    )?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

const PRESERVED: [&str; 6] = [
    "recordings",
    "recording_intervals",
    "recording_gaps",
    "analysis_inputs",
    "capture_jobs",
    "recording_segment_clocks",
];

#[test]
fn genuine_v29_migrates_to_v30_preserving_rows_triggers_and_children() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    v29_catalog(&path)?;
    let (before, triggers) = {
        let connection = Connection::open(&path)?;
        let mut snapshot = Vec::new();
        for table in PRESERVED {
            snapshot.push(rows(&connection, table)?);
        }
        (snapshot, dependents(&connection)?)
    };
    assert!(before[2].len() == 1, "the paused capture has one gap");
    let store = Store::open(&path)?;
    let version: u32 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, SCHEMA_VERSION);
    for (table, expected) in PRESERVED.iter().zip(&before) {
        assert_eq!(&rows(&store.connection, table)?, expected, "{table}");
    }
    assert_eq!(dependents(&store.connection)?, triggers);
    let violations: i64 =
        store
            .connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
    assert_eq!(violations, 0);
    let entry = store.playlist("list")?;
    assert_eq!(entry.entries[0].kind, crate::sources::CandidateKind::Entry);
    assert_eq!(entry.entries[0].bandwidth, None);
    assert_eq!(
        store.recording("two")?.gaps[0].cause,
        GapCause::CapturePause
    );
    Ok(())
}

#[test]
fn v30_accepts_transport_streams_and_stream_gaps_and_still_rejects_unknown_values() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        10_000,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("path")?,
    )?;
    store.register_source(
        "live:v1",
        &HttpSource::new(
            "Live",
            "https://example.com/live.m3u8",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    let job = store
        .admit_recording("live", "live:v1", 60, 600, Retention::Temporary, false)?
        .ok_or("admission")?;
    // A gap needs the stream-gap end reason, and the reverse.
    assert!(matches!(
        store.publish_recording(
            &job.version,
            &publication("mpegts", "end_of_body", Some(GapCause::SequenceSkip))
        ),
        Err(Error::StorageIntegrity)
    ));
    assert!(matches!(
        store.publish_recording(&job.version, &publication("mpegts", "stream_gap", None)),
        Err(Error::StorageIntegrity)
    ));
    store.publish_recording(
        &job.version,
        &publication("mpegts", "stream_gap", Some(GapCause::SequenceSkip)),
    )?;
    let record = store.recording("live")?;
    assert_eq!(record.format.as_deref(), Some("mpegts"));
    assert_eq!(record.end_reason.as_deref(), Some("stream_gap"));
    assert_eq!(record.intervals.len(), 1);
    assert_eq!(record.intervals[0].format, "mpegts");
    assert_eq!(record.gaps.len(), 1);
    assert_eq!(record.gaps[0].cause, GapCause::SequenceSkip);
    assert_eq!(record.gaps[0].start_us, 2_000_000);
    assert_eq!(record.gaps[0].end_us, 60_000_000);
    let unknown_gap = store.connection.execute(
        "INSERT INTO recording_gaps(recording_id, ordinal, cause, start_us, end_us) VALUES ('live', 1, 'invented', 1, 2)",
        [],
    );
    assert!(unknown_gap.is_err());
    let path = directory.path().join("catalog.sqlite3");
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(
        reopened.recording("live")?.gaps[0].cause,
        GapCause::SequenceSkip
    );
    Ok(())
}

#[test]
fn rebuild_widens_one_check_and_keeps_rowids_triggers_and_child_keys() -> TestResult {
    let mut connection = Connection::open_in_memory()?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.execute_batch(
        "CREATE TABLE parent_rows (id TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('a'))) STRICT;
         CREATE INDEX parent_kind ON parent_rows(kind);
         CREATE TRIGGER parent_rows_no_delete BEFORE DELETE ON parent_rows BEGIN SELECT RAISE(ABORT, 'kept'); END;
         CREATE TABLE child_rows (parent TEXT NOT NULL REFERENCES parent_rows(id)) STRICT;
         INSERT INTO parent_rows(rowid, id, kind) VALUES (7, 'x', 'a'), (9, 'y', 'a');
         INSERT INTO child_rows(parent) VALUES ('x'), ('y');",
    )?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    assert!(matches!(
        widen::rebuild(&tx, "parent_rows", &[("CHECK(kind IN ('z'))", "")], &[]),
        Err(Error::CatalogIntegrity)
    ));
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    widen::rebuild(
        &tx,
        "parent_rows",
        &[("CHECK(kind IN ('a'))", "CHECK(kind IN ('a', 'b'))")],
        &[],
    )?;
    tx.commit()?;
    let violations: i64 =
        connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    assert_eq!(violations, 0);
    let rowids: Vec<i64> = connection
        .prepare("SELECT rowid FROM parent_rows ORDER BY rowid")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert_eq!(rowids, [7, 9]);
    connection.execute("INSERT INTO parent_rows(id, kind) VALUES ('z', 'b')", [])?;
    assert!(
        connection
            .execute("INSERT INTO parent_rows(id, kind) VALUES ('w', 'c')", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM parent_rows WHERE id = 'z'", [])
            .is_err()
    );
    assert!(
        connection
            .execute("INSERT INTO child_rows(parent) VALUES ('missing')", [])
            .is_err()
    );
    let index: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'parent_kind'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(index, 1);
    Ok(())
}
