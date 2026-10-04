//! Resolve immutable citation metadata inside the same transaction as protection.

use rusqlite::{Connection, params};

use super::super::{RetainedCitation, read};
use crate::{
    Error, Result,
    storage::cue_media::{self, CueMedia},
};

pub(super) struct Selection {
    pub recording_id: String,
    pub start_us: u64,
    pub end_us: u64,
    pub citation: RetainedCitation,
}

pub(super) fn resolve(connection: &Connection, monitor: &str, finding: &str) -> Result<Selection> {
    let mut query = connection.prepare("SELECT f.original_state,f.recording_id,f.start_us,f.end_us,f.transcript_id,f.transcript_revision,f.translation_revision,f.cue_ordinal,t.media_sha256,EXISTS(SELECT 1 FROM transcript_cues tc WHERE tc.transcript_id=f.transcript_id AND tc.revision=f.transcript_revision AND tc.ordinal=f.cue_ordinal AND tc.start_us=f.start_us AND tc.end_us=f.end_us AND t.recording_id=f.recording_id AND t.role='original' AND t.outcome='text' AND t.kind IN ('recognition','correction') AND EXISTS(SELECT 1 FROM translation_cues tr WHERE tr.transcript_id=f.transcript_id AND tr.transcript_revision=f.transcript_revision AND tr.revision=f.translation_revision AND tr.ordinal=f.cue_ordinal)) FROM monitor_findings f JOIN transcripts t ON t.id=f.transcript_id AND t.revision=f.transcript_revision WHERE f.monitor_id=?1 AND f.id=?2")?;
    let mut rows = query.query(params![monitor, finding])?;
    let row = rows.next()?.ok_or(Error::NotFound)?;
    if read::text(row, 0, 16)? != "retained" {
        return Err(Error::InvalidInput(
            "finding has no retained cited interval",
        ));
    }
    if !row.get::<_, bool>(9)? {
        return Err(Error::StorageIntegrity);
    }
    let selection = Selection {
        recording_id: read::text(row, 1, 128)?,
        start_us: read::number(row, 2)?,
        end_us: read::number(row, 3)?,
        citation: RetainedCitation {
            monitor_id: monitor.to_owned(),
            finding_id: finding.to_owned(),
            transcript_id: read::text(row, 4, 128)?,
            transcript_revision: row.get(5)?,
            translation_revision: row.get(6)?,
            cue_ordinal: u32::try_from(read::number(row, 7)?)
                .map_err(|_| Error::StorageIntegrity)?,
        },
    };
    let checksum = read::text(row, 8, 64)?;
    if selection.start_us >= selection.end_us {
        return Err(Error::StorageIntegrity);
    }
    if cue_media::classify(
        connection,
        &selection.recording_id,
        &checksum,
        read::signed(selection.start_us)?,
        read::signed(selection.end_us)?,
    )? != CueMedia::Retained
    {
        return Err(Error::InvalidInput("cited interval is no longer retained"));
    }
    Ok(selection)
}
