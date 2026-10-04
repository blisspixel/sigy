use rusqlite::{Connection, Row};

use super::{RetainedReadSpec, RetainedReadView};
use crate::{Error, Result};

mod borrowed;

const COLUMNS: &str = "id,generation,recording_id,source_revision,ordinal,object_key,sha256,format,bytes,timeline_start_us,timeline_end_us,file_seek_us,file_duration_us,spec_sha256,state,seek_us,admitted_ms,updated_ms,completion_reason,recovery_reason";

pub(super) fn find(connection: &Connection, id: &str) -> Result<Option<RetainedReadView>> {
    let mut query = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM retained_readers WHERE id=?1"
    ))?;
    let mut rows = query.query([id])?;
    rows.next()?.map(decode).transpose()
}

pub(super) fn list(connection: &Connection) -> Result<Vec<RetainedReadView>> {
    let mut query = connection.prepare(&format!("SELECT {COLUMNS} FROM retained_readers ORDER BY (state IN ('running','cancelling','recovery_held')) DESC,admitted_ms DESC,id LIMIT 16"))?;
    let mut rows = query.query([])?;
    let mut result = Vec::with_capacity(16);
    while let Some(row) = rows.next()? {
        result.push(decode(row)?);
    }
    Ok(result)
}

pub(super) fn audit_all(connection: &Connection) -> Result<()> {
    let columns = COLUMNS
        .split(',')
        .map(|column| format!("q.{column}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut query = connection.prepare(&format!("SELECT {columns},EXISTS(SELECT 1 FROM recording_intervals i JOIN capture_jobs c ON c.id=i.recording_id JOIN recordings r ON r.id=i.recording_id WHERE i.recording_id=q.recording_id AND i.ordinal=q.ordinal AND c.source_revision=q.source_revision AND i.object_key=q.object_key AND i.sha256=q.sha256 AND i.format=q.format AND i.byte_end-i.byte_start=q.bytes AND i.decoded_start_us=q.timeline_start_us AND i.decoded_end_us=q.timeline_end_us AND (q.state IN ('completed','failed') OR (r.storage_state IN ('reserved','retained') AND NOT EXISTS(SELECT 1 FROM recording_releases x WHERE x.recording_id=i.recording_id AND x.segment_ordinal=i.ordinal)))) FROM retained_readers q ORDER BY q.id LIMIT 4097"))?;
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
        if !row.get::<_, bool>(20)? {
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
    let s = &view.spec;
    let valid: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM recording_intervals i JOIN capture_jobs c ON c.id=i.recording_id JOIN recordings r ON r.id=i.recording_id WHERE i.recording_id=?1 AND i.ordinal=?2 AND c.source_revision=?3 AND i.object_key=?4 AND i.sha256=?5 AND i.format=?6 AND i.byte_end-i.byte_start=?7 AND i.decoded_start_us=?8 AND i.decoded_end_us=?9 AND (?10 OR (r.storage_state IN ('reserved','retained') AND NOT EXISTS(SELECT 1 FROM recording_releases x WHERE x.recording_id=i.recording_id AND x.segment_ordinal=i.ordinal))))",
        rusqlite::params![s.recording_id,s.ordinal,s.source_revision,s.object_key,s.sha256,s.format,signed(s.bytes)?,signed(s.timeline_start_us)?,signed(s.timeline_end_us)?,matches!(view.state.as_str(),"completed"|"failed")],
        |row| row.get(0),
    )?;
    if !valid {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
