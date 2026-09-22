//! Bounded directory cache and refresh intent owned by the existing catalog actor.

#[cfg(test)]
mod tests;

use super::{
    Store, now_ms,
    sources::{SourceAdmission, register_source_in},
    validate_key,
};
use crate::{
    Error, Result,
    discovery::{
        MAX_CACHED_STATIONS, RefreshBatch, RefreshRequest, Station, StationFilter,
        validate_station_id,
    },
    sources::{HttpSource, NetworkScope, RedirectPolicy},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

pub const DIRECTORY_FRESH_MS: i64 = 24 * 60 * 60 * 1000;

#[cfg(test)]
impl Store {
    pub(crate) fn set_cached_observation_age(&mut self, observed_ms: i64) -> Result<()> {
        self.connection.execute(
            "UPDATE directory_stations SET metadata_json = json_set(metadata_json, '$.observed_ms', ?1)",
            [observed_ms],
        )?;
        Ok(())
    }

    pub(crate) fn set_decoder_path(&mut self, path: &str) -> Result<()> {
        self.connection
            .execute("UPDATE dvr_policy SET decoder = ?1", [path])?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshStatus {
    pub id: String,
    pub request: RefreshRequest,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub accepted: u32,
    pub skipped: u32,
    pub mirror_origin: Option<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryStatus {
    pub cached_stations: u32,
    pub favorite_stations: u32,
    pub maximum_stations: u32,
    pub oldest_observed_ms: Option<i64>,
    pub newest_observed_ms: Option<i64>,
    pub stale_stations: u32,
    pub latest_refresh: Option<RefreshStatus>,
}

impl Store {
    pub(crate) fn begin_refresh(&mut self, id: &str, request: &RefreshRequest) -> Result<bool> {
        self.begin_refresh_at(id, request, now_ms()?)
    }

    pub(crate) fn begin_refresh_at(
        &mut self,
        id: &str,
        request: &RefreshRequest,
        now: i64,
    ) -> Result<bool> {
        validate_key(id, "refresh ID")?;
        request.validate()?;
        let json = serde_json::to_string(request)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT request_json FROM directory_refreshes WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != json {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(false);
        }
        let (count, active, last): (u32, u32, Option<i64>) = tx.query_row("SELECT count(*), coalesce(sum(state = 'running'), 0), max(started_ms) FROM directory_refreshes", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        if count >= 4096 || active != 0 {
            return Err(Error::InvalidInput("directory refresh capacity reached"));
        }
        if last.is_some_and(|last| now < last.saturating_add(2000)) {
            return Err(Error::InvalidInput(
                "directory refresh interval is at least two seconds",
            ));
        }
        tx.execute("INSERT INTO directory_refreshes(id, request_json, state, started_ms) VALUES (?1, ?2, 'running', ?3)", params![id, json, now])?;
        tx.commit()?;
        Ok(true)
    }

    /// # Errors
    /// Rejects invalid identifiers or corrupt refresh metadata.
    pub fn directory_refresh(&self, id: &str) -> Result<RefreshStatus> {
        validate_key(id, "refresh ID")?;
        let row = self.connection.query_row("SELECT request_json, state, started_ms, completed_ms, accepted, skipped, mirror_origin, failure FROM directory_refreshes WHERE id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))).optional()?.ok_or(Error::NotFound)?;
        let status = RefreshStatus {
            id: id.into(),
            request: serde_json::from_str(&row.0)?,
            state: row.1,
            started_ms: row.2,
            completed_ms: row.3,
            accepted: row.4,
            skipped: row.5,
            mirror_origin: row.6,
            failure: row.7,
        };
        status.request.validate()?;
        if let Some(origin) = &status.mirror_origin {
            crate::discovery::validate_text(origin, 2048)?;
        }
        if let Some(failure) = &status.failure {
            crate::discovery::validate_text(failure, 256)?;
        }
        Ok(status)
    }

    /// # Errors
    /// Returns catalog read or validation errors.
    pub fn directory_status(&self) -> Result<DirectoryStatus> {
        let cached_stations =
            self.connection
                .query_row("SELECT count(*) FROM directory_stations", [], |r| r.get(0))?;
        let cutoff = now_ms()?.saturating_sub(DIRECTORY_FRESH_MS);
        let (oldest_observed_ms, newest_observed_ms, stale_stations): (Option<i64>, Option<i64>, u32) = self.connection.query_row(
            "SELECT min(json_extract(metadata_json, '$.observed_ms')), max(json_extract(metadata_json, '$.observed_ms')), coalesce(sum(json_extract(metadata_json, '$.observed_ms') < ?1), 0) FROM directory_stations",
            [cutoff],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM directory_refreshes ORDER BY started_ms DESC, id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(DirectoryStatus {
            cached_stations,
            favorite_stations: self.connection.query_row(
                "SELECT count(*) FROM station_favorites",
                [],
                |r| r.get(0),
            )?,
            maximum_stations: MAX_CACHED_STATIONS,
            oldest_observed_ms,
            newest_observed_ms,
            stale_stations,
            latest_refresh: id.map(|id| self.directory_refresh(&id)).transpose()?,
        })
    }

    pub(crate) fn finish_refresh(&mut self, id: &str, mut batch: RefreshBatch) -> Result<()> {
        let status = self.directory_refresh(id)?;
        let now = now_ms()?;
        if status.state != "running"
            || batch.candidates.len() as u64 + u64::from(batch.skipped)
                > u64::from(status.request.limit)
        {
            return Err(Error::RequestState);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute("UPDATE directory_refreshes SET state = 'completed', completed_ms = ?2, accepted = ?3, skipped = ?4, mirror_origin = ?5 WHERE id = ?1 AND state = 'running'", params![id, now, i64::try_from(batch.candidates.len()).map_err(|_| Error::SourceIntegrity)?, batch.skipped, batch.origin])? != 1 { return Err(Error::RequestState); }
        for candidate in &mut batch.candidates {
            let station = &mut candidate.station;
            station.observed_ms = now;
            station.refresh_id = id.into();
            station.validate()?;
            let source = HttpSource::new(
                &station.name,
                &candidate.endpoint,
                NetworkScope::PublicInternet {},
            )?;
            if source.origin() != station.stream_origin {
                return Err(Error::SourceIntegrity);
            }
            let json = serde_json::to_string(station)?;
            if json.len() > 8192 {
                return Err(Error::InvalidInput("station metadata size"));
            }
            let folded = |list: &[String]| -> Result<String> {
                Ok(serde_json::to_string(
                    &list.iter().map(|v| v.to_lowercase()).collect::<Vec<_>>(),
                )?)
            };
            tx.execute("INSERT INTO directory_stations(provider, id, metadata_json, endpoint, name_folded, country, languages_folded, tags_folded, healthy, refresh_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(provider, id) DO UPDATE SET metadata_json=excluded.metadata_json, endpoint=excluded.endpoint, name_folded=excluded.name_folded, country=excluded.country, languages_folded=excluded.languages_folded, tags_folded=excluded.tags_folded, healthy=excluded.healthy, refresh_id=excluded.refresh_id", params![station.provider, station.id, json, source.endpoint(), station.name.to_lowercase(), station.country, folded(&station.languages)?, folded(&station.tags)?, station.last_check_ok, id])?;
        }
        let count: u32 =
            tx.query_row("SELECT count(*) FROM directory_stations", [], |r| r.get(0))?;
        if count > MAX_CACHED_STATIONS {
            return Err(Error::InvalidInput("directory cache capacity reached"));
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn fail_refresh(&mut self, id: &str, error: &Error) -> Result<()> {
        let detail = match error {
            Error::Acquisition(detail) | Error::InvalidInput(detail) => *detail,
            _ => "directory refresh failed; previous cache retained",
        };
        if self.connection.execute("UPDATE directory_refreshes SET state = 'failed', failure = ?2 WHERE id = ?1 AND state = 'running'", params![id, detail])? != 1 { return Err(Error::RequestState); }
        Ok(())
    }

    pub(crate) fn recover_directory_refreshes(&mut self) -> Result<()> {
        self.connection.execute("UPDATE directory_refreshes SET state = 'interrupted', failure = 'service stopped before directory publication' WHERE state = 'running'", [])?;
        Ok(())
    }

    /// # Errors
    /// Rejects malformed station identities and corrupt cached metadata.
    pub fn station(&self, id: &str) -> Result<Station> {
        validate_station_id(id)?;
        let (json, endpoint): (String, String) = self.connection.query_row("SELECT metadata_json, endpoint FROM directory_stations WHERE provider = 'radio_browser' AND id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.ok_or(Error::NotFound)?;
        let station: Station = serde_json::from_str(&json)?;
        station.validate()?;
        let source = HttpSource::new(&station.name, &endpoint, NetworkScope::PublicInternet {})?;
        if station.id != id || source.origin() != station.stream_origin {
            return Err(Error::SourceIntegrity);
        }
        Ok(station)
    }

    /// Searches cached names with Unicode lowercase substring matching; labels use exact matching.
    /// # Errors
    /// Rejects unsafe filters, invalid cursors or page sizes outside 1..=16.
    pub fn search_stations(
        &self,
        filter: &StationFilter,
        favorites_only: bool,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Station>> {
        filter.validate()?;
        if !(1..=16).contains(&limit) {
            return Err(Error::InvalidInput("station page size"));
        }
        if let Some(after) = after {
            validate_station_id(after)?;
        }
        let mut query = self.connection.prepare("SELECT id FROM directory_stations WHERE provider = 'radio_browser' AND id > ?1 AND instr(name_folded, ?2) > 0 AND (?3 = '' OR country = ?3) AND (?4 = '' OR EXISTS(SELECT 1 FROM json_each(languages_folded) WHERE value = ?4)) AND (?5 = '' OR EXISTS(SELECT 1 FROM json_each(tags_folded) WHERE value = ?5)) AND (NOT ?6 OR healthy = 1) AND (NOT ?8 OR EXISTS(SELECT 1 FROM station_favorites WHERE provider = directory_stations.provider AND station_id = directory_stations.id)) ORDER BY id LIMIT ?7")?;
        let ids = query
            .query_map(
                params![
                    after.unwrap_or(""),
                    filter.name.to_lowercase(),
                    filter.country,
                    filter.language.to_lowercase(),
                    filter.tag.to_lowercase(),
                    filter.healthy_only,
                    limit,
                    favorites_only
                ],
                |r| r.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.iter().map(|id| self.station(id)).collect()
    }

    /// Saves user preference independently of provider observations. Never opens a stream.
    /// # Errors
    /// Rejects unknown or invalid identities and corrupt cached metadata.
    pub fn set_station_favorite(&mut self, id: &str, favorite: bool) -> Result<()> {
        self.station(id)?;
        if favorite {
            self.connection.execute(
                "INSERT INTO station_favorites(provider, station_id) VALUES ('radio_browser', ?1) ON CONFLICT(provider, station_id) DO NOTHING", [id],
            )?;
        } else {
            self.connection.execute(
                "DELETE FROM station_favorites WHERE provider = 'radio_browser' AND station_id = ?1", [id],
            )?;
        }
        Ok(())
    }

    /// # Errors
    /// Rejects malformed identities or catalog read errors.
    pub fn is_station_favorite(&self, id: &str) -> Result<bool> {
        validate_station_id(id)?;
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM station_favorites WHERE provider = 'radio_browser' AND station_id = ?1)", [id], |r| r.get(0),
        )?)
    }

    /// Register an explicitly selected station without contacting its stream.
    /// # Errors
    /// Rejects conflicting replay and atomically preserves directory provenance.
    pub fn add_station_source(
        &mut self,
        id: &str,
        revision: &str,
        redirects: RedirectPolicy,
    ) -> Result<SourceAdmission> {
        validate_station_id(id)?;
        validate_key(revision, "source revision ID")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (json, endpoint): (String, String) = tx.query_row("SELECT metadata_json, endpoint FROM directory_stations WHERE provider = 'radio_browser' AND id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.ok_or(Error::NotFound)?;
        let station: Station = serde_json::from_str(&json)?;
        station.validate()?;
        let source = HttpSource::new(&station.name, &endpoint, NetworkScope::PublicInternet {})?;
        if station.id != id || station.stream_origin != source.origin() {
            return Err(Error::SourceIntegrity);
        }
        let source = source.with_redirects(redirects)?;
        let admission = register_source_in(&tx, revision, &source)?;
        let link: Option<(String, String)> = tx.query_row("SELECT provider, station_id FROM source_directory_links WHERE source_revision = ?1", [revision], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some(link) = link {
            if link != (station.provider.clone(), station.id.clone()) {
                return Err(Error::IdempotencyConflict);
            }
        } else {
            if !admission.newly_created {
                return Err(Error::IdempotencyConflict);
            }
            tx.execute("INSERT INTO source_directory_links(source_revision, provider, station_id, metadata_json) VALUES (?1, ?2, ?3, ?4)", params![revision, station.provider, id, json])?;
        }
        tx.commit()?;
        Ok(admission)
    }

    pub(crate) fn audit_discovery(&self) -> Result<()> {
        let status = self.directory_status()?;
        if status.cached_stations > MAX_CACHED_STATIONS {
            return Err(Error::SourceIntegrity);
        }
        let mut query = self
            .connection
            .prepare("SELECT id FROM directory_stations")?;
        for row in query.query_map([], |r| r.get::<_, String>(0))? {
            self.station(&row?)?;
        }
        Ok(())
    }
}
