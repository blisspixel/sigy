//! Explicit directory clicks. The provider response URL is not catalog state.

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::{Store, now_ms, validate_key};
use crate::{
    Error, Result,
    discovery::{ClickRequest, validate_station_id},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickStatus {
    pub id: String,
    pub station_id: String,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub mirror_origin: Option<String>,
    pub acknowledged: bool,
    pub failure: Option<String>,
}

impl Store {
    /// Returns whether a new request should be sent. An existing id is not sent again.
    /// # Errors
    /// Rejects an unknown station, a conflicting replay, or admission limits.
    pub(crate) fn begin_click(&mut self, id: &str, request: &ClickRequest) -> Result<bool> {
        validate_key(id, "click ID")?;
        request.validate()?;
        self.station(&request.station_id)?;
        let json = serde_json::to_string(request)?;
        let now = now_ms()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT request_json FROM directory_clicks WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != json {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(false);
        }
        let (count, active, last): (u32, u32, Option<i64>) = tx.query_row(
            "SELECT count(*), coalesce(sum(state = 'running'), 0), max(started_ms) FROM directory_clicks",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if count >= 4096 || active != 0 {
            return Err(Error::InvalidInput("directory click capacity reached"));
        }
        if last.is_some_and(|last| now < last.saturating_add(2000)) {
            return Err(Error::InvalidInput(
                "directory click interval is at least two seconds",
            ));
        }
        tx.execute(
            "INSERT INTO directory_clicks(id, station_id, request_json, state, started_ms) VALUES (?1, ?2, ?3, 'running', ?4)",
            params![id, request.station_id, json, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn finish_click(&mut self, id: &str, mirror_origin: &str) -> Result<()> {
        validate_key(id, "click ID")?;
        crate::discovery::validate_text(mirror_origin, 2048)?;
        if mirror_origin.contains("/json/url/") || mirror_origin.contains("/json/vote/") {
            return Err(Error::StorageIntegrity);
        }
        let now = now_ms()?;
        if self.connection.execute(
            "UPDATE directory_clicks SET state = 'completed', completed_ms = ?2, mirror_origin = ?3, acknowledged = 1 WHERE id = ?1 AND state = 'running'",
            params![id, now, mirror_origin],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    pub(crate) fn fail_click(&mut self, id: &str, error: &Error) -> Result<()> {
        let detail = match error {
            Error::Acquisition(detail) | Error::InvalidInput(detail) => *detail,
            Error::DestinationDenied => "directory click destination is not authorized",
            _ => "directory click failed",
        };
        if self.connection.execute(
            "UPDATE directory_clicks SET state = 'failed', failure = ?2 WHERE id = ?1 AND state = 'running'",
            params![id, detail],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    pub(crate) fn recover_clicks(&mut self) -> Result<()> {
        self.connection.execute(
            "UPDATE directory_clicks SET state = 'interrupted', failure = 'service stopped before the click completed' WHERE state = 'running'",
            [],
        )?;
        Ok(())
    }

    /// # Errors
    /// Rejects an unknown click or a row that would expose a stream URL.
    pub fn directory_click(&self, id: &str) -> Result<ClickStatus> {
        validate_key(id, "click ID")?;
        let row = self
            .connection
            .query_row(
                "SELECT station_id, state, started_ms, completed_ms, mirror_origin, acknowledged, failure FROM directory_clicks WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        validate_station_id(&row.0)?;
        if let Some(origin) = &row.4 {
            crate::discovery::validate_text(origin, 2048)?;
        }
        if let Some(failure) = &row.6 {
            crate::discovery::validate_text(failure, 256)?;
        }
        Ok(ClickStatus {
            id: id.into(),
            station_id: row.0,
            state: row.1,
            started_ms: row.2,
            completed_ms: row.3,
            mirror_origin: row.4,
            acknowledged: row.5 == 1,
            failure: row.6,
        })
    }

    pub(crate) fn audit_clicks(&self) -> Result<()> {
        let invalid: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM directory_clicks WHERE (state = 'completed' AND acknowledged != 1) OR (state != 'completed' AND acknowledged != 0) OR (state = 'running' AND completed_ms IS NOT NULL))",
            [],
            |row| row.get(0),
        )?;
        if invalid {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }
}
