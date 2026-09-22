//! Immutable, bounded language evidence. Worker dispatch is not implemented by this storage boundary.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    domain::language::{EvidenceOrigin, MediaRange, Resolution},
    languages::{
        LANGUAGE_PAGE_SIZE, LanguageEvidence, LanguageMethod, LanguageSpan, MAX_EVIDENCE_BYTES,
        TranscriptReference,
    },
};

use super::{Store, analysis::AnalysisRecord, validate_key};

const MAX_CATALOG_EVIDENCE_BYTES: i64 = 64 * 1024 * 1024;
const MAX_EVIDENCE_TRACKS: i64 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguagePublication {
    Created,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageSummary {
    pub id: String,
    pub revision: u32,
    pub analysis_id: String,
    pub analysis_revision: i64,
    pub transcript: Option<TranscriptReference>,
    pub method: LanguageMethod,
    pub outcome: String,
    pub reason: Option<String>,
    pub span_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "view", rename_all = "snake_case", deny_unknown_fields)]
pub enum LanguagePage {
    List {
        records: Vec<LanguageSummary>,
        next_after: Option<String>,
    },
    Evidence {
        summary: Box<LanguageSummary>,
        spans: Vec<LanguageSpan>,
        next_after: Option<u32>,
    },
}

impl From<&LanguageEvidence> for LanguageSummary {
    fn from(evidence: &LanguageEvidence) -> Self {
        Self {
            id: evidence.id.clone(),
            revision: evidence.revision,
            analysis_id: evidence.analysis_id.clone(),
            analysis_revision: evidence.analysis_revision,
            transcript: evidence.transcript.clone(),
            method: evidence.method.clone(),
            outcome: evidence.outcome.clone(),
            reason: evidence.reason.clone(),
            span_count: evidence.spans.len(),
        }
    }
}

impl Store {
    /// Publish one validated evidence revision. This does not run a detector or infer from hints.
    /// # Errors
    /// Rejects invalid evidence, stale input, conflicting replay, or exhausted catalog capacity.
    pub fn publish_language_evidence(
        &mut self,
        evidence: LanguageEvidence,
        now: i64,
    ) -> Result<(LanguagePublication, LanguageEvidence)> {
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        let evidence = evidence.validate()?;
        if let Some(existing) = read_evidence(&self.connection, &evidence.id, evidence.revision)? {
            if existing != evidence {
                return Err(Error::InvalidInput("language evidence replay conflicts"));
            }
            self.validate_language_lineage(&existing)?;
            return Ok((LanguagePublication::Unchanged, existing));
        }
        if evidence.outcome == "succeeded" {
            self.published_analysis(&evidence.analysis_id, evidence.analysis_revision)?;
        }
        self.validate_language_lineage(&evidence)?;
        let payload = serde_json::to_string(&evidence)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_capacity(&transaction, &evidence.id, payload.len())?;
        let latest: Option<u32> = transaction.query_row(
            "SELECT max(revision) FROM language_evidence WHERE id = ?1",
            [&evidence.id],
            |row| row.get(0),
        )?;
        if latest.unwrap_or(0) + 1 != evidence.revision {
            return Err(Error::InvalidInput("language evidence revision conflicts"));
        }
        insert_evidence(&transaction, &evidence, &payload, now)?;
        transaction.commit()?;
        Ok((LanguagePublication::Created, evidence))
    }

    /// Read historical evidence even when its original recording has expired.
    /// # Errors
    /// Returns missing revision, invalid key, or catalog-integrity errors.
    pub fn language_evidence(&self, id: &str, revision: u32) -> Result<LanguageEvidence> {
        validate_key(id, "language evidence ID")?;
        let evidence = read_evidence(&self.connection, id, revision)?.ok_or(Error::NotFound)?;
        self.validate_language_lineage(&evidence)?;
        Ok(evidence)
    }

    pub(crate) fn language_evidence_page(
        &self,
        id: &str,
        revision: u32,
        after: Option<u32>,
    ) -> Result<LanguagePage> {
        let evidence = self.language_evidence(id, revision)?;
        let summary = LanguageSummary::from(&evidence);
        let mut spans: Vec<_> = evidence
            .spans
            .into_iter()
            .filter(|span| after.is_none_or(|ordinal| span.ordinal > ordinal))
            .take(LANGUAGE_PAGE_SIZE + 1)
            .collect();
        let next_after = if spans.len() > LANGUAGE_PAGE_SIZE {
            spans.truncate(LANGUAGE_PAGE_SIZE);
            spans.last().map(|span| span.ordinal)
        } else {
            None
        };
        Ok(LanguagePage::Evidence {
            summary: Box::new(summary),
            spans,
            next_after,
        })
    }

    pub(crate) fn language_evidence_list(
        &self,
        analysis_id: &str,
        revision: i64,
        after: Option<&str>,
    ) -> Result<LanguagePage> {
        self.analysis_revision(analysis_id, revision)?;
        if let Some(after) = after {
            validate_key(after, "language evidence cursor")?;
        }
        let mut statement = self.connection.prepare(
            "SELECT id, max(revision) FROM language_evidence WHERE analysis_id = ?1 AND analysis_revision = ?2 AND (?3 IS NULL OR id > ?3) GROUP BY id ORDER BY id LIMIT 17",
        )?;
        let rows = statement.query_map(params![analysis_id, revision, after], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (id, evidence_revision) = row?;
            records.push(LanguageSummary::from(
                &self.language_evidence(&id, evidence_revision)?,
            ));
        }
        let next_after = if records.len() > LANGUAGE_PAGE_SIZE {
            records.truncate(LANGUAGE_PAGE_SIZE);
            records.last().map(|record| record.id.clone())
        } else {
            None
        };
        Ok(LanguagePage::List {
            records,
            next_after,
        })
    }

    pub(crate) fn audit_language_evidence(&self) -> Result<()> {
        let invalid_history: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM language_evidence GROUP BY id HAVING min(revision) != 1 OR max(revision) != count(*) OR count(DISTINCT analysis_id) != 1)",
            [], |row| row.get(0),
        )?;
        if invalid_history {
            return Err(Error::CatalogIntegrity);
        }
        let mut statement = self
            .connection
            .prepare("SELECT id, revision FROM language_evidence ORDER BY id, revision")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?;
        for row in rows {
            let (id, revision) = row?;
            self.language_evidence(&id, revision)
                .map_err(|_| Error::CatalogIntegrity)?;
        }
        let (bytes, tracks): (i64, i64) = self.connection.query_row(
            "SELECT coalesce(sum(length(CAST(payload_json AS BLOB))), 0), count(DISTINCT id) FROM language_evidence", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if bytes > MAX_CATALOG_EVIDENCE_BYTES || tracks > MAX_EVIDENCE_TRACKS {
            return Err(Error::CatalogIntegrity);
        }
        Ok(())
    }

    fn validate_language_lineage(&self, evidence: &LanguageEvidence) -> Result<()> {
        let pin = self.analysis_revision(&evidence.analysis_id, evidence.analysis_revision)?;
        if pin.state != "published" {
            return Err(Error::InvalidInput("language input is not published"));
        }
        if let Some(transcript) = &evidence.transcript {
            let matches: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM transcripts WHERE id = ?1 AND revision = ?2 AND analysis_id = ?3 AND analysis_revision = ?4)",
                params![transcript.id, transcript.revision, evidence.analysis_id, evidence.analysis_revision], |row| row.get(0),
            )?;
            if !matches {
                return Err(Error::InvalidInput(
                    "language transcript does not match input",
                ));
            }
        }
        for span in &evidence.spans {
            validate_media_span(&pin, span)?;
            self.validate_language_cue(evidence, span)?;
        }
        Ok(())
    }

    fn validate_language_cue(
        &self,
        evidence: &LanguageEvidence,
        span: &LanguageSpan,
    ) -> Result<()> {
        let Some(ordinal) = span.cue_ordinal else {
            if evidence.method.origin == EvidenceOrigin::Text.as_str()
                || evidence.method.resolution == Resolution::Word.as_str()
            {
                return Err(Error::InvalidInput(
                    "language evidence requires a source cue",
                ));
            }
            return Ok(());
        };
        let transcript = evidence.transcript.as_ref().ok_or(Error::InvalidInput(
            "language cue needs a transcript revision",
        ))?;
        let cue: Option<(i64, i64, String)> = self.connection.query_row(
            "SELECT start_us, end_us, script FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 AND ordinal = ?3",
            params![transcript.id, transcript.revision, ordinal],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let (start, end, script) =
            cue.ok_or(Error::InvalidInput("language source cue is missing"))?;
        let cue_range = range(
            u64::try_from(start).map_err(|_| Error::StorageIntegrity)?,
            u64::try_from(end).map_err(|_| Error::StorageIntegrity)?,
        )?;
        let span_range = range(span.start_us, span.end_us)?;
        if script.is_empty()
            || !cue_range.contains(span_range)
            || (evidence.method.resolution == Resolution::Block.as_str() && cue_range != span_range)
        {
            return Err(Error::InvalidInput(
                "language evidence exceeds observed transcript cue",
            ));
        }
        Ok(())
    }
}

fn range(start: u64, end: u64) -> Result<MediaRange> {
    MediaRange::new(start, end).map_err(|error| Error::InvalidInput(error.0))
}

fn validate_media_span(pin: &AnalysisRecord, span: &LanguageSpan) -> Result<()> {
    let interval = pin
        .intervals
        .iter()
        .find(|interval| interval.ordinal == span.interval_ordinal)
        .ok_or(Error::InvalidInput("language media interval is missing"))?;
    let interval_range = range(interval.start_us, interval.end_us)?;
    let span_range = range(span.start_us, span.end_us)?;
    if !interval_range.contains(span_range) {
        return Err(Error::InvalidInput("language span exceeds published audio"));
    }
    for gap in &pin.gaps {
        if span_range.intersects(range(gap.start_us, gap.end_us)?) {
            return Err(Error::InvalidInput("language span intersects a gap"));
        }
    }
    Ok(())
}

fn read_evidence(
    connection: &Connection,
    id: &str,
    revision: u32,
) -> Result<Option<LanguageEvidence>> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload_json FROM language_evidence WHERE id = ?1 AND revision = ?2",
            params![id, revision],
            |row| row.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    if payload.len() > MAX_EVIDENCE_BYTES {
        return Err(Error::StorageIntegrity);
    }
    let stored: LanguageEvidence =
        serde_json::from_str(&payload).map_err(|_| Error::StorageIntegrity)?;
    let normalized = stored
        .clone()
        .validate()
        .map_err(|_| Error::StorageIntegrity)?;
    if stored != normalized || stored.id != id || stored.revision != revision {
        return Err(Error::StorageIntegrity);
    }
    Ok(Some(stored))
}

fn check_capacity(connection: &Connection, id: &str, additional_bytes: usize) -> Result<()> {
    let (bytes, tracks, exists): (i64, i64, bool) = connection.query_row(
        "SELECT coalesce(sum(length(CAST(payload_json AS BLOB))), 0), count(DISTINCT id), EXISTS(SELECT 1 FROM language_evidence WHERE id = ?1) FROM language_evidence",
        [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let additional = i64::try_from(additional_bytes)
        .map_err(|_| Error::InvalidInput("language evidence byte limit"))?;
    if bytes > MAX_CATALOG_EVIDENCE_BYTES - additional || (!exists && tracks >= MAX_EVIDENCE_TRACKS)
    {
        return Err(Error::InvalidInput(
            "language evidence catalog capacity reached",
        ));
    }
    Ok(())
}

fn insert_evidence(
    connection: &Connection,
    evidence: &LanguageEvidence,
    payload: &str,
    now: i64,
) -> Result<()> {
    connection.execute(
        "INSERT INTO language_evidence(id, revision, analysis_id, analysis_revision, transcript_id, transcript_revision, payload_json, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![evidence.id, evidence.revision, evidence.analysis_id, evidence.analysis_revision,
            evidence.transcript.as_ref().map(|value| &value.id), evidence.transcript.as_ref().map(|value| value.revision), payload, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
