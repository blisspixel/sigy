//! Durable playlist requests. Accepting an entry performs no network I/O.

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::{
    Store, now_ms,
    sources::{read_source, register_source_in},
    validate_key,
};
use crate::{
    Error, Result,
    sources::{
        HttpSource,
        playlist::{ResolvedPlaylist, entry_matches_policy},
    },
};

const MAX_PLAYLIST_REQUESTS: u32 = 4096;

pub(crate) struct PlaylistEntryRecord {
    pub index: u32,
    pub endpoint: String,
    pub origin: String,
}

pub(crate) struct PlaylistAcceptanceRecord {
    pub index: u32,
    pub child_revision: String,
    pub parent_revision: String,
    pub document_sha256: String,
}

pub(crate) struct PlaylistRecord {
    pub id: String,
    pub parent_revision: String,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub document_sha256: Option<String>,
    pub final_origin: Option<String>,
    pub failure: Option<String>,
    pub entries: Vec<PlaylistEntryRecord>,
    pub acceptances: Vec<PlaylistAcceptanceRecord>,
}

impl std::fmt::Debug for PlaylistRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlaylistRecord")
            .field("id", &self.id)
            .field("parent_revision", &self.parent_revision)
            .field("state", &self.state)
            .field("entries", &self.entries.len())
            .field("acceptances", &self.acceptances.len())
            .finish_non_exhaustive()
    }
}

impl Store {
    pub(crate) fn begin_playlist(&mut self, id: &str, parent_revision: &str) -> Result<bool> {
        validate_key(id, "playlist request ID")?;
        validate_key(parent_revision, "source revision ID")?;
        if self.source(parent_revision)?.is_none() {
            return Err(Error::NotFound);
        }
        let now = now_ms()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT parent_revision FROM playlist_resolves WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != parent_revision {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(false);
        }
        let (count, active): (u32, u32) = tx.query_row(
            "SELECT count(*), coalesce(sum(state = 'running'), 0) FROM playlist_resolves",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if count >= MAX_PLAYLIST_REQUESTS || active != 0 {
            return Err(Error::InvalidInput("playlist resolve capacity reached"));
        }
        tx.execute(
            "INSERT INTO playlist_resolves(id, parent_revision, state, started_ms) VALUES (?1, ?2, 'running', ?3)",
            params![id, parent_revision, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn finish_playlist(&mut self, id: &str, resolved: &ResolvedPlaylist) -> Result<()> {
        validate_hash(&resolved.document_sha256)?;
        validate_origin(&resolved.final_origin)?;
        if resolved.entries.is_empty()
            || resolved.entries.len() > crate::sources::playlist::MAX_PLAYLIST_ENTRIES
        {
            return Err(Error::InvalidInput("playlist entry limit"));
        }
        let current = self.playlist(id)?;
        if current.state != "running" {
            return Err(Error::RequestState);
        }
        let parent = self
            .source(&current.parent_revision)?
            .ok_or(Error::SourceIntegrity)?;
        let mut prepared = Vec::with_capacity(resolved.entries.len());
        for (index, entry) in resolved.entries.iter().enumerate() {
            let candidate = HttpSource::new(
                parent.source.name(),
                &entry.endpoint,
                parent.source.network(),
            )
            .and_then(|source| source.with_redirects(parent.source.redirects()))
            .map_err(|_| Error::SourceIntegrity)?;
            if candidate.endpoint() != entry.endpoint || candidate.origin() != entry.origin {
                return Err(Error::SourceIntegrity);
            }
            entry_matches_policy(&parent.source, &resolved.final_origin, &candidate)?;
            prepared.push((
                u32::try_from(index).map_err(|_| Error::SourceIntegrity)?,
                candidate,
            ));
        }
        let now = now_ms()?;
        let count = i64::try_from(prepared.len()).map_err(|_| Error::SourceIntegrity)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "UPDATE playlist_resolves SET state = 'completed', completed_ms = ?2, document_sha256 = ?3, final_origin = ?4, entry_count = ?5 WHERE id = ?1 AND state = 'running'",
            params![id, now, resolved.document_sha256, resolved.final_origin, count],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        for (index, candidate) in &prepared {
            tx.execute(
                "INSERT INTO playlist_entries(resolve_id, entry_index, endpoint, origin) VALUES (?1, ?2, ?3, ?4)",
                params![id, i64::from(*index), candidate.endpoint(), candidate.origin()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn fail_playlist(&mut self, id: &str, error: &Error) -> Result<()> {
        let detail = match error {
            Error::Acquisition(detail) | Error::InvalidInput(detail) => *detail,
            Error::DestinationDenied => "playlist entry is outside the source policy",
            _ => "playlist resolve failed",
        };
        if self.connection.execute(
            "UPDATE playlist_resolves SET state = 'failed', failure = ?2 WHERE id = ?1 AND state = 'running'",
            params![id, detail],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    pub(crate) fn recover_playlist_resolves(&mut self) -> Result<()> {
        self.connection.execute(
            "UPDATE playlist_resolves SET state = 'interrupted', failure = 'service stopped before playlist resolution' WHERE state = 'running'",
            [],
        )?;
        Ok(())
    }

    /// # Errors
    /// Rejects unknown requests and corrupt playlist rows.
    pub(crate) fn playlist(&self, id: &str) -> Result<PlaylistRecord> {
        validate_key(id, "playlist request ID")?;
        read_playlist(&self.connection, id)?.ok_or(Error::NotFound)
    }

    pub(crate) fn linked_station_is_hls(&self, revision: &str) -> Result<bool> {
        validate_key(revision, "source revision ID")?;
        let station_id: Option<String> = self
            .connection
            .query_row(
                "SELECT station_id FROM source_directory_links WHERE source_revision = ?1 AND provider = 'radio_browser'",
                [revision],
                |row| row.get(0),
            )
            .optional()?;
        let Some(station_id) = station_id else {
            return Ok(false);
        };
        Ok(self.station(&station_id)?.hls)
    }

    /// Registers one previously resolved entry. Does not fetch or connect.
    /// # Errors
    /// Rejects unknown requests, policy failures, and conflicting replays.
    pub fn accept_playlist_entry(
        &mut self,
        id: &str,
        index: u32,
        child_revision: &str,
        name: &str,
    ) -> Result<super::sources::SourceAdmission> {
        validate_key(id, "playlist request ID")?;
        validate_key(child_revision, "source revision ID")?;
        if index >= 32 {
            return Err(Error::InvalidInput("playlist entry index"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = read_playlist(&tx, id)?.ok_or(Error::NotFound)?;
        if record.state != "completed" {
            return Err(Error::RequestState);
        }
        let hash = record
            .document_sha256
            .as_deref()
            .ok_or(Error::SourceIntegrity)?;
        let final_origin = record
            .final_origin
            .as_deref()
            .ok_or(Error::SourceIntegrity)?;
        validate_hash(hash)?;
        validate_origin(final_origin)?;
        let entry = record
            .entries
            .iter()
            .find(|entry| entry.index == index)
            .ok_or(Error::NotFound)?;
        let parent = read_source(&tx, &record.parent_revision)?.ok_or(Error::SourceIntegrity)?;
        let candidate = HttpSource::new(name, &entry.endpoint, parent.source.network())?
            .with_redirects(parent.source.redirects())?;
        if candidate.endpoint() != entry.endpoint || candidate.origin() != entry.origin {
            return Err(Error::SourceIntegrity);
        }
        entry_matches_policy(&parent.source, final_origin, &candidate)?;
        if let Some(existing) = record
            .acceptances
            .iter()
            .find(|acceptance| acceptance.index == index)
        {
            if existing.child_revision != child_revision
                || existing.parent_revision != record.parent_revision
                || existing.document_sha256 != hash
            {
                return Err(Error::IdempotencyConflict);
            }
            let stored = read_source(&tx, child_revision)?.ok_or(Error::SourceIntegrity)?;
            if stored.source != candidate {
                return Err(Error::IdempotencyConflict);
            }
            tx.commit()?;
            return Ok(super::sources::SourceAdmission {
                revision: stored,
                newly_created: false,
            });
        }
        let admission = register_source_in(&tx, child_revision, &candidate)?;
        if !admission.newly_created {
            return Err(Error::IdempotencyConflict);
        }
        tx.execute(
            "INSERT INTO playlist_acceptances(resolve_id, entry_index, child_revision, parent_revision, document_sha256) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, i64::from(index), child_revision, record.parent_revision, hash],
        )?;
        tx.commit()?;
        Ok(admission)
    }

    pub(crate) fn audit_playlists(&self) -> Result<()> {
        let mut query = self
            .connection
            .prepare("SELECT id FROM playlist_resolves ORDER BY id")?;
        let ids = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for id in ids {
            audit_one(self, &id)?;
        }
        Ok(())
    }
}

fn audit_one(store: &Store, id: &str) -> Result<()> {
    let record = read_playlist(&store.connection, id)?.ok_or(Error::SourceIntegrity)?;
    if let Some(failure) = &record.failure {
        crate::discovery::validate_text(failure, 256).map_err(|_| Error::SourceIntegrity)?;
    }
    match record.state.as_str() {
        "running" => {
            if record.failure.is_some()
                || !record.entries.is_empty()
                || record.document_sha256.is_some()
            {
                return Err(Error::SourceIntegrity);
            }
        }
        "failed" | "interrupted" => {
            if record.failure.is_none()
                || !record.entries.is_empty()
                || !record.acceptances.is_empty()
                || record.document_sha256.is_some()
                || record.final_origin.is_some()
            {
                return Err(Error::SourceIntegrity);
            }
        }
        "completed" => audit_completed(store, &record)?,
        _ => return Err(Error::SourceIntegrity),
    }
    Ok(())
}

fn audit_completed(store: &Store, record: &PlaylistRecord) -> Result<()> {
    if record.failure.is_some() {
        return Err(Error::SourceIntegrity);
    }
    let hash = record
        .document_sha256
        .as_deref()
        .ok_or(Error::SourceIntegrity)?;
    let origin = record
        .final_origin
        .as_deref()
        .ok_or(Error::SourceIntegrity)?;
    validate_hash(hash)?;
    validate_origin(origin)?;
    if record.entries.is_empty()
        || record.entries.len() > crate::sources::playlist::MAX_PLAYLIST_ENTRIES
    {
        return Err(Error::SourceIntegrity);
    }
    let parent = store
        .source(&record.parent_revision)?
        .ok_or(Error::SourceIntegrity)?;
    for (expected, entry) in record.entries.iter().enumerate() {
        if entry.index != u32::try_from(expected).map_err(|_| Error::SourceIntegrity)? {
            return Err(Error::SourceIntegrity);
        }
        let candidate = HttpSource::new(
            parent.source.name(),
            &entry.endpoint,
            parent.source.network(),
        )
        .and_then(|source| source.with_redirects(parent.source.redirects()))
        .map_err(|_| Error::SourceIntegrity)?;
        if candidate.endpoint() != entry.endpoint || candidate.origin() != entry.origin {
            return Err(Error::SourceIntegrity);
        }
        entry_matches_policy(&parent.source, origin, &candidate)
            .map_err(|_| Error::SourceIntegrity)?;
    }
    for acceptance in &record.acceptances {
        if acceptance.parent_revision != record.parent_revision
            || acceptance.document_sha256 != hash
        {
            return Err(Error::SourceIntegrity);
        }
        let entry = record
            .entries
            .iter()
            .find(|entry| entry.index == acceptance.index)
            .ok_or(Error::SourceIntegrity)?;
        let child = store
            .source(&acceptance.child_revision)?
            .ok_or(Error::SourceIntegrity)?;
        if child.source.endpoint() != entry.endpoint
            || child.source.network() != parent.source.network()
            || child.source.redirects() != parent.source.redirects()
        {
            return Err(Error::SourceIntegrity);
        }
    }
    Ok(())
}

fn read_playlist(connection: &rusqlite::Connection, id: &str) -> Result<Option<PlaylistRecord>> {
    let raw = connection
        .query_row(
            "SELECT parent_revision, state, started_ms, completed_ms, document_sha256, final_origin, failure, entry_count FROM playlist_resolves WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((parent, state, started, completed, hash, origin, failure, count)) = raw else {
        return Ok(None);
    };
    if started < 0
        || completed.is_some_and(|completed| completed < started)
        || !(0..=32).contains(&count)
    {
        return Err(Error::SourceIntegrity);
    }
    let entries = read_entries(connection, id)?;
    let acceptances = read_acceptances(connection, id)?;
    if i64::try_from(entries.len()).is_ok_and(|len| len != count) {
        return Err(Error::SourceIntegrity);
    }
    Ok(Some(PlaylistRecord {
        id: id.to_owned(),
        parent_revision: parent,
        state,
        started_ms: started,
        completed_ms: completed,
        document_sha256: hash,
        final_origin: origin,
        failure,
        entries,
        acceptances,
    }))
}

fn read_entries(connection: &rusqlite::Connection, id: &str) -> Result<Vec<PlaylistEntryRecord>> {
    let mut query = connection.prepare(
        "SELECT entry_index, endpoint, origin FROM playlist_entries WHERE resolve_id = ?1 ORDER BY entry_index",
    )?;
    query
        .query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .map(|row| {
            let (index, endpoint, origin) = row?;
            let index = u32::try_from(index).map_err(|_| Error::SourceIntegrity)?;
            Ok(PlaylistEntryRecord {
                index,
                endpoint,
                origin,
            })
        })
        .collect()
}

fn read_acceptances(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Vec<PlaylistAcceptanceRecord>> {
    let mut query = connection.prepare(
        "SELECT entry_index, child_revision, parent_revision, document_sha256 FROM playlist_acceptances WHERE resolve_id = ?1 ORDER BY entry_index",
    )?;
    query
        .query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .map(|row| {
            let (index, child, parent, hash) = row?;
            Ok(PlaylistAcceptanceRecord {
                index: u32::try_from(index).map_err(|_| Error::SourceIntegrity)?,
                child_revision: child,
                parent_revision: parent,
                document_sha256: hash,
            })
        })
        .collect()
}

fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(Error::SourceIntegrity)
    }
}

fn validate_origin(origin: &str) -> Result<()> {
    if origin.len() > 2048
        || origin.contains('?')
        || origin.contains('\\')
        || origin.chars().any(crate::sources::unsafe_display)
    {
        return Err(Error::SourceIntegrity);
    }
    let url = reqwest::Url::parse(&format!("{origin}/")).map_err(|_| Error::SourceIntegrity)?;
    if url.origin().ascii_serialization() != origin || url.path() != "/" || url.query().is_some() {
        return Err(Error::SourceIntegrity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        discovery::radio_browser,
        sources::{NetworkScope, RedirectPolicy, playlist::ResolvedEntry},
        storage::SCHEMA_VERSION,
    };

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn parent() -> Result<HttpSource> {
        HttpSource::new(
            "Parent",
            "http://127.0.0.1:9/secret/list.m3u?token=hidden",
            NetworkScope::PinnedAddress {
                address: std::net::Ipv4Addr::LOCALHOST.into(),
            },
        )
    }

    fn resolved() -> ResolvedPlaylist {
        ResolvedPlaylist {
            document_sha256: "ab".repeat(32),
            final_origin: "http://127.0.0.1:9".into(),
            entries: vec![ResolvedEntry {
                endpoint: "http://127.0.0.1:9/secret/live/main".into(),
                origin: "http://127.0.0.1:9".into(),
            }],
        }
    }

    fn endpoint(store: &Store, id: &str) -> Result<String> {
        Ok(store.connection.query_row(
            "SELECT endpoint FROM source_revisions WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?)
    }

    #[test]
    fn accept_keeps_the_parent_and_replay_does_not_register_again() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        let source = parent()?;
        store.register_source("parent:v1", &source)?;
        let before = endpoint(&store, "parent:v1")?;
        let created = store
            .source("parent:v1")?
            .ok_or("missing parent")?
            .created_ms;
        assert!(store.begin_playlist("one", "parent:v1")?);
        store.finish_playlist("one", &resolved())?;
        let admission = store.accept_playlist_entry("one", 0, "child:v1", "Chosen")?;
        assert!(admission.newly_created);
        assert_eq!(endpoint(&store, "parent:v1")?, before);
        assert_eq!(
            store
                .source("parent:v1")?
                .ok_or("missing parent")?
                .created_ms,
            created
        );
        assert_eq!(
            endpoint(&store, "child:v1")?,
            "http://127.0.0.1:9/secret/live/main"
        );
        let replay = store.accept_playlist_entry("one", 0, "child:v1", "Chosen")?;
        assert!(!replay.newly_created);
        let count: i64 =
            store
                .connection
                .query_row("SELECT count(*) FROM source_revisions", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(count, 2);
        assert!(!store.begin_playlist("one", "parent:v1")?);
        assert!(
            store
                .accept_playlist_entry("one", 0, "other:v1", "Chosen")
                .is_err()
        );
        assert!(store.begin_playlist("one", "other:v1").is_err());
        let path = directory.path().join("catalog.sqlite3");
        drop(store);
        let store = Store::open(&path)?;
        assert_eq!(store.playlist("one")?.entries.len(), 1);
        assert_eq!(store.playlist("one")?.acceptances.len(), 1);
        Ok(())
    }

    #[test]
    fn hls_failure_and_directory_flag_store_no_entries() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        store.register_source("parent:v1", &parent()?)?;
        assert!(store.begin_playlist("marker", "parent:v1")?);
        store.fail_playlist(
            "marker",
            &Error::InvalidInput("HLS playlist is not accepted"),
        )?;
        let failed = store.playlist("marker")?;
        assert_eq!(failed.state, "failed");
        assert!(failed.entries.is_empty());
        assert!(failed.acceptances.is_empty());
        assert!(!store.begin_playlist("marker", "parent:v1")?);

        let body = serde_json::json!([{
            "stationuuid": "12345678-1234-1234-1234-123456789abc",
            "name": "HLS Station",
            "url": "https://stream.example/live.m3u",
            "hls": 1,
            "countrycode": "US"
        }]);
        let batch = radio_browser::parse(
            &serde_json::to_vec(&body)?,
            1,
            "https://directory.example".into(),
        )?;
        let request = crate::discovery::RefreshRequest {
            filter: crate::discovery::StationFilter::default(),
            limit: 1,
            offset: 0,
            mirror: None,
            network: NetworkScope::PublicInternet {},
        };
        assert!(store.begin_refresh("dir", &request)?);
        store.finish_refresh("dir", batch)?;
        store.add_station_source(
            "12345678-1234-1234-1234-123456789abc",
            "hls:v1",
            RedirectPolicy::Deny,
        )?;
        assert!(store.linked_station_is_hls("hls:v1")?);
        assert!(!store.linked_station_is_hls("parent:v1")?);
        assert!(store.begin_playlist("flag", "hls:v1")?);
        store.fail_playlist(
            "flag",
            &Error::InvalidInput("directory marks this source as HLS"),
        )?;
        assert!(store.playlist("flag")?.entries.is_empty());
        Ok(())
    }

    #[test]
    fn playlist_migration_preserves_v7_and_rolls_back_conflicts() -> TestResult {
        let directory = tempfile::tempdir()?;
        for fail in [false, true] {
            let path = directory
                .path()
                .join(if fail { "bad.sqlite3" } else { "good.sqlite3" });
            let connection = rusqlite::Connection::open(&path)?;
            for migration in [
                include_str!("001-foundation.sql"),
                include_str!("002-captures.sql"),
                include_str!("003-sources.sql"),
                include_str!("004-dvr.sql"),
                include_str!("005-discovery.sql"),
                include_str!("006-redirects.sql"),
                include_str!("007-favorites.sql"),
            ] {
                connection.execute_batch(migration)?;
            }
            connection.execute(
                "INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms, redirect_policy) VALUES ('parent:v1', 'http_audio', 'Parent', 'https://radio.example/list.m3u', 'public_internet', 1, 'deny')",
                [],
            )?;
            if fail {
                connection.execute("CREATE TABLE playlist_resolves(existing TEXT)", [])?;
            }
            let result = Store::open(&path);
            let version: u32 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if fail {
                assert!(result.is_err());
                assert_eq!(version, 7);
            } else {
                let store = result?;
                assert_eq!(version, SCHEMA_VERSION);
                assert_eq!(
                    store
                        .source("parent:v1")?
                        .ok_or("missing")?
                        .source
                        .endpoint(),
                    "https://radio.example/list.m3u"
                );
            }
        }
        Ok(())
    }
}
