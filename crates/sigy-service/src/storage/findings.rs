//! One immutable finding citation per monitor. Text does not insert a row.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Store, validate_key};
use crate::{
    Error, Result,
    monitor::{FindingCite, FindingOriginal, FindingPage},
    task::TaskCitation,
};

const PAGE_SQL: &str = "SELECT f.monitor_id, f.id, f.transcript_id, f.transcript_revision, f.translation_revision, f.cue_ordinal, f.recording_id, f.original_state, f.start_us, f.end_us, c.script, tc.english, tc.reason, EXISTS (SELECT 1 FROM transcripts AS n WHERE n.id = f.transcript_id AND n.revision > f.transcript_revision AND n.kind IN ('recognition', 'correction') AND n.outcome = 'text'), EXISTS (SELECT 1 FROM translations AS tr WHERE tr.transcript_id = f.transcript_id AND tr.transcript_revision = f.transcript_revision AND tr.revision > f.translation_revision) FROM monitor_findings AS f JOIN transcript_cues AS c ON c.transcript_id = f.transcript_id AND c.revision = f.transcript_revision AND c.ordinal = f.cue_ordinal JOIN translation_cues AS tc ON tc.transcript_id = f.transcript_id AND tc.transcript_revision = f.transcript_revision AND tc.revision = f.translation_revision AND tc.ordinal = f.cue_ordinal WHERE f.monitor_id = ?1 AND f.id = ?2";

struct TranscriptRow {
    recording_id: String,
    media_sha256: String,
    kind: String,
    outcome: String,
    role: String,
}

struct CueSpan {
    start_us: i64,
    end_us: i64,
}

struct Facts {
    recording_id: String,
    start_us: i64,
    end_us: i64,
    storage_state: String,
    sha_matches: bool,
    covered: bool,
    gapped: bool,
    released_segment: Option<i64>,
}

enum Truth {
    Retained,
    Expired,
    Missing,
    Unavailable,
}

/// Publish one already scope-validated task citation inside the caller's transaction.
/// Replay preserves the original media statement after retention changes.
pub(super) fn write_task_finding(
    connection: &Connection,
    monitor: &str,
    id: &str,
    citation: &TaskCitation,
    now: i64,
) -> Result<FindingPage> {
    let translation_revision = citation
        .translation_revision
        .ok_or(Error::Analysis("task-translation-unavailable"))?;
    if citation.cue_ordinal > 255 {
        return Err(Error::Analysis("task-cue-unsupported"));
    }
    let mut cite = FindingCite {
        transcript_id: citation.transcript_id.clone(),
        transcript_revision: citation.transcript_revision,
        translation_revision,
        cue_ordinal: citation.cue_ordinal,
        original: FindingOriginal::Retained,
    };
    validate_finding(monitor, id, &cite, now)?;
    let facts = load_facts(connection, &cite)?;
    let source: String = connection.query_row(
        "SELECT source_revision FROM capture_jobs WHERE id = ?1",
        [&facts.recording_id],
        |row| row.get(0),
    )?;
    if facts.recording_id != citation.recording_id
        || source != citation.source
        || u64::try_from(facts.start_us).ok() != Some(citation.start_us)
        || u64::try_from(facts.end_us).ok() != Some(citation.end_us)
    {
        return Err(Error::Analysis("task-citation-conflict"));
    }
    if let Some(page) = load(connection, monitor, id)? {
        cite.original = page.original;
        return if same(&page, &cite) && page.recording_id == citation.recording_id {
            Ok(page)
        } else {
            Err(Error::Analysis("finding-conflict"))
        };
    }
    cite.original = match media_truth(&facts) {
        Truth::Retained => FindingOriginal::Retained,
        Truth::Expired => FindingOriginal::Expired,
        Truth::Missing => FindingOriginal::Missing,
        Truth::Unavailable => return Err(Error::Analysis("task-original-unavailable")),
    };
    write_finding(connection, monitor, id, &cite, now)
}

impl Store {
    /// Store one citation, or return the stored page when that same citation is repeated.
    /// # Errors
    /// A missing monitor, transcript, cue, or translation is not found. A retained citation
    /// whose interval is absent is `finding-range` and writes nothing. A false expired or
    /// missing statement is `finding-original`. The same id with a different citation conflicts.
    pub(crate) fn publish_finding(
        &mut self,
        monitor: &str,
        id: &str,
        cite: &FindingCite,
        now: i64,
    ) -> Result<FindingPage> {
        validate_finding(monitor, id, cite, now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let page = write_finding(&tx, monitor, id, cite, now)?;
        tx.commit()?;
        Ok(page)
    }

    /// Read one stored citation. A newer transcript or translation is stale, and the row
    /// is not rewritten.
    /// # Errors
    /// Returns not found when that finding was never stored.
    pub(crate) fn finding(&self, monitor: &str, id: &str) -> Result<FindingPage> {
        validate_key(monitor, "monitor ID")?;
        validate_key(id, "finding ID")?;
        load(&self.connection, monitor, id)?.ok_or(Error::NotFound)
    }

    pub(crate) fn audit_findings(&self) -> Result<()> {
        let invalid: bool =
            self.connection
                .query_row(include_str!("findings_audit.sql"), [], |row| row.get(0))?;
        if invalid {
            return Err(Error::CatalogIntegrity);
        }
        Ok(())
    }
}

fn validate_finding(monitor: &str, id: &str, cite: &FindingCite, now: i64) -> Result<()> {
    validate_key(monitor, "monitor ID")?;
    validate_key(id, "finding ID")?;
    validate_key(&cite.transcript_id, "transcript ID")?;
    if now < 0 {
        return Err(Error::InvalidInput("finding clock"));
    }
    if !(1..=64).contains(&cite.transcript_revision)
        || !(1..=64).contains(&cite.translation_revision)
    {
        return Err(Error::InvalidInput("finding revision"));
    }
    if cite.cue_ordinal > 255 {
        return Err(Error::InvalidInput("finding cue"));
    }
    Ok(())
}

fn write_finding(
    connection: &Connection,
    monitor: &str,
    id: &str,
    cite: &FindingCite,
    now: i64,
) -> Result<FindingPage> {
    if let Some(page) = load(connection, monitor, id)? {
        return if same(&page, cite) {
            Ok(page)
        } else {
            Err(Error::Analysis("finding-conflict"))
        };
    }
    if !monitor_exists(connection, monitor)? {
        return Err(Error::NotFound);
    }
    let facts = load_facts(connection, cite)?;
    accept(cite.original, media_truth(&facts))?;
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM monitor_findings WHERE monitor_id = ?1",
        [monitor],
        |row| row.get(0),
    )?;
    if count >= 1024 {
        return Err(Error::Analysis("finding-limit"));
    }
    insert(connection, monitor, id, cite, &facts, now)?;
    load(connection, monitor, id)?.ok_or(Error::StorageIntegrity)
}

fn same(page: &FindingPage, cite: &FindingCite) -> bool {
    page.transcript_id == cite.transcript_id
        && page.transcript_revision == cite.transcript_revision
        && page.translation_revision == cite.translation_revision
        && page.cue_ordinal == cite.cue_ordinal
        && page.original == cite.original
}

fn monitor_exists(connection: &Connection, id: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM monitors WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )?)
}

fn load_facts(connection: &Connection, cite: &FindingCite) -> Result<Facts> {
    let transcript = transcript(connection, cite)?;
    if !is_original_text(&transcript) {
        return Err(Error::Analysis("finding-range"));
    }
    let cue = cue(connection, cite)?;
    if !translation_exists(connection, cite)? || !translation_cue_exists(connection, cite)? {
        return Err(Error::NotFound);
    }
    let (storage_state, sha_matches) = recording(connection, &transcript)?;
    Ok(Facts {
        recording_id: transcript.recording_id.clone(),
        start_us: cue.start_us,
        end_us: cue.end_us,
        storage_state,
        sha_matches,
        covered: flag(
            connection,
            "SELECT EXISTS(SELECT 1 FROM recording_intervals WHERE recording_id = ?1 AND decoded_start_us <= ?2 AND decoded_end_us >= ?3)",
            &transcript.recording_id,
            cue.start_us,
            cue.end_us,
        )?,
        gapped: flag(
            connection,
            "SELECT EXISTS(SELECT 1 FROM recording_gaps WHERE recording_id = ?1 AND start_us < ?3 AND end_us > ?2)",
            &transcript.recording_id,
            cue.start_us,
            cue.end_us,
        )?,
        released_segment: connection
            .query_row(
                "SELECT i.ordinal FROM recording_intervals i JOIN recording_releases x ON x.recording_id = i.recording_id AND x.segment_ordinal = i.ordinal WHERE i.recording_id = ?1 AND i.decoded_start_us <= ?2 AND i.decoded_end_us >= ?3 ORDER BY i.ordinal LIMIT 1",
                params![transcript.recording_id, cue.start_us, cue.end_us],
                |row| row.get(0),
            )
            .optional()?,
    })
}

fn is_original_text(row: &TranscriptRow) -> bool {
    row.role == "original"
        && row.outcome == "text"
        && matches!(row.kind.as_str(), "recognition" | "correction")
}

fn transcript(connection: &Connection, cite: &FindingCite) -> Result<TranscriptRow> {
    connection
        .query_row(
            "SELECT recording_id, media_sha256, kind, outcome, role FROM transcripts WHERE id = ?1 AND revision = ?2",
            params![cite.transcript_id, cite.transcript_revision],
            |row| {
                Ok(TranscriptRow {
                    recording_id: row.get(0)?,
                    media_sha256: row.get(1)?,
                    kind: row.get(2)?,
                    outcome: row.get(3)?,
                    role: row.get(4)?,
                })
            },
        )
        .optional()?
        .ok_or(Error::NotFound)
}

fn cue(connection: &Connection, cite: &FindingCite) -> Result<CueSpan> {
    connection
        .query_row(
            "SELECT start_us, end_us FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 AND ordinal = ?3",
            params![cite.transcript_id, cite.transcript_revision, cite.cue_ordinal],
            |row| {
                Ok(CueSpan {
                    start_us: row.get(0)?,
                    end_us: row.get(1)?,
                })
            },
        )
        .optional()?
        .ok_or(Error::NotFound)
}

fn translation_exists(connection: &Connection, cite: &FindingCite) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM translations WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3)",
        params![cite.transcript_id, cite.transcript_revision, cite.translation_revision],
        |row| row.get(0),
    )?)
}

fn translation_cue_exists(connection: &Connection, cite: &FindingCite) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM translation_cues WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3 AND ordinal = ?4)",
        params![
            cite.transcript_id,
            cite.transcript_revision,
            cite.translation_revision,
            cite.cue_ordinal
        ],
        |row| row.get(0),
    )?)
}

fn recording(connection: &Connection, transcript: &TranscriptRow) -> Result<(String, bool)> {
    connection
        .query_row(
            "SELECT storage_state, sha256 IS NOT NULL AND sha256 = ?2 FROM recordings WHERE id = ?1",
            params![transcript.recording_id, transcript.media_sha256],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(Error::StorageIntegrity)
}

fn flag(connection: &Connection, sql: &str, recording: &str, start: i64, end: i64) -> Result<bool> {
    Ok(connection.query_row(sql, params![recording, start, end], |row| row.get(0))?)
}

fn media_truth(facts: &Facts) -> Truth {
    let expired = matches!(facts.storage_state.as_str(), "deleting" | "deleted");
    if expired || facts.released_segment.is_some() {
        return Truth::Expired;
    }
    if facts.storage_state == "retained" && facts.sha_matches && facts.covered && !facts.gapped {
        return Truth::Retained;
    }
    if !facts.covered || facts.gapped {
        return Truth::Missing;
    }
    Truth::Unavailable
}

fn accept(requested: FindingOriginal, truth: Truth) -> Result<()> {
    let accepted = matches!(
        (requested, truth),
        (FindingOriginal::Retained, Truth::Retained)
            | (FindingOriginal::Expired, Truth::Expired)
            | (FindingOriginal::Missing, Truth::Missing)
    );
    if accepted {
        return Ok(());
    }
    Err(Error::Analysis(if requested == FindingOriginal::Retained {
        "finding-range"
    } else {
        "finding-original"
    }))
}

fn insert(
    connection: &Connection,
    monitor: &str,
    id: &str,
    cite: &FindingCite,
    facts: &Facts,
    now: i64,
) -> Result<()> {
    let (start, end) = match cite.original {
        FindingOriginal::Retained => (Some(facts.start_us), Some(facts.end_us)),
        FindingOriginal::Expired | FindingOriginal::Missing => (None, None),
    };
    connection
        .execute(
            "INSERT INTO monitor_findings(monitor_id, id, transcript_id, transcript_revision, translation_revision, cue_ordinal, recording_id, original_state, start_us, end_us, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                monitor,
                id,
                cite.transcript_id,
                cite.transcript_revision,
                cite.translation_revision,
                cite.cue_ordinal,
                facts.recording_id,
                original_label(cite.original),
                start,
                end,
                now
            ],
        )
        .map_err(finding_error)?;
    Ok(())
}

fn original_label(original: FindingOriginal) -> &'static str {
    match original {
        FindingOriginal::Retained => "retained",
        FindingOriginal::Expired => "expired",
        FindingOriginal::Missing => "missing",
    }
}

fn finding_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    if message.contains("finding limit") {
        Error::Analysis("finding-limit")
    } else if message.contains("finding citation") {
        Error::StorageIntegrity
    } else {
        Error::Database(error)
    }
}

struct Stored {
    monitor_id: String,
    id: String,
    transcript_id: String,
    transcript_revision: i64,
    translation_revision: i64,
    cue_ordinal: u32,
    recording_id: String,
    original: String,
    start_us: Option<i64>,
    end_us: Option<i64>,
    original_script: String,
    english: Option<String>,
    untranslated_reason: Option<String>,
    stale_transcript: bool,
    stale_translation: bool,
}

fn load(connection: &Connection, monitor: &str, id: &str) -> Result<Option<FindingPage>> {
    let stored = connection
        .query_row(PAGE_SQL, params![monitor, id], stored_row)
        .optional()?;
    stored.map(page_from).transpose()
}

fn stored_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Stored> {
    Ok(Stored {
        monitor_id: row.get(0)?,
        id: row.get(1)?,
        transcript_id: row.get(2)?,
        transcript_revision: row.get(3)?,
        translation_revision: row.get(4)?,
        cue_ordinal: row.get(5)?,
        recording_id: row.get(6)?,
        original: row.get(7)?,
        start_us: row.get(8)?,
        end_us: row.get(9)?,
        original_script: row.get(10)?,
        english: row.get(11)?,
        untranslated_reason: row.get(12)?,
        stale_transcript: row.get(13)?,
        stale_translation: row.get(14)?,
    })
}

fn page_from(stored: Stored) -> Result<FindingPage> {
    Ok(FindingPage {
        monitor_id: stored.monitor_id,
        id: stored.id,
        transcript_id: stored.transcript_id,
        transcript_revision: stored.transcript_revision,
        translation_revision: stored.translation_revision,
        cue_ordinal: stored.cue_ordinal,
        recording_id: stored.recording_id,
        original: parse_original(&stored.original)?,
        start_us: clock(stored.start_us)?,
        end_us: clock(stored.end_us)?,
        original_script: stored.original_script,
        english: stored.english,
        untranslated_reason: stored.untranslated_reason,
        stale_transcript: stored.stale_transcript.then_some(true),
        stale_translation: stored.stale_translation.then_some(true),
    })
}

fn parse_original(state: &str) -> Result<FindingOriginal> {
    match state {
        "retained" => Ok(FindingOriginal::Retained),
        "expired" => Ok(FindingOriginal::Expired),
        "missing" => Ok(FindingOriginal::Missing),
        _ => Err(Error::StorageIntegrity),
    }
}

fn clock(value: Option<i64>) -> Result<Option<u64>> {
    value
        .map(|us| u64::try_from(us).map_err(|_| Error::StorageIntegrity))
        .transpose()
}
