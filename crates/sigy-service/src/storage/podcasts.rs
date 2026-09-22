//! Local podcast subscriptions. No DNS, fetch, capture, or audio revision.

use std::fmt;

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{Store, now_ms, validate_key};
use crate::{
    Error, Result,
    sources::{HttpSource, NetworkScope, RedirectPolicy},
};

/// Stopped rows remain, so this bounds every subscription ever stored.
pub const MAX_PODCAST_SUBSCRIPTIONS: u32 = 1024;
pub const MAX_PODCAST_PAGE: u32 = 32;

/// Whether a later feed poll may run. Unsubscribe only moves active to stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodcastPolls {
    Active,
    Stopped,
}

impl PodcastPolls {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stopped => "stopped",
        }
    }
}

impl fmt::Display for PodcastPolls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One stored subscription. Ordinary debug output omits the feed path and query.
#[derive(Clone, PartialEq, Eq)]
pub struct PodcastSubscription {
    pub id: String,
    endpoint: String,
    pub origin: String,
    pub network: NetworkScope,
    pub redirects: RedirectPolicy,
    pub polls: PodcastPolls,
    pub created_ms: i64,
}

impl fmt::Debug for PodcastSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PodcastSubscription")
            .field("id", &self.id)
            .field("origin", &self.origin)
            .field("network", &self.network)
            .field("redirects", &self.redirects)
            .field("polls", &self.polls)
            .field("created_ms", &self.created_ms)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodcastAdmission {
    pub subscription: PodcastSubscription,
    pub newly_created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodcastStop {
    pub subscription: PodcastSubscription,
    pub newly_stopped: bool,
}

impl Store {
    /// Stores one feed subscription. A matching replay does not change it.
    ///
    /// The feed URL, network scope, address pin, and redirect policy are immutable.
    /// Validation is syntactic plus literal-address policy. This does not resolve
    /// DNS, fetch the feed, register an audio revision, or create a capture.
    /// A stopped subscription stays stopped. Another id may subscribe to that
    /// feed only after the active row is stopped.
    ///
    /// # Errors
    /// Rejects malformed keys and URLs, denied destinations, conflicting replays,
    /// a second active subscription for the same feed URL, a full catalog, or
    /// write failures.
    pub fn subscribe_podcast(
        &mut self,
        id: &str,
        url: &str,
        network: NetworkScope,
        redirects: RedirectPolicy,
    ) -> Result<PodcastAdmission> {
        let source = feed_authority(url, network, redirects)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let admission = subscribe_in(&tx, id, &source)?;
        tx.commit()?;
        Ok(admission)
    }

    /// Stops future polls for one subscription. The row stays, and nothing else
    /// in the catalog is removed. Repeating this has no further effect.
    ///
    /// # Errors
    /// Rejects a malformed or unknown subscription id, or a failed write.
    pub fn unsubscribe_podcast(&mut self, id: &str) -> Result<PodcastStop> {
        validate_key(id, "podcast subscription ID")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(current) = read_subscription(&tx, id)? else {
            return Err(Error::NotFound);
        };
        if current.polls == PodcastPolls::Stopped {
            tx.commit()?;
            return Ok(PodcastStop {
                subscription: current,
                newly_stopped: false,
            });
        }
        let changed = tx.execute(
            "UPDATE podcast_subscriptions SET polls = 'stopped' WHERE id = ?1 AND polls = 'active'",
            [id],
        )?;
        if changed != 1 {
            return Err(Error::PodcastIntegrity);
        }
        let subscription = read_subscription(&tx, id)?.ok_or(Error::PodcastIntegrity)?;
        if subscription.polls != PodcastPolls::Stopped {
            return Err(Error::PodcastIntegrity);
        }
        tx.commit()?;
        Ok(PodcastStop {
            subscription,
            newly_stopped: true,
        })
    }

    /// # Errors
    /// Rejects a malformed id, corrupt authority, or a catalog read error.
    pub fn podcast_subscription(&self, id: &str) -> Result<Option<PodcastSubscription>> {
        validate_key(id, "podcast subscription ID")?;
        read_subscription(&self.connection, id)
    }

    /// # Errors
    /// Rejects a malformed cursor or a page size outside 1..=32.
    pub fn podcast_subscriptions(
        &self,
        after_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<PodcastSubscription>> {
        if !(1..=MAX_PODCAST_PAGE).contains(&limit) {
            return Err(Error::InvalidInput("podcast page size"));
        }
        if let Some(id) = after_id {
            validate_key(id, "podcast cursor")?;
        }
        let mut query = self
            .connection
            .prepare("SELECT id FROM podcast_subscriptions WHERE id > ?1 ORDER BY id LIMIT ?2")?;
        let ids = query
            .query_map(params![after_id.unwrap_or(""), limit], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| {
                self.podcast_subscription(&id)?
                    .ok_or(Error::PodcastIntegrity)
            })
            .collect()
    }

    /// # Errors
    /// Rejects a subscription that no longer passes the current trust boundary.
    pub fn audit_podcasts(&self) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let mut query = tx.prepare("SELECT id FROM podcast_subscriptions ORDER BY id")?;
        let ids = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(query);
        for id in ids {
            read_subscription(&tx, &id)?.ok_or(Error::PodcastIntegrity)?;
        }
        Ok(())
    }
}

fn feed_authority(
    url: &str,
    network: NetworkScope,
    redirects: RedirectPolicy,
) -> Result<HttpSource> {
    let source = HttpSource::new("feed", url, network).map_err(|error| match error {
        Error::InvalidInput(_) => Error::InvalidInput("feed URL"),
        other => other,
    })?;
    source.with_redirects(redirects)
}

fn subscribe_in(
    tx: &rusqlite::Connection,
    id: &str,
    source: &HttpSource,
) -> Result<PodcastAdmission> {
    validate_key(id, "podcast subscription ID")?;
    if let Some(existing) = read_subscription(tx, id)? {
        if !same_authority(&existing, source) {
            return Err(Error::IdempotencyConflict);
        }
        return Ok(PodcastAdmission {
            subscription: existing,
            newly_created: false,
        });
    }
    let count: u32 = tx.query_row("SELECT count(*) FROM podcast_subscriptions", [], |row| {
        row.get(0)
    })?;
    if count >= MAX_PODCAST_SUBSCRIPTIONS {
        return Err(Error::PodcastCapacity);
    }
    let active: Option<String> = tx
        .query_row(
            "SELECT id FROM podcast_subscriptions WHERE feed_url = ?1 AND polls = 'active'",
            [source.endpoint()],
            |row| row.get(0),
        )
        .optional()?;
    if active.is_some() {
        return Err(Error::InvalidInput(
            "an active subscription already stores this feed URL",
        ));
    }
    let (scope, address) = match source.network() {
        NetworkScope::PublicInternet {} => ("public_internet", None),
        NetworkScope::PinnedAddress { address } => ("pinned_address", Some(address.to_string())),
    };
    let created_ms = now_ms()?;
    tx.execute(
        "INSERT INTO podcast_subscriptions(id, feed_url, network_scope, pinned_address, redirect_policy, created_ms, polls) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active')",
        params![
            id,
            source.endpoint(),
            scope,
            address,
            source.redirects().to_string(),
            created_ms
        ],
    )?;
    let subscription = read_subscription(tx, id)?.ok_or(Error::PodcastIntegrity)?;
    Ok(PodcastAdmission {
        subscription,
        newly_created: true,
    })
}

fn same_authority(existing: &PodcastSubscription, source: &HttpSource) -> bool {
    existing.endpoint == source.endpoint()
        && existing.network == source.network()
        && existing.redirects == source.redirects()
}

fn read_subscription(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<PodcastSubscription>> {
    let raw = connection
        .query_row(
            "SELECT feed_url, network_scope, pinned_address, redirect_policy, polls, created_ms FROM podcast_subscriptions WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((endpoint, scope, pinned, redirects, polls, created_ms)) = raw else {
        return Ok(None);
    };
    validate_key(id, "podcast subscription ID").map_err(|_| Error::PodcastIntegrity)?;
    if created_ms < 0 {
        return Err(Error::PodcastIntegrity);
    }
    let network = network_from(&scope, pinned.as_deref())?;
    let source = feed_authority(&endpoint, network, redirects.parse()?)
        .map_err(|_| Error::PodcastIntegrity)?;
    if source.endpoint() != endpoint {
        return Err(Error::PodcastIntegrity);
    }
    Ok(Some(PodcastSubscription {
        id: id.into(),
        endpoint,
        origin: source.origin(),
        network: source.network(),
        redirects: source.redirects(),
        polls: parse_polls(&polls)?,
        created_ms,
    }))
}

fn network_from(scope: &str, pinned: Option<&str>) -> Result<NetworkScope> {
    match (scope, pinned) {
        ("public_internet", None) => Ok(NetworkScope::PublicInternet {}),
        ("pinned_address", Some(value)) => {
            let address = value.parse().map_err(|_| Error::PodcastIntegrity)?;
            let network = NetworkScope::PinnedAddress { address };
            if address.to_string() != value {
                return Err(Error::PodcastIntegrity);
            }
            Ok(network)
        }
        _ => Err(Error::PodcastIntegrity),
    }
}

fn parse_polls(value: &str) -> Result<PodcastPolls> {
    match value {
        "active" => Ok(PodcastPolls::Active),
        "stopped" => Ok(PodcastPolls::Stopped),
        _ => Err(Error::PodcastIntegrity),
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PODCAST_SUBSCRIPTIONS, PodcastPolls};
    use crate::{
        Error,
        sources::{HttpSource, NetworkScope, RedirectPolicy},
        storage::{SCHEMA_VERSION, Store, captures::CapturePlan},
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const PUBLIC: NetworkScope = NetworkScope::PublicInternet {};

    fn open() -> Result<(tempfile::TempDir, Store), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        Ok((directory, store))
    }

    fn count(store: &Store, sql: &str) -> Result<i64, rusqlite::Error> {
        store.connection.query_row(sql, [], |row| row.get(0))
    }

    #[test]
    fn subscribe_stores_authority_without_dns_capture_or_audio_revision() -> TestResult {
        let (_directory, mut store) = open()?;
        let source = HttpSource::new("Radio", "https://example.com/audio", PUBLIC)?;
        store.register_source("shared:v1", &source)?;
        let plan = CapturePlan::new("shared:v1", 0, 60_000, 1_024)?;
        store.create_capture("keep-me", &plan)?;
        store.connection.execute(
            "INSERT INTO directory_refreshes(id, request_json, state, started_ms) VALUES ('kept-refresh', '{}', 'completed', 1)",
            [],
        )?;
        store.connection.execute(
            "INSERT INTO directory_stations(provider, id, metadata_json, endpoint, name_folded, country, languages_folded, tags_folded, refresh_id) VALUES ('radio_browser', 'station-1', '{}', 'https://radio.example/a', 'station', 'CA', '[]', '[]', 'kept-refresh')",
            [],
        )?;
        store.connection.execute(
            "INSERT INTO station_favorites(provider, station_id) VALUES ('radio_browser', 'station-1')",
            [],
        )?;
        let before = (
            count(&store, "SELECT count(*) FROM capture_jobs")?,
            count(&store, "SELECT count(*) FROM source_revisions")?,
            count(&store, "SELECT count(*) FROM station_favorites")?,
        );
        let url = "https://Unresolved.INVALID/secret/feed.xml?token=hidden";
        let admission = store.subscribe_podcast("show:v1", url, PUBLIC, RedirectPolicy::Deny)?;
        assert!(admission.newly_created);
        assert_eq!(admission.subscription.polls, PodcastPolls::Active);
        assert_eq!(admission.subscription.origin, "https://unresolved.invalid");
        assert_eq!(
            admission.subscription.endpoint,
            "https://unresolved.invalid/secret/feed.xml?token=hidden"
        );
        let debug = format!("{:?}", admission.subscription);
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("hidden"));
        let replay = store.subscribe_podcast("show:v1", url, PUBLIC, RedirectPolicy::Deny)?;
        assert!(!replay.newly_created);
        assert_eq!(
            replay.subscription.created_ms,
            admission.subscription.created_ms
        );
        assert_eq!(
            (
                count(&store, "SELECT count(*) FROM capture_jobs")?,
                count(&store, "SELECT count(*) FROM source_revisions")?,
                count(&store, "SELECT count(*) FROM station_favorites")?,
                count(&store, "SELECT count(*) FROM podcast_subscriptions")?,
            ),
            (before.0, before.1, before.2, 1)
        );
        assert!(store.source("shared:v1")?.is_some());
        assert!(store.capture("keep-me")?.is_some());
        assert!(
            store
                .subscribe_podcast(
                    "show:v1",
                    "https://unresolved.invalid/other.xml",
                    PUBLIC,
                    RedirectPolicy::Deny,
                )
                .is_err_and(|error| matches!(error, Error::IdempotencyConflict))
        );
        assert!(
            store
                .subscribe_podcast("show:v1", url, PUBLIC, RedirectPolicy::Public)
                .is_err_and(|error| matches!(error, Error::IdempotencyConflict))
        );
        Ok(())
    }

    #[test]
    fn unsubscribe_stops_polls_retains_the_row_and_deletes_nothing() -> TestResult {
        let (directory, mut store) = open()?;
        let source = HttpSource::new("Radio", "https://example.com/audio", PUBLIC)?;
        store.register_source("radio:v1", &source)?;
        store.create_capture("keep-me", &CapturePlan::new("radio:v1", 0, 60_000, 1_024)?)?;
        let url = "https://show.example/feed.xml?token=hidden";
        store.subscribe_podcast("show:v1", url, PUBLIC, RedirectPolicy::SameOrigin)?;
        assert!(
            store
                .connection
                .execute(
                    "UPDATE podcast_subscriptions SET feed_url = 'https://other.example/feed.xml' WHERE id = 'show:v1'",
                    [],
                )
                .is_err()
        );
        assert!(
            store
                .connection
                .execute("DELETE FROM podcast_subscriptions WHERE id = 'show:v1'", [])
                .is_err()
        );
        let stopped = store.unsubscribe_podcast("show:v1")?;
        assert!(stopped.newly_stopped);
        assert_eq!(stopped.subscription.polls, PodcastPolls::Stopped);
        assert_eq!(stopped.subscription.endpoint, url);
        let again = store.unsubscribe_podcast("show:v1")?;
        assert!(!again.newly_stopped);
        assert!(
            store
                .connection
                .execute(
                    "UPDATE podcast_subscriptions SET polls = 'active' WHERE id = 'show:v1'",
                    [],
                )
                .is_err()
        );
        let replay = store.subscribe_podcast("show:v1", url, PUBLIC, RedirectPolicy::SameOrigin)?;
        assert!(!replay.newly_created);
        assert_eq!(replay.subscription.polls, PodcastPolls::Stopped);
        let replacement =
            store.subscribe_podcast("show:v2", url, PUBLIC, RedirectPolicy::SameOrigin)?;
        assert!(replacement.newly_created);
        assert_eq!(replacement.subscription.polls, PodcastPolls::Active);
        assert!(
            store
                .subscribe_podcast("show:v3", url, PUBLIC, RedirectPolicy::Deny)
                .is_err_and(|error| matches!(error, Error::InvalidInput(_)))
        );
        assert_eq!(
            count(&store, "SELECT count(*) FROM podcast_subscriptions")?,
            2
        );
        assert_eq!(count(&store, "SELECT count(*) FROM source_revisions")?, 1);
        assert_eq!(count(&store, "SELECT count(*) FROM capture_jobs")?, 1);
        assert_eq!(count(&store, "SELECT count(*) FROM station_favorites")?, 0);
        assert!(matches!(
            store.unsubscribe_podcast("missing"),
            Err(Error::NotFound)
        ));
        drop(store);
        let store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        assert_eq!(
            store
                .podcast_subscription("show:v1")?
                .ok_or("missing")?
                .polls,
            PodcastPolls::Stopped
        );
        assert_eq!(
            store
                .podcast_subscription("show:v2")?
                .ok_or("missing")?
                .polls,
            PodcastPolls::Active
        );
        Ok(())
    }

    #[test]
    fn denied_urls_store_nothing_and_lists_are_bounded() -> TestResult {
        let (_directory, mut store) = open()?;
        for url in [
            "https://user:secret@show.example/feed.xml",
            "file:///tmp/feed.xml",
            "http://127.0.0.1/feed.xml",
            "https://show.example/feed.xml#part",
        ] {
            assert!(
                store
                    .subscribe_podcast("bad", url, PUBLIC, RedirectPolicy::Deny)
                    .is_err(),
                "{url}"
            );
        }
        let pinned = NetworkScope::PinnedAddress {
            address: "127.0.0.1".parse()?,
        };
        assert!(matches!(
            store.subscribe_podcast(
                "local",
                "http://127.0.0.1/secret/feed.xml?token=hidden",
                pinned,
                RedirectPolicy::Public,
            ),
            Err(Error::InvalidInput(_))
        ));
        let stored = store.subscribe_podcast(
            "local",
            "http://127.0.0.1/secret/feed.xml?token=hidden",
            pinned,
            RedirectPolicy::Deny,
        )?;
        assert_eq!(stored.subscription.origin, "http://127.0.0.1");
        assert!(!format!("{stored:?}").contains("hidden"));
        store.subscribe_podcast(
            "public",
            "https://show.example/feed.xml",
            PUBLIC,
            RedirectPolicy::Deny,
        )?;
        let page = store.podcast_subscriptions(None, 1)?;
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, "local");
        let next = store.podcast_subscriptions(Some("local"), 1)?;
        assert_eq!(next[0].id, "public");
        assert!(store.podcast_subscriptions(None, 33).is_err());
        assert!(store.podcast_subscription("missing")?.is_none());
        assert_eq!(count(&store, "SELECT count(*) FROM capture_jobs")?, 0);
        assert_eq!(count(&store, "SELECT count(*) FROM source_revisions")?, 0);
        Ok(())
    }

    #[test]
    fn subscription_capacity_is_exact() -> TestResult {
        let (_directory, mut store) = open()?;
        for index in 0..MAX_PODCAST_SUBSCRIPTIONS {
            let id = format!("feed-{index}");
            let url = format!("https://feed-{index}.example/podcast.xml");
            store.subscribe_podcast(&id, &url, PUBLIC, RedirectPolicy::Deny)?;
        }
        assert!(matches!(
            store.subscribe_podcast(
                "feed-more",
                "https://feed-more.example/podcast.xml",
                PUBLIC,
                RedirectPolicy::Deny,
            ),
            Err(Error::PodcastCapacity)
        ));
        assert_eq!(
            count(&store, "SELECT count(*) FROM podcast_subscriptions")?,
            i64::from(MAX_PODCAST_SUBSCRIPTIONS)
        );
        Ok(())
    }

    #[test]
    fn corrupt_authority_fails_on_reopen() -> TestResult {
        let (directory, store) = open()?;
        store.connection.execute(
            "INSERT INTO podcast_subscriptions(id, feed_url, network_scope, pinned_address, redirect_policy, created_ms, polls) VALUES ('bad', 'not a url', 'public_internet', NULL, 'deny', 1, 'active')",
            [],
        )?;
        drop(store);
        assert!(Store::open(&directory.path().join("catalog.sqlite3")).is_err());
        Ok(())
    }

    #[test]
    fn migration_preserves_v11_and_rolls_back_conflicts() -> TestResult {
        let directory = tempfile::tempdir()?;
        for fail in [false, true] {
            let path = directory
                .path()
                .join(if fail { "bad.sqlite3" } else { "good.sqlite3" });
            let connection = rusqlite::Connection::open(&path)?;
            for sql in [
                include_str!("001-foundation.sql"),
                include_str!("002-captures.sql"),
                include_str!("003-sources.sql"),
                include_str!("004-dvr.sql"),
                include_str!("005-discovery.sql"),
                include_str!("006-redirects.sql"),
                include_str!("007-favorites.sql"),
                include_str!("008-playlists.sql"),
                include_str!("009-clicks.sql"),
                include_str!("010-listens.sql"),
                include_str!("011-icy.sql"),
            ] {
                connection.execute_batch(sql)?;
            }
            connection.execute("UPDATE budgets SET limit_micros = 4242", [])?;
            connection.execute(
                "INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms, redirect_policy) VALUES ('radio:v1', 'http_audio', 'Radio', 'https://example.com/audio', 'public_internet', 1, 'deny')",
                [],
            )?;
            if fail {
                connection.execute("CREATE TABLE podcast_subscriptions(existing TEXT)", [])?;
            }
            let opened = Store::open(&path);
            let version: u32 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            let feed_column: i64 = connection.query_row(
                "SELECT count(*) FROM pragma_table_info('podcast_subscriptions') WHERE name = 'feed_url'",
                [],
                |row| row.get(0),
            )?;
            if fail {
                assert!(opened.is_err());
                assert_eq!((version, feed_column), (11, 0));
                let limit: i64 =
                    connection
                        .query_row("SELECT limit_micros FROM budgets", [], |row| row.get(0))?;
                assert_eq!(limit, 4242);
            } else {
                let store = opened?;
                assert_eq!((version, feed_column), (SCHEMA_VERSION, 1));
                assert_eq!(store.budget("global")?.limit().micros(), 4242);
                assert_eq!(
                    store
                        .source("radio:v1")?
                        .ok_or("source missing")?
                        .source
                        .name(),
                    "Radio"
                );
                assert_eq!(
                    count(&store, "SELECT count(*) FROM podcast_subscriptions")?,
                    0
                );
                assert_eq!(count(&store, "SELECT count(*) FROM station_favorites")?, 0);
            }
        }
        Ok(())
    }
}
