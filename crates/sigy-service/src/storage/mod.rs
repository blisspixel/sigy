//! The single transactional catalog implementation for application state.

use std::{path::Path, time::Duration};

use rusqlite::{Connection, TransactionBehavior};

use crate::{Error, Result};

pub mod captures;
mod clicks;
pub mod discovery;
pub mod dvr;
pub mod languages;
pub mod ledger;
mod listens;
mod playlists;
pub(crate) mod podcast_feeds;
pub(crate) mod podcast_text;
pub mod podcasts;
pub mod recognition;
pub use clicks::ClickStatus;
pub(crate) mod analysis;
pub(crate) mod analysis_jobs;
pub(crate) mod directory_policy;
pub(crate) mod providers;
pub(crate) mod schedules;
pub mod sources;
pub(crate) mod transcripts;

pub const SCHEMA_VERSION: u32 = 28;
const APPLICATION_ID: i64 = 1_397_311_321;

#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens a catalog and applies known migrations atomically.
    /// # Errors
    /// Fails on inaccessible storage, newer schemas, or inconsistent accounting.
    pub fn open(path: &Path) -> Result<Self> {
        if path.try_exists()? && path.metadata()?.len() < 100 {
            // An existing truncated catalog is damage, not a fresh library.
            return Err(Error::CatalogIntegrity);
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "trusted_schema", false)?;
        let application_id: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if application_id != APPLICATION_ID && (application_id != 0 || version != 0) {
            return Err(Error::ForeignCatalog);
        }
        if application_id == 0 {
            let tables: i64 = connection.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            if tables != 0 {
                return Err(Error::ForeignCatalog);
            }
        }
        if version > i64::from(SCHEMA_VERSION) {
            return Err(Error::FutureSchema {
                found: version,
                supported: i64::from(SCHEMA_VERSION),
            });
        }
        let integrity: String =
            connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(Error::CatalogIntegrity);
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 =
            transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        migrate(&transaction, version)?;
        transaction.commit()?;
        let store = Self { connection };
        let foreign_key_errors: i64 = store.connection.query_row(
            "SELECT count(*) FROM pragma_foreign_key_check",
            [],
            |row| row.get(0),
        )?;
        if foreign_key_errors != 0 {
            return Err(Error::CatalogIntegrity);
        }
        store.audit_ledger()?;
        store.audit_captures()?;
        store.audit_sources()?;
        store.audit_dvr()?;
        store.audit_discovery()?;
        store.audit_playlists()?;
        store.audit_clicks()?;
        store.audit_listens()?;
        store.audit_podcasts()?;
        store.audit_schedules()?;
        store.audit_transcripts()?;
        store.audit_language_evidence()?;
        store.audit_analysis_jobs()?;
        store.audit_providers()?;
        Ok(store)
    }

    /// # Errors
    /// Returns catalog read errors.
    pub(crate) fn catalog_quick_check(&self) -> Result<bool> {
        let mut statement = self.connection.prepare("PRAGMA quick_check")?;
        let messages = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(messages.as_slice() == ["ok"])
    }

    /// # Errors
    /// Returns catalog read errors.
    pub fn sqlite_version(&self) -> Result<String> {
        Ok(self
            .connection
            .query_row("SELECT sqlite_version()", [], |row| row.get(0))?)
    }
}

fn migrate(transaction: &rusqlite::Transaction<'_>, version: i64) -> Result<()> {
    if version == 0 {
        transaction.execute_batch(include_str!("001-foundation.sql"))?;
    }
    if (0..=1).contains(&version) {
        transaction.execute_batch(include_str!("002-captures.sql"))?;
    }
    if (0..=2).contains(&version) {
        transaction.execute_batch(include_str!("003-sources.sql"))?;
    }
    if (0..=3).contains(&version) {
        transaction.execute_batch(include_str!("004-dvr.sql"))?;
    }
    if (0..=4).contains(&version) {
        transaction.execute_batch(include_str!("005-discovery.sql"))?;
    }
    if (0..=5).contains(&version) {
        transaction.execute_batch(include_str!("006-redirects.sql"))?;
    }
    if (0..=6).contains(&version) {
        transaction.execute_batch(include_str!("007-favorites.sql"))?;
    }
    if (0..=7).contains(&version) {
        transaction.execute_batch(include_str!("008-playlists.sql"))?;
    }
    if (0..=8).contains(&version) {
        transaction.execute_batch(include_str!("009-clicks.sql"))?;
    }
    if (0..=9).contains(&version) {
        transaction.execute_batch(include_str!("010-listens.sql"))?;
    }
    if (0..=10).contains(&version) {
        transaction.execute_batch(include_str!("011-icy.sql"))?;
    }
    if (0..=11).contains(&version) {
        transaction.execute_batch(include_str!("012-podcasts.sql"))?;
    }
    if (0..=12).contains(&version) {
        transaction.execute_batch(include_str!("013-podcast-feeds.sql"))?;
    }
    if (0..=13).contains(&version) {
        transaction.execute_batch(include_str!("014-enclosures.sql"))?;
    }
    if (0..=14).contains(&version) {
        transaction.execute_batch(include_str!("015-publisher-text.sql"))?;
    }
    if (0..=15).contains(&version) {
        transaction.execute_batch(include_str!("016-recording-intervals.sql"))?;
    }
    if (0..=16).contains(&version) {
        transaction.execute_batch(include_str!("017-segment-seals.sql"))?;
    }
    if (0..=17).contains(&version) {
        transaction.execute_batch(include_str!("018-recording-gaps.sql"))?;
    }
    if (0..=18).contains(&version) {
        transaction.execute_batch(include_str!("019-segment-retention.sql"))?;
    }
    if (0..=19).contains(&version) {
        transaction.execute_batch(include_str!("020-schedules.sql"))?;
    }
    if (0..=20).contains(&version) {
        transaction.execute_batch(include_str!("021-directory-policy.sql"))?;
    }
    if (0..=21).contains(&version) {
        transaction.execute_batch(include_str!("022-analysis-inputs.sql"))?;
    }
    if (0..=22).contains(&version) {
        transaction.execute_batch(include_str!("023-transcripts.sql"))?;
    }
    if (0..=23).contains(&version) {
        transaction.execute_batch(include_str!("024-language-evidence.sql"))?;
    }
    if (0..=24).contains(&version) {
        transaction.execute_batch(include_str!("025-analysis-jobs.sql"))?;
    }
    if (0..=25).contains(&version) {
        transaction.execute_batch(include_str!("026-transcript-revisions.sql"))?;
        transaction.execute_batch(include_str!("026-transcript-invariants.sql"))?;
        transcripts::audit(transaction)?;
        analysis_jobs::audit(transaction)?;
        let violations: i64 =
            transaction.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        if violations != 0 {
            return Err(Error::CatalogIntegrity);
        }
    }
    if (0..=26).contains(&version) {
        transaction.execute_batch(include_str!("027-recognition-profiles.sql"))?;
    }
    if (0..=27).contains(&version) {
        transaction.execute_batch(include_str!("028-provider-routes.sql"))?;
    } else if version != i64::from(SCHEMA_VERSION) {
        return Err(Error::FutureSchema {
            found: version,
            supported: i64::from(SCHEMA_VERSION),
        });
    }
    Ok(())
}

pub(crate) fn validate_key(value: &str, field: &'static str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
    {
        return Err(Error::InvalidInput(field));
    }
    Ok(())
}

pub(crate) fn now_ms() -> Result<i64> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    i64::try_from(millis).map_err(|_| Error::InvalidInput("clock range"))
}
