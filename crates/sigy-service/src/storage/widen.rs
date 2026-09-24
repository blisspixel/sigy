//! Widen a CHECK constraint by rebuilding one table from its own stored definition.
//!
//! SQLite cannot alter a CHECK constraint. The rebuild copies rows with their rowids,
//! drops the table, recreates it from the stored SQL with exactly one textual change per
//! replacement, restores the rows, and recreates the table's own indexes and triggers
//! verbatim. Child foreign keys are deferred for the enclosing transaction and checked
//! again after commit by the catalog open path.

use rusqlite::Transaction;

use crate::{Error, Result};

/// Rebuilds `table` with each `(old, new)` replacement applied once to its definition.
/// # Errors
/// Fails closed when a replacement does not match exactly once, a column name is not a
/// plain identifier, or the row count changes.
pub(super) fn rebuild(
    tx: &Transaction<'_>,
    table: &'static str,
    replacements: &[(&str, &str)],
) -> Result<()> {
    let definition: String = tx.query_row(
        "SELECT sql FROM main.sqlite_schema WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    let mut rebuilt = definition;
    for (old, new) in replacements {
        if rebuilt.matches(old).count() != 1 {
            return Err(Error::CatalogIntegrity);
        }
        rebuilt = rebuilt.replacen(old, new, 1);
    }
    let dependents = {
        let mut statement = tx.prepare(
            "SELECT sql FROM main.sqlite_schema WHERE tbl_name = ?1 AND type IN ('index', 'trigger') AND sql IS NOT NULL ORDER BY rowid",
        )?;
        statement
            .query_map([table], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    let columns = {
        let mut statement = tx.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?;
        statement
            .query_map([table], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    if columns.is_empty()
        || !columns.iter().all(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
    {
        return Err(Error::CatalogIntegrity);
    }
    let list = columns.join(", ");
    let before: i64 = tx.query_row(&format!("SELECT count(*) FROM main.{table}"), [], |row| {
        row.get(0)
    })?;
    tx.execute_batch(&format!(
        "CREATE TEMP TABLE widen_copy AS SELECT rowid AS widen_rowid, {list} FROM main.{table}; DROP TABLE main.{table};"
    ))?;
    tx.execute_batch(&rebuilt)?;
    tx.execute_batch(&format!(
        "INSERT INTO main.{table}(rowid, {list}) SELECT widen_rowid, {list} FROM temp.widen_copy ORDER BY widen_rowid; DROP TABLE temp.widen_copy;"
    ))?;
    for statement in &dependents {
        tx.execute_batch(statement)?;
    }
    let after: i64 = tx.query_row(&format!("SELECT count(*) FROM main.{table}"), [], |row| {
        row.get(0)
    })?;
    if before != after {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}

const RECORDINGS_030: [(&str, &str); 2] = [
    (
        "format TEXT CHECK(format IN ('mp3', 'aac', 'flac', 'ogg', 'wav'))",
        "format TEXT CHECK(format IN ('mp3', 'aac', 'flac', 'ogg', 'wav', 'mpegts'))",
    ),
    (
        "end_reason TEXT CHECK(end_reason IN ('end_of_body', 'byte_limit', 'duration_limit', 'user_stop'))",
        "end_reason TEXT CHECK(end_reason IN ('end_of_body', 'byte_limit', 'duration_limit', 'user_stop', 'stream_gap'))",
    ),
];
const INTERVALS_030: [(&str, &str); 1] = [(
    "format TEXT NOT NULL CHECK(format IN ('mp3', 'aac', 'flac', 'ogg', 'wav'))",
    "format TEXT NOT NULL CHECK(format IN ('mp3', 'aac', 'flac', 'ogg', 'wav', 'mpegts'))",
)];
const GAPS_030: [(&str, &str); 1] = [(
    "'late_start'",
    "'late_start',\n        'sequence_skip',\n        'discontinuity',\n        'reload_failure'",
)];

/// Migration 030: HLS transport streams, a stream-gap end, and live HLS gap causes.
/// # Errors
/// Fails closed when a stored definition differs from the expected v29 text.
pub(super) fn migrate_030(tx: &Transaction<'_>) -> Result<()> {
    // Parent rows are dropped before children point at the rebuilt table again.
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    rebuild(tx, "recordings", &RECORDINGS_030)?;
    rebuild(tx, "recording_intervals", &INTERVALS_030)?;
    rebuild(tx, "recording_gaps", &GAPS_030)?;
    let violations: i64 =
        tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations != 0 {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}

/// Returns a v30 test catalog to the v29 table definitions so older migration tests can
/// replay their own downgrades. It narrows the checks and drops the candidate columns.
#[cfg(test)]
pub(crate) fn revert_030_for_tests(connection: &mut rusqlite::Connection) -> Result<()> {
    fn reverse(pairs: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        pairs.iter().map(|(old, new)| (*new, *old)).collect()
    }
    let tx = connection.transaction()?;
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    rebuild(&tx, "recordings", &reverse(&RECORDINGS_030))?;
    rebuild(&tx, "recording_intervals", &reverse(&INTERVALS_030))?;
    rebuild(&tx, "recording_gaps", &reverse(&GAPS_030))?;
    tx.execute_batch(
        "ALTER TABLE playlist_entries DROP COLUMN audio_only; ALTER TABLE playlist_entries DROP COLUMN codecs; ALTER TABLE playlist_entries DROP COLUMN bandwidth; ALTER TABLE playlist_entries DROP COLUMN kind; PRAGMA user_version = 29;",
    )?;
    tx.commit()?;
    Ok(())
}
