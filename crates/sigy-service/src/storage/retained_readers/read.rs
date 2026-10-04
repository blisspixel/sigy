use rusqlite::{Connection, Row};

use super::{RetainedCitation, RetainedExcerpt, RetainedReadSpec, RetainedReadView};
use crate::{Error, Result};

mod borrowed;

const COLUMNS: &str = "q.id,q.generation,q.recording_id,q.source_revision,q.ordinal,q.object_key,q.sha256,q.format,q.bytes,q.timeline_start_us,q.timeline_end_us,q.file_seek_us,q.file_duration_us,q.spec_sha256,q.state,q.seek_us,q.admitted_ms,q.updated_ms,q.completion_reason,q.recovery_reason,q.excerpt_version,q.excerpt_end_us,q.citation_monitor,q.citation_finding,q.citation_transcript,q.citation_transcript_revision,q.citation_translation_revision,q.citation_cue_ordinal";

const IDENTITY_VALID: &str = "EXISTS(SELECT 1 FROM recording_intervals i JOIN capture_jobs c ON c.id=i.recording_id JOIN recordings r ON r.id=i.recording_id WHERE i.recording_id=q.recording_id AND i.ordinal=q.ordinal AND c.source_revision=q.source_revision AND i.object_key=q.object_key AND i.sha256=q.sha256 AND i.format=q.format AND i.byte_end-i.byte_start=q.bytes AND i.decoded_start_us=q.timeline_start_us AND i.decoded_end_us=q.timeline_end_us AND (q.state IN ('completed','failed') OR (r.storage_state IN ('reserved','retained') AND NOT EXISTS(SELECT 1 FROM recording_releases x WHERE x.recording_id=i.recording_id AND x.segment_ordinal=i.ordinal))) AND (q.excerpt_version IS NULL OR NOT EXISTS(SELECT 1 FROM recording_gaps g WHERE g.recording_id=q.recording_id AND g.start_us<q.excerpt_end_us AND g.end_us>q.seek_us)) AND (q.citation_monitor IS NULL OR EXISTS(SELECT 1 FROM monitor_findings f JOIN transcripts t ON t.id=f.transcript_id AND t.revision=f.transcript_revision JOIN transcript_cues tc ON tc.transcript_id=t.id AND tc.revision=t.revision AND tc.ordinal=f.cue_ordinal WHERE f.monitor_id=q.citation_monitor AND f.id=q.citation_finding AND f.transcript_id=q.citation_transcript AND f.transcript_revision=q.citation_transcript_revision AND f.translation_revision=q.citation_translation_revision AND f.cue_ordinal=q.citation_cue_ordinal AND f.recording_id=q.recording_id AND f.original_state='retained' AND f.start_us=q.seek_us AND f.end_us=q.excerpt_end_us AND tc.start_us=f.start_us AND tc.end_us=f.end_us AND t.recording_id=q.recording_id AND t.role='original' AND t.outcome='text' AND t.kind IN ('recognition','correction') AND t.media_sha256=r.sha256 AND (q.state IN ('completed','failed') OR r.storage_state='retained') AND EXISTS(SELECT 1 FROM translation_cues tr WHERE tr.transcript_id=f.transcript_id AND tr.transcript_revision=f.transcript_revision AND tr.revision=f.translation_revision AND tr.ordinal=f.cue_ordinal))))";

pub(super) fn find(connection: &Connection, id: &str) -> Result<Option<RetainedReadView>> {
    let mut query = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM retained_readers q WHERE q.id=?1"
    ))?;
    let mut rows = query.query([id])?;
    rows.next()?.map(decode).transpose()
}

pub(super) fn list(connection: &Connection) -> Result<Vec<RetainedReadView>> {
    let mut query = connection.prepare(&format!("SELECT {COLUMNS} FROM retained_readers q ORDER BY (q.state IN ('running','cancelling','recovery_held')) DESC,q.admitted_ms DESC,q.id LIMIT 16"))?;
    let mut rows = query.query([])?;
    let mut result = Vec::with_capacity(16);
    while let Some(row) = rows.next()? {
        result.push(decode(row)?);
    }
    Ok(result)
}

pub(super) fn audit_all(connection: &Connection) -> Result<()> {
    let mut query = connection.prepare(&format!(
        "SELECT {COLUMNS},{IDENTITY_VALID} FROM retained_readers q ORDER BY q.id LIMIT 4097"
    ))?;
    let mut rows = query.query([])?;
    let mut total = 0;
    let mut active = 0;
    while let Some(row) = rows.next()? {
        total += 1;
        if total > 4096 {
            return Err(Error::StorageIntegrity);
        }
        let stored = borrowed::inspect(row)?;
        if !matches!(stored.texts[7], "completed" | "failed") {
            active += 1;
        }
        if active > 4 {
            return Err(Error::StorageIntegrity);
        }
        if !row.get::<_, bool>(28)? {
            return Err(Error::StorageIntegrity);
        }
    }
    Ok(())
}

pub(super) fn text(row: &Row<'_>, index: usize, maximum: usize) -> Result<String> {
    Ok(borrowed::text(row, index, maximum)?.to_owned())
}

pub(super) fn number(row: &Row<'_>, index: usize) -> Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|_| Error::StorageIntegrity)
}

pub(super) fn signed(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::StorageIntegrity)
}

fn decode(row: &Row<'_>) -> Result<RetainedReadView> {
    let stored = borrowed::inspect(row)?;
    let [
        request,
        recording,
        source,
        object,
        checksum,
        format,
        spec_hash,
        state,
    ] = stored.texts;
    let [
        generation,
        ordinal,
        bytes,
        start,
        end,
        seek,
        duration,
        caller_seek,
    ] = stored.numbers;
    Ok(RetainedReadView {
        spec: RetainedReadSpec {
            request_id: request.to_owned(),
            generation,
            recording_id: recording.to_owned(),
            source_revision: source.to_owned(),
            ordinal: u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?,
            object_key: object.to_owned(),
            sha256: checksum.to_owned(),
            format: format.to_owned(),
            bytes,
            timeline_start_us: start,
            timeline_end_us: end,
            file_seek_us: seek,
            file_duration_us: duration,
            spec_sha256: spec_hash.to_owned(),
            excerpt: stored.excerpt.map(|excerpt| RetainedExcerpt {
                version: excerpt.version,
                timeline_end_us: excerpt.timeline_end_us,
                citation: excerpt.citation.map(|citation| RetainedCitation {
                    monitor_id: citation.monitor_id.to_owned(),
                    finding_id: citation.finding_id.to_owned(),
                    transcript_id: citation.transcript_id.to_owned(),
                    transcript_revision: citation.transcript_revision,
                    translation_revision: citation.translation_revision,
                    cue_ordinal: citation.cue_ordinal,
                }),
            }),
        },
        state: state.to_owned(),
        seek_us: caller_seek,
        admitted_ms: stored.admitted_ms,
        updated_ms: stored.updated_ms,
        completion_reason: stored.completion_reason.map(str::to_owned),
        recovery_reason: stored.recovery_reason.map(str::to_owned),
    })
}
pub(super) fn audit(connection: &Connection, view: &RetainedReadView) -> Result<()> {
    let valid: bool = connection.query_row(
        &format!("SELECT {IDENTITY_VALID} FROM retained_readers q WHERE q.id=?1"),
        [&view.spec.request_id],
        |row| row.get(0),
    )?;
    if !valid {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
