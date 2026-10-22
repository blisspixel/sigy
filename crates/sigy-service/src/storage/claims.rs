//! Manual claim storage. Only these functions insert a claim row.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::{Store, validate_key};
use crate::{
    Error, Result,
    claim::{
        CLAIM_LIMIT, ClaimCarrier, ClaimEvidenceKind, ClaimEvidenceLink, ClaimRelation,
        ClaimSighting, ClaimStatus, ClaimTextRevision, ClaimTimeline, ClaimView, EVIDENCE_LIMIT,
        NOTE_CHARS, SIGHTING_LIMIT, STATUS_LIMIT, TEXT_CHARS, TEXT_REVISION_LIMIT,
    },
    monitor::FindingOriginal,
};

const ORIGIN: &str = "user";
const UNSPECIFIED: &str = "unspecified";
const UNKNOWN_SOURCE: &str = "unknown";

/// Fields of one evidence link. The origin is always the user.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EvidenceRequest<'a> {
    pub claim: &'a str,
    pub evidence: &'a str,
    pub sighting: &'a str,
    pub kind: ClaimEvidenceKind,
    pub note: &'a str,
    pub relation: Option<ClaimRelation>,
}

/// One user status change. It cites an evidence link that is already stored.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StatusRequest<'a> {
    pub claim: &'a str,
    pub expected_revision: i64,
    pub status: ClaimStatus,
    pub evidence: &'a str,
}

impl Store {
    /// Store a new claim, or return it when the same first text is repeated.
    ///
    /// # Errors
    /// A different text for an existing id conflicts and writes nothing.
    /// Empty text, control characters, and a full library are refused.
    pub(crate) fn create_claim(&mut self, id: &str, text: &str, now: i64) -> Result<ClaimView> {
        let text = bounded_text(text, TEXT_CHARS, "claim text")?;
        validate_key(id, "claim ID")?;
        clock(now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load_created(&tx, id)?.is_some() {
            let page = load(&tx, id)?;
            if page.revisions.len() == 1 && page.text == text {
                tx.commit()?;
                return Ok(page);
            }
            return Err(Error::Analysis("claim-conflict"));
        }
        ensure_room(
            &tx,
            "SELECT count(*) FROM claims",
            CLAIM_LIMIT,
            "claim-limit",
        )?;
        tx.execute(
            "INSERT INTO claims(id, created_ms) VALUES (?1, ?2)",
            params![id, now],
        )
        .map_err(claim_error)?;
        tx.execute(
            "INSERT INTO claim_revisions(claim_id, revision, text, created_ms) VALUES (?1, 1, ?2, ?3)",
            params![id, text, now],
        )
        .map_err(claim_error)?;
        commit_page(tx, id)
    }

    /// Append one text revision after the revision the user last saw.
    ///
    /// # Errors
    /// A missing claim is not found. The wrong expected revision conflicts and writes nothing.
    /// The same text changes nothing. Revision 65 is refused.
    pub(crate) fn revise_claim(
        &mut self,
        id: &str,
        expected_revision: i64,
        text: &str,
        now: i64,
    ) -> Result<ClaimView> {
        let text = bounded_text(text, TEXT_CHARS, "claim text")?;
        validate_key(id, "claim ID")?;
        clock(now)?;
        if !(1..=TEXT_REVISION_LIMIT).contains(&expected_revision) {
            return Err(Error::InvalidInput("claim revision"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(page) = load_optional(&tx, id)? else {
            return Err(Error::NotFound);
        };
        if page.text_revision != expected_revision {
            return Err(Error::Analysis("claim-conflict"));
        }
        if page.text == text {
            tx.commit()?;
            return Ok(page);
        }
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM claim_revisions WHERE claim_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        if count >= TEXT_REVISION_LIMIT {
            return Err(Error::Analysis("claim-revision-limit"));
        }
        let next = expected_revision + 1;
        tx.execute(
            "INSERT INTO claim_revisions(claim_id, revision, text, created_ms) VALUES (?1, ?2, ?3, ?4)",
            params![id, next, text, now],
        )
        .map_err(claim_error)?;
        commit_page(tx, id)
    }

    /// List claim ids in identifier order.
    ///
    /// # Errors
    /// Returns a catalog read error.
    pub(crate) fn claim_ids(&self) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM claims ORDER BY id")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Read one claim. A newer transcript or translation is stale and is not rewritten.
    ///
    /// # Errors
    /// Returns not found when that claim was never stored.
    pub(crate) fn claim(&self, id: &str) -> Result<ClaimView> {
        validate_key(id, "claim ID")?;
        load(&self.connection, id)
    }

    /// Link one stored finding. The same finding for the same sighting id writes nothing.
    ///
    /// # Errors
    /// A missing claim or finding writes nothing. A passage that is not a stored finding
    /// is refused. The same sighting id with a different finding conflicts.
    pub(crate) fn add_sighting(
        &mut self,
        claim: &str,
        sighting: &str,
        monitor: &str,
        finding: &str,
        now: i64,
    ) -> Result<ClaimView> {
        validate_key(claim, "claim ID")?;
        validate_key(sighting, "sighting ID")?;
        validate_key(monitor, "monitor ID")?;
        validate_key(finding, "finding ID")?;
        clock(now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load_created(&tx, claim)?.is_none() {
            return Err(Error::NotFound);
        }
        if let Some((stored_monitor, stored_finding)) = sighting_link(&tx, claim, sighting)? {
            if stored_monitor == monitor && stored_finding == finding {
                return commit_page(tx, claim);
            }
            return Err(Error::Analysis("claim-conflict"));
        }
        if !finding_exists(&tx, monitor, finding)? {
            return Err(Error::InvalidInput("claim sighting; link a stored finding"));
        }
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM claim_sightings WHERE claim_id = ?1",
            [claim],
            |row| row.get(0),
        )?;
        if count >= SIGHTING_LIMIT {
            return Err(Error::Analysis("claim-sighting-limit"));
        }
        tx.execute(
            "INSERT INTO claim_sightings(claim_id, id, monitor_id, finding_id, created_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![claim, sighting, monitor, finding, now],
        )
        .map_err(claim_error)?;
        commit_page(tx, claim)
    }

    /// Store one evidence link. The same id and fields write nothing.
    ///
    /// # Errors
    /// The sighting must already belong to this claim. A different body for the same id conflicts.
    pub(crate) fn add_evidence(
        &mut self,
        request: EvidenceRequest<'_>,
        now: i64,
    ) -> Result<ClaimView> {
        let note = bounded_text(request.note, NOTE_CHARS, "claim note")?;
        validate_key(request.claim, "claim ID")?;
        validate_key(request.evidence, "evidence ID")?;
        validate_key(request.sighting, "sighting ID")?;
        clock(now)?;
        let relation = request.relation.map_or(UNSPECIFIED, ClaimRelation::as_str);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load_created(&tx, request.claim)?.is_none() {
            return Err(Error::NotFound);
        }
        if let Some(stored) = evidence_row(&tx, request.claim, request.evidence)? {
            if stored.sighting_id == request.sighting
                && stored.kind == request.kind
                && stored.note == note
                && stored.relation == relation
                && stored.origin == ORIGIN
            {
                return commit_page(tx, request.claim);
            }
            return Err(Error::Analysis("claim-conflict"));
        }
        if !sighting_exists(&tx, request.claim, request.sighting)? {
            return Err(Error::InvalidInput("claim evidence sighting"));
        }
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM claim_evidence WHERE claim_id = ?1",
            [request.claim],
            |row| row.get(0),
        )?;
        if count >= EVIDENCE_LIMIT {
            return Err(Error::Analysis("claim-evidence-limit"));
        }
        tx.execute(
            "INSERT INTO claim_evidence(claim_id, id, sighting_id, kind, origin, note, relation, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                request.claim,
                request.evidence,
                request.sighting,
                request.kind.as_str(),
                ORIGIN,
                note,
                relation,
                now
            ],
        )
        .map_err(claim_error)?;
        commit_page(tx, request.claim)
    }

    /// Append a status the user set. Unverified is the absence of a row and is not written here.
    ///
    /// # Errors
    /// The expected revision is the current count of status rows. A mismatch conflicts.
    /// A missing evidence link writes nothing. No status is chosen by this function.
    pub(crate) fn set_claim_status(
        &mut self,
        request: StatusRequest<'_>,
        now: i64,
    ) -> Result<ClaimView> {
        validate_key(request.claim, "claim ID")?;
        validate_key(request.evidence, "evidence ID")?;
        clock(now)?;
        if !(0..=STATUS_LIMIT).contains(&request.expected_revision) {
            return Err(Error::InvalidInput("claim revision"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load_created(&tx, request.claim)?.is_none() {
            return Err(Error::NotFound);
        }
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM claim_status WHERE claim_id = ?1",
            [request.claim],
            |row| row.get(0),
        )?;
        if count != request.expected_revision {
            return Err(Error::Analysis("claim-conflict"));
        }
        if count >= STATUS_LIMIT {
            return Err(Error::Analysis("claim-status-limit"));
        }
        let evidence_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM claim_evidence WHERE claim_id = ?1 AND id = ?2)",
            params![request.claim, request.evidence],
            |row| row.get(0),
        )?;
        if !evidence_exists {
            return Err(Error::InvalidInput("claim status evidence"));
        }
        tx.execute(
            "INSERT INTO claim_status(claim_id, revision, status, evidence_id, setter, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                request.claim,
                count + 1,
                request.status.as_str(),
                request.evidence,
                ORIGIN,
                now
            ],
        )
        .map_err(claim_error)?;
        commit_page(tx, request.claim)
    }

    pub(crate) fn audit_claims(&self) -> Result<()> {
        let invalid: bool =
            self.connection
                .query_row(include_str!("claims_audit.sql"), [], |row| row.get(0))?;
        if invalid {
            return Err(Error::CatalogIntegrity);
        }
        Ok(())
    }
}

const fn clock(now: i64) -> Result<()> {
    if now < 0 {
        return Err(Error::InvalidInput("claim clock"));
    }
    Ok(())
}

fn bounded_text(value: &str, max_chars: usize, field: &'static str) -> Result<String> {
    let text = value.trim();
    if text.is_empty()
        || text.len() > max_chars.saturating_mul(4)
        || text.chars().count() > max_chars
        || text.chars().any(char::is_control)
    {
        return Err(Error::InvalidInput(field));
    }
    Ok(text.to_owned())
}

fn ensure_room(
    connection: &rusqlite::Connection,
    sql: &str,
    limit: i64,
    code: &'static str,
) -> Result<()> {
    let count: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    if count >= limit {
        return Err(Error::Analysis(code));
    }
    Ok(())
}

fn commit_page(tx: rusqlite::Transaction<'_>, id: &str) -> Result<ClaimView> {
    let page = load(&tx, id)?;
    tx.commit()?;
    Ok(page)
}

fn load_created(connection: &rusqlite::Connection, id: &str) -> Result<Option<i64>> {
    Ok(connection
        .query_row("SELECT created_ms FROM claims WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?)
}

fn load_optional(connection: &rusqlite::Connection, id: &str) -> Result<Option<ClaimView>> {
    if load_created(connection, id)?.is_none() {
        return Ok(None);
    }
    Ok(Some(load(connection, id)?))
}

fn finding_exists(connection: &rusqlite::Connection, monitor: &str, finding: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM monitor_findings WHERE monitor_id = ?1 AND id = ?2)",
        params![monitor, finding],
        |row| row.get(0),
    )?)
}

fn sighting_exists(connection: &rusqlite::Connection, claim: &str, sighting: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM claim_sightings WHERE claim_id = ?1 AND id = ?2)",
        params![claim, sighting],
        |row| row.get(0),
    )?)
}

fn sighting_link(
    connection: &rusqlite::Connection,
    claim: &str,
    sighting: &str,
) -> Result<Option<(String, String)>> {
    Ok(connection
        .query_row(
            "SELECT monitor_id, finding_id FROM claim_sightings WHERE claim_id = ?1 AND id = ?2",
            params![claim, sighting],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

struct StoredEvidence {
    sighting_id: String,
    kind: ClaimEvidenceKind,
    origin: String,
    note: String,
    relation: String,
}

fn evidence_row(
    connection: &rusqlite::Connection,
    claim: &str,
    evidence: &str,
) -> Result<Option<StoredEvidence>> {
    connection
        .query_row(
            "SELECT sighting_id, kind, origin, note, relation FROM claim_evidence WHERE claim_id = ?1 AND id = ?2",
            params![claim, evidence],
            |row| {
                let kind: String = row.get(1)?;
                Ok(StoredEvidence {
                    sighting_id: row.get(0)?,
                    kind: kind_from(&kind).ok_or(rusqlite::Error::InvalidQuery)?,
                    origin: row.get(2)?,
                    note: row.get(3)?,
                    relation: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(Error::from)
}

fn load(connection: &rusqlite::Connection, id: &str) -> Result<ClaimView> {
    let created_ms = load_created(connection, id)?.ok_or(Error::NotFound)?;
    let revisions = load_revisions(connection, id)?;
    let current = revisions.last().ok_or(Error::CatalogIntegrity)?;
    let sightings = load_sightings(connection, id)?;
    let evidence = load_evidence(connection, id)?;
    let current_status = load_status(connection, id)?;
    let timeline = ClaimTimeline {
        searched: ClaimTimeline::SEARCHED.to_owned(),
        library_search: false,
        languages: ClaimTimeline::LANGUAGES.to_owned(),
        window: window_of(&sightings),
        gaps: ClaimTimeline::GAPS.to_owned(),
    };
    let carriers = carriers_of(&sightings);
    Ok(ClaimView {
        id: id.to_owned(),
        created_ms,
        text_revision: current.revision,
        text: current.text.clone(),
        revisions,
        timeline,
        sightings,
        evidence,
        status: current_status.token,
        status_label: current_status.label,
        status_revision: current_status.revision,
        status_setter: current_status.setter,
        status_evidence: current_status.evidence,
        carriers,
    })
}

fn load_revisions(connection: &rusqlite::Connection, id: &str) -> Result<Vec<ClaimTextRevision>> {
    let mut statement = connection.prepare(
        "SELECT revision, text, created_ms FROM claim_revisions WHERE claim_id = ?1 ORDER BY revision",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok(ClaimTextRevision {
            revision: row.get(0)?,
            text: row.get(1)?,
            created_ms: row.get(2)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn load_sightings(connection: &rusqlite::Connection, id: &str) -> Result<Vec<ClaimSighting>> {
    let mut statement = connection.prepare(
        "SELECT s.id, s.monitor_id, s.finding_id, f.recording_id, f.original_state, f.start_us, f.end_us, j.source_revision, src.name, EXISTS (SELECT 1 FROM transcripts AS n WHERE n.id = f.transcript_id AND n.revision > f.transcript_revision AND n.kind IN ('recognition', 'correction') AND n.outcome = 'text'), EXISTS (SELECT 1 FROM translations AS tr WHERE tr.transcript_id = f.transcript_id AND tr.transcript_revision = f.transcript_revision AND tr.revision > f.translation_revision) FROM claim_sightings AS s JOIN monitor_findings AS f ON f.monitor_id = s.monitor_id AND f.id = s.finding_id LEFT JOIN capture_jobs AS j ON j.id = f.recording_id LEFT JOIN source_revisions AS src ON src.id = j.source_revision WHERE s.claim_id = ?1 ORDER BY s.created_ms, s.id",
    )?;
    let rows = statement.query_map([id], stored_sighting)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn stored_sighting(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaimSighting> {
    let original_state: String = row.get(4)?;
    let original = original_from(&original_state).ok_or(rusqlite::Error::InvalidQuery)?;
    let start: Option<i64> = row.get(5)?;
    let end: Option<i64> = row.get(6)?;
    let source: Option<String> = row.get(7)?;
    let stale_transcript: bool = row.get(9)?;
    let stale_translation: bool = row.get(10)?;
    Ok(ClaimSighting {
        id: row.get(0)?,
        monitor_id: row.get(1)?,
        finding_id: row.get(2)?,
        recording_id: row.get(3)?,
        source_revision: source.unwrap_or_else(|| UNKNOWN_SOURCE.to_owned()),
        source_name: row.get(8)?,
        original,
        start_us: micros(start).map_err(|_| rusqlite::Error::InvalidQuery)?,
        end_us: micros(end).map_err(|_| rusqlite::Error::InvalidQuery)?,
        time_basis: time_basis(original).to_owned(),
        stale_transcript: stale_transcript.then_some(true),
        stale_translation: stale_translation.then_some(true),
    })
}

fn load_evidence(connection: &rusqlite::Connection, id: &str) -> Result<Vec<ClaimEvidenceLink>> {
    let mut statement = connection.prepare(
        "SELECT id, sighting_id, kind, origin, note, relation FROM claim_evidence WHERE claim_id = ?1 ORDER BY created_ms, id",
    )?;
    let rows = statement.query_map([id], |row| {
        let kind: String = row.get(2)?;
        Ok(ClaimEvidenceLink {
            id: row.get(0)?,
            sighting_id: row.get(1)?,
            kind: kind_from(&kind).ok_or(rusqlite::Error::InvalidQuery)?,
            origin: row.get(3)?,
            note: row.get(4)?,
            relation: row.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

struct CurrentStatus {
    token: String,
    label: String,
    revision: i64,
    setter: Option<String>,
    evidence: Option<String>,
}

fn load_status(connection: &rusqlite::Connection, id: &str) -> Result<CurrentStatus> {
    let mut statement = connection.prepare(
        "SELECT revision, status, evidence_id, setter FROM claim_status WHERE claim_id = ?1 ORDER BY revision",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let stored = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    let Some((_, status, evidence, setter)) = stored.last() else {
        return Ok(CurrentStatus {
            token: "unverified".to_owned(),
            label: "unverified".to_owned(),
            revision: 0,
            setter: None,
            evidence: None,
        });
    };
    let parsed = status_from(status).ok_or(Error::CatalogIntegrity)?;
    let revision = i64::try_from(stored.len()).map_err(|_| Error::CatalogIntegrity)?;
    Ok(CurrentStatus {
        token: parsed.as_str().to_owned(),
        label: parsed.label().to_owned(),
        revision,
        setter: Some(setter.clone()),
        evidence: Some(evidence.clone()),
    })
}

fn window_of(sightings: &[ClaimSighting]) -> String {
    let mut earliest = None;
    let mut latest = None;
    for sighting in sightings {
        if let (Some(start), Some(end)) = (sighting.start_us, sighting.end_us) {
            earliest = Some(earliest.map_or(start, |value: u64| value.min(start)));
            latest = Some(latest.map_or(end, |value: u64| value.max(end)));
        }
    }
    match (earliest, latest) {
        (Some(start), Some(end)) => {
            format!("cited retained intervals from {start} us to {end} us")
        }
        _ => "none".to_owned(),
    }
}

fn carriers_of(sightings: &[ClaimSighting]) -> Vec<ClaimCarrier> {
    let mut grouped: BTreeMap<&str, ClaimCarrier> = BTreeMap::new();
    for sighting in sightings {
        let carrier = grouped
            .entry(sighting.source_revision.as_str())
            .or_insert_with(|| ClaimCarrier {
                source_revision: sighting.source_revision.clone(),
                source_name: sighting.source_name.clone(),
                sighting_ids: Vec::new(),
                repetition_group: "unknown".to_owned(),
                independence: "unknown".to_owned(),
            });
        carrier.sighting_ids.push(sighting.id.clone());
    }
    grouped.into_values().collect()
}

const fn time_basis(original: FindingOriginal) -> &'static str {
    match original {
        FindingOriginal::Retained => "recording media clock",
        FindingOriginal::Expired | FindingOriginal::Missing => "no retained interval",
    }
}

fn micros(value: Option<i64>) -> Result<Option<u64>> {
    value
        .map(|raw| u64::try_from(raw).map_err(|_| Error::CatalogIntegrity))
        .transpose()
}

fn original_from(value: &str) -> Option<FindingOriginal> {
    match value {
        "retained" => Some(FindingOriginal::Retained),
        "expired" => Some(FindingOriginal::Expired),
        "missing" => Some(FindingOriginal::Missing),
        _ => None,
    }
}

fn kind_from(value: &str) -> Option<ClaimEvidenceKind> {
    match value {
        "supports" => Some(ClaimEvidenceKind::Supports),
        "disputes" => Some(ClaimEvidenceKind::Disputes),
        "context" => Some(ClaimEvidenceKind::Context),
        "unresolved" => Some(ClaimEvidenceKind::Unresolved),
        _ => None,
    }
}

fn status_from(value: &str) -> Option<ClaimStatus> {
    match value {
        "disputed" => Some(ClaimStatus::Disputed),
        "corroborated" => Some(ClaimStatus::Corroborated),
        "retracted" => Some(ClaimStatus::Retracted),
        _ => None,
    }
}

/// Removes claim tables that have no rows so a v51 fixture can replay this migration.
#[cfg(test)]
pub(crate) fn revert_052_for_tests(connection: &rusqlite::Connection) -> Result<()> {
    super::completion_hold::revert_053_for_tests(connection)?;
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='claims')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let used: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM claims)
            OR EXISTS(SELECT 1 FROM claim_revisions)
            OR EXISTS(SELECT 1 FROM claim_sightings)
            OR EXISTS(SELECT 1 FROM claim_evidence)
            OR EXISTS(SELECT 1 FROM claim_status)",
        [],
        |row| row.get(0),
    )?;
    if used {
        return Err(Error::RequestState);
    }
    connection.execute_batch(
        "DROP TABLE IF EXISTS claim_status;
         DROP TABLE IF EXISTS claim_evidence;
         DROP TABLE IF EXISTS claim_sightings;
         DROP TABLE IF EXISTS claim_revisions;
         DROP TABLE IF EXISTS claims;",
    )?;
    Ok(())
}

fn claim_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    if message.contains("claim revision limit") {
        Error::Analysis("claim-revision-limit")
    } else if message.contains("claim sighting limit") {
        Error::Analysis("claim-sighting-limit")
    } else if message.contains("claim evidence limit") {
        Error::Analysis("claim-evidence-limit")
    } else if message.contains("claim status limit") {
        Error::Analysis("claim-status-limit")
    } else if message.contains("claim limit") {
        Error::Analysis("claim-limit")
    } else if message.contains("claim revision order") || message.contains("claim status order") {
        Error::Analysis("claim-conflict")
    } else {
        Error::Database(error)
    }
}
