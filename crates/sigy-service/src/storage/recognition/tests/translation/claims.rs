//! Manual claims cite stored findings. Transcript text does not create one.

use std::path::Path;

use super::*;
use crate::claim::{
    CLAIM_LIMIT, ClaimEvidenceKind, ClaimRelation, ClaimStatus, ClaimView, EVIDENCE_LIMIT,
    SIGHTING_LIMIT, STATUS_LIMIT, TEXT_REVISION_LIMIT,
};
use crate::control::Operation;
use crate::monitor::{ActionOrigin, FindingCite, FindingOriginal, Proposal};
use crate::storage::claims::{EvidenceRequest, StatusRequest};
use crate::translation::{TranslationOutcome, TranslationResult};

fn cite(original: FindingOriginal) -> FindingCite {
    FindingCite {
        transcript_id: "pin".into(),
        transcript_revision: 1,
        translation_revision: 1,
        cue_ordinal: 0,
        original,
    }
}

fn desk(path: &Path) -> Result<Store> {
    let mut store = super::transcribed(path)?;
    store.create_monitor("fair", &super::super::follow("radio:v1"), 25)?;
    let (job, work) = store.admit_translation(&super::request("mt-1")?, 30)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![super::translated(0, "A world fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 31)?;
    Ok(store)
}

fn tally(store: &Store) -> Result<(i64, i64, i64, i64, i64)> {
    Ok(store.connection.query_row(
        "SELECT (SELECT count(*) FROM claims), (SELECT count(*) FROM claim_revisions), (SELECT count(*) FROM claim_sightings), (SELECT count(*) FROM claim_evidence), (SELECT count(*) FROM claim_status)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?)
}

fn contains_key(value: &serde_json::Value, key: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.contains_key(key) || map.values().any(|child| contains_key(child, key))
        }
        serde_json::Value::Array(items) => items.iter().any(|child| contains_key(child, key)),
        _ => false,
    }
}

#[test]
fn wording_history_is_kept_and_other_text_inserts_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    let start: i64 = store.connection.query_row(
        "SELECT starts_ms FROM capture_jobs WHERE id = 'one'",
        [],
        |row| row.get(0),
    )?;
    store.propose_monitor_action(
        "fair",
        "note",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "سد".into(),
        },
        32,
    )?;
    store.monitor_matches("fair", start.saturating_sub(1).max(0), start + 60_000)?;
    assert_eq!(tally(&store)?, (0, 0, 0, 0, 0));
    assert!(matches!(
        store.create_claim("dam", " \n ", 40),
        Err(Error::InvalidInput("claim text"))
    ));
    assert!(matches!(
        store.create_claim("dam", "   ", 40),
        Err(Error::InvalidInput("claim text"))
    ));
    assert!(matches!(
        store.create_claim("dam", "one", -1),
        Err(Error::InvalidInput("claim clock"))
    ));
    assert_eq!(tally(&store)?, (0, 0, 0, 0, 0));

    let created = store.create_claim("dam", "  سد  ", 40)?;
    assert_eq!(created.text, "سد");
    assert_eq!(created.text_revision, 1);
    assert_eq!(created.revisions.len(), 1);
    assert_eq!(created.status, "unverified");
    assert_eq!(created.status_revision, 0);
    assert_eq!(created.status_setter, None);
    assert_eq!(tally(&store)?, (1, 1, 0, 0, 0));
    let replay = store.create_claim("dam", "سد", 41)?;
    assert_eq!(replay.created_ms, created.created_ms);
    assert_eq!(tally(&store)?, (1, 1, 0, 0, 0));
    assert!(matches!(
        store.create_claim("dam", "other", 42),
        Err(Error::Analysis("claim-conflict"))
    ));
    let revised = store.revise_claim("dam", 1, "The dam report changed.", 43)?;
    assert_eq!(revised.text_revision, 2);
    assert_eq!(revised.revisions[0].text, "سد");
    assert_eq!(revised.revisions[1].text, "The dam report changed.");
    assert_eq!(tally(&store)?, (1, 2, 0, 0, 0));
    assert!(matches!(
        store.revise_claim("dam", 1, "lost", 44),
        Err(Error::Analysis("claim-conflict"))
    ));
    assert_eq!(
        store
            .revise_claim("dam", 2, "The dam report changed.", 45)?
            .text_revision,
        2
    );
    assert_eq!(tally(&store)?, (1, 2, 0, 0, 0));
    store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    assert_eq!(tally(&store)?, (1, 2, 0, 0, 0));
    assert_eq!(store.claim_ids()?, vec!["dam".to_owned()]);
    drop(store);
    let store = Store::open(&path)?;
    let page = store.claim("dam")?;
    assert_eq!(page.revisions.len(), 2);
    assert_eq!(page.revisions[0].text, "سد");
    assert!(
        store
            .connection
            .execute("UPDATE claims SET created_ms = 1", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM claim_revisions", [])
            .is_err()
    );
    assert!(store.connection.execute("DELETE FROM claims", []).is_err());
    Ok(())
}

#[test]
fn a_sighting_requires_a_stored_finding_and_reports_staleness() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.publish_finding("fair", "copy", &cite(FindingOriginal::Retained), 41)?;
    store.create_claim("dam", "Two accounts describe the same fair.", 42)?;
    assert!(matches!(
        store.add_sighting("dam", "see", "fair", "missing", 43),
        Err(Error::InvalidInput("claim sighting; link a stored finding"))
    ));
    assert!(matches!(
        store.add_sighting("missing", "see", "fair", "world", 43),
        Err(Error::NotFound)
    ));
    assert_eq!(tally(&store)?.2, 0);
    let linked = store.add_sighting("dam", "see", "fair", "world", 44)?;
    assert_eq!(linked.sightings.len(), 1);
    assert_eq!(linked.sightings[0].time_basis, "recording media clock");
    assert_eq!(linked.sightings[0].source_revision, "radio:v1");
    assert_eq!(
        (linked.sightings[0].start_us, linked.sightings[0].end_us),
        (Some(0), Some(1_000_000))
    );
    assert_eq!(linked.sightings[0].stale_transcript, None);
    assert_eq!(
        store
            .add_sighting("dam", "see", "fair", "world", 45)?
            .sightings
            .len(),
        1
    );
    assert!(matches!(
        store.add_sighting("dam", "see", "fair", "copy", 46),
        Err(Error::Analysis("claim-conflict"))
    ));
    assert_eq!(tally(&store)?.2, 1);
    store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    let (job, work) = store.admit_translation(&super::request("mt-2")?, 70)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![super::translated(0, "A local fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 71)?;
    let stale = store.claim("dam")?;
    assert_eq!(stale.sightings[0].finding_id, "world");
    assert_eq!(stale.sightings[0].stale_transcript, Some(true));
    assert_eq!(stale.sightings[0].stale_translation, Some(true));
    let kept: i64 =
        store
            .connection
            .query_row("SELECT count(*) FROM claim_sightings", [], |row| row.get(0))?;
    assert_eq!(kept, 1);

    store.begin_delete("one", false)?;
    store.publish_finding("fair", "gone", &cite(FindingOriginal::Expired), 80)?;
    let expired = store.add_sighting("dam", "old", "fair", "gone", 81)?;
    let old = expired
        .sightings
        .iter()
        .find(|sighting| sighting.id == "old")
        .ok_or(Error::NotFound)?;
    assert_eq!(old.time_basis, "no retained interval");
    assert_eq!((old.start_us, old.end_us), (None, None));
    Ok(())
}

#[test]
fn status_requires_user_evidence_and_never_starts_verified() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.create_claim("dam", "The report was corrected.", 41)?;
    store.add_sighting("dam", "see", "fair", "world", 42)?;
    assert_eq!(store.claim("dam")?.status, "unverified");
    assert!(matches!(
        store.set_claim_status(
            StatusRequest {
                claim: "dam",
                expected_revision: 0,
                status: ClaimStatus::Disputed,
                evidence: "missing",
            },
            43,
        ),
        Err(Error::InvalidInput("claim status evidence"))
    ));
    assert_eq!(tally(&store)?.4, 0);
    store.add_evidence(
        EvidenceRequest {
            claim: "dam",
            evidence: "note",
            sighting: "see",
            kind: ClaimEvidenceKind::Disputes,
            note: "The later bulletin corrects the date.",
            relation: Some(ClaimRelation::Correction),
        },
        44,
    )?;
    assert_eq!(store.claim("dam")?.status, "unverified");
    assert!(matches!(
        store.set_claim_status(
            StatusRequest {
                claim: "dam",
                expected_revision: 1,
                status: ClaimStatus::Retracted,
                evidence: "note",
            },
            45,
        ),
        Err(Error::Analysis("claim-conflict"))
    ));
    let retracted = store.set_claim_status(
        StatusRequest {
            claim: "dam",
            expected_revision: 0,
            status: ClaimStatus::Retracted,
            evidence: "note",
        },
        46,
    )?;
    assert_eq!(retracted.status, "retracted");
    assert_eq!(retracted.status_label, "retracted or corrected by source");
    assert_eq!(retracted.status_revision, 1);
    assert_eq!(retracted.status_setter.as_deref(), Some("user"));
    assert_eq!(retracted.status_evidence.as_deref(), Some("note"));
    assert_eq!(retracted.evidence[0].origin, "user");
    let disputed = store.set_claim_status(
        StatusRequest {
            claim: "dam",
            expected_revision: 1,
            status: ClaimStatus::Disputed,
            evidence: "note",
        },
        47,
    )?;
    assert_eq!(disputed.status, "disputed");
    assert_eq!(disputed.status_revision, 2);
    assert_eq!(tally(&store)?.4, 2);
    let stored: i64 = store.connection.query_row(
        "SELECT count(*) FROM claim_status WHERE setter = 'user'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(stored, 2);
    Ok(())
}

fn paired(path: &Path) -> Result<Store> {
    let mut store = desk(path)?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.publish_finding("fair", "copy", &cite(FindingOriginal::Retained), 41)?;
    store.create_claim("dam", "Several stations carried one bulletin.", 42)?;
    store.add_sighting("dam", "first", "fair", "world", 43)?;
    store.add_sighting("dam", "second", "fair", "copy", 44)?;
    Ok(store)
}

fn relation_cases() -> [(ClaimEvidenceKind, Option<ClaimRelation>, &'static str); 8] {
    [
        (
            ClaimEvidenceKind::Context,
            Some(ClaimRelation::Quotation),
            "quote",
        ),
        (
            ClaimEvidenceKind::Disputes,
            Some(ClaimRelation::Negation),
            "negate",
        ),
        (
            ClaimEvidenceKind::Unresolved,
            Some(ClaimRelation::Sarcasm),
            "sarcasm",
        ),
        (
            ClaimEvidenceKind::Disputes,
            Some(ClaimRelation::ChangedDate),
            "date",
        ),
        (
            ClaimEvidenceKind::Context,
            Some(ClaimRelation::Translation),
            "translation",
        ),
        (
            ClaimEvidenceKind::Context,
            Some(ClaimRelation::SharedPressRelease),
            "release",
        ),
        (
            ClaimEvidenceKind::Disputes,
            Some(ClaimRelation::Correction),
            "correction",
        ),
        (ClaimEvidenceKind::Unresolved, None, "open"),
    ]
}

#[test]
fn timeline_and_carriers_precede_sightings_without_a_score() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = paired(&directory.path().join("catalog"))?;
    let page = store.claim("dam")?;
    assert_eq!(page.timeline.searched, "linked findings only");
    assert!(!page.timeline.library_search);
    assert_eq!(
        page.timeline.languages,
        "not assessed outside the cited cues"
    );
    assert_eq!(
        page.timeline.window,
        "cited retained intervals from 0 us to 1000000 us"
    );
    assert_eq!(
        page.timeline.gaps,
        "Material outside the linked findings was not examined."
    );
    assert_eq!(page.carriers.len(), 1);
    assert_eq!(
        page.carriers[0].sighting_ids,
        ["first".to_owned(), "second".to_owned()]
    );
    assert_eq!(page.carriers[0].repetition_group, "unknown");
    assert_eq!(page.carriers[0].independence, "unknown");
    assert_eq!(page.status, "unverified");
    let text = serde_json::to_string(&page)?;
    let timeline = text.find("\"timeline\"").ok_or(Error::StorageIntegrity)?;
    let sightings = text.find("\"sightings\"").ok_or(Error::StorageIntegrity)?;
    assert!(timeline < sightings);
    let value = serde_json::to_value(&page)?;
    assert!(!contains_key(&value, "score"));
    assert!(!contains_key(&value, "anomaly"));
    Ok(())
}

#[test]
fn stored_relations_and_pages_have_no_score() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = paired(&directory.path().join("catalog"))?;
    for (index, (kind, relation, id)) in relation_cases().into_iter().enumerate() {
        store.add_evidence(
            EvidenceRequest {
                claim: "dam",
                evidence: id,
                sighting: "first",
                kind,
                note: "The user stored this relation.",
                relation,
            },
            50_i64 + i64::try_from(index)?,
        )?;
    }
    let page = store.claim("dam")?;
    let labels: Vec<(String, String)> = page
        .evidence
        .iter()
        .map(|link| (link.relation.clone(), link.kind.as_str().to_owned()))
        .collect();
    assert_eq!(
        labels,
        vec![
            ("quotation".into(), "context".into()),
            ("negation".into(), "disputes".into()),
            ("sarcasm".into(), "unresolved".into()),
            ("changed-date".into(), "disputes".into()),
            ("translation".into(), "context".into()),
            ("shared-press-release".into(), "context".into()),
            ("correction".into(), "disputes".into()),
            ("unspecified".into(), "unresolved".into()),
        ]
    );
    assert_eq!(page.status, "unverified");
    let mut injected = serde_json::to_value(&page)?;
    assert!(!contains_key(&injected, "score"));
    assert!(!contains_key(&injected, "anomaly"));
    injected
        .as_object_mut()
        .ok_or(Error::StorageIntegrity)?
        .insert("score".to_owned(), serde_json::json!(1));
    assert!(serde_json::from_value::<ClaimView>(injected).is_err());
    assert!(
        serde_json::from_str::<Operation>(
            r#"{"kind":"claim","command":{"action":"show","id":"dam","score":1}}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<ClaimRelation>("\"truth\"").is_err());
    let anomaly_tables: i64 = store.connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE lower(name) LIKE '%anomaly%'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(anomaly_tables, 0);
    let definition: String = store.connection.query_row(
        "SELECT coalesce(group_concat(sql, char(10)), '') FROM sqlite_schema WHERE name LIKE 'claim%'",
        [],
        |row| row.get(0),
    )?;
    let definition = definition.to_ascii_lowercase();
    assert!(!definition.contains("score"));
    assert!(!definition.contains("anomaly"));
    assert!(!definition.contains("credibility"));
    Ok(())
}

#[test]
fn caps_stop_at_the_declared_bounds() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    for index in 0..CLAIM_LIMIT {
        store.connection.execute(
            "INSERT INTO claims(id, created_ms) VALUES (?1, 1)",
            [format!("filled-{index}")],
        )?;
        store.connection.execute(
            "INSERT INTO claim_revisions(claim_id, revision, text, created_ms) VALUES (?1, 1, 'kept', 1)",
            [format!("filled-{index}")],
        )?;
    }
    assert!(matches!(
        store.create_claim("extra", "one more", 2),
        Err(Error::Analysis("claim-limit"))
    ));
    assert!(store.claim("extra").is_err());

    let room_path = directory.path().join("room");
    let mut store = desk(&room_path)?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.create_claim("dam", "bounded", 41)?;
    for revision in 1..TEXT_REVISION_LIMIT {
        store.revise_claim(
            "dam",
            revision,
            &format!("word {revision}"),
            41_i64 + revision,
        )?;
    }
    assert!(matches!(
        store.revise_claim("dam", TEXT_REVISION_LIMIT, "too far", 200),
        Err(Error::Analysis("claim-revision-limit"))
    ));
    assert_eq!(store.claim("dam")?.text_revision, TEXT_REVISION_LIMIT);
    for index in 0..SIGHTING_LIMIT {
        store.add_sighting(
            "dam",
            &format!("s{index}"),
            "fair",
            "world",
            300_i64 + index,
        )?;
    }
    assert!(matches!(
        store.add_sighting("dam", "overflow", "fair", "world", 400),
        Err(Error::Analysis("claim-sighting-limit"))
    ));
    for index in 0..EVIDENCE_LIMIT {
        store.connection.execute(
            "INSERT INTO claim_evidence(claim_id, id, sighting_id, kind, origin, note, relation, created_ms) VALUES ('dam', ?1, 's0', 'context', 'user', 'note', 'unspecified', ?2)",
            params_evidence(index),
        )?;
    }
    assert!(matches!(
        store.add_evidence(
            EvidenceRequest {
                claim: "dam",
                evidence: "overflow",
                sighting: "s0",
                kind: ClaimEvidenceKind::Context,
                note: "another",
                relation: None,
            },
            500,
        ),
        Err(Error::Analysis("claim-evidence-limit"))
    ));
    for index in 1..=STATUS_LIMIT {
        store.connection.execute(
            "INSERT INTO claim_status(claim_id, revision, status, evidence_id, setter, created_ms) VALUES ('dam', ?1, 'disputed', 'e0', 'user', ?1)",
            [index],
        )?;
    }
    assert!(matches!(
        store.set_claim_status(
            StatusRequest {
                claim: "dam",
                expected_revision: STATUS_LIMIT,
                status: ClaimStatus::Disputed,
                evidence: "e0",
            },
            600,
        ),
        Err(Error::Analysis("claim-status-limit"))
    ));
    assert_eq!(tally(&store)?.4, STATUS_LIMIT);
    Ok(())
}

#[test]
fn v51_catalog_gains_claim_tables_and_a_conflict_rolls_back() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let store = Store::open(&path)?;
    crate::storage::claims::revert_052_for_tests(&store.connection)?;
    store.connection.pragma_update(None, "user_version", 51)?;
    drop(store);
    let store = Store::open(&path)?;
    assert_eq!(tally(&store)?, (0, 0, 0, 0, 0));
    let version: i64 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, i64::from(crate::storage::SCHEMA_VERSION));
    let holds: i64 =
        store
            .connection
            .query_row("SELECT count(*) FROM completion_holds", [], |row| {
                row.get(0)
            })?;
    assert_eq!(holds, 0);
    drop(store);

    let conflict = directory.path().join("conflict");
    let store = Store::open(&conflict)?;
    crate::storage::claims::revert_052_for_tests(&store.connection)?;
    store
        .connection
        .execute_batch("CREATE TABLE claims(conflict INTEGER); PRAGMA user_version = 51;")?;
    drop(store);
    assert!(Store::open(&conflict).is_err());
    let connection = rusqlite::Connection::open(&conflict)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 51);
    let shape: String = connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE name = 'claims'",
        [],
        |row| row.get(0),
    )?;
    assert!(shape.contains("conflict"));
    let revisions: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'claim_revisions'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(revisions, 0);
    Ok(())
}

fn params_evidence(index: i64) -> (String, i64) {
    (format!("e{index}"), index)
}
