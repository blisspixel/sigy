//! Borrow and size-check cue values before allocating literal comparison strings.

use crate::{
    Error, Result,
    monitor::{MonitorTerm, searches_english, term_matches},
    task::{TaskCitation, snapshot::TaskEvidenceSnapshot},
};
use rusqlite::{Connection, OptionalExtension, Row, params, types::ValueRef};

fn text<'a>(row: &'a Row<'_>, column: usize, nullable: bool) -> rusqlite::Result<Option<&'a str>> {
    match row.get_ref(column)? {
        ValueRef::Null if nullable => Ok(None),
        ValueRef::Text(bytes) if !bytes.is_empty() && bytes.len() <= 4096 => {
            std::str::from_utf8(bytes)
                .map(Some)
                .map_err(|_| rusqlite::Error::InvalidQuery)
        }
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

pub(super) fn validate(
    connection: &Connection,
    snapshot: &TaskEvidenceSnapshot,
    terms: &[MonitorTerm],
    citation: &TaskCitation,
) -> Result<()> {
    let valid = connection.query_row(
        "SELECT c.source_revision,c.starts_ms,cue.start_us,cue.end_us,cue.script,e.english FROM transcripts t JOIN capture_jobs c ON c.id=t.recording_id JOIN transcript_cues cue ON cue.transcript_id=t.id AND cue.revision=t.revision LEFT JOIN translation_cues e ON e.transcript_id=t.id AND e.transcript_revision=t.revision AND e.revision=?4 AND e.ordinal=cue.ordinal AND e.state='translated' WHERE t.id=?1 AND t.revision=?2 AND cue.ordinal=?3 AND t.recording_id=?5 AND t.kind='recognition' AND t.outcome='text'",
        params![citation.transcript_id,citation.transcript_revision,citation.cue_ordinal,citation.translation_revision,citation.recording_id],
        |row| {
            let script=text(row,4,false)?.ok_or(rusqlite::Error::InvalidQuery)?;
            let english=text(row,5,true)?;
            let source=row.get_ref(0)?.as_str()?;
            let capture:i64=row.get(1)?;
            let start:i64=row.get(2)?;
            let end:i64=row.get(3)?;
            Ok(source==citation.source && capture>=snapshot.scope.from_ms && capture<snapshot.scope.to_ms
                && u64::try_from(start).ok()==Some(citation.start_us) && u64::try_from(end).ok()==Some(citation.end_us) && end>start
                && terms.iter().any(|term|term_matches(&term.text,script)||(searches_english(&term.language)&&english.is_some_and(|value|term_matches(&term.text,value)))))
        },
    ).optional()?.unwrap_or(false);
    if !valid {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

/// A complete zero-match result must be checked against all exact immutable cues.
pub(super) fn validate_membership(
    connection: &Connection,
    snapshot: &TaskEvidenceSnapshot,
    terms: &[MonitorTerm],
) -> Result<()> {
    if snapshot.evidence.more {
        return Ok(());
    }
    let mut matched = 0_usize;
    let mut headers = 0_usize;
    let mut examined = 0_usize;
    for entry in &snapshot.evidence.entries {
        if entry.transcript_outcome.as_deref() != Some("text") {
            continue;
        }
        let job = snapshot
            .jobs
            .iter()
            .find(|j| j.ordinal == entry.ordinal && j.stage == "recognition")
            .ok_or(Error::StorageIntegrity)?;
        let mut statement=connection.prepare("SELECT t.id,c.ordinal,c.start_us,c.end_us,c.script,e.english FROM transcripts t JOIN transcript_cues c ON c.transcript_id=t.id AND c.revision=t.revision LEFT JOIN translation_cues e ON e.transcript_id=t.id AND e.transcript_revision=t.revision AND e.revision=?2 AND e.ordinal=c.ordinal AND e.state='translated' WHERE t.job_id=?1 AND t.revision=?3 ORDER BY c.ordinal")?;
        let mut rows = statement.query(params![
            job.job_id,
            entry.translation_revision,
            entry.transcript_revision
        ])?;
        let mut transcript_headers = 0_usize;
        while let Some(row) = rows.next()? {
            headers += 1;
            transcript_headers += 1;
            if headers > 512 || transcript_headers > 256 {
                return Err(Error::StorageIntegrity);
            }
            let script = text(row, 4, false)?.ok_or(Error::StorageIntegrity)?;
            let english = text(row, 5, true)?;
            examined = examined
                .checked_add(script.len() + english.map_or(0, str::len))
                .ok_or(Error::StorageIntegrity)?;
            if examined > 4 * 1024 * 1024 {
                return Err(Error::StorageIntegrity);
            }
            if !terms.iter().any(|term| {
                term_matches(&term.text, script)
                    || (searches_english(&term.language)
                        && english.is_some_and(|value| term_matches(&term.text, value)))
            }) {
                continue;
            }
            let citation = snapshot
                .evidence
                .citations
                .get(matched)
                .ok_or(Error::StorageIntegrity)?;
            let transcript = row
                .get_ref(0)?
                .as_str()
                .map_err(|_| Error::StorageIntegrity)?;
            let ordinal: u32 = row.get(1)?;
            let start: i64 = row.get(2)?;
            let end: i64 = row.get(3)?;
            if citation.transcript_id != transcript
                || citation.cue_ordinal != ordinal
                || u64::try_from(start).ok() != Some(citation.start_us)
                || u64::try_from(end).ok() != Some(citation.end_us)
                || citation.source != entry.source_revision
                || Some(&citation.recording_id) != entry.recording_id.as_ref()
                || Some(citation.transcript_revision) != entry.transcript_revision
                || citation.translation_revision != entry.translation_revision
            {
                return Err(Error::StorageIntegrity);
            }
            matched += 1;
        }
    }
    if matched != snapshot.evidence.citations.len() {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
