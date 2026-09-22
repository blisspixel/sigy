//! Immutable, explicitly authorized source configuration. No discovery or I/O.

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::{Store, now_ms, validate_key};
use crate::{
    Error, Result,
    sources::{HttpSource, NetworkScope},
};

pub const MAX_SOURCE_REVISIONS: u32 = 4096;
pub const MAX_SOURCE_PAGE: u32 = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRevision {
    pub id: String,
    pub source: HttpSource,
    pub created_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAdmission {
    pub revision: SourceRevision,
    pub newly_created: bool,
}

impl Store {
    /// Registers a complete configuration once. A changed URL, label or grant
    /// requires a new key so existing captures retain their original authority.
    /// # Errors
    /// Rejects malformed keys, conflicting replays, full catalogs or write errors.
    pub fn register_source(&mut self, id: &str, source: &HttpSource) -> Result<SourceAdmission> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let admission = register_source_in(&tx, id, source)?;
        tx.commit()?;
        Ok(admission)
    }

    /// # Errors
    /// Rejects malformed identifiers, corrupt configurations and catalog errors.
    pub fn source(&self, id: &str) -> Result<Option<SourceRevision>> {
        validate_key(id, "source revision ID")?;
        read_source(&self.connection, id)
    }

    /// # Errors
    /// Rejects malformed cursors or page sizes outside 1..=32.
    pub fn sources(&self, after_id: Option<&str>, limit: u32) -> Result<Vec<SourceRevision>> {
        if !(1..=MAX_SOURCE_PAGE).contains(&limit) {
            return Err(Error::InvalidInput("source page size"));
        }
        if let Some(id) = after_id {
            validate_key(id, "source cursor")?;
        }
        let mut query = self
            .connection
            .prepare("SELECT id FROM source_revisions WHERE id > ?1 ORDER BY id LIMIT ?2")?;
        let ids = query
            .query_map(params![after_id.unwrap_or(""), limit], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.source(&id)?.ok_or(Error::SourceIntegrity))
            .collect()
    }

    /// # Errors
    /// Rejects configuration that no longer passes the current trust boundary.
    /// Policy changes require explicit migration, not silent privilege expansion.
    pub fn audit_sources(&self) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let mut query = tx.prepare("SELECT id FROM source_revisions ORDER BY id")?;
        let ids = query.query_map([], |row| row.get::<_, String>(0))?;
        for id in ids {
            read_source(&tx, &id?)?.ok_or(Error::SourceIntegrity)?;
        }
        Ok(())
    }
}

pub(super) fn register_source_in(
    tx: &rusqlite::Connection,
    id: &str,
    source: &HttpSource,
) -> Result<SourceAdmission> {
    validate_key(id, "source revision ID")?;
    if let Some(revision) = read_source(tx, id)? {
        if revision.source != *source {
            return Err(Error::IdempotencyConflict);
        }
        return Ok(SourceAdmission {
            revision,
            newly_created: false,
        });
    }
    let count: u32 = tx.query_row("SELECT count(*) FROM source_revisions", [], |row| {
        row.get(0)
    })?;
    if count >= MAX_SOURCE_REVISIONS {
        return Err(Error::SourceCapacity);
    }
    let (scope, address) = match source.network() {
        NetworkScope::PublicInternet {} => ("public_internet", None),
        NetworkScope::PinnedAddress { address } => ("pinned_address", Some(address.to_string())),
    };
    let created_ms = now_ms()?;
    tx.execute("INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, pinned_address, created_ms, redirect_policy) VALUES (?1, 'http_audio', ?2, ?3, ?4, ?5, ?6, ?7)", params![id, source.name(), source.endpoint(), scope, address, created_ms, source.redirects().to_string()])?;
    Ok(SourceAdmission {
        revision: SourceRevision {
            id: id.into(),
            source: source.clone(),
            created_ms,
        },
        newly_created: true,
    })
}

pub(super) fn read_source(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<SourceRevision>> {
    let raw = connection.query_row("SELECT kind, name, endpoint, network_scope, pinned_address, created_ms, redirect_policy FROM source_revisions WHERE id = ?1", [id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, i64>(5)?, row.get::<_, String>(6)?))
    }).optional()?;
    let Some((kind, name, endpoint, scope, pinned, created_ms, redirects)) = raw else {
        return Ok(None);
    };
    validate_key(id, "source revision ID").map_err(|_| Error::SourceIntegrity)?;
    if kind != "http_audio" || created_ms < 0 {
        return Err(Error::SourceIntegrity);
    }
    let network = match (scope.as_str(), pinned.as_deref()) {
        ("public_internet", None) => NetworkScope::PublicInternet {},
        ("pinned_address", Some(value)) => {
            let address = value.parse().map_err(|_| Error::SourceIntegrity)?;
            let network = NetworkScope::PinnedAddress { address };
            if address.to_string() != value {
                return Err(Error::SourceIntegrity);
            }
            network
        }
        _ => return Err(Error::SourceIntegrity),
    };
    let source = HttpSource::new(&name, &endpoint, network)
        .and_then(|source| source.with_redirects(redirects.parse()?))
        .map_err(|_| Error::SourceIntegrity)?;
    if source.endpoint() != endpoint {
        return Err(Error::SourceIntegrity);
    }
    Ok(Some(SourceRevision {
        id: id.into(),
        source,
        created_ms,
    }))
}
