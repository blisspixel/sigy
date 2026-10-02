//! What the catalog says about one cue's audio. Stored findings and archive search share
//! this reading so a citation and a search hit cannot disagree about the same interval.
//! It reads metadata only: no file is opened or hash-verified, and nothing is protected.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CueMedia {
    /// Retained with the transcript's checksum, covered by one published interval, no
    /// overlapping gap, and the covering segment is not released.
    Retained,
    /// The recording is retained but the covering segment was released.
    Released,
    /// The recording is deleting or deleted.
    Expired,
    /// The cue lies outside every published interval or overlaps a gap.
    Missing,
    /// Covered, but the recording is not verified retained.
    Unavailable,
}

/// Classify the half-open media interval `[start_us, end_us)` of one recording.
/// # Errors
/// A transcript whose recording row is absent is a storage integrity failure.
pub(super) fn classify(
    connection: &Connection,
    recording_id: &str,
    media_sha256: &str,
    start_us: i64,
    end_us: i64,
) -> Result<CueMedia> {
    let (storage_state, sha_matches): (String, bool) = connection
        .query_row(
            "SELECT storage_state, sha256 IS NOT NULL AND sha256 = ?2 FROM recordings WHERE id = ?1",
            params![recording_id, media_sha256],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(Error::StorageIntegrity)?;
    if matches!(storage_state.as_str(), "deleting" | "deleted") {
        return Ok(CueMedia::Expired);
    }
    let released = flag(
        connection,
        "SELECT EXISTS(SELECT 1 FROM recording_intervals i JOIN recording_releases x ON x.recording_id = i.recording_id AND x.segment_ordinal = i.ordinal WHERE i.recording_id = ?1 AND i.decoded_start_us <= ?2 AND i.decoded_end_us >= ?3)",
        recording_id,
        start_us,
        end_us,
    )?;
    if released {
        return Ok(CueMedia::Released);
    }
    let covered = flag(
        connection,
        "SELECT EXISTS(SELECT 1 FROM recording_intervals WHERE recording_id = ?1 AND decoded_start_us <= ?2 AND decoded_end_us >= ?3)",
        recording_id,
        start_us,
        end_us,
    )?;
    let gapped = flag(
        connection,
        "SELECT EXISTS(SELECT 1 FROM recording_gaps WHERE recording_id = ?1 AND start_us < ?3 AND end_us > ?2)",
        recording_id,
        start_us,
        end_us,
    )?;
    Ok(
        if storage_state == "retained" && sha_matches && covered && !gapped {
            CueMedia::Retained
        } else if !covered || gapped {
            CueMedia::Missing
        } else {
            CueMedia::Unavailable
        },
    )
}

fn flag(connection: &Connection, sql: &str, recording: &str, start: i64, end: i64) -> Result<bool> {
    Ok(connection.query_row(sql, params![recording, start, end], |row| row.get(0))?)
}
