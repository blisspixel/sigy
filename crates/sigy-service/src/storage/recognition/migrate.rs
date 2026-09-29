//! Catalog v34: many coverage rows per transcript, and more than one retained file.

use rusqlite::Transaction;

use crate::{Error, Result};

const FILES_CHECK: (&str, &str) = (
    "expected_files = 1 AND expected_bytes <= 67108864",
    "expected_files BETWEEN 1 AND 1024 AND expected_bytes <= 67108864",
);

const COVERAGE_REBUILD: &str = r"
CREATE TABLE transcript_coverage_v34 (
    transcript_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 1023),
    interval_ordinal INTEGER NOT NULL CHECK(interval_ordinal BETWEEN 0 AND 1000000),
    start_us INTEGER NOT NULL CHECK(start_us >= 0),
    end_us INTEGER NOT NULL CHECK(end_us > start_us AND end_us - start_us <= 60000000),
    source_sha256 TEXT NOT NULL CHECK(length(source_sha256) = 64 AND source_sha256 NOT GLOB '*[^0-9a-f]*'),
    decoded_sha256 TEXT NOT NULL CHECK(length(decoded_sha256) = 64 AND decoded_sha256 NOT GLOB '*[^0-9a-f]*'),
    sample_rate INTEGER NOT NULL CHECK(sample_rate BETWEEN 1 AND 384000),
    sample_count INTEGER NOT NULL CHECK(sample_count BETWEEN 1 AND 23040000),
    PRIMARY KEY(transcript_id, revision, ordinal),
    FOREIGN KEY(transcript_id, revision) REFERENCES transcripts(id, revision),
    CHECK(sample_count <= sample_rate * 60)
) STRICT;
INSERT INTO transcript_coverage_v34(
    transcript_id, revision, ordinal, interval_ordinal, start_us, end_us,
    source_sha256, decoded_sha256, sample_rate, sample_count)
SELECT transcript_id, revision, 0, interval_ordinal, start_us, end_us,
    source_sha256, decoded_sha256, sample_rate, sample_count
FROM transcript_coverage;
DROP TRIGGER IF EXISTS transcript_cue_insert;
DROP TRIGGER IF EXISTS transcript_decision_complete;
DROP TRIGGER IF EXISTS transcript_coverage_insert;
DROP TRIGGER IF EXISTS transcript_coverage_no_update;
DROP TRIGGER IF EXISTS transcript_coverage_no_delete;
DROP TABLE transcript_coverage;
ALTER TABLE transcript_coverage_v34 RENAME TO transcript_coverage;
CREATE TRIGGER transcript_coverage_no_update BEFORE UPDATE ON transcript_coverage
BEGIN SELECT RAISE(ABORT, 'transcript coverage is immutable'); END;
CREATE TRIGGER transcript_coverage_no_delete BEFORE DELETE ON transcript_coverage
BEGIN SELECT RAISE(ABORT, 'transcript coverage is retained'); END;
";

/// Widen recognition to a chunk plan. A copied coverage row keeps ordinal zero.
/// # Errors
/// Fails closed when the stored job definition or coverage rows do not match v33.
pub(in crate::storage) fn migrate_034(tx: &Transaction<'_>) -> Result<()> {
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    rebuild_coverage(tx)?;
    widen_expected_files(tx)?;
    tx.execute_batch(include_str!("../034-chunked-recognition.sql"))?;
    super::super::transcripts::audit(tx)?;
    let violations: i64 =
        tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations != 0 {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}

/// A downgraded test catalog can already carry the v34 check. A genuine v33 catalog
/// still has the single-file check, and that is the only text this migration rewrites.
fn widen_expected_files(tx: &Transaction<'_>) -> Result<()> {
    let definition: String = tx.query_row(
        "SELECT sql FROM main.sqlite_schema WHERE type = 'table' AND name = 'analysis_jobs'",
        [],
        |row| row.get(0),
    )?;
    if definition.contains(FILES_CHECK.1) {
        return Ok(());
    }
    super::super::widen::rebuild(tx, "analysis_jobs", &[FILES_CHECK], &[])
}

/// The recognition admission trigger shipped with v26 and still present at v30.
#[cfg(test)]
const ASR_INPUT_V30: &str = "\
DROP TRIGGER analysis_asr_input;
CREATE TRIGGER analysis_asr_input
BEFORE INSERT ON analysis_jobs
WHEN NEW.kind = 'local_asr' AND (
    NEW.expected_parent_revision != coalesce((SELECT max(revision) FROM transcripts WHERE id = NEW.analysis_id), 0)
    OR NOT EXISTS (
        SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.intervals') s
        WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
          AND json_array_length(a.timeline_json, '$.intervals') = 1
          AND json_extract(s.value, '$.end_us') - json_extract(s.value, '$.start_us') BETWEEN 1 AND 60000000
    )
)
BEGIN SELECT RAISE(ABORT, 'invalid recognition request'); END;
";

/// Restores the v30 recognition file-count check and admission trigger.
///
/// Opening a current catalog rewrites both. A test that then rewinds `user_version`
/// and compares `sqlite_schema` with a file-built v30 catalog must undo that rewrite.
/// # Errors
/// Fails when the widened check is not present exactly once or the trigger cannot be restored.
#[cfg(test)]
pub(in crate::storage) fn revert_034_for_tests(tx: &Transaction<'_>) -> Result<()> {
    super::super::widen::rebuild(tx, "analysis_jobs", &[(FILES_CHECK.1, FILES_CHECK.0)], &[])?;
    tx.execute_batch(ASR_INPUT_V30)?;
    Ok(())
}

fn rebuild_coverage(tx: &Transaction<'_>) -> Result<()> {
    let before = coverage_rows(tx)?;
    tx.execute_batch(COVERAGE_REBUILD)?;
    if coverage_rows(tx)? != before {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}

fn coverage_rows(tx: &Transaction<'_>) -> Result<i64> {
    Ok(
        tx.query_row("SELECT count(*) FROM transcript_coverage", [], |row| {
            row.get(0)
        })?,
    )
}
