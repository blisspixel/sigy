use std::path::Path;

use super::*;
use crate::{
    control::{AnalysisOperation, LanguageOperation, apply},
    languages::{LanguageLabel, LanguageRoute},
    sources::{HttpHop, HttpSource, NetworkScope},
    storage::dvr::{Publication, Retention},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn setup(path: &Path) -> Result<Store> {
    let mut store = Store::open(path)?;
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
    Ok(store)
}

fn fixture(id: &str) -> LanguageEvidence {
    LanguageEvidence {
        id: id.into(),
        revision: 1,
        analysis_id: "pin".into(),
        analysis_revision: 1,
        transcript: None,
        method: LanguageMethod {
            origin: "acoustic".into(),
            profile: "fixture-detector".into(),
            profile_sha256: "b".repeat(64),
            resolution: "block".into(),
            alias_map: "fixture-v1".into(),
        },
        outcome: "succeeded".into(),
        reason: None,
        spans: vec![LanguageSpan {
            ordinal: 0,
            interval_ordinal: 0,
            start_us: 0,
            end_us: 1_000_000,
            cue_ordinal: None,
            observation: "identified".into(),
            languages: vec![LanguageLabel {
                tag: "FR-ca".into(),
                provider_label: "fr_CA".into(),
            }],
            route: LanguageRoute {
                task: "translation_en".into(),
                capability: "unsupported".into(),
                profile: "fixture-translator".into(),
                profile_sha256: "c".repeat(64),
                basis: "declared".into(),
                basis_sha256: "d".repeat(64),
            },
        }],
    }
}

fn legacy_transcript(store: &Store) -> Result<()> {
    store.connection.execute_batch(
        "INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, state, created_ms) SELECT id, 1, id, revision, recording_id, media_sha256, 'original', 'local-unmeasured', 'published', 11 FROM analysis_inputs WHERE id = 'pin' AND revision = 1;
         INSERT INTO transcript_cues VALUES ('pin', 1, 0, 0, 1000000, '', 'uncertain');
         INSERT INTO analysis_decisions VALUES ('pin', 1, 0, NULL, 11);"
    )?;
    Ok(())
}

#[test]
fn publication_is_normalized_immutable_and_idempotent() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog.sqlite3");
    let mut store = setup(&path)?;
    let (outcome, evidence) = store.publish_language_evidence(fixture("lid"), 20)?;
    assert_eq!(outcome, LanguagePublication::Created);
    assert_eq!(evidence.spans[0].languages[0].tag, "fr-CA");
    assert_eq!(evidence.spans[0].languages[0].provider_label, "fr_CA");
    assert_eq!(evidence.spans[0].route.capability, "unsupported");
    assert_eq!(
        store.publish_language_evidence(fixture("lid"), 21)?.0,
        LanguagePublication::Unchanged
    );
    let mut changed = evidence.clone();
    changed.spans[0].languages[0].tag = "es".into();
    assert!(
        store
            .publish_language_evidence(changed.clone(), 22)
            .is_err()
    );
    changed.revision = 3;
    assert!(
        store
            .publish_language_evidence(changed.clone(), 22)
            .is_err()
    );
    changed.revision = 2;
    store.publish_language_evidence(changed, 22)?;
    assert_eq!(store.language_evidence("lid", 1)?, evidence);
    assert!(
        store
            .connection
            .execute("UPDATE language_evidence SET created_ms = 30", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM language_evidence", [])
            .is_err()
    );
    drop(store);
    let reopened = Store::open(&path)?;
    assert_eq!(reopened.language_evidence("lid", 1)?, evidence);
    let (requests, reserved): (i64, i64) = reopened.connection.query_row(
        "SELECT (SELECT count(*) FROM requests), (SELECT sum(reserved_micros) FROM budgets)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!((requests, reserved), (0, 0));
    Ok(())
}

#[test]
fn blocks_mixed_unknown_non_speech_and_overlap_keep_their_meanings() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = setup(&root.path().join("catalog.sqlite3"))?;
    let mut evidence = fixture("lid");
    evidence.spans[0].end_us = 500_000;
    evidence.spans[0].observation = "mixed".into();
    let mut unknown = evidence.spans[0].clone();
    unknown.ordinal = 1;
    unknown.start_us = 250_000;
    unknown.end_us = 750_000;
    unknown.observation = "unknown".into();
    unknown.languages.clear();
    let mut silence = unknown.clone();
    silence.ordinal = 2;
    silence.start_us = 750_000;
    silence.end_us = 1_000_000;
    silence.observation = "non_speech".into();
    evidence.spans.extend([unknown, silence]);
    let (_, saved) = store.publish_language_evidence(evidence, 20)?;
    assert_eq!(saved.spans.len(), 3);
    assert_eq!(saved.spans[0].languages.len(), 1);
    assert_eq!(saved.spans[1].start_us, 250_000);
    assert!(saved.spans[1].languages.is_empty());
    Ok(())
}

#[test]
fn invalid_observations_ranges_and_source_hints_never_publish() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = setup(&root.path().join("catalog.sqlite3"))?;
    let mut cases = Vec::new();
    let mut invalid = fixture("lid");
    invalid.spans[0].languages[0].tag = "zh-Hans".into();
    cases.push(invalid);
    let mut invalid = fixture("lid");
    invalid.spans[0].end_us = 1_000_001;
    cases.push(invalid);
    let mut invalid = fixture("lid");
    invalid.spans[0].start_us = 1_000_000;
    invalid.spans[0].end_us = 2_000_000;
    cases.push(invalid);
    let mut invalid = fixture("lid");
    invalid.spans[0].interval_ordinal = 1;
    cases.push(invalid);
    let mut invalid = fixture("lid");
    invalid.outcome = "failed".into();
    invalid.reason = Some("detector-failed".into());
    cases.push(invalid);
    let mut invalid = fixture("lid");
    invalid.method.origin = "directory_hint".into();
    cases.push(invalid);
    let mut invalid = fixture("lid");
    invalid.method.resolution = "word".into();
    cases.push(invalid);
    let mut invalid = fixture("lid");
    invalid.spans[0].observation = "mixed".into();
    let label = invalid.spans[0].languages[0].clone();
    invalid.spans[0].languages.push(label);
    cases.push(invalid);
    for invalid in cases {
        assert!(store.publish_language_evidence(invalid, 20).is_err());
    }
    assert!(matches!(
        store.language_evidence("lid", 1),
        Err(Error::NotFound)
    ));
    Ok(())
}

#[test]
fn empty_legacy_transcripts_do_not_imply_unknown_or_text_detection() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = setup(&root.path().join("catalog.sqlite3"))?;
    legacy_transcript(&store)?;
    assert!(
        matches!(store.language_evidence_list("pin", 1, None)?, LanguagePage::List { records, .. } if records.is_empty())
    );
    let mut evidence = fixture("lid");
    evidence.transcript = Some(TranscriptReference {
        id: "pin".into(),
        revision: 1,
    });
    evidence.method.origin = "text".into();
    evidence.spans[0].cue_ordinal = Some(0);
    assert!(
        store
            .publish_language_evidence(evidence.clone(), 20)
            .is_err()
    );
    evidence.outcome = "not_attempted".into();
    evidence.reason = Some("no-recognizer-selected".into());
    evidence.spans.clear();
    store.publish_language_evidence(evidence, 21)?;
    Ok(())
}

#[test]
fn expired_audio_keeps_history_and_allows_an_unavailable_outcome() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog.sqlite3");
    let mut store = setup(&path)?;
    let (_, first) = store.publish_language_evidence(fixture("lid"), 20)?;
    store.begin_delete("one", false)?;
    store.finish_delete("one")?;
    assert_eq!(store.language_evidence("lid", 1)?, first);
    assert_eq!(
        store.publish_language_evidence(fixture("lid"), 21)?.0,
        LanguagePublication::Unchanged
    );
    assert!(store.publish_language_evidence(fixture("new"), 21).is_err());
    let mut terminal = fixture("unavailable");
    terminal.outcome = "unavailable_input".into();
    terminal.reason = Some("retention-expired".into());
    terminal.spans.clear();
    store.publish_language_evidence(terminal.clone(), 21)?;
    drop(store);
    assert_eq!(
        Store::open(&path)?.language_evidence("unavailable", 1)?,
        terminal
    );
    Ok(())
}

#[test]
fn stale_pin_and_mismatched_transcript_are_refused() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = setup(&root.path().join("catalog.sqlite3"))?;
    legacy_transcript(&store)?;
    store.admit_analysis("another", "one", false, 12)?;
    store.publish_analysis("another", 1)?;
    let mut mismatch = fixture("lid");
    mismatch.analysis_id = "another".into();
    mismatch.transcript = Some(TranscriptReference {
        id: "pin".into(),
        revision: 1,
    });
    assert!(store.publish_language_evidence(mismatch, 20).is_err());
    store.connection.execute(
        "INSERT INTO analysis_inputs SELECT id, 2, recording_id, media_sha256, timeline_json, 'published', 30 FROM analysis_inputs WHERE id = 'pin' AND revision = 1", [])?;
    assert!(store.publish_language_evidence(fixture("lid"), 31).is_err());
    let mut terminal = fixture("lid");
    terminal.spans.clear();
    terminal.outcome = "interrupted".into();
    terminal.reason = Some("restart".into());
    assert!(store.publish_language_evidence(terminal, 31).is_err());
    Ok(())
}

#[test]
fn aborted_insert_rolls_back_without_consuming_a_revision() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = setup(&root.path().join("catalog.sqlite3"))?;
    store.connection.execute_batch("CREATE TRIGGER fail_language AFTER INSERT ON language_evidence BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;")?;
    assert!(store.publish_language_evidence(fixture("lid"), 20).is_err());
    assert!(matches!(
        store.language_evidence("lid", 1),
        Err(Error::NotFound)
    ));
    store
        .connection
        .execute_batch("DROP TRIGGER fail_language;")?;
    assert_eq!(
        store.publish_language_evidence(fixture("lid"), 21)?.0,
        LanguagePublication::Created
    );
    Ok(())
}

#[test]
fn inspection_pages_are_bounded_and_do_not_dispatch() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = setup(&root.path().join("catalog.sqlite3"))?;
    for index in 0..18 {
        store.publish_language_evidence(fixture(&format!("lid-{index:02}")), 20)?;
    }
    let LanguagePage::List {
        records,
        next_after,
    } = store.language_evidence_list("pin", 1, None)?
    else {
        return Err("wrong page".into());
    };
    assert_eq!(records.len(), 16);
    let LanguagePage::List {
        records,
        next_after: end,
    } = store.language_evidence_list("pin", 1, next_after.as_deref())?
    else {
        return Err("wrong page".into());
    };
    assert_eq!(records.len(), 2);
    assert!(end.is_none());
    let mut many = fixture("many");
    let span = many.spans[0].clone();
    many.spans = (0..18)
        .map(|ordinal| LanguageSpan {
            ordinal,
            ..span.clone()
        })
        .collect();
    store.publish_language_evidence(many, 21)?;
    let snapshot = apply(
        &mut store,
        AnalysisOperation::Languages {
            command: LanguageOperation::Show {
                id: "many".into(),
                revision: 1,
                after: None,
            },
        }
        .into(),
    )?;
    assert!(serde_json::to_vec(&snapshot)?.len() < 256 * 1024);
    let page = snapshot.analysis.ok_or("no analysis")?;
    assert!(page.transcript.is_none() && page.decision.is_none());
    assert!(
        matches!(page.languages, Some(LanguagePage::Evidence { spans, next_after: Some(15), .. }) if spans.len() == 16)
    );
    assert!(
        matches!(store.language_evidence_page("many", 1, Some(15))?, LanguagePage::Evidence { spans, next_after: None, .. } if spans.len() == 2)
    );
    Ok(())
}

#[test]
fn v23_upgrade_keeps_legacy_rows_and_rolls_back_schema_conflicts() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog.sqlite3");
    let store = super::super::transcripts::migration_tests::setup_at_version(&path, 23)?;
    legacy_transcript(&store)?;
    store
        .connection
        .execute_batch("CREATE INDEX language_evidence_input ON transcripts(id);")?;
    drop(store);
    assert!(Store::open(&path).is_err());
    let connection = Connection::open(&path)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 23);
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'language_evidence')",
        [],
        |row| row.get(0),
    )?;
    assert!(!exists);
    connection.execute_batch("DROP INDEX language_evidence_input;")?;
    drop(connection);
    let store = Store::open(&path)?;
    let transcript = store
        .local_transcript("pin", 1)?
        .ok_or("missing legacy transcript")?;
    assert!(transcript.cues[0].script.is_empty());
    assert_eq!(transcript.amount_micros, 0);
    assert!(
        matches!(store.language_evidence_list("pin", 1, None)?, LanguagePage::List { records, .. } if records.is_empty())
    );
    Ok(())
}

#[test]
fn evidence_capacity_and_revision_bounds_fail_closed() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = setup(&root.path().join("catalog.sqlite3"))?;
    for index in 0..256 {
        store.publish_language_evidence(fixture(&format!("lid-{index:03}")), 20)?;
    }
    assert!(
        store
            .publish_language_evidence(fixture("overflow"), 20)
            .is_err()
    );
    assert!(check_capacity(&store.connection, "lid-000", 64 * 1024 * 1024).is_err());
    let mut revision = fixture("lid-000");
    for number in 2..=64 {
        revision.revision = number;
        store.publish_language_evidence(revision.clone(), 21)?;
    }
    revision.revision = 65;
    assert!(store.publish_language_evidence(revision, 22).is_err());
    let mut oversized = fixture("lid-000");
    oversized.revision = 2;
    let span = oversized.spans[0].clone();
    oversized.spans = (0..1024)
        .map(|ordinal| LanguageSpan {
            ordinal,
            ..span.clone()
        })
        .collect();
    assert!(matches!(
        store.publish_language_evidence(oversized, 22),
        Err(Error::InvalidInput("language evidence byte limit"))
    ));
    store.audit_language_evidence()?;
    Ok(())
}

#[test]
fn a_malformed_stored_payload_is_refused_on_read_and_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog.sqlite3");
    let mut store = setup(&path)?;
    let (_, mut evidence) = store.publish_language_evidence(fixture("lid"), 20)?;
    evidence.revision = 2;
    evidence.spans[0].languages[0].tag = "not_a_tag".into();
    insert_evidence(
        &store.connection,
        &evidence,
        &serde_json::to_string(&evidence)?,
        21,
    )?;
    assert!(matches!(
        store.language_evidence("lid", 2),
        Err(Error::StorageIntegrity)
    ));
    drop(store);
    assert!(matches!(Store::open(&path), Err(Error::CatalogIntegrity)));
    Ok(())
}
