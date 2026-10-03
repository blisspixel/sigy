//! History needs exact citation facts, not allocated text or live staleness labels.

use super::{Error, FindingOriginal, OptionalExtension, Result, stored_text};
use rusqlite::{Statement, params};

// Retain both cue joins and the text type checks previously enforced by FindingPage
// construction. Full text content is checked by the selected observation audit.
pub(super) const SQL: &str = "SELECT f.transcript_id,f.transcript_revision,f.translation_revision,f.cue_ordinal,f.recording_id,f.original_state,f.start_us,f.end_us,f.created_ms FROM monitor_findings f JOIN transcript_cues c ON c.transcript_id=f.transcript_id AND c.revision=f.transcript_revision AND c.ordinal=f.cue_ordinal JOIN translation_cues tc ON tc.transcript_id=f.transcript_id AND tc.transcript_revision=f.transcript_revision AND tc.revision=f.translation_revision AND tc.ordinal=f.cue_ordinal WHERE f.monitor_id=?1 AND f.id=?2 AND typeof(c.script)='text' AND typeof(tc.english) IN ('text','null') AND typeof(tc.reason) IN ('text','null')";

pub(super) struct Facts {
    pub transcript_id: String,
    pub transcript_revision: i64,
    pub translation_revision: i64,
    pub cue_ordinal: u32,
    pub recording_id: String,
    pub original: FindingOriginal,
    pub start_us: Option<u64>,
    pub end_us: Option<u64>,
    pub created_ms: i64,
}

pub(super) fn read(statement: &mut Statement<'_>, monitor: &str, id: &str) -> Result<Facts> {
    let (
        transcript_id,
        transcript_revision,
        translation_revision,
        cue_ordinal,
        recording_id,
        original,
        start,
        end,
        created_ms,
    ) = statement
        .query_row(params![monitor, id], |row| {
            Ok((
                stored_text(row, 0, 128)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, u32>(3)?,
                stored_text(row, 4, 128)?,
                stored_text(row, 5, 16)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })
        .optional()?
        .ok_or(Error::NotFound)?;
    let original = match original.as_str() {
        "retained" => FindingOriginal::Retained,
        "expired" => FindingOriginal::Expired,
        "missing" => FindingOriginal::Missing,
        _ => return Err(Error::StorageIntegrity),
    };
    let clock = |value: i64| u64::try_from(value).map_err(|_| Error::StorageIntegrity);
    Ok(Facts {
        transcript_id,
        transcript_revision,
        translation_revision,
        cue_ordinal,
        recording_id,
        original,
        start_us: start.map(clock).transpose()?,
        end_us: end.map(clock).transpose()?,
        created_ms,
    })
}
