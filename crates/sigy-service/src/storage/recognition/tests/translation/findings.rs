//! A stored finding cites one translation cue. Transcript text does not create one.

use super::*;
use crate::monitor::{ActionOrigin, FindingCite, FindingOriginal, Proposal};
use crate::translation::{TranslationOutcome, TranslationResult};

mod retained;
mod task;

fn cite(original: FindingOriginal) -> FindingCite {
    FindingCite {
        transcript_id: "pin".into(),
        transcript_revision: 1,
        translation_revision: 1,
        cue_ordinal: 0,
        original,
    }
}

fn desk(path: &std::path::Path) -> Result<Store> {
    let mut store = transcribed(path)?;
    store.create_monitor("fair", &super::super::follow("radio:v1"), 25)?;
    let (job, work) = store.admit_translation(&request("mt-1")?, 30)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 31)?;
    Ok(store)
}

fn activity(store: &Store) -> Result<(i64, i64, i64, i64, i64)> {
    Ok(store.connection.query_row(
        "SELECT (SELECT count(*) FROM analysis_jobs), (SELECT count(*) FROM translation_jobs), (SELECT count(*) FROM provider_attempts), (SELECT count(*) FROM requests), (SELECT count(*) FROM ledger_events)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?)
}

fn stored(store: &Store) -> Result<i64> {
    Ok(store
        .connection
        .query_row("SELECT count(*) FROM monitor_findings", [], |row| {
            row.get(0)
        })?)
}

#[test]
fn a_finding_cites_the_cue_and_text_creates_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    let before = activity(&store)?;
    store.propose_monitor_action(
        "fair",
        "note",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "Una feria mundial".into(),
        },
        32,
    )?;
    let start: i64 = store.connection.query_row(
        "SELECT starts_ms FROM capture_jobs WHERE id = 'one'",
        [],
        |row| row.get(0),
    )?;
    store.monitor_matches("fair", start.saturating_sub(1).max(0), start + 60_000)?;
    assert_eq!(stored(&store)?, 0);
    assert_eq!(activity(&store)?, before);

    let page = store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    assert_eq!(activity(&store)?, before);
    assert_eq!(stored(&store)?, 1);
    assert_eq!(page.original_script, "Una feria mundial");
    assert_eq!(page.english.as_deref(), Some("A world fair"));
    assert_eq!((page.start_us, page.end_us), (Some(0), Some(1_000_000)));
    assert_eq!(page.recording_id, "one");
    assert_eq!(page.stale_transcript, None);
    assert_eq!(page.stale_translation, None);
    assert_eq!(
        store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 41)?,
        page
    );

    let mut other = cite(FindingOriginal::Retained);
    other.cue_ordinal = 1;
    assert!(matches!(
        store.publish_finding("fair", "world", &other, 42),
        Err(Error::Analysis("finding-conflict"))
    ));
    assert!(matches!(
        store.publish_finding("fair", "world", &cite(FindingOriginal::Missing), 42),
        Err(Error::Analysis("finding-conflict"))
    ));
    assert_eq!(stored(&store)?, 1);
    other.cue_ordinal = 4;
    assert!(matches!(
        store.publish_finding("fair", "else", &other, 43),
        Err(Error::NotFound)
    ));
    let mut missing_translation = cite(FindingOriginal::Retained);
    missing_translation.translation_revision = 2;
    assert!(matches!(
        store.publish_finding("fair", "else", &missing_translation, 43),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.publish_finding("missing", "world", &cite(FindingOriginal::Retained), 43),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), -1),
        Err(Error::InvalidInput("finding clock"))
    ));
    assert_eq!(stored(&store)?, 1);

    let inserted = store.connection.execute(
        "INSERT INTO monitor_findings(monitor_id, id, transcript_id, transcript_revision, translation_revision, cue_ordinal, recording_id, original_state, start_us, end_us, created_ms) VALUES ('fair', 'bad', 'pin', 1, 1, 0, 'one', 'retained', 1, 1000000, 50)",
        [],
    );
    match inserted {
        Err(error) => assert!(error.to_string().contains("finding citation"), "{error}"),
        Ok(count) => return Err(format!("a mismatched interval was stored ({count})").into()),
    }
    assert!(
        store
            .connection
            .execute("UPDATE monitor_findings SET created_ms = created_ms", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM monitor_findings", [])
            .is_err()
    );
    drop(store);
    let store = Store::open(&path)?;
    let page = store.finding("fair", "world")?;
    assert_eq!(page.original_script, "Una feria mundial");
    assert_eq!(page.english.as_deref(), Some("A world fair"));
    Ok(())
}

#[test]
fn a_gap_rejects_a_retained_range_and_stores_missing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.connection.execute(
        "INSERT INTO recording_gaps VALUES ('one', 0, 'disconnect', 250000, 500000)",
        [],
    )?;
    assert!(matches!(
        store.publish_finding("fair", "span", &cite(FindingOriginal::Retained), 40),
        Err(Error::Analysis("finding-range"))
    ));
    assert_eq!(stored(&store)?, 0);
    let page = store.publish_finding("fair", "span", &cite(FindingOriginal::Missing), 40)?;
    assert_eq!(page.original, FindingOriginal::Missing);
    assert_eq!((page.start_us, page.end_us), (None, None));
    assert_eq!(page.original_script, "Una feria mundial");
    assert_eq!(page.english.as_deref(), Some("A world fair"));
    assert!(matches!(
        store.publish_finding("fair", "other", &cite(FindingOriginal::Expired), 41),
        Err(Error::Analysis("finding-original"))
    ));
    assert_eq!(stored(&store)?, 1);
    Ok(())
}

#[test]
fn a_deleted_recording_stores_expired_without_an_interval() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.begin_delete("one", false)?;
    assert!(matches!(
        store.publish_finding("fair", "gone", &cite(FindingOriginal::Retained), 40),
        Err(Error::Analysis("finding-range"))
    ));
    assert!(matches!(
        store.publish_finding("fair", "gone", &cite(FindingOriginal::Missing), 40),
        Err(Error::Analysis("finding-original"))
    ));
    assert_eq!(stored(&store)?, 0);
    let page = store.publish_finding("fair", "gone", &cite(FindingOriginal::Expired), 40)?;
    assert_eq!(page.original, FindingOriginal::Expired);
    assert_eq!((page.start_us, page.end_us), (None, None));
    assert_eq!(page.recording_id, "one");
    assert_eq!(page.original_script, "Una feria mundial");
    assert_eq!(stored(&store)?, 1);
    Ok(())
}

#[test]
fn a_later_revision_marks_the_finding_stale_without_rewriting_it() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    let before = activity(&store)?;
    let revision = store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    assert_eq!(revision, 2);
    assert_eq!(activity(&store)?, before);
    let page = store.finding("fair", "world")?;
    assert_eq!(page.transcript_revision, 1);
    assert_eq!(page.original_script, "Una feria mundial");
    assert_eq!(page.stale_transcript, Some(true));
    assert_eq!(page.stale_translation, None);
    assert_eq!(
        store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 61)?,
        page
    );
    let kept: i64 = store.connection.query_row(
        "SELECT transcript_revision FROM monitor_findings WHERE monitor_id = 'fair' AND id = 'world'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(kept, 1);

    let (job, work) = store.admit_translation(&request("mt-2")?, 70)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, "A local fair")]),
    );
    store.finish_translation(&work.ok_or("work")?, &outcome, 71)?;
    let page = store.finding("fair", "world")?;
    assert_eq!(page.translation_revision, 1);
    assert_eq!(page.english.as_deref(), Some("A world fair"));
    assert_eq!(page.stale_transcript, Some(true));
    assert_eq!(page.stale_translation, Some(true));
    let (analysis, translations, attempts, requests, ledger) = activity(&store)?;
    assert_eq!(
        (analysis, attempts, requests, ledger),
        (before.0, before.2, before.3, before.4)
    );
    assert_eq!(translations, before.1 + 1);
    Ok(())
}

#[test]
fn a_legacy_placeholder_is_not_a_citable_range() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), true)?;
    store.create_monitor("fair", &super::super::follow("radio:v1"), 25)?;
    assert!(matches!(
        store.publish_finding("fair", "old", &cite(FindingOriginal::Retained), 40),
        Err(Error::Analysis("finding-range"))
    ));
    assert_eq!(stored(&store)?, 0);
    Ok(())
}
