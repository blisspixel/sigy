//! Exact read-only refusal reasons in the same admission snapshot.

use rusqlite::{Connection, params, types::ValueRef};

use super::read;
use crate::{Error, Result, storage::dvr::GapCause};

pub(super) fn explain(connection: &Connection, recording_id: &str, seek: i64) -> Result<Error> {
    let mut query=connection.prepare("SELECT r.storage_state,c.state,r.open_ceiling,r.open_object_key,(SELECT i.decoded_end_us FROM recording_intervals i WHERE i.recording_id=r.id ORDER BY i.ordinal DESC LIMIT 1) FROM recordings r JOIN capture_jobs c ON c.id=r.id WHERE r.id=?1")?;
    let mut rows = query.query([recording_id])?;
    let Some(row) = rows.next()? else {
        return Ok(Error::NotFound);
    };
    let storage = read::text(row, 0, 16)?;
    let capture = read::text(row, 1, 16)?;
    let ceiling = read::number(row, 2)?;
    let open_key = match row.get_ref(3)? {
        ValueRef::Null => None,
        _ => Some(read::text(row, 3, 32)?),
    };
    if let Some(key) = &open_key {
        crate::storage::dvr::validate_object_key(key)?;
    }
    let live_end = match row.get_ref(4)? {
        ValueRef::Null => None,
        _ => Some(read::number(row, 4)?),
    };
    if !matches!(storage.as_str(), "reserved" | "retained") {
        return Ok(Error::InvalidInput(
            "recording has no verified retained media",
        ));
    }
    if let Some(cause) = gap(connection, recording_id, seek)? {
        return Ok(Error::InvalidInput(cause.seek_denial()));
    }
    let Some(live_end) = live_end else {
        return Ok(Error::InvalidInput(
            "recording has no verified retained media",
        ));
    };
    if capture == "running"
        && ceiling > 0
        && open_key.is_some()
        && u64::try_from(seek).map_err(|_| Error::StorageIntegrity)? >= live_end
    {
        return Ok(Error::InvalidInput("open tail is not readable"));
    }
    Ok(Error::InvalidInput("seek is outside the retained audio"))
}

fn gap(connection: &Connection, recording_id: &str, seek: i64) -> Result<Option<GapCause>> {
    let mut query=connection.prepare("SELECT cause FROM recording_gaps WHERE recording_id=?1 AND start_us<=?2 AND end_us>?2 ORDER BY ordinal LIMIT 2")?;
    let mut rows = query.query(params![recording_id, seek])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let cause = GapCause::parse(&read::text(row, 0, 32)?)?;
    if rows.next()?.is_some() {
        return Err(Error::StorageIntegrity);
    }
    Ok(Some(cause))
}
