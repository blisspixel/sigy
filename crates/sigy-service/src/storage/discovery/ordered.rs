//! Indexed directory keysets with operation-local, revision-bound cursors.

use super::super::query_work::{Limits, QueryWork};
use super::{Station, StationFilter, Store};
use crate::{
    Error, Result,
    discovery::{
        countries::station_key,
        ordered::{
            COMPARISON, DirectoryCatalog, MAX_CURSOR_BYTES, MAX_PAGE_BYTES, OrderedStationPage,
        },
        validate_station_id,
    },
    sources::{HttpSource, NetworkScope},
};
use rusqlite::{Connection, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

const SELECT: &str = "SELECT d.id,d.metadata_json,d.endpoint,d.name_ordered,EXISTS(SELECT 1 FROM station_favorites f WHERE f.provider=d.provider AND f.station_id=d.id) FROM directory_stations d WHERE d.provider='radio_browser' AND (d.name_ordered,d.id)>(?1,?2) AND instr(d.name_folded,?3)>0 AND (?4='' OR d.country=?4) AND (?5='' OR EXISTS(SELECT 1 FROM json_each(d.languages_folded) WHERE value=?5)) AND (?6='' OR EXISTS(SELECT 1 FROM json_each(d.tags_folded) WHERE value=?6)) AND (NOT ?7 OR d.healthy=1) AND (NOT ?8 OR EXISTS(SELECT 1 FROM station_favorites f WHERE f.provider=d.provider AND f.station_id=d.id)) ORDER BY d.name_ordered,d.id LIMIT ?9";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    catalog: DirectoryCatalog,
    scope: String,
    key: String,
    id: String,
}

struct Capped {
    bytes: Vec<u8>,
    maximum: usize,
}

impl Write for Capped {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("ordered directory encoding limit"));
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = required
                .max(self.bytes.capacity().saturating_mul(2))
                .min(self.maximum);
            self.bytes.reserve_exact(capacity - self.bytes.len());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>> {
    let mut writer = Capped {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}

fn text(row: &Row<'_>, column: usize, maximum: usize) -> rusqlite::Result<String> {
    let rusqlite::types::ValueRef::Text(bytes) = row.get_ref(column)? else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    if bytes.len() > maximum {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| rusqlite::Error::InvalidQuery)?
        .to_owned())
}

pub(super) fn blob_key(row: &Row<'_>, column: usize) -> rusqlite::Result<String> {
    let rusqlite::types::ValueRef::Blob(bytes) = row.get_ref(column)? else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    if bytes.len() > 3072 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| rusqlite::Error::InvalidQuery)?
        .to_owned())
}
pub(in crate::storage) fn advance_catalog(connection: &Connection) -> Result<()> {
    if connection.execute("UPDATE directory_catalog SET revision=revision+1 WHERE singleton=1 AND revision<9223372036854775807", [])? != 1 {
        return Err(Error::SourceIntegrity);
    }
    Ok(())
}

pub(in crate::storage) fn migrate_048(connection: &Connection) -> Result<()> {
    connection.execute_batch(include_str!("../048-directory-order.sql"))?;
    let mut statement = connection
        .prepare("SELECT id,metadata_json FROM directory_stations ORDER BY provider,id")?;
    let mut rows = statement.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > crate::discovery::MAX_CACHED_STATIONS {
            return Err(Error::SourceIntegrity);
        }
        let id = text(row, 0, 36)?;
        let station: Station = serde_json::from_str(&text(row, 1, 8192)?)?;
        station.validate()?;
        if station.id != id {
            return Err(Error::SourceIntegrity);
        }
        connection.execute("UPDATE directory_stations SET name_ordered=?1 WHERE provider='radio_browser' AND id=?2", params![station_key(&station.name)?.as_bytes(),id])?;
    }
    Ok(())
}

#[cfg(test)]
pub(in crate::storage) fn revert_048_for_tests(connection: &Connection) -> Result<()> {
    let present:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('directory_stations') WHERE name='name_ordered')",[],|row|row.get(0))?;
    connection.execute_batch(
        "DROP INDEX IF EXISTS directory_name_order; DROP TABLE IF EXISTS directory_catalog",
    )?;
    if present {
        connection.execute_batch(
            "ALTER TABLE directory_stations DROP COLUMN name_ordered; PRAGMA user_version=47;",
        )?;
    }
    Ok(())
}

impl Store {
    /// Current disposable directory scope, independent of immutable evidence.
    /// # Errors
    /// Refuses corrupt namespace or revision metadata.
    pub fn directory_catalog(&self) -> Result<DirectoryCatalog> {
        let (namespace, revision) = self.connection.query_row(
            "SELECT namespace,revision FROM directory_catalog WHERE singleton=1",
            [],
            |row| Ok((text(row, 0, 32)?, row.get::<_, i64>(1)?)),
        )?;
        if namespace.len() != 32
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::SourceIntegrity);
        }
        Ok(DirectoryCatalog {
            namespace,
            revision: u64::try_from(revision).map_err(|_| Error::SourceIntegrity)?,
            comparison: COMPARISON.into(),
        })
    }

    pub(crate) fn renew_directory_namespace(&self) -> Result<()> {
        if self.connection.execute("UPDATE directory_catalog SET namespace=lower(hex(randomblob(16))),revision=0 WHERE singleton=1", [])? != 1 {
            return Err(Error::SourceIntegrity);
        }
        Ok(())
    }

    /// Read one bounded name-ordered page. This does not contact a station.
    /// # Errors
    /// Refuses invalid, stale or cross-query cursors, corrupt rows and exhausted work.
    pub fn search_stations_ordered(
        &self,
        filter: &StationFilter,
        favorites_only: bool,
        after: Option<&str>,
        limit: u32,
    ) -> Result<OrderedStationPage> {
        self.guarded_directory_read(|store| {
            store.ordered_stations_in_work(filter, favorites_only, after, limit)
        })
    }

    pub(crate) fn guarded_directory_read<T>(
        &self,
        read: impl FnOnce(&Self) -> Result<T>,
    ) -> Result<T> {
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let result = (|| {
            let tx = rusqlite::Transaction::new_unchecked(
                &self.connection,
                TransactionBehavior::Deferred,
            )?;
            let page = read(self)?;
            work.check()?;
            tx.commit()?;
            Ok(page)
        })();
        work.finish()?;
        result
    }

    pub(crate) fn ordered_stations_in_work(
        &self,
        filter: &StationFilter,
        favorites_only: bool,
        after: Option<&str>,
        limit: u32,
    ) -> Result<OrderedStationPage> {
        filter.validate()?;
        if !(1..=16).contains(&limit) {
            return Err(Error::InvalidInput("station page size"));
        }
        let catalog = self.directory_catalog()?;
        let effective = StationFilter {
            name: filter.name.to_lowercase(),
            country: filter.country.clone(),
            language: filter.language.to_lowercase(),
            tag: filter.tag.to_lowercase(),
            healthy_only: filter.healthy_only,
        };
        let scope = crate::recognition::sha256_hex(&encode(
            &(&effective, favorites_only, limit, "directory-page-v1"),
            4096,
        )?);
        let cursor = cursor(after, &catalog, &scope)?;
        if let Some(cursor) = &cursor {
            let member:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM directory_stations d WHERE d.provider='radio_browser' AND d.id=?1 AND d.name_ordered=?2 AND instr(d.name_folded,?3)>0 AND (?4='' OR d.country=?4) AND (?5='' OR EXISTS(SELECT 1 FROM json_each(d.languages_folded) WHERE value=?5)) AND (?6='' OR EXISTS(SELECT 1 FROM json_each(d.tags_folded) WHERE value=?6)) AND (NOT ?7 OR d.healthy=1) AND (NOT ?8 OR EXISTS(SELECT 1 FROM station_favorites f WHERE f.provider=d.provider AND f.station_id=d.id)))",params![cursor.id,cursor.key.as_bytes(),effective.name,effective.country,effective.language,effective.tag,effective.healthy_only,favorites_only],|row|row.get(0))?;
            if !member {
                return Err(Error::InvalidInput("ordered station cursor"));
            }
        }
        let mut statement = self.connection.prepare(SELECT)?;
        let (key, id) = cursor
            .as_ref()
            .map_or(("", ""), |c| (c.key.as_str(), c.id.as_str()));
        let mut rows = statement.query(params![
            key.as_bytes(),
            id,
            effective.name,
            effective.country,
            effective.language,
            effective.tag,
            effective.healthy_only,
            favorites_only,
            limit + 1
        ])?;
        let mut entries = Vec::new();
        let mut favorite_ids = Vec::new();
        let mut last = None;
        let mut more = false;
        while let Some(row) = rows.next()? {
            if entries.len() == limit as usize {
                more = true;
                break;
            }
            let (station, key, favorite) = station_row(row)?;
            if favorite {
                favorite_ids.push(station.id.clone());
            }
            last = Some((key, station.id.clone()));
            entries.push(station);
        }
        let next_after = if more {
            let (key, id) = last.ok_or(Error::SourceIntegrity)?;
            Some(crate::storage::dvr::hex(&encode(
                &Cursor {
                    catalog: catalog.clone(),
                    scope,
                    key,
                    id,
                },
                MAX_CURSOR_BYTES / 2,
            )?))
        } else {
            None
        };
        let page = OrderedStationPage {
            entries,
            favorite_ids,
            next_after,
            catalog,
        };
        encode(&page, MAX_PAGE_BYTES)?;
        Ok(page)
    }
}

fn cursor(after: Option<&str>, catalog: &DirectoryCatalog, scope: &str) -> Result<Option<Cursor>> {
    let Some(after) = after else {
        return Ok(None);
    };
    if after.len() > MAX_CURSOR_BYTES {
        return Err(Error::InvalidInput("ordered station cursor"));
    }
    let bytes = decode(after)?;
    let cursor: Cursor = serde_json::from_slice(&bytes)
        .map_err(|_| Error::InvalidInput("ordered station cursor"))?;
    validate_station_id(&cursor.id)?;
    if cursor.key.is_empty() || cursor.key.len() > 3072 || cursor.scope.len() != 64 {
        return Err(Error::InvalidInput("ordered station cursor"));
    }
    if cursor.catalog != *catalog || cursor.scope != scope {
        return Err(Error::InvalidInput(
            "ordered station cursor scope changed; restart search",
        ));
    }
    Ok(Some(cursor))
}

fn decode(value: &str) -> Result<Vec<u8>> {
    if value.len() > MAX_CURSOR_BYTES
        || !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("ordered station cursor"));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().as_chunks::<2>().0 {
        let digit = |byte: u8| {
            if byte <= b'9' {
                byte - b'0'
            } else {
                byte - b'a' + 10
            }
        };
        bytes.push(digit(pair[0]) * 16 + digit(pair[1]));
    }
    Ok(bytes)
}

fn station_row(row: &Row<'_>) -> Result<(Station, String, bool)> {
    let id = text(row, 0, 36)?;
    let json = text(row, 1, 8192)?;
    let endpoint = text(row, 2, 2048)?;
    let key = blob_key(row, 3)?;
    let favorite = row.get(4)?;
    let station: Station = serde_json::from_str(&json)?;
    station.validate()?;
    let source = HttpSource::new(&station.name, &endpoint, NetworkScope::PublicInternet {})?;
    if station.id != id
        || source.origin() != station.stream_origin
        || key != station_key(&station.name)?
    {
        return Err(Error::SourceIntegrity);
    }
    Ok((station, key, favorite))
}

#[cfg(test)]
mod tests;
