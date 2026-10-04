//! Validate historical rows without allocating an owned transport view per row.

use rusqlite::{Row, types::ValueRef};
use sha2::Digest;

use crate::{Error, Result};

const HEX: &[u8; 16] = b"0123456789abcdef";

pub(super) struct Stored<'a> {
    // request, recording, source, object, checksum, format, spec hash, state
    pub texts: [&'a str; 8],
    // generation, ordinal, bytes, start, end, file seek, duration, caller seek
    pub numbers: [u64; 8],
    pub admitted_ms: i64,
    pub updated_ms: i64,
    pub completion_reason: Option<&'a str>,
    pub recovery_reason: Option<&'a str>,
}

pub(super) fn inspect<'a>(row: &'a Row<'_>) -> Result<Stored<'a>> {
    let value = Stored {
        texts: [
            text(row, 0, 128)?,
            text(row, 2, 128)?,
            text(row, 3, 128)?,
            text(row, 5, 32)?,
            text(row, 6, 64)?,
            text(row, 7, 6)?,
            text(row, 13, 64)?,
            text(row, 14, 16)?,
        ],
        numbers: [
            super::number(row, 1)?,
            super::number(row, 4)?,
            super::number(row, 8)?,
            super::number(row, 9)?,
            super::number(row, 10)?,
            super::number(row, 11)?,
            super::number(row, 12)?,
            super::number(row, 15)?,
        ],
        admitted_ms: row.get(16)?,
        updated_ms: row.get(17)?,
        completion_reason: optional(row, 18)?,
        recovery_reason: optional(row, 19)?,
    };
    validate(&value)?;
    Ok(value)
}

pub(super) fn text<'a>(row: &'a Row<'_>, index: usize, maximum: usize) -> Result<&'a str> {
    let ValueRef::Text(bytes) = row.get_ref(index)? else {
        return Err(Error::StorageIntegrity);
    };
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(Error::StorageIntegrity);
    }
    std::str::from_utf8(bytes).map_err(|_| Error::StorageIntegrity)
}

fn optional<'a>(row: &'a Row<'_>, index: usize) -> Result<Option<&'a str>> {
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        _ => Ok(Some(text(row, index, 128)?)),
    }
}

fn validate(value: &Stored<'_>) -> Result<()> {
    let [
        id,
        recording,
        source,
        object,
        checksum,
        format,
        spec_hash,
        state,
    ] = value.texts;
    let [
        generation,
        ordinal,
        bytes,
        start,
        end,
        seek,
        duration,
        caller_seek,
    ] = value.numbers;
    for key in [id, recording, source] {
        super::super::validate_key(key, "retained identity")
            .map_err(|_| Error::StorageIntegrity)?;
    }
    let hex = |text: &str, length| {
        text.len() == length
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if generation != 1
        || ordinal >= 1024
        || bytes == 0
        || bytes > super::super::MAX_RETAINED_READ_BYTES
        || !hex(object, 32)
        || !hex(checksum, 64)
        || !hex(spec_hash, 64)
        || !matches!(format, "wav" | "mp3" | "aac" | "flac" | "ogg" | "mpegts")
        || end.checked_sub(start) != Some(duration)
        || duration == 0
        || duration > super::super::RETAINED_READ_DEADLINE_SECONDS * 1_000_000
        || caller_seek.checked_sub(start) != Some(seek)
        || seek >= duration
        || value.admitted_ms < 0
        || value.updated_ms < value.admitted_ms
        || !matches!(
            state,
            "running" | "cancelling" | "recovery_held" | "completed" | "failed"
        )
        || matches!(state, "completed" | "failed") != value.completion_reason.is_some()
        || (state == "recovery_held" && value.recovery_reason.is_none())
    {
        return Err(Error::StorageIntegrity);
    }
    reasons(value)?;
    let digest = super::super::spec_digest(
        [id, recording, source, object, checksum, format],
        [generation, ordinal, bytes, start, end, seek, duration],
    )?
    .finalize();
    if !digest.iter().enumerate().all(|(index, byte)| {
        spec_hash.as_bytes()[index * 2] == HEX[usize::from(byte >> 4)]
            && spec_hash.as_bytes()[index * 2 + 1] == HEX[usize::from(byte & 15)]
    }) {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

fn reasons(value: &Stored<'_>) -> Result<()> {
    if let Some(reason) = value.recovery_reason {
        super::super::validate_key(reason, "retained recovery reason")
            .map_err(|_| Error::StorageIntegrity)?;
    }
    if let Some(reason) = value.completion_reason
        && ((value.texts[7] == "completed" && reason != "completed")
            || (value.texts[7] == "failed"
                && !matches!(
                    reason,
                    "cancelled"
                        | "deadline"
                        | "input-unavailable"
                        | "input-size-mismatch"
                        | "input-checksum-mismatch"
                        | "retained-pipe-failed"
                        | "retained-read-failed"
                        | "retained-read-spec-invalid"
                )))
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
