//! The durable job pool: history beyond the old cap, the open-work bound, restart
//! attempts, immutable terminal rows and migration from v30.

use rusqlite::{Connection, types::Value};

use super::*;
use crate::storage::job_pool::{self, JobKind, MAX_ATTEMPTS};

fn finish(store: &mut Store, id: &str, now: i64) -> Result<()> {
    let (job, _) = store
        .claim_verification(id, TEST_OWNER, now)?
        .ok_or(Error::StorageIntegrity)?;
    assert!(store.finish_verification(id, job.generation, Ok(receipt(&job)), now)?);
    Ok(())
}

#[test]
fn more_than_three_hundred_jobs_are_admitted_over_time() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
    for index in 0..310 {
        let id = format!("verify-{index:03}");
        let now = 100 + index;
        let (job, created) = store.enqueue_verification(&id, "pin", 1, now)?;
        assert!(created);
        assert_eq!(job.state, "queued");
        assert_eq!(store.next_queued(JobKind::Verification)?, Some(id.clone()));
        finish(&mut store, &id, now)?;
    }
    let verified: i64 = store.connection.query_row(
        "SELECT count(*) FROM analysis_jobs WHERE state = 'verified'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(verified, 310);
    drop(store);
    let reopened = Store::open(&directory.path().join("catalog.sqlite3"))?;
    reopened.audit_analysis_jobs()?;
    assert_eq!(reopened.analysis_job("verify-000")?.state, "verified");
    Ok(())
}

#[test]
fn open_work_is_bounded_and_terminal_history_does_not_count() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
    let (template, _) = store.enqueue_verification("seed", "pin", 1, 20)?;
    let tx = store.connection.transaction()?;
    for index in 1..job_pool::MAX_OPEN_JOBS {
        tx.execute(
            "INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage) VALUES (?1, 1, 'pin', 1, 'one', 'retained-sha256-v1', 'queued', ?2, 1, ?3, 0, 21, 'pin')",
            rusqlite::params![format!("fill-{index}"), i64::try_from(template.expected_bytes)?, template.manifest_sha256],
        )?;
    }
    tx.commit()?;
    assert!(matches!(
        store.enqueue_verification("overflow", "pin", 1, 22),
        Err(Error::Analysis("queue-full"))
    ));
    // The SQL bound holds even when the service check is bypassed.
    assert!(
        store
            .connection
            .execute(
                "INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage) VALUES ('direct', 1, 'pin', 1, 'one', 'retained-sha256-v1', 'queued', ?1, 1, ?2, 0, 22, 'pin')",
                rusqlite::params![i64::try_from(template.expected_bytes)?, template.manifest_sha256],
            )
            .is_err()
    );
    // Ending one queued job frees one place.
    store.cancel_analysis_job("fill-1", 1)?;
    let (admitted, created) = store.enqueue_verification("overflow", "pin", 1, 23)?;
    assert!(created && admitted.state == "queued");
    Ok(())
}

#[test]
fn restart_requeues_until_the_attempt_limit_and_records_every_attempt() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = setup(&path)?;
    store.enqueue_verification("verify", "pin", 1, 20)?;
    for attempt in 1..=MAX_ATTEMPTS {
        let (job, _) = store
            .claim_verification("verify", TEST_OWNER, 30)?
            .ok_or("claim")?;
        assert_eq!((job.attempt, job.generation), (attempt, attempt));
        assert_eq!(job.started_ms, Some(30));
        drop(store);
        store = Store::open(&path)?;
        store.recover_analysis_jobs()?;
    }
    let job = store.analysis_job("verify")?;
    assert_eq!(job.state, "interrupted");
    assert_eq!(job.generation, MAX_ATTEMPTS + 1);
    assert_eq!(job.reason.as_deref(), Some("service-restarted"));
    assert_eq!(
        store.ended_attempts("analysis", "verify")?,
        (1..=MAX_ATTEMPTS).map(|n| (n, n)).collect::<Vec<_>>()
    );
    assert!(
        store
            .claim_verification("verify", TEST_OWNER, 40)?
            .is_none()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM job_attempts", [])
            .is_err()
    );
    Ok(())
}

#[test]
fn terminal_rows_and_requeues_are_enforced_by_sql() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
    let (job, _) = store.admit_verification("verify", "pin", 1, 20)?;
    // A requeue without a recorded attempt is refused.
    assert!(
        store
            .connection
            .execute(
                "UPDATE analysis_jobs SET state = 'queued', generation = 2, attempt = 2, lease_owner = NULL, lease_expires_ms = NULL, started_ms = NULL WHERE id = 'verify'",
                [],
            )
            .is_err()
    );
    // Two active jobs in one lineage are refused by the index.
    store.enqueue_verification("second", "pin", 1, 21)?;
    assert!(
        store
            .connection
            .execute(
                "UPDATE analysis_jobs SET state = 'running', lease_owner = 'x', lease_expires_ms = 1, started_ms = 1 WHERE id = 'second'",
                [],
            )
            .is_err()
    );
    store.finish_verification("verify", job.generation, Ok(receipt(&job)), 22)?;
    for sql in [
        "UPDATE analysis_jobs SET state = 'queued', finished_ms = NULL, verified_bytes = NULL WHERE id = 'verify'",
        "UPDATE analysis_jobs SET lease_owner = 'other' WHERE id = 'verify'",
        "UPDATE analysis_jobs SET attempt = 2 WHERE id = 'verify'",
        "UPDATE analysis_jobs SET lineage = 'moved' WHERE id = 'verify'",
    ] {
        assert!(store.connection.execute(sql, []).is_err(), "{sql}");
    }
    assert_eq!(store.analysis_job("verify")?.state, "verified");
    Ok(())
}

fn table_rows(connection: &Connection, table: &str, columns: &str) -> Result<Vec<Vec<Value>>> {
    let mut statement = connection.prepare(&format!(
        "SELECT rowid, {columns} FROM {table} ORDER BY rowid"
    ))?;
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

const V30_COLUMNS: &str = "id, generation, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, state, expected_bytes, expected_files, manifest_sha256, verified_bytes, reason, amount_micros, created_ms, finished_ms";

/// A v30 catalog with verified, cancelled and running verification rows.
fn populated_v30(path: &Path) -> Result<()> {
    let mut store = setup(path)?;
    let (verified, _) = store.admit_verification("verified", "pin", 1, 20)?;
    store.finish_verification("verified", 1, Ok(receipt(&verified)), 21)?;
    store.admit_verification("cancelled", "pin", 1, 22)?;
    store.cancel_analysis_job("cancelled", 1)?;
    store.finish_verification("cancelled", 1, Err(Error::Analysis("cancelled")), 23)?;
    store.admit_verification("running", "pin", 1, 24)?;
    drop(store);
    let mut connection = Connection::open(path)?;
    job_pool::revert_031_for_tests(&mut connection)?;
    Ok(())
}

fn schema(connection: &Connection) -> Result<Vec<(String, String)>> {
    let mut statement = connection.prepare(
        "SELECT name, sql FROM sqlite_schema WHERE tbl_name IN ('analysis_jobs', 'translation_jobs', 'recordings') AND sql IS NOT NULL ORDER BY name",
    )?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[test]
fn a_populated_v30_catalog_migrates_preserving_rows_and_leases() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    populated_v30(&path)?;
    let (before, v30_schema) = {
        let connection = Connection::open(&path)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        assert_eq!(version, 30);
        (
            table_rows(&connection, "analysis_jobs", V30_COLUMNS)?,
            schema(&connection)?,
        )
    };
    assert_eq!(before.len(), 3);
    let mut store = Store::open(&path)?;
    let version: u32 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, crate::storage::SCHEMA_VERSION);
    assert_eq!(
        table_rows(&store.connection, "analysis_jobs", V30_COLUMNS)?,
        before
    );
    let running = store.analysis_job("running")?;
    assert_eq!((running.state.as_str(), running.attempt), ("running", 1));
    assert_eq!(running.started_ms, Some(running.created_ms));
    let lease: (String, String) = store.connection.query_row(
        "SELECT lineage, lease_owner FROM analysis_jobs WHERE id = 'running'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(lease, ("pin".to_owned(), "catalog-v30".to_owned()));
    assert!(store.begin_delete("one", false).is_err(), "the lease holds");
    store.recover_analysis_jobs()?;
    assert_eq!(store.analysis_job("running")?.state, "queued");
    // The test downgrade reproduces the genuine v30 definitions, built from the files.
    assert_eq!(
        genuine_v30_schema(&directory.path().join("v30.sqlite3"))?,
        v30_schema
    );
    Ok(())
}

fn genuine_v30_schema(path: &Path) -> Result<Vec<(String, String)>> {
    let mut connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let tx = connection.transaction()?;
    for sql in [
        include_str!("../../001-foundation.sql"),
        include_str!("../../002-captures.sql"),
        include_str!("../../003-sources.sql"),
        include_str!("../../004-dvr.sql"),
        include_str!("../../005-discovery.sql"),
        include_str!("../../006-redirects.sql"),
        include_str!("../../007-favorites.sql"),
        include_str!("../../008-playlists.sql"),
        include_str!("../../009-clicks.sql"),
        include_str!("../../010-listens.sql"),
        include_str!("../../011-icy.sql"),
        include_str!("../../012-podcasts.sql"),
        include_str!("../../013-podcast-feeds.sql"),
        include_str!("../../014-enclosures.sql"),
        include_str!("../../015-publisher-text.sql"),
        include_str!("../../016-recording-intervals.sql"),
        include_str!("../../017-segment-seals.sql"),
        include_str!("../../018-recording-gaps.sql"),
        include_str!("../../019-segment-retention.sql"),
        include_str!("../../020-schedules.sql"),
        include_str!("../../021-directory-policy.sql"),
        include_str!("../../022-analysis-inputs.sql"),
        include_str!("../../023-transcripts.sql"),
        include_str!("../../024-language-evidence.sql"),
        include_str!("../../025-analysis-jobs.sql"),
        include_str!("../../026-transcript-revisions.sql"),
        include_str!("../../026-transcript-invariants.sql"),
        include_str!("../../027-recognition-profiles.sql"),
        include_str!("../../028-provider-routes.sql"),
        include_str!("../../029-translations.sql"),
    ] {
        tx.execute_batch(sql)?;
    }
    crate::storage::widen::migrate_030(&tx)?;
    tx.execute_batch(include_str!("../../030-live-hls.sql"))?;
    tx.commit()?;
    schema(&connection)
}

#[test]
fn an_interrupted_migration_leaves_the_v30_catalog_intact() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    populated_v30(&path)?;
    let (before, v30_schema) = {
        let connection = Connection::open(&path)?;
        // A conflicting object makes migration 031 fail after both tables are rebuilt.
        connection.execute_batch("CREATE TABLE job_attempts(conflict INTEGER);")?;
        (
            table_rows(&connection, "analysis_jobs", V30_COLUMNS)?,
            schema(&connection)?,
        )
    };
    assert!(Store::open(&path).is_err());
    let connection = Connection::open(&path)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 30);
    assert_eq!(schema(&connection)?, v30_schema);
    assert_eq!(
        table_rows(&connection, "analysis_jobs", V30_COLUMNS)?,
        before
    );
    connection.execute_batch("DROP TABLE job_attempts;")?;
    drop(connection);
    let store = Store::open(&path)?;
    assert_eq!(store.analysis_job("verified")?.state, "verified");
    assert_eq!(store.analysis_job("running")?.attempt, 1);
    Ok(())
}
