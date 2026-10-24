//! Manual claims cite stored findings or one retained cue. Transcript text does not create one.

use std::path::Path;

use super::*;
use crate::archive::{ArchiveQuery, ArchiveRevisions};
use crate::claim::{
    CLAIM_LIMIT, ClaimBasisReason, ClaimEvidenceKind, ClaimRelation, ClaimStatus, ClaimView,
    EVIDENCE_LIMIT, SIGHTING_LIMIT, STATUS_LIMIT, TEXT_REVISION_LIMIT,
};
use crate::control::Operation;
use crate::monitor::{ActionOrigin, FindingCite, FindingOriginal, Proposal};
use crate::storage::claims::{EvidenceRequest, PassageRequest, StatusRequest};
use crate::translation::{TranslationOutcome, TranslationResult};

fn cite(original: FindingOriginal) -> FindingCite {
    cite_revision(1, original)
}

fn cite_revision(revision: i64, original: FindingOriginal) -> FindingCite {
    FindingCite {
        transcript_id: "pin".into(),
        transcript_revision: revision,
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
    assert_eq!(created.basis.state, "none");
    assert_eq!(created.basis.reasons, []);
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
    assert_eq!(stale.sightings[0].finding_id.as_deref(), Some("world"));
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
    assert_eq!(page.timeline.searched, "linked findings and stored cues");
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
        "Material outside the linked findings and stored cues was not examined."
    );
    assert_eq!(page.carriers.len(), 1);
    assert_eq!(
        page.carriers[0].sighting_ids,
        ["first".to_owned(), "second".to_owned()]
    );
    assert_eq!(page.carriers[0].repetition_group, "unknown");
    assert_eq!(page.carriers[0].independence, "unknown");
    assert_eq!(page.status, "unverified");
    assert_eq!(page.basis.state, "none");
    assert_eq!(page.basis.reasons, []);
    assert!(page.basis.evidence_id.is_none());
    let text = serde_json::to_string(&page)?;
    let restored: ClaimView = serde_json::from_str(&text)?;
    assert_eq!(restored.basis, page.basis);
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

fn corroborate(
    store: &mut Store,
    claim: &str,
    evidence: &str,
    sighting: &str,
    now: i64,
) -> Result<ClaimView> {
    store.add_evidence(
        EvidenceRequest {
            claim,
            evidence,
            sighting,
            kind: ClaimEvidenceKind::Supports,
            note: "The user stored this relation.",
            relation: None,
        },
        now,
    )?;
    store.set_claim_status(
        StatusRequest {
            claim,
            expected_revision: 0,
            status: ClaimStatus::Corroborated,
            evidence,
        },
        now + 1,
    )
}

fn status_row(store: &Store, claim: &str, revision: i64) -> Result<(String, String)> {
    Ok(store.connection.query_row(
        "SELECT status, evidence_id FROM claim_status WHERE claim_id = ?1 AND revision = ?2",
        (claim, revision),
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

fn translate_at(store: &mut Store, id: &str, revision: i64, english: &str, now: i64) -> Result<()> {
    let mut request = super::request(id)?;
    request.transcript_revision = revision;
    let (job, work) = store.admit_translation(&request, now)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![super::translated(0, english)]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, now + 1)?;
    Ok(())
}

fn cited_claim(path: &Path) -> Result<Store> {
    let mut store = desk(path)?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.create_claim("dam", "The bulletin named the fair.", 41)?;
    store.add_sighting("dam", "see", "fair", "world", 42)?;
    store.create_claim("open", "No status is stored.", 43)?;
    store.add_sighting("open", "see", "fair", "world", 44)?;
    corroborate(&mut store, "dam", "note", "see", 45)?;
    Ok(store)
}

#[test]
fn a_status_keeps_its_token_when_the_cited_basis_changes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = cited_claim(&directory.path().join("catalog"))?;
    let decided = store.claim("dam")?;
    assert_eq!(decided.status, "corroborated");
    assert_eq!(decided.status_revision, 1);
    assert_eq!(decided.status_evidence.as_deref(), Some("note"));
    assert_eq!(decided.basis.state, "current");
    assert_eq!(decided.basis.reasons, []);
    assert_eq!(decided.basis.evidence_id.as_deref(), Some("note"));
    assert_eq!(decided.basis.sighting_id.as_deref(), Some("see"));
    assert_eq!(decided.basis.finding_id.as_deref(), Some("world"));
    assert_eq!(
        status_row(&store, "dam", 1)?,
        ("corroborated".to_owned(), "note".to_owned())
    );
    let open = store.claim("open")?;
    assert_eq!(open.status, "unverified");
    assert_eq!(open.basis.state, "none");
    assert!(open.basis.evidence_id.is_none());

    store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    let reviewed = store.claim("dam")?;
    assert_eq!(reviewed.status, "corroborated");
    assert_eq!(reviewed.status_revision, 1);
    assert_eq!(reviewed.status_evidence.as_deref(), Some("note"));
    assert_eq!(reviewed.basis.state, "review");
    assert_eq!(reviewed.basis.reasons, vec![ClaimBasisReason::Correction]);
    assert_eq!(reviewed.basis.finding_id.as_deref(), Some("world"));
    let still_open = store.claim("open")?;
    assert_eq!(still_open.basis.state, "none");
    assert_eq!(still_open.sightings[0].stale_transcript, Some(true));
    assert_eq!(
        status_row(&store, "dam", 1)?,
        ("corroborated".to_owned(), "note".to_owned())
    );
    let sightings: i64 = store.connection.query_row(
        "SELECT count(*) FROM claim_sightings WHERE claim_id = 'dam'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(sightings, 1);
    let statuses: i64 = store.connection.query_row(
        "SELECT count(*) FROM claim_status WHERE claim_id = 'dam'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(statuses, 1);
    Ok(())
}

#[test]
fn a_new_status_can_cite_the_corrected_revision() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = cited_claim(&directory.path().join("catalog"))?;
    store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    translate_at(&mut store, "mt-fix", 2, "A local fair", 70)?;
    store.publish_finding(
        "fair",
        "later",
        &cite_revision(2, FindingOriginal::Retained),
        72,
    )?;
    store.create_claim("next", "The corrected bulletin is the one cited.", 73)?;
    store.add_sighting("next", "see", "fair", "later", 74)?;
    let replacement = corroborate(&mut store, "next", "note", "see", 75)?;
    assert_eq!(replacement.status, "corroborated");
    assert_eq!(replacement.basis.state, "current");
    assert_eq!(replacement.basis.reasons, []);
    assert_eq!(replacement.basis.finding_id.as_deref(), Some("later"));
    assert_eq!(store.claim("dam")?.basis.state, "review");

    store.add_evidence(
        EvidenceRequest {
            claim: "dam",
            evidence: "again",
            sighting: "see",
            kind: ClaimEvidenceKind::Supports,
            note: "The user looked again.",
            relation: None,
        },
        80,
    )?;
    let appended = store.set_claim_status(
        StatusRequest {
            claim: "dam",
            expected_revision: 1,
            status: ClaimStatus::Corroborated,
            evidence: "again",
        },
        81,
    )?;
    assert_eq!(appended.status_revision, 2);
    assert_eq!(appended.status_evidence.as_deref(), Some("again"));
    assert_eq!(appended.basis.state, "review");
    assert_eq!(appended.basis.reasons, vec![ClaimBasisReason::Correction]);
    assert_eq!(
        status_row(&store, "dam", 1)?,
        ("corroborated".to_owned(), "note".to_owned())
    );
    let value = serde_json::to_value(&appended)?;
    assert!(!contains_key(&value, "score"));
    assert!(!contains_key(&value, "anomaly"));
    assert_eq!(value["basis"]["reasons"], serde_json::json!(["correction"]));
    let mut injected = value;
    injected
        .as_object_mut()
        .ok_or(Error::StorageIntegrity)?
        .insert("score".to_owned(), serde_json::json!(1));
    assert!(serde_json::from_value::<ClaimView>(injected).is_err());
    assert!(serde_json::from_str::<ClaimBasisReason>("\"withdrawal\"").is_err());
    Ok(())
}

#[test]
fn a_newer_translation_marks_superseded_without_a_correction() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.create_claim("dam", "The first translation is the one cited.", 41)?;
    store.add_sighting("dam", "see", "fair", "world", 42)?;
    corroborate(&mut store, "dam", "note", "see", 43)?;
    let (job, work) = store.admit_translation(&super::request("mt-2")?, 70)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![super::translated(0, "A local fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 71)?;
    let page = store.claim("dam")?;
    assert_eq!(page.status, "corroborated");
    assert_eq!(page.status_revision, 1);
    assert_eq!(page.basis.state, "review");
    assert_eq!(page.basis.reasons, vec![ClaimBasisReason::Superseded]);
    assert_eq!(page.sightings[0].stale_transcript, None);
    assert_eq!(page.sightings[0].stale_translation, Some(true));
    assert_eq!(
        status_row(&store, "dam", 1)?,
        ("corroborated".to_owned(), "note".to_owned())
    );
    let statuses: i64 = store.connection.query_row(
        "SELECT count(*) FROM claim_status WHERE claim_id = 'dam'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(statuses, 1);
    Ok(())
}

#[test]
fn another_findings_media_does_not_review_this_decision() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.create_claim("dam", "The retained account stands.", 41)?;
    store.add_sighting("dam", "see", "fair", "world", 42)?;
    let kept = corroborate(&mut store, "dam", "note", "see", 43)?;
    assert_eq!(kept.basis.state, "current");
    assert_eq!(kept.basis.reasons, []);

    store.connection.execute(
        "INSERT INTO recording_gaps VALUES ('one', 0, 'disconnect', 250000, 500000)",
        [],
    )?;
    store.publish_finding("fair", "hole", &cite(FindingOriginal::Missing), 50)?;
    store.create_claim("gap", "The cue crosses a gap.", 51)?;
    store.add_sighting("gap", "see", "fair", "hole", 52)?;
    let missing = corroborate(&mut store, "gap", "note", "see", 53)?;
    assert_eq!(missing.basis.state, "review");
    assert_eq!(missing.basis.reasons, vec![ClaimBasisReason::MissingMedia]);
    assert_eq!(missing.basis.finding_id.as_deref(), Some("hole"));
    assert_eq!(store.claim("dam")?.basis.state, "current");

    store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    assert_eq!(store.claim("dam")?.status, "corroborated");
    assert_eq!(
        store.claim("dam")?.basis.reasons,
        vec![ClaimBasisReason::Correction]
    );
    assert_eq!(
        store.claim("gap")?.basis.reasons,
        vec![ClaimBasisReason::Correction, ClaimBasisReason::MissingMedia]
    );

    store.begin_delete("one", false)?;
    store.publish_finding("fair", "gone", &cite(FindingOriginal::Expired), 70)?;
    store.create_claim("old", "The recording was released.", 71)?;
    store.add_sighting("old", "see", "fair", "gone", 72)?;
    let expired = corroborate(&mut store, "old", "note", "see", 73)?;
    assert_eq!(
        expired.basis.reasons,
        vec![
            ClaimBasisReason::Correction,
            ClaimBasisReason::RetentionExpired
        ]
    );
    assert_eq!(
        store.claim("dam")?.basis.reasons,
        vec![ClaimBasisReason::Correction]
    );
    assert_eq!(
        store.claim("gap")?.basis.reasons,
        vec![ClaimBasisReason::Correction, ClaimBasisReason::MissingMedia]
    );
    assert_eq!(
        status_row(&store, "dam", 1)?,
        ("corroborated".to_owned(), "note".to_owned())
    );
    assert_eq!(
        status_row(&store, "gap", 1)?,
        ("corroborated".to_owned(), "note".to_owned())
    );
    assert_eq!(
        status_row(&store, "old", 1)?,
        ("corroborated".to_owned(), "note".to_owned())
    );
    Ok(())
}

fn params_evidence(index: i64) -> (String, i64) {
    (format!("e{index}"), index)
}

fn feria() -> Result<ArchiveQuery> {
    ArchiveQuery::new("feria").validate()
}

#[test]
fn a_search_hit_is_a_candidate_only_when_a_finding_cites_that_cue() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    let empty = store.claim_candidates(feria()?)?;
    assert_eq!(empty.candidates, []);
    assert!(empty.hits_read >= 1);
    assert_eq!(tally(&store)?, (0, 0, 0, 0, 0));

    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    let page = store.claim_candidates(feria()?)?;
    assert_eq!(page.candidates.len(), 1);
    let hit = &page.candidates[0];
    assert_eq!(hit.original, "Una feria mundial");
    assert_eq!(hit.english.as_deref(), Some("A world fair"));
    assert_eq!(hit.translation_revision, 1);
    assert!(!hit.stale_transcript);
    assert!(!hit.stale_translation);
    assert_eq!(hit.findings.len(), 1);
    assert_eq!(hit.findings[0].monitor_id, "fair");
    assert_eq!(hit.findings[0].finding_id, "world");
    assert_eq!(hit.findings[0].original_state, "retained");
    assert!(!hit.more_findings);
    assert_eq!(tally(&store)?, (0, 0, 0, 0, 0));
    for index in 0..8i64 {
        store.publish_finding(
            "fair",
            &format!("extra{index}"),
            &cite(FindingOriginal::Retained),
            41 + index,
        )?;
    }
    let capped = store.claim_candidates(feria()?)?;
    assert_eq!(capped.candidates.len(), 1);
    assert_eq!(capped.candidates[0].findings.len(), 8);
    assert!(capped.candidates[0].more_findings);
    assert_eq!(capped.candidates[0].findings[0].finding_id, "extra0");
    assert!(
        capped.candidates[0]
            .findings
            .iter()
            .all(|finding| finding.finding_id != "world")
    );

    let missed = store.claim_candidates(ArchiveQuery::new("zzzz").validate()?)?;
    assert_eq!(missed.hits_read, 0);
    assert_eq!(missed.candidates, []);
    let value = serde_json::to_value(&page)?;
    assert!(!contains_key(&value, "score"));
    Ok(())
}

#[test]
fn a_newer_translation_stays_off_the_current_candidate_page() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    translate_at(&mut store, "mt-2", 1, "A later fair", 50)?;
    let current = store.claim_candidates(feria()?)?;
    assert!(current.hits_read >= 1);
    assert_eq!(current.candidates, []);

    let mut history = feria()?;
    history.revisions = ArchiveRevisions::All;
    let same_term = store.claim_candidates(history)?;
    assert!(same_term.hits_read >= 1);
    assert_eq!(same_term.candidates, []);

    let mut older = ArchiveQuery::new("world").validate()?;
    older.revisions = ArchiveRevisions::All;
    let page = store.claim_candidates(older)?;
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(page.candidates[0].translation_revision, 1);
    assert!(page.candidates[0].stale_translation);
    assert_eq!(page.candidates[0].findings[0].finding_id, "world");
    assert_eq!(tally(&store)?, (0, 0, 0, 0, 0));
    Ok(())
}

fn passage<'a>(claim: &'a str, sighting: &'a str, revision: i64, cue: u32) -> PassageRequest<'a> {
    PassageRequest {
        claim,
        sighting,
        transcript_id: "pin",
        transcript_revision: revision,
        translation_revision: 1,
        cue_ordinal: cue,
    }
}

#[test]
fn a_retained_cue_links_without_a_finding() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.create_claim("dam", "The bulletin named the fair.", 40)?;
    assert!(matches!(
        store.add_passage(passage("dam", "see", 1, 9), 41),
        Err(Error::NotFound)
    ));
    assert_eq!(tally(&store)?.2, 0);
    let linked = store.add_passage(passage("dam", "see", 1, 0), 42)?;
    let sighting = &linked.sightings[0];
    assert!(sighting.monitor_id.is_none() && sighting.finding_id.is_none());
    assert_eq!(sighting.transcript_id.as_deref(), Some("pin"));
    assert_eq!(
        (
            sighting.transcript_revision,
            sighting.translation_revision,
            sighting.cue_ordinal
        ),
        (Some(1), Some(1), Some(0))
    );
    assert_eq!(sighting.original, FindingOriginal::Retained);
    assert_eq!(sighting.time_basis, "recording media clock");
    assert_eq!(
        (sighting.start_us, sighting.end_us),
        (Some(0), Some(1_000_000))
    );
    let findings: i64 =
        store
            .connection
            .query_row("SELECT count(*) FROM monitor_findings", [], |row| {
                row.get(0)
            })?;
    assert_eq!(findings, 0);
    let created: i64 = store.connection.query_row(
        "SELECT created_ms FROM claim_sightings WHERE claim_id = 'dam' AND id = 'see'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        store
            .add_passage(passage("dam", "see", 1, 0), 43)?
            .sightings
            .len(),
        1
    );
    let replayed: i64 = store.connection.query_row(
        "SELECT created_ms FROM claim_sightings WHERE claim_id = 'dam' AND id = 'see'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(replayed, created);
    assert!(matches!(
        store.add_passage(passage("dam", "see", 2, 0), 44),
        Err(Error::Analysis("claim-conflict"))
    ));
    assert!(matches!(
        store.add_sighting("dam", "see", "fair", "world", 45),
        Err(Error::Analysis("claim-conflict"))
    ));
    match store.connection.execute(
        "INSERT INTO claim_sightings(claim_id, id, monitor_id, finding_id, transcript_id, transcript_revision, translation_revision, cue_ordinal, recording_id, original_state, start_us, end_us, created_ms) VALUES ('dam', 'bad', NULL, NULL, 'pin', 1, 1, 0, 'one', 'retained', 0, 2, 46)",
        [],
    ) {
        Err(error) => assert!(error.to_string().contains("claim passage citation")),
        Ok(_) => return Err("a mismatched passage was stored".into()),
    }
    assert_eq!(tally(&store)?.2, 1);
    let value = serde_json::to_value(store.claim("dam")?)?;
    assert!(!contains_key(&value, "score"));
    assert!(!contains_key(&value, "anomaly"));
    assert!(!contains_key(&value, "monitor_id"));
    assert!(!contains_key(&value, "finding_id"));
    assert!(contains_key(&value, "transcript_id"));
    let anomaly: i64 = store.connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name LIKE '%anomaly%'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(anomaly, 0);
    Ok(())
}

#[test]
fn a_passage_keeps_user_status_and_its_frozen_retained_state() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    store.create_claim("dam", "The bulletin named the fair.", 40)?;
    store.add_passage(passage("dam", "see", 1, 0), 42)?;
    let corroborated = corroborate(&mut store, "dam", "note", "see", 45)?;
    assert_eq!(corroborated.status, "corroborated");
    assert_eq!(corroborated.basis.state, "current");
    assert!(corroborated.basis.finding_id.is_none());
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 50)?;
    store.create_claim("other", "A second wording.", 51)?;
    store.add_sighting("other", "see", "fair", "world", 52)?;
    assert!(matches!(
        store.add_passage(passage("other", "see", 1, 0), 53),
        Err(Error::Analysis("claim-conflict"))
    ));
    store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    let (job, work) = store.admit_translation(&super::request("mt-2")?, 70)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![super::translated(0, "A local fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 71)?;
    let stale = store.claim("dam")?;
    assert_eq!(stale.status, "corroborated");
    assert_eq!(stale.sightings[0].stale_transcript, Some(true));
    assert_eq!(stale.sightings[0].stale_translation, Some(true));
    assert_eq!(stale.basis.finding_id, None);
    assert_eq!(stale.basis.state, "review");
    assert_eq!(
        stale.basis.reasons,
        vec![ClaimBasisReason::Correction, ClaimBasisReason::Superseded]
    );
    assert_eq!(tally(&store)?.2, 2);
    store.begin_delete("one", false)?;
    let frozen: String = store.connection.query_row(
        "SELECT original_state FROM claim_sightings WHERE claim_id = 'dam' AND id = 'see'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(frozen, "retained");
    assert_eq!(
        store.claim("dam")?.sightings[0].time_basis,
        "recording media clock"
    );
    assert!(matches!(
        store.add_passage(passage("dam", "late", 1, 0), 80),
        Err(Error::InvalidInput("claim sighting; link a retained cue"))
    ));
    assert_eq!(tally(&store)?.2, 2);
    Ok(())
}

#[test]
fn a_v53_finding_sighting_keeps_its_evidence_and_status() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    store.connection.execute_batch(
        "DROP TABLE claim_status;
         DROP TABLE claim_evidence;
         DROP TABLE claim_sightings;
         DROP TABLE claim_revisions;
         DROP TABLE claims;",
    )?;
    store
        .connection
        .execute_batch(include_str!("../../../052-claims.sql"))?;
    store
        .connection
        .execute("INSERT INTO claims(id, created_ms) VALUES ('dam', 41)", [])?;
    store.connection.execute(
        "INSERT INTO claim_revisions(claim_id, revision, text, created_ms) VALUES ('dam', 1, 'The bulletin named the fair.', 41)",
        [],
    )?;
    store.connection.execute(
        "INSERT INTO claim_sightings(claim_id, id, monitor_id, finding_id, created_ms) VALUES ('dam', 'see', 'fair', 'world', 42)",
        [],
    )?;
    store.connection.execute(
        "INSERT INTO claim_evidence(claim_id, id, sighting_id, kind, origin, note, relation, created_ms) VALUES ('dam', 'note', 'see', 'supports', 'user', 'The user stored this relation.', 'unspecified', 45)",
        [],
    )?;
    store.connection.execute(
        "INSERT INTO claim_status(claim_id, revision, status, evidence_id, setter, created_ms) VALUES ('dam', 1, 'corroborated', 'note', 'user', 46)",
        [],
    )?;
    store.connection.pragma_update(None, "user_version", 53)?;
    drop(store);
    let store = Store::open(&path)?;
    let page = store.claim("dam")?;
    assert_eq!(page.text, "The bulletin named the fair.");
    assert_eq!(page.status, "corroborated");
    assert_eq!(page.evidence.len(), 1);
    assert_eq!(page.sightings[0].monitor_id.as_deref(), Some("fair"));
    assert_eq!(page.sightings[0].finding_id.as_deref(), Some("world"));
    assert!(page.sightings[0].transcript_id.is_none());
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
    Ok(())
}
