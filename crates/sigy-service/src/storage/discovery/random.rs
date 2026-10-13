//! A bounded full-cache count and unbiased random rank in one read snapshot.

use super::{StationFilter, Store, ordered};
use crate::{Error, Result, discovery::ordered::RandomStation};
use rusqlite::params;

const ELIGIBLE: &str = "FROM directory_stations d WHERE d.provider='radio_browser' AND instr(d.name_folded,?1)>0 AND (?2='' OR d.country=?2) AND (?3='' OR EXISTS(SELECT 1 FROM json_each(d.languages_folded) WHERE value=?3)) AND (?4='' OR EXISTS(SELECT 1 FROM json_each(d.tags_folded) WHERE value=?4)) AND (NOT ?5 OR d.healthy=1) AND (NOT ?6 OR EXISTS(SELECT 1 FROM station_favorites f WHERE f.provider=d.provider AND f.station_id=d.id)) AND json_type(d.metadata_json,'$.latitude') IN ('real','integer') AND json_extract(d.metadata_json,'$.latitude') BETWEEN -90 AND 90 AND json_type(d.metadata_json,'$.longitude') IN ('real','integer') AND json_extract(d.metadata_json,'$.longitude') BETWEEN -180 AND 180";

impl Store {
    pub(crate) fn random_station_in_work(
        &self,
        filter: &StationFilter,
        favorites_only: bool,
    ) -> Result<RandomStation> {
        self.random_station_with(filter, favorites_only, || {
            let mut bytes = [0; 8];
            getrandom::fill(&mut bytes)
                .map_err(|_| Error::Acquisition("station selection entropy"))?;
            Ok(u64::from_le_bytes(bytes))
        })
    }

    fn random_station_with(
        &self,
        filter: &StationFilter,
        favorites_only: bool,
        entropy: impl FnMut() -> Result<u64>,
    ) -> Result<RandomStation> {
        filter.validate()?;
        let catalog = self.directory_catalog()?;
        let name = filter.name.to_lowercase();
        let language = filter.language.to_lowercase();
        let tag = filter.tag.to_lowercase();
        let arguments = params![
            name,
            filter.country,
            language,
            tag,
            filter.healthy_only,
            favorites_only
        ];
        let eligible_count: u32 = self.connection.query_row(
            &format!("SELECT count(*) {ELIGIBLE}"),
            arguments,
            |row| row.get(0),
        )?;
        if eligible_count > crate::discovery::MAX_CACHED_STATIONS {
            return Err(Error::SourceIntegrity);
        }
        let mut result = RandomStation {
            station: None,
            favorite: false,
            eligible_count,
            catalog,
        };
        if eligible_count != 0 {
            let rank = uniform_rank(eligible_count, entropy)?;
            let sql = format!(
                "SELECT d.id,d.metadata_json,d.endpoint,d.name_ordered,EXISTS(SELECT 1 FROM station_favorites f WHERE f.provider=d.provider AND f.station_id=d.id) {ELIGIBLE} ORDER BY d.id LIMIT 1 OFFSET ?7"
            );
            let mut statement = self.connection.prepare(&sql)?;
            let mut rows = statement.query(params![
                name,
                filter.country,
                language,
                tag,
                filter.healthy_only,
                favorites_only,
                rank
            ])?;
            let row = rows.next()?.ok_or(Error::SourceIntegrity)?;
            let (station, _, favorite) = ordered::station_row(row)?;
            result.station = Some(station);
            result.favorite = favorite;
        }
        ordered::encode(&result, 16 * 1024)?;
        Ok(result)
    }
}

fn uniform_rank(count: u32, mut entropy: impl FnMut() -> Result<u64>) -> Result<u32> {
    if count == 0 {
        return Err(Error::InvalidInput("empty station selection"));
    }
    let bound = u64::from(count);
    let threshold = bound.wrapping_neg() % bound;
    // Bound even a broken entropy source. Failure never falls back to a biased rank.
    for _ in 0..16 {
        let value = entropy()?;
        if value >= threshold {
            return u32::try_from(value % bound).map_err(|_| Error::SourceIntegrity);
        }
    }
    Err(Error::Acquisition("station selection entropy exhausted"))
}

#[cfg(test)]
mod tests;
