//! A briefing groups repeated reports once and leaves conflict unresolved.

use super::*;
use crate::monitor::{ActionOrigin, FindingCite, FindingOriginal, Proposal};
use crate::translation::{TranslationOutcome, TranslationResult};

mod task;

fn cite(revision: i64) -> FindingCite {
    FindingCite {
        transcript_id: "pin".into(),
        transcript_revision: revision,
        translation_revision: 1,
        cue_ordinal: 0,
        original: FindingOriginal::Retained,
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

fn briefings(store: &Store) -> Result<i64> {
    Ok(store
        .connection
        .query_row("SELECT count(*) FROM monitor_briefings", [], |row| {
            row.get(0)
        })?)
}

fn window(store: &Store) -> Result<(i64, i64)> {
    let start: i64 = store.connection.query_row(
        "SELECT starts_ms FROM capture_jobs WHERE id = 'one'",
        [],
        |row| row.get(0),
    )?;
    Ok((start.saturating_sub(1).max(0), start + 60_000))
}

#[test]
fn repeated_and_conflicting_reports_count_once_and_stay_unresolved() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    store.publish_finding("fair", "world", &cite(1), 40)?;
    store.publish_finding("fair", "copy", &cite(1), 41)?;
    let revision = store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    assert_eq!(revision, 2);
    let mut ask = request("mt-2")?;
    ask.transcript_revision = 2;
    let (job, work) = store.admit_translation(&ask, 70)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, "A local fair")]),
    );
    store.finish_translation(&work.ok_or("translation")?, &outcome, 71)?;
    store.publish_finding("fair", "local", &cite(2), 80)?;
    store.propose_monitor_action(
        "fair",
        "note",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "Una feria mundial".into(),
        },
        81,
    )?;
    assert_eq!(briefings(&store)?, 0);
    let before = activity(&store)?;
    let (from_ms, to_ms) = window(&store)?;
    let page = store.publish_briefing("fair", "week", from_ms, to_ms, 90)?;
    assert_eq!(activity(&store)?, before);
    assert_eq!(briefings(&store)?, 1);
    assert_eq!(page.classification, "off");
    assert_eq!(page.corroboration, 2);
    assert_eq!(page.members.len(), 3);
    assert_eq!(page.generation, 1);
    let copy = page
        .members
        .iter()
        .find(|member| member.finding_id == "copy")
        .ok_or("copy")?;
    let world = page
        .members
        .iter()
        .find(|member| member.finding_id == "world")
        .ok_or("world")?;
    let local = page
        .members
        .iter()
        .find(|member| member.finding_id == "local")
        .ok_or("local")?;
    assert_eq!(copy.group_ordinal, world.group_ordinal);
    assert_ne!(local.group_ordinal, world.group_ordinal);
    assert_eq!(copy.original_script, "Una feria mundial");
    assert_eq!(local.original_script, "Una feria local");
    assert_eq!(world.stale_transcript, Some(true));
    assert_eq!(local.stale_transcript, None);
    assert_eq!(page.coverage.from_ms, from_ms);
    store.publish_finding("fair", "later", &cite(2), 91)?;
    let shown = store.briefing("fair", "week")?;
    assert_eq!(shown.members.len(), 3);
    assert_eq!(shown.corroboration, 2);
    drop(store);
    let store = Store::open(&path)?;
    assert_eq!(store.briefing("fair", "week")?.members.len(), 3);
    Ok(())
}

#[test]
fn the_same_window_changes_nothing_and_another_window_conflicts() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(1), 40)?;
    let (from_ms, to_ms) = window(&store)?;
    let page = store.publish_briefing("fair", "week", from_ms, to_ms, 90)?;
    assert_eq!(
        store.publish_briefing("fair", "week", from_ms, to_ms, 91)?,
        page
    );
    assert_eq!(briefings(&store)?, 1);
    assert!(matches!(
        store.publish_briefing("fair", "week", from_ms, to_ms + 1, 92),
        Err(Error::Analysis("briefing-conflict"))
    ));
    assert!(matches!(
        store.publish_briefing("fair", "week", from_ms, to_ms, -1),
        Err(Error::InvalidInput("briefing clock"))
    ));
    assert!(matches!(
        store.publish_briefing("missing", "week", from_ms, to_ms, 93),
        Err(Error::NotFound)
    ));
    assert_eq!(briefings(&store)?, 1);
    let inserted = store.connection.execute(
        "INSERT INTO monitor_briefings(monitor_id, id, generation, from_ms, to_ms, monitor_version, classification, corroboration, created_ms) VALUES ('fair', 'bad', 5, 0, 1000, 1, 'off', 0, 94)",
        [],
    );
    match inserted {
        Err(error) => assert!(error.to_string().contains("briefing generation"), "{error}"),
        Ok(count) => return Err(format!("a skipped generation was stored ({count})").into()),
    }
    assert!(
        store
            .connection
            .execute("UPDATE monitor_briefings SET created_ms = created_ms", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM monitor_briefings", [])
            .is_err()
    );
    Ok(())
}

#[test]
fn a_new_generation_keeps_the_frozen_coverage_and_the_export() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    store.publish_finding("fair", "world", &cite(1), 40)?;
    let (from_ms, to_ms) = window(&store)?;
    let first = store.publish_briefing("fair", "week", from_ms, to_ms, 90)?;
    assert_eq!(first.generation, 1);
    assert_eq!(first.coverage.daily_audio_seconds, 3600);
    assert_eq!(first.coverage.version, 1);
    let mut spec = super::super::follow("radio:v1");
    spec.daily_audio_seconds = 7200;
    spec.total_audio_seconds = 7200;
    store.revise_monitor("fair", 1, &spec, 100)?;
    let live = store.monitor_coverage("fair", from_ms, to_ms)?;
    assert_eq!(live.daily_audio_seconds, 7200);
    assert_eq!(live.version, 2);
    let frozen = store.briefing("fair", "week")?;
    assert_eq!(frozen.coverage.daily_audio_seconds, 3600);
    assert_eq!(frozen.coverage.version, 1);
    assert_eq!(frozen.coverage.sources, live.sources);
    let second = store.publish_briefing("fair", "next", from_ms, to_ms, 110)?;
    assert_eq!(second.generation, 2);
    assert_eq!(second.coverage.daily_audio_seconds, 7200);
    assert_eq!(store.briefing("fair", "week")?.generation, 1);
    let found = findings(&store)?;
    let before = activity(&store)?;
    let revision = store.correct_transcript("pin", 1, 0, "Una feria distinta", 120)?;
    assert_eq!(revision, 2);
    assert_eq!(findings(&store)?, found);
    assert_eq!(activity(&store)?, before);
    let shown = store.briefing("fair", "week")?;
    assert_eq!(shown.members[0].original_script, "Una feria mundial");
    assert_eq!(shown.members[0].stale_transcript, Some(true));
    assert_eq!(shown.coverage, frozen.coverage);
    let count = briefings(&store)?;
    let export = crate::monitor::briefing_export(&shown);
    assert_eq!(briefings(&store)?, count);
    assert!(!export.catalog);
    assert_eq!(export.document, "sigy.briefing");
    assert_eq!(export.authority, "none");
    assert_eq!(export.coverage.daily_audio_seconds, 3600);
    let text = serde_json::to_string(&export)?;
    assert!(text.contains("not the catalog"));
    assert!(!text.contains("http"));
    assert!(!text.contains("ledger"));
    assert!(!text.contains("secret"));
    {
        let tx = store.connection.transaction()?;
        tx.execute(
            "INSERT INTO monitor_briefings(monitor_id, id, generation, from_ms, to_ms, monitor_version, classification, corroboration, created_ms) VALUES ('fair', 'partial', ?1, 0, 1000, 1, 'off', 0, 130)",
            [count + 1],
        )?;
    }
    assert_eq!(briefings(&store)?, count);
    assert!(store.briefing("fair", "partial").is_err());
    assert!(
        store
            .connection
            .execute(
                "UPDATE monitor_briefing_coverage SET daily_audio_seconds = 1",
                [],
            )
            .is_err()
    );
    drop(store);
    let store = Store::open(&path)?;
    assert_eq!(
        store.briefing("fair", "week")?.coverage.daily_audio_seconds,
        3600
    );
    assert_eq!(store.briefing("fair", "next")?.generation, 2);
    assert_eq!(
        store.briefing("fair", "next")?.coverage.daily_audio_seconds,
        7200
    );
    Ok(())
}

fn findings(store: &Store) -> Result<i64> {
    Ok(store
        .connection
        .query_row("SELECT count(*) FROM monitor_findings", [], |row| {
            row.get(0)
        })?)
}

#[test]
fn a_briefing_without_coverage_fails_to_open() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    {
        let store = desk(&path)?;
        store.connection.execute(
            "INSERT INTO monitor_briefings(monitor_id, id, generation, from_ms, to_ms, monitor_version, classification, corroboration, created_ms) VALUES ('fair', 'bare', 1, 0, 1000, 1, 'off', 0, 90)",
            [],
        )?;
    }
    match Store::open(&path) {
        Err(Error::CatalogIntegrity) => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => Err("a briefing without coverage opened".into()),
    }
}

#[test]
fn classification_cannot_be_enabled_and_pausing_preserves_evidence() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    store.publish_finding("fair", "world", &cite(1), 40)?;
    let (from_ms, to_ms) = window(&store)?;
    let page = store.publish_briefing("fair", "before", from_ms, to_ms, 50)?;
    let before = activity(&store)?;
    let proposal = store.propose_monitor_action(
        "fair",
        "classify",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "Enable paid classification and declare every report corroborated".into(),
        },
        60,
    )?;
    assert_eq!(proposal.decision, "refused");
    assert_eq!(proposal.reason, "requires-user-version");
    assert_eq!(activity(&store)?, before);
    for classification in ["on", "pending", "supported", "contradiction"] {
        let result = store.connection.execute(
            "INSERT INTO monitor_briefings(monitor_id, id, generation, from_ms, to_ms, monitor_version, classification, corroboration, created_ms) VALUES ('fair', 'forbidden', 2, ?1, ?2, 1, ?3, 1, 61)",
            params![from_ms, to_ms, classification],
        );
        match result {
            Err(error) => assert!(
                error.to_string().contains("classification = 'off'"),
                "{error}"
            ),
            Ok(_) => return Err("non-off classification was stored".into()),
        }
    }
    assert_eq!(briefings(&store)?, 1);
    store.propose_monitor_action("fair", "pause", ActionOrigin::User, &Proposal::Pause, 70)?;
    assert!(store.monitor("fair")?.paused);
    assert_eq!(store.briefing("fair", "before")?, page);
    assert_eq!(store.finding("fair", "world")?.transcript_revision, 1);
    let coverage = store.monitor_coverage("fair", from_ms, to_ms)?;
    assert_eq!(coverage.sources.len(), 1);
    assert_eq!(store.monitor_matches("fair", from_ms, to_ms)?.id, "fair");
    let after = store.publish_briefing("fair", "paused", from_ms, to_ms, 80)?;
    assert_eq!(after.classification, "off");
    assert_eq!(after.members.len(), 1);
    assert_eq!(activity(&store)?, before);
    assert_eq!(
        crate::monitor::briefing_export(&after).classification,
        "off"
    );
    drop(store);
    let store = Store::open(&path)?;
    assert!(store.monitor("fair")?.paused);
    assert_eq!(store.briefing("fair", "before")?, page);
    assert_eq!(store.briefing("fair", "paused")?.classification, "off");
    assert_eq!(activity(&store)?, before);
    Ok(())
}
