use std::path::Path;

use rusqlite::params;

use super::*;
use crate::{
    languages::{LanguageEvidence, LanguageMethod, TranscriptReference},
    sources::{HttpHop, HttpSource, NetworkScope},
    storage::dvr::{Publication, Retention},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

pub(in crate::storage) fn setup_at_version(path: &Path, version: usize) -> Result<Store> {
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let migrations = [
        include_str!("../001-foundation.sql"),
        include_str!("../002-captures.sql"),
        include_str!("../003-sources.sql"),
        include_str!("../004-dvr.sql"),
        include_str!("../005-discovery.sql"),
        include_str!("../006-redirects.sql"),
        include_str!("../007-favorites.sql"),
        include_str!("../008-playlists.sql"),
        include_str!("../009-clicks.sql"),
        include_str!("../010-listens.sql"),
        include_str!("../011-icy.sql"),
        include_str!("../012-podcasts.sql"),
        include_str!("../013-podcast-feeds.sql"),
        include_str!("../014-enclosures.sql"),
        include_str!("../015-publisher-text.sql"),
        include_str!("../016-recording-intervals.sql"),
        include_str!("../017-segment-seals.sql"),
        include_str!("../018-recording-gaps.sql"),
        include_str!("../019-segment-retention.sql"),
        include_str!("../020-schedules.sql"),
        include_str!("../021-directory-policy.sql"),
        include_str!("../022-analysis-inputs.sql"),
        include_str!("../023-transcripts.sql"),
        include_str!("../024-language-evidence.sql"),
        include_str!("../025-analysis-jobs.sql"),
    ];
    for sql in migrations.iter().take(version) {
        connection.execute_batch(sql)?;
    }
    let mut store = Store { connection };
    initialize(&mut store)?;
    Ok(store)
}

fn initialize(store: &mut Store) -> Result<()> {
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
    store.publish_recording(
        &job.version,
        &Publication {
            bytes: 100,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![HttpHop {
                origin: "https://example.com".into(),
                peer: ([8, 8, 8, 8], 443).into(),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
        },
    )?;
    store.admit_analysis("pin", "one", false, 10)?;
    store.publish_analysis("pin", 1)?;
    Ok(())
}

fn legacy(store: &Store) -> Result<()> {
    insert_transcript(&store.connection, &store.analysis_revision("pin", 1)?, 11)
}

fn evidence() -> LanguageEvidence {
    LanguageEvidence {
        id: "legacy-evidence".into(),
        revision: 1,
        analysis_id: "pin".into(),
        analysis_revision: 1,
        transcript: Some(TranscriptReference {
            id: "pin".into(),
            revision: 1,
        }),
        method: LanguageMethod {
            origin: "text".into(),
            profile: "fixture".into(),
            profile_sha256: "b".repeat(64),
            resolution: "block".into(),
            alias_map: "fixture-v1".into(),
        },
        outcome: "not_attempted".into(),
        reason: Some("not-configured".into()),
        spans: Vec::new(),
    }
}

fn legacy_rows(connection: &Connection) -> Result<String> {
    let value = connection.query_row(
        "SELECT json_array((SELECT json_group_array(json_array(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, state, created_ms)) FROM transcripts), (SELECT json_group_array(json_array(transcript_id, revision, ordinal, start_us, end_us, script, wording)) FROM transcript_cues), (SELECT json_group_array(json_array(transcript_id, transcript_revision, amount_micros, request_id, created_ms)) FROM analysis_decisions))",
        [], |row| row.get(0),
    )?;
    Ok(value)
}

fn schema(connection: &Connection) -> Result<String> {
    Ok(connection.query_row("SELECT json_group_array(json_array(type, name, tbl_name, sql)) FROM (SELECT * FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name)", [], |row| row.get(0))?)
}

fn verification_rows(connection: &Connection) -> Result<String> {
    Ok(connection.query_row("SELECT json_group_array(json_array(id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, manifest_sha256, verified_bytes, reason, amount_micros, created_ms, finished_ms)) FROM analysis_jobs ORDER BY id", [], |row| row.get(0))?)
}

#[test]
fn genuine_v23_v24_v25_migrations_preserve_legacy_language_and_verification() -> TestResult {
    for version in [23, 24, 25] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog.sqlite3");
        let mut store = setup_at_version(&path, version)?;
        legacy(&store)?;
        let before = legacy_rows(&store.connection)?;
        let expected_evidence = if version >= 24 {
            Some(store.publish_language_evidence(evidence(), 12)?.1)
        } else {
            None
        };
        let payload_before = (version >= 24)
            .then(|| {
                store.connection.query_row(
                    "SELECT payload_json FROM language_evidence WHERE id = 'legacy-evidence'",
                    [],
                    |row| row.get::<_, String>(0),
                )
            })
            .transpose()?;
        if version == 25 {
            store.connection.execute("INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms) VALUES ('verify', 1, 'pin', 1, 'one', 'retained-sha256-v1', 'running', 100, 1, ?1, 0, 20)", ["c".repeat(64)])?;
        }
        let jobs_before = (version == 25)
            .then(|| verification_rows(&store.connection))
            .transpose()?;
        drop(store);
        let mut reopened = Store::open(&path)?;
        assert_eq!(legacy_rows(&reopened.connection)?, before);
        assert!(
            reopened
                .local_transcript("pin", 1)?
                .ok_or("missing legacy")?
                .cues[0]
                .script
                .is_empty()
        );
        if let Some(expected) = expected_evidence {
            assert_eq!(reopened.language_evidence("legacy-evidence", 1)?, expected);
        }
        if let Some(payload) = payload_before {
            assert_eq!(
                reopened.connection.query_row(
                    "SELECT payload_json FROM language_evidence WHERE id = 'legacy-evidence'",
                    [],
                    |row| row.get::<_, String>(0)
                )?,
                payload
            );
        }
        if let Some(jobs) = jobs_before {
            assert_eq!(verification_rows(&reopened.connection)?, jobs);
        }
        if version == 25 {
            let job = reopened.analysis_job("verify")?;
            assert_eq!(
                (
                    job.generation,
                    job.state.as_str(),
                    job.manifest_sha256.as_str()
                ),
                (1, "running", "c".repeat(64).as_str())
            );
            assert!(reopened.begin_delete("one", true).is_err());
            assert!(
                reopened
                    .admit_verification("verify", "pin", 1, 21)?
                    .1
                    .is_none()
            );
            reopened.recover_analysis_jobs()?;
            let interrupted = reopened.analysis_job("verify")?;
            assert_eq!(
                (interrupted.generation, interrupted.state.as_str()),
                (2, "interrupted")
            );
        }
        assert_eq!(
            reopened
                .connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
            26
        );
        drop(reopened);
        Store::open(&path)?;
    }
    Ok(())
}

#[test]
fn migration_rolls_back_at_table_index_trigger_and_semantic_audit_failures() -> TestResult {
    for fault in ["table", "index", "trigger", "audit"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog.sqlite3");
        let store = setup_at_version(&path, 25)?;
        legacy(&store)?;
        match fault {
            "table" => store.connection.execute_batch("CREATE TABLE transcript_coverage(x INTEGER);"),
            "index" => store.connection.execute_batch("DROP INDEX analysis_one_worker; CREATE INDEX analysis_one_worker ON budgets(id);"),
            "trigger" => store.connection.execute_batch("CREATE TRIGGER transcript_decision_complete BEFORE UPDATE ON budgets BEGIN SELECT RAISE(ABORT, 'fixture'); END;"),
            _ => store.connection.execute_batch("DROP TRIGGER analysis_decisions_no_delete; DELETE FROM analysis_decisions;"),
        }?;
        let before_schema = schema(&store.connection)?;
        let before_rows = legacy_rows(&store.connection)?;
        drop(store);
        assert!(Store::open(&path).is_err(), "{fault}");
        let connection = Connection::open(&path)?;
        assert_eq!(
            connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
            25,
            "{fault}"
        );
        assert_eq!(schema(&connection)?, before_schema, "{fault}");
        assert_eq!(legacy_rows(&connection)?, before_rows, "{fault}");
    }
    Ok(())
}

fn current(path: &Path) -> Result<Store> {
    let mut store = Store::open(path)?;
    initialize(&mut store)?;
    Ok(store)
}

fn admit_fixture(connection: &Connection, job: &str, parent: i64) -> rusqlite::Result<usize> {
    connection.execute("INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms) VALUES (?1, 1, 'pin', 1, 'one', 'fixture-profile', 'local_asr', ?2, ?3, 'running', 100, 1, ?4, 0, 20)", params![job, "b".repeat(64), parent, "c".repeat(64)])
}

fn header(
    connection: &Connection,
    job: &str,
    revision: i64,
    text: &str,
) -> rusqlite::Result<usize> {
    let has_text = !text.is_empty();
    connection.execute("INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, kind, outcome, parent_revision, job_id, job_generation, profile_sha256, cue_count, text_bytes, state, created_ms) VALUES ('pin', ?1, 'pin', 1, 'one', ?2, 'original', 'fixture-profile', 'recognition', ?3, ?4, ?5, 1, ?6, ?7, ?8, 'published', 21)", params![revision, "a".repeat(64), if has_text { "text" } else { "no_text" }, (revision > 1).then_some(revision - 1), job, "b".repeat(64), i64::from(has_text), u32::try_from(text.len()).map_err(|_| rusqlite::Error::InvalidQuery)?])
}

fn coverage(connection: &Connection, revision: i64) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO transcript_coverage VALUES ('pin', ?1, 0, 0, 1000000, ?2, ?3, 16000, 16000)",
        params![revision, "a".repeat(64), "d".repeat(64)],
    )
}

fn seal(connection: &Connection, job: &str, revision: i64) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO analysis_decisions VALUES ('pin', ?1, 0, NULL, 21)",
        [revision],
    )?;
    connection.execute(
        "UPDATE analysis_jobs SET state = 'succeeded', finished_ms = 22 WHERE id = ?1",
        [job],
    )
}

fn result_fixture(store: &mut Store, job: &str, revision: i64, text: &str) -> Result<()> {
    let transaction = store.connection.transaction()?;
    admit_fixture(&transaction, job, revision - 1)?;
    header(&transaction, job, revision, text)?;
    if !text.is_empty() {
        transaction.execute(
            "INSERT INTO transcript_cues VALUES ('pin', ?1, 0, 0, 1000000, ?2, 'uncertain')",
            params![revision, text],
        )?;
    }
    coverage(&transaction, revision)?;
    seal(&transaction, job, revision)?;
    audit(&transaction)?;
    transaction.commit()?;
    Ok(())
}

#[test]
fn append_only_text_and_zero_cue_success_preserve_legacy_and_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = current(&path)?;
    legacy(&store)?;
    let placeholder = store.local_transcript("pin", 1)?.ok_or("legacy missing")?;
    store.publish_language_evidence(evidence(), 12)?;
    result_fixture(&mut store, "text", 2, "مرحبا بالعالم")?;
    result_fixture(&mut store, "no-text", 3, "")?;
    assert_eq!(
        store.local_transcript("pin", 1)?.ok_or("legacy missing")?,
        placeholder
    );
    assert_eq!(
        store.connection.query_row(
            "SELECT count(*) FROM transcript_cues WHERE revision = 3",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );
    assert_eq!(
        store.connection.query_row(
            "SELECT count(*) FROM transcript_coverage WHERE revision = 3",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        1
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM requests", [], |row| row
                .get::<_, i64>(0))?,
        0
    );
    assert!(
        store
            .connection
            .execute(
                "UPDATE transcripts SET profile = 'changed' WHERE revision = 2",
                []
            )
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM transcript_coverage", [])
            .is_err()
    );
    for sql in [
        "INSERT OR REPLACE INTO transcripts SELECT * FROM transcripts WHERE revision = 1",
        "INSERT OR REPLACE INTO transcripts SELECT * FROM transcripts WHERE revision = 2",
        "INSERT OR REPLACE INTO analysis_decisions SELECT * FROM analysis_decisions WHERE transcript_revision = 1",
        "INSERT OR REPLACE INTO analysis_jobs SELECT * FROM analysis_jobs WHERE id = 'text'",
    ] {
        assert!(store.connection.execute(sql, []).is_err(), "{sql}");
    }
    assert!(
        store
            .connection
            .execute(
                "INSERT INTO transcript_cues VALUES ('pin', 2, 1, 0, 1, 'later', 'uncertain')",
                []
            )
            .is_err()
    );
    let evidence_before = store.language_evidence("legacy-evidence", 1)?;
    store.begin_delete("one", false)?;
    store.finish_delete("one")?;
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(reopened.recording("one")?.storage_state, "deleted");
    assert_eq!(
        reopened.language_evidence("legacy-evidence", 1)?,
        evidence_before
    );
    assert_eq!(
        reopened.connection.query_row(
            "SELECT script FROM transcript_cues WHERE revision = 2",
            [],
            |row| row.get::<_, String>(0)
        )?,
        "مرحبا بالعالم"
    );
    Ok(())
}

#[test]
fn recognition_sql_refuses_wrong_identity_incomplete_output_and_cancellation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = current(&directory.path().join("catalog.sqlite3"))?;
    assert!(admit_fixture(&store.connection, "wrong-parent", 1).is_err());
    admit_fixture(&store.connection, "asr", 0)?;
    assert!(admit_fixture(&store.connection, "second", 0).is_err());
    assert!(
        store
            .connection
            .execute(
                "UPDATE analysis_jobs SET profile_sha256 = ?1 WHERE id = 'asr'",
                ["e".repeat(64)]
            )
            .is_err()
    );
    assert!(
        store
            .connection
            .execute(
                "UPDATE analysis_jobs SET state = 'succeeded', finished_ms = 22 WHERE id = 'asr'",
                []
            )
            .is_err()
    );
    assert!(store.connection.execute("UPDATE analysis_jobs SET state = 'verified', verified_bytes = 100, finished_ms = 22 WHERE id = 'asr'", []).is_err());
    assert!(header(&store.connection, "missing", 1, "speech").is_err());
    let transaction = store.connection.transaction()?;
    header(&transaction, "asr", 1, "speech")?;
    assert!(transaction.execute("INSERT INTO transcript_cues VALUES ('pin', 1, 0, 0, 1000001, 'speech', 'uncertain')", []).is_err());
    assert!(
        transaction
            .execute(
                "INSERT INTO transcript_cues VALUES ('pin', 1, 0, 0, 1, '', 'uncertain')",
                []
            )
            .is_err()
    );
    assert!(seal(&transaction, "asr", 1).is_err());
    coverage(&transaction, 1)?;
    assert!(seal(&transaction, "asr", 1).is_err());
    transaction.rollback()?;
    store.connection.execute(
        "UPDATE analysis_jobs SET state = 'cancelling' WHERE id = 'asr'",
        [],
    )?;
    assert!(header(&store.connection, "asr", 1, "").is_err());
    assert!(matches!(
        store.recover_analysis_jobs(),
        Err(Error::Analysis("native-recovery-unavailable"))
    ));
    assert!(store.begin_delete("one", true).is_err());
    assert!(
        store
            .connection
            .execute(
                "UPDATE analysis_jobs SET state = 'succeeded', finished_ms = 22 WHERE id = 'asr'",
                []
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn incomplete_and_corrupt_recognition_results_fail_reopen_audit() -> TestResult {
    for fault in ["partial", "coverage", "generation", "decision"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog.sqlite3");
        let mut store = current(&path)?;
        if fault == "partial" {
            admit_fixture(&store.connection, "asr", 0)?;
            header(&store.connection, "asr", 1, "")?;
        } else {
            result_fixture(&mut store, "asr", 1, "speech")?;
            let sql = match fault {
                "coverage" => {
                    "DROP TRIGGER transcript_coverage_no_update; UPDATE transcript_coverage SET source_sha256 = printf('%064d', 0);"
                }
                "generation" => {
                    "DROP TRIGGER transcripts_no_update; UPDATE transcripts SET job_generation = 2;"
                }
                _ => "DROP TRIGGER analysis_decisions_no_delete; DELETE FROM analysis_decisions;",
            };
            store.connection.execute_batch(sql)?;
        }
        drop(store);
        assert!(Store::open(&path).is_err(), "{fault}");
    }
    Ok(())
}

#[test]
fn exact_generation_profile_pin_and_parent_are_required_by_sql() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = current(&path)?;
    admit_fixture(&store.connection, "asr", 0)?;
    assert!(matches!(
        store.admit_verification("asr", "pin", 1, 21),
        Err(Error::IdempotencyConflict)
    ));
    for (generation, profile, digest, input_revision, parent, transcript_revision) in [
        (2, "fixture-profile", "b", 1, None, 1),
        (1, "different-profile", "b", 1, None, 1),
        (1, "fixture-profile", "e", 1, None, 1),
        (1, "fixture-profile", "b", 2, None, 1),
        (1, "fixture-profile", "b", 1, Some(1), 2),
    ] {
        assert!(store.connection.execute(
            "INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, kind, outcome, parent_revision, job_id, job_generation, profile_sha256, cue_count, text_bytes, state, created_ms) VALUES ('pin', ?1, 'pin', ?2, 'one', ?3, 'original', ?4, 'recognition', 'no_text', ?5, 'asr', ?6, ?7, 0, 0, 'published', 21)",
            params![transcript_revision, input_revision, "a".repeat(64), profile, parent, generation, digest.repeat(64)],
        ).is_err());
    }
    let transaction = store.connection.transaction()?;
    header(&transaction, "asr", 1, "")?;
    coverage(&transaction, 1)?;
    assert!(
        transaction
            .execute(
                "INSERT INTO transcript_cues VALUES ('pin', 1, 0, 0, 1, 'invented', 'uncertain')",
                []
            )
            .is_err()
    );
    transaction.rollback()?;
    store.connection.execute("INSERT INTO analysis_inputs SELECT id, 2, recording_id, media_sha256, timeline_json, 'published', 23 FROM analysis_inputs WHERE id = 'pin' AND revision = 1", [])?;
    assert!(header(&store.connection, "asr", 1, "").is_err());
    Ok(())
}

#[test]
fn completion_failure_rolls_back_every_result_row_and_keeps_native_lease() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = current(&path)?;
    admit_fixture(&store.connection, "asr", 0)?;
    store.connection.execute_batch("CREATE TRIGGER fail_recognition_finish AFTER UPDATE ON analysis_jobs WHEN NEW.state = 'succeeded' BEGIN SELECT RAISE(ABORT, 'fixture terminal failure'); END;")?;
    let transaction = store.connection.transaction()?;
    header(&transaction, "asr", 1, "")?;
    coverage(&transaction, 1)?;
    assert!(seal(&transaction, "asr", 1).is_err());
    transaction.rollback()?;
    let counts: (i64, i64, i64) = store.connection.query_row("SELECT (SELECT count(*) FROM transcripts), (SELECT count(*) FROM transcript_coverage), (SELECT count(*) FROM analysis_decisions)", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    assert_eq!(counts, (0, 0, 0));
    assert!(store.begin_delete("one", true).is_err());
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert!(matches!(
        reopened.recover_analysis_jobs(),
        Err(Error::Analysis("native-recovery-unavailable"))
    ));
    assert!(reopened.begin_delete("one", true).is_err());
    assert!(reopened.prune_candidates(true)?.is_empty());
    Ok(())
}
