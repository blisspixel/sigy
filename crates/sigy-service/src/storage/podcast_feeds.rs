//! RSS snapshot and episode rows. Omission does not delete an episode.

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{
    Store,
    dvr::{self, RecordingProfile, Retention},
    now_ms, validate_key,
};
use crate::{
    Error, Result,
    podcast::{AssetRef, EpisodeIdentity, FeedCommit, ParsedEpisode},
    sources::{HttpSource, unsafe_display},
    storage::captures::CaptureJob,
};

const MAX_REFRESH_HISTORY: u32 = 4_096;
const MAX_RETAINED_EPISODES: i64 = 2_000;
const REFRESH_INTERVAL_MS: i64 = 2_000;
pub(crate) const MAX_EPISODE_PAGE: u32 = 32;

/// Whether the episode key came from the publisher or from enclosure plus time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodcastIdentityKind {
    PublisherGuid,
    DerivedEnclosure,
}

impl PodcastIdentityKind {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "publisher_guid" => Ok(Self::PublisherGuid),
            "derived_enclosure" => Ok(Self::DerivedEnclosure),
            _ => Err(Error::PodcastIntegrity),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PodcastRefreshStatus {
    pub id: String,
    pub subscription_id: String,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub committed_items: u32,
    pub truncated: bool,
    pub live_count: u32,
    pub skipped_items: u32,
    pub document_sha256: Option<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PodcastCacheHealth {
    pub subscriptions: i64,
    pub missing_snapshots: i64,
    pub stale_snapshots: i64,
    pub newest_observed_ms: Option<i64>,
}

pub(crate) struct PodcastSnapshotRecord {
    pub refresh_id: String,
    pub observed_ms: i64,
    pub document_sha256: String,
    pub committed_items: u32,
    pub truncated: bool,
    pub live_count: u32,
}

/// Ordinary episode fields. Enclosure and asset URLs stay in the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodcastEpisodeRecord {
    pub id: String,
    pub identity: PodcastIdentityKind,
    pub title: Option<String>,
    pub published_ms: Option<i64>,
    pub enclosure: bool,
    pub enclosure_type: Option<String>,
    pub transcripts: u32,
    pub chapters: u32,
    pub in_latest: bool,
}

impl Store {
    pub(crate) fn begin_podcast_refresh(
        &mut self,
        id: &str,
        subscription_id: &str,
    ) -> Result<bool> {
        self.begin_podcast_refresh_at(id, subscription_id, now_ms()?)
    }

    fn begin_podcast_refresh_at(
        &mut self,
        id: &str,
        subscription_id: &str,
        now: i64,
    ) -> Result<bool> {
        validate_key(id, "podcast refresh ID")?;
        validate_key(subscription_id, "podcast subscription ID")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT subscription_id FROM podcast_refreshes WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != subscription_id {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(false);
        }
        let polls: Option<String> = tx
            .query_row(
                "SELECT polls FROM podcast_subscriptions WHERE id = ?1",
                [subscription_id],
                |row| row.get(0),
            )
            .optional()?;
        match polls.as_deref() {
            Some("active") => {}
            Some("stopped") => {
                return Err(Error::InvalidInput("podcast polling is stopped"));
            }
            Some(_) => return Err(Error::PodcastIntegrity),
            None => return Err(Error::NotFound),
        }
        let (count, active, last): (u32, u32, Option<i64>) = tx.query_row(
            "SELECT count(*), coalesce(sum(state = 'running'), 0), max(started_ms) FROM podcast_refreshes",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if count >= MAX_REFRESH_HISTORY || active != 0 {
            return Err(Error::InvalidInput("podcast refresh capacity reached"));
        }
        if last.is_some_and(|last| now < last.saturating_add(REFRESH_INTERVAL_MS)) {
            return Err(Error::InvalidInput(
                "podcast refresh interval is at least two seconds",
            ));
        }
        tx.execute(
            "INSERT INTO podcast_refreshes(id, subscription_id, state, started_ms) VALUES (?1, ?2, 'running', ?3)",
            params![id, subscription_id, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// # Errors
    /// Rejects a malformed id, an unknown refresh, or a corrupt row.
    pub(crate) fn podcast_refresh(&self, id: &str) -> Result<PodcastRefreshStatus> {
        validate_key(id, "podcast refresh ID")?;
        let row = self.connection.query_row(
            "SELECT subscription_id, state, started_ms, completed_ms, committed_items, truncated, live_count, skipped_items, document_sha256, failure FROM podcast_refreshes WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        ).optional()?.ok_or(Error::NotFound)?;
        refresh_status(id, row)
    }

    pub(crate) fn podcast_cache_health(&self, fresh_ms: i64) -> Result<PodcastCacheHealth> {
        let cutoff = super::now_ms()?.saturating_sub(fresh_ms);
        let subscriptions: i64 =
            self.connection
                .query_row("SELECT count(*) FROM podcast_subscriptions", [], |row| {
                    row.get(0)
                })?;
        let missing_snapshots: i64 = self.connection.query_row(
            "SELECT count(*) FROM podcast_subscriptions s LEFT JOIN podcast_snapshots p ON p.subscription_id = s.id WHERE p.subscription_id IS NULL",
            [],
            |row| row.get(0),
        )?;
        let (stale_snapshots, newest_observed_ms): (i64, Option<i64>) = self.connection.query_row(
            "SELECT coalesce(sum(observed_ms < ?1), 0), max(observed_ms) FROM podcast_snapshots",
            [cutoff],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(PodcastCacheHealth {
            subscriptions,
            missing_snapshots,
            stale_snapshots,
            newest_observed_ms,
        })
    }

    /// # Errors
    /// Rejects a malformed subscription id or a corrupt snapshot.
    pub(crate) fn podcast_snapshot(
        &self,
        subscription_id: &str,
    ) -> Result<Option<PodcastSnapshotRecord>> {
        validate_key(subscription_id, "podcast subscription ID")?;
        let row = self.connection.query_row(
            "SELECT refresh_id, observed_ms, document_sha256, committed_items, truncated, live_count FROM podcast_snapshots WHERE subscription_id = ?1",
            [subscription_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        ).optional()?;
        row.map(snapshot_record).transpose()
    }

    /// # Errors
    /// Rejects an unknown subscription, a bad cursor, or a page outside 1..=32.
    pub fn podcast_episodes(
        &self,
        subscription_id: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<Vec<PodcastEpisodeRecord>> {
        validate_key(subscription_id, "podcast subscription ID")?;
        if self.podcast_subscription(subscription_id)?.is_none() {
            return Err(Error::NotFound);
        }
        if !(1..=MAX_EPISODE_PAGE).contains(&limit) {
            return Err(Error::InvalidInput("podcast episode page size"));
        }
        let after_sequence = if let Some(episode_id) = after {
            Some(self.episode_sequence(subscription_id, episode_id)?)
        } else {
            None
        };
        let mut query = self.connection.prepare(
            "SELECT episode_id, identity_kind, guid, title, published_ms, enclosure_url, enclosure_type, assets_json, in_latest FROM podcast_episodes WHERE subscription_id = ?1 AND sequence > ?2 ORDER BY sequence LIMIT ?3",
        )?;
        let rows = query.query_map(
            params![subscription_id, after_sequence.unwrap_or(-1), limit],
            read_episode_row,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(episode_record)
            .collect()
    }

    pub(crate) fn finish_podcast_refresh(&mut self, id: &str, commit: &FeedCommit) -> Result<()> {
        if commit.parsed.episodes.len() > 500 || !hex64(&commit.sha256) {
            return Err(Error::InvalidInput("feed item limit"));
        }
        let status = self.podcast_refresh(id)?;
        if status.state != "running" {
            return Err(Error::RequestState);
        }
        let now = now_ms()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE podcast_episodes SET in_latest = 0 WHERE subscription_id = ?1",
            [&status.subscription_id],
        )?;
        let mut stored = 0_u32;
        let mut truncated = commit.parsed.truncated;
        for episode in &commit.parsed.episodes {
            if upsert_episode(&tx, &status.subscription_id, id, now, episode)? {
                stored = stored
                    .checked_add(1)
                    .ok_or(Error::InvalidInput("feed item limit"))?;
            } else {
                truncated = true;
            }
        }
        let live = i64::from(commit.parsed.live_count);
        let skipped = i64::from(commit.parsed.skipped);
        if tx.execute(
            "UPDATE podcast_refreshes SET state = 'completed', completed_ms = ?2, committed_items = ?3, truncated = ?4, live_count = ?5, skipped_items = ?6, document_sha256 = ?7 WHERE id = ?1 AND state = 'running'",
            params![id, now, stored, i64::from(truncated), live, skipped, commit.sha256],
        )? != 1 {
            return Err(Error::RequestState);
        }
        tx.execute(
            "INSERT INTO podcast_snapshots(subscription_id, refresh_id, observed_ms, document_sha256, committed_items, truncated, live_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(subscription_id) DO UPDATE SET refresh_id = excluded.refresh_id, observed_ms = excluded.observed_ms, document_sha256 = excluded.document_sha256, committed_items = excluded.committed_items, truncated = excluded.truncated, live_count = excluded.live_count",
            params![status.subscription_id, id, now, commit.sha256, stored, i64::from(truncated), live],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn fail_podcast_refresh(&mut self, id: &str, error: &Error) -> Result<()> {
        let detail = failure_detail(error);
        if self.connection.execute(
            "UPDATE podcast_refreshes SET state = 'failed', failure = ?2 WHERE id = ?1 AND state = 'running'",
            params![id, detail],
        )? != 1 {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    pub(crate) fn recover_podcast_refreshes(&mut self) -> Result<()> {
        self.connection.execute(
            "UPDATE podcast_refreshes SET state = 'interrupted', failure = 'service stopped before feed publication' WHERE state = 'running'",
            [],
        )?;
        Ok(())
    }

    pub(crate) fn audit_podcast_feeds(&self) -> Result<()> {
        let mut refreshes = self
            .connection
            .prepare("SELECT id FROM podcast_refreshes")?;
        let ids = refreshes
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(refreshes);
        for id in ids {
            self.podcast_refresh(&id)?;
        }
        let mut subscriptions = self
            .connection
            .prepare("SELECT id FROM podcast_subscriptions")?;
        let subscription_ids = subscriptions
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(subscriptions);
        for id in subscription_ids {
            self.audit_subscription_feed(&id)?;
        }
        let inconsistent: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM podcast_downloads d LEFT JOIN podcast_episodes e ON e.subscription_id = d.subscription_id AND e.episode_id = d.episode_id LEFT JOIN recordings r ON r.id = d.recording_id LEFT JOIN capture_jobs c ON c.id = d.recording_id WHERE e.episode_id IS NULL OR r.profile != 'episode' OR c.source_revision != d.source_revision)",
            [],
            |row| row.get(0),
        )?;
        if inconsistent {
            return Err(Error::PodcastIntegrity);
        }
        Ok(())
    }

    fn audit_subscription_feed(&self, subscription_id: &str) -> Result<()> {
        let Some(snapshot) = self.podcast_snapshot(subscription_id)? else {
            let episodes: i64 = self.connection.query_row(
                "SELECT count(*) FROM podcast_episodes WHERE subscription_id = ?1",
                [subscription_id],
                |row| row.get(0),
            )?;
            if episodes != 0 {
                return Err(Error::PodcastIntegrity);
            }
            return Ok(());
        };
        let refresh = self.podcast_refresh(&snapshot.refresh_id)?;
        if refresh.state != "completed"
            || refresh.subscription_id != subscription_id
            || refresh.document_sha256.as_deref() != Some(snapshot.document_sha256.as_str())
        {
            return Err(Error::PodcastIntegrity);
        }
        let mut query = self.connection.prepare(
            "SELECT episode_id, identity_kind, guid, published_ms, enclosure_url, assets_json, latest_refresh_id, in_latest FROM podcast_episodes WHERE subscription_id = ?1",
        )?;
        let rows = query
            .query_map([subscription_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if i64::try_from(rows.len()).map_err(|_| Error::PodcastIntegrity)? > MAX_RETAINED_EPISODES {
            return Err(Error::PodcastIntegrity);
        }
        let mut latest = 0_u32;
        for (episode_id, kind, guid, published_ms, enclosure_url, assets, refresh_id, in_latest) in
            rows
        {
            let identity = stored_identity(&kind, guid, enclosure_url, published_ms)?;
            if identity.key() != episode_id {
                return Err(Error::PodcastIntegrity);
            }
            let _ = asset_counts(&assets)?;
            if in_latest == 1 {
                if refresh_id != snapshot.refresh_id {
                    return Err(Error::PodcastIntegrity);
                }
                latest = latest.checked_add(1).ok_or(Error::PodcastIntegrity)?;
            } else if in_latest != 0 {
                return Err(Error::PodcastIntegrity);
            }
        }
        if latest != snapshot.committed_items {
            return Err(Error::PodcastIntegrity);
        }
        Ok(())
    }

    fn episode_sequence(&self, subscription_id: &str, episode_id: &str) -> Result<i64> {
        if !hex64(episode_id) {
            return Err(Error::InvalidInput("podcast episode cursor"));
        }
        self.connection
            .query_row(
                "SELECT sequence FROM podcast_episodes WHERE subscription_id = ?1 AND episode_id = ?2",
                params![subscription_id, episode_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)
    }

    /// Reserve one enclosure download. A declared length above 512 MiB fails before any connect.
    /// Exact replay of the recording id does not admit another attempt.
    /// # Errors
    /// Rejects a missing episode, an unauthorized enclosure, a full quota, or a conflicting replay.
    pub(crate) fn begin_episode_download(
        &mut self,
        recording_id: &str,
        subscription_id: &str,
        episode_id: &str,
        revision_id: &str,
    ) -> Result<EpisodeAdmission> {
        validate_key(recording_id, "recording ID")?;
        validate_key(subscription_id, "podcast subscription ID")?;
        if !hex64(episode_id) {
            return Err(Error::InvalidInput("episode ID"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let admission =
            begin_episode_download_in(&tx, recording_id, subscription_id, episode_id, revision_id)?;
        tx.commit()?;
        Ok(admission)
    }
}

/// A new reservation, or the same request which must not connect again.
pub(crate) enum EpisodeAdmission {
    Started(CaptureJob),
    Replay,
}

type RefreshColumns = (
    String,
    String,
    i64,
    Option<i64>,
    i64,
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
);

const EPISODE_SOURCE_NAME: &str = "Episode";

fn begin_episode_download_in(
    tx: &rusqlite::Connection,
    recording_id: &str,
    subscription_id: &str,
    episode_id: &str,
    revision_id: &str,
) -> Result<EpisodeAdmission> {
    let subscription =
        super::podcasts::read_subscription(tx, subscription_id)?.ok_or(Error::NotFound)?;
    if let Some(existing) = download_row(tx, recording_id)? {
        if existing.subscription_id != subscription_id
            || existing.episode_id != episode_id
            || existing.source_revision != revision_id
        {
            return Err(Error::IdempotencyConflict);
        }
        let profile: String = tx.query_row(
            "SELECT profile FROM recordings WHERE id = ?1",
            [recording_id],
            |row| row.get(0),
        )?;
        if profile != RecordingProfile::Episode.as_str() {
            return Err(Error::PodcastIntegrity);
        }
        return Ok(EpisodeAdmission::Replay);
    }
    let (url, length) = episode_enclosure(tx, subscription_id, episode_id)?;
    let ceiling = i64::try_from(crate::sources::http::EPISODE_BODY_BYTES)
        .map_err(|_| Error::StorageIntegrity)?;
    if length.is_some_and(|declared| declared > ceiling) {
        return Err(Error::InvalidInput("enclosure length above 512 MiB"));
    }
    if episode_is_occupied(tx, subscription_id, episode_id)? {
        return Err(Error::InvalidInput("episode enclosure already recorded"));
    }
    let source = HttpSource::new(EPISODE_SOURCE_NAME, &url, subscription.network)
        .map_err(|error| match error {
            Error::InvalidInput(_) => Error::InvalidInput("episode enclosure URL"),
            other => other,
        })?
        .with_redirects(subscription.redirects)?;
    super::sources::register_source_in(tx, revision_id, &source)?;
    let job = dvr::admit_recording_in(
        tx,
        &dvr::RecordingInsert {
            id: recording_id,
            source: revision_id,
            seconds: crate::sources::http::EPISODE_DURATION.as_secs(),
            maximum: crate::sources::http::EPISODE_BODY_BYTES,
            retention: Retention::Temporary,
            metadata: false,
        },
    )?;
    let Some(job) = job else {
        return Err(Error::PodcastIntegrity);
    };
    tx.execute(
        "INSERT INTO podcast_downloads(recording_id, subscription_id, episode_id, source_revision) VALUES (?1, ?2, ?3, ?4)",
        params![recording_id, subscription_id, episode_id, revision_id],
    )?;
    Ok(EpisodeAdmission::Started(job))
}

struct DownloadRow {
    subscription_id: String,
    episode_id: String,
    source_revision: String,
}

fn download_row(tx: &rusqlite::Connection, recording_id: &str) -> Result<Option<DownloadRow>> {
    tx.query_row(
        "SELECT subscription_id, episode_id, source_revision FROM podcast_downloads WHERE recording_id = ?1",
        [recording_id],
        |row| {
            Ok(DownloadRow {
                subscription_id: row.get(0)?,
                episode_id: row.get(1)?,
                source_revision: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(Error::from)
}

fn episode_enclosure(
    tx: &rusqlite::Connection,
    subscription_id: &str,
    episode_id: &str,
) -> Result<(String, Option<i64>)> {
    let row = tx
        .query_row(
            "SELECT enclosure_url, enclosure_length FROM podcast_episodes WHERE subscription_id = ?1 AND episode_id = ?2",
            params![subscription_id, episode_id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()?;
    let Some((url, length)) = row else {
        return Err(Error::NotFound);
    };
    let Some(url) = url else {
        return Err(Error::InvalidInput("episode without an enclosure"));
    };
    if length.is_some_and(|value| value < 0) {
        return Err(Error::PodcastIntegrity);
    }
    Ok((url, length))
}

fn episode_is_occupied(
    tx: &rusqlite::Connection,
    subscription_id: &str,
    episode_id: &str,
) -> Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM podcast_downloads d JOIN recordings r ON r.id = d.recording_id WHERE d.subscription_id = ?1 AND d.episode_id = ?2 AND r.storage_state != 'deleted')",
        params![subscription_id, episode_id],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn refresh_status(id: &str, row: RefreshColumns) -> Result<PodcastRefreshStatus> {
    let (
        subscription_id,
        state,
        started_ms,
        completed_ms,
        committed_items,
        truncated,
        live_count,
        skipped_items,
        document_sha256,
        failure,
    ) = row;
    validate_key(&subscription_id, "podcast subscription ID")
        .map_err(|_| Error::PodcastIntegrity)?;
    if !matches!(
        state.as_str(),
        "running" | "completed" | "failed" | "interrupted"
    ) || started_ms < 0
        || completed_ms.is_some_and(|value| value < started_ms)
        || !(0..=500).contains(&committed_items)
        || !matches!(truncated, 0 | 1)
        || live_count < 0
        || skipped_items < 0
        || document_sha256.as_ref().is_some_and(|value| !hex64(value))
        || failure
            .as_ref()
            .is_some_and(|value| !plain_text(value, 256))
    {
        return Err(Error::PodcastIntegrity);
    }
    if state == "completed" && document_sha256.is_none() {
        return Err(Error::PodcastIntegrity);
    }
    Ok(PodcastRefreshStatus {
        id: id.to_owned(),
        subscription_id,
        state,
        started_ms,
        completed_ms,
        committed_items: u32::try_from(committed_items).map_err(|_| Error::PodcastIntegrity)?,
        truncated: truncated == 1,
        live_count: u32::try_from(live_count).map_err(|_| Error::PodcastIntegrity)?,
        skipped_items: u32::try_from(skipped_items).map_err(|_| Error::PodcastIntegrity)?,
        document_sha256,
        failure,
    })
}

fn snapshot_record(row: (String, i64, String, i64, i64, i64)) -> Result<PodcastSnapshotRecord> {
    let (refresh_id, observed_ms, document_sha256, committed_items, truncated, live_count) = row;
    validate_key(&refresh_id, "podcast refresh ID").map_err(|_| Error::PodcastIntegrity)?;
    if observed_ms < 0
        || !hex64(&document_sha256)
        || !(0..=500).contains(&committed_items)
        || !matches!(truncated, 0 | 1)
        || live_count < 0
    {
        return Err(Error::PodcastIntegrity);
    }
    Ok(PodcastSnapshotRecord {
        refresh_id,
        observed_ms,
        document_sha256,
        committed_items: u32::try_from(committed_items).map_err(|_| Error::PodcastIntegrity)?,
        truncated: truncated == 1,
        live_count: u32::try_from(live_count).map_err(|_| Error::PodcastIntegrity)?,
    })
}

fn read_episode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EpisodeRow> {
    Ok(EpisodeRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        title: row.get(3)?,
        published_ms: row.get(4)?,
        enclosure_url: row.get(5)?,
        enclosure_type: row.get(6)?,
        assets_json: row.get(7)?,
        in_latest: row.get(8)?,
    })
}

struct EpisodeRow {
    id: String,
    kind: String,
    title: Option<String>,
    published_ms: Option<i64>,
    enclosure_url: Option<String>,
    enclosure_type: Option<String>,
    assets_json: String,
    in_latest: i64,
}

fn episode_record(row: EpisodeRow) -> Result<PodcastEpisodeRecord> {
    if !hex64(&row.id) || !matches!(row.in_latest, 0 | 1) {
        return Err(Error::PodcastIntegrity);
    }
    if let Some(title) = &row.title
        && !plain_text(title, 512)
    {
        return Err(Error::PodcastIntegrity);
    }
    let (transcripts, chapters) = asset_counts(&row.assets_json)?;
    Ok(PodcastEpisodeRecord {
        id: row.id,
        identity: PodcastIdentityKind::parse(&row.kind)?,
        title: row.title,
        published_ms: row.published_ms,
        enclosure: row.enclosure_url.is_some(),
        enclosure_type: row.enclosure_type,
        transcripts,
        chapters,
        in_latest: row.in_latest == 1,
    })
}

fn upsert_episode(
    tx: &rusqlite::Connection,
    subscription_id: &str,
    refresh_id: &str,
    now: i64,
    episode: &ParsedEpisode,
) -> Result<bool> {
    let episode_id = episode.identity.key();
    let assets = asset_json(&episode.transcripts, &episode.chapters)?;
    let (guid, enclosure_url, published_ms) = identity_columns(&episode.identity, episode);
    let updated = tx.execute(
        "UPDATE podcast_episodes SET title = ?3, published_ms = CASE WHEN identity_kind = 'derived_enclosure' THEN published_ms ELSE ?4 END, enclosure_url = CASE WHEN identity_kind = 'derived_enclosure' THEN enclosure_url ELSE ?5 END, enclosure_type = ?6, enclosure_length = ?7, assets_json = ?8, last_observed_ms = ?9, latest_refresh_id = ?10, in_latest = 1 WHERE subscription_id = ?1 AND episode_id = ?2",
        params![
            subscription_id,
            episode_id,
            episode.title,
            published_ms,
            enclosure_url,
            episode.enclosure_type,
            episode.enclosure_length,
            assets,
            now,
            refresh_id
        ],
    )?;
    if updated == 1 {
        return Ok(true);
    }
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM podcast_episodes WHERE subscription_id = ?1",
        [subscription_id],
        |row| row.get(0),
    )?;
    if count >= MAX_RETAINED_EPISODES {
        return Ok(false);
    }
    let sequence: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sequence), -1) + 1 FROM podcast_episodes WHERE subscription_id = ?1",
        [subscription_id],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO podcast_episodes(subscription_id, episode_id, identity_kind, guid, title, published_ms, enclosure_url, enclosure_type, enclosure_length, assets_json, first_observed_ms, last_observed_ms, latest_refresh_id, in_latest, sequence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12, 1, ?13)",
        params![
            subscription_id,
            episode_id,
            episode.identity.kind(),
            guid,
            episode.title,
            published_ms,
            enclosure_url,
            episode.enclosure_type,
            episode.enclosure_length,
            assets,
            now,
            refresh_id,
            sequence
        ],
    )?;
    Ok(true)
}

fn identity_columns<'a>(
    identity: &'a EpisodeIdentity,
    episode: &'a ParsedEpisode,
) -> (Option<&'a str>, Option<&'a str>, Option<i64>) {
    match identity {
        EpisodeIdentity::PublisherGuid(guid) => (
            Some(guid.as_str()),
            episode.enclosure_url.as_deref(),
            episode.published_ms,
        ),
        EpisodeIdentity::Derived { url, published_ms } => {
            (None, Some(url.as_str()), Some(*published_ms))
        }
    }
}

fn stored_identity(
    kind: &str,
    guid: Option<String>,
    enclosure_url: Option<String>,
    published_ms: Option<i64>,
) -> Result<EpisodeIdentity> {
    match (kind, guid, enclosure_url, published_ms) {
        ("publisher_guid", Some(guid), _, _) => Ok(EpisodeIdentity::PublisherGuid(guid)),
        ("derived_enclosure", None, Some(url), Some(published_ms)) => {
            let normalized =
                crate::podcast::normalize_http_url(&url).ok_or(Error::PodcastIntegrity)?;
            if normalized != url {
                return Err(Error::PodcastIntegrity);
            }
            Ok(EpisodeIdentity::Derived { url, published_ms })
        }
        _ => Err(Error::PodcastIntegrity),
    }
}

fn asset_json(transcripts: &[AssetRef], chapters: &[AssetRef]) -> Result<String> {
    let mut assets = Vec::with_capacity(transcripts.len() + chapters.len());
    for asset in transcripts {
        assets.push(asset_value("transcript", asset));
    }
    for asset in chapters {
        assets.push(asset_value("chapters", asset));
    }
    let json = serde_json::to_string(&assets)?;
    if !(2..=32_768).contains(&json.len()) {
        return Err(Error::InvalidInput("feed asset record"));
    }
    Ok(json)
}

fn asset_value<'a>(kind: &'a str, asset: &'a AssetRef) -> AssetJson<'a> {
    AssetJson {
        kind,
        url: &asset.url,
        media_type: asset.media_type.as_deref(),
        language: asset.language.as_deref(),
    }
}

fn asset_counts(json: &str) -> Result<(u32, u32)> {
    let assets: Vec<AssetJsonOwned> =
        serde_json::from_str(json).map_err(|_| Error::PodcastIntegrity)?;
    if assets.len() > 12 {
        return Err(Error::PodcastIntegrity);
    }
    let mut transcripts = 0_u32;
    let mut chapters = 0_u32;
    for asset in assets {
        if crate::podcast::normalize_http_url(&asset.url).as_deref() != Some(asset.url.as_str())
            || asset
                .media_type
                .as_ref()
                .is_some_and(|value| !plain_text(value, 128))
            || asset
                .language
                .as_ref()
                .is_some_and(|value| !plain_text(value, 32))
        {
            return Err(Error::PodcastIntegrity);
        }
        match asset.kind.as_str() {
            "transcript" => transcripts = transcripts.saturating_add(1),
            "chapters" => chapters = chapters.saturating_add(1),
            _ => return Err(Error::PodcastIntegrity),
        }
    }
    if transcripts > 8 || chapters > 4 {
        return Err(Error::PodcastIntegrity);
    }
    Ok((transcripts, chapters))
}

#[derive(Serialize)]
struct AssetJson<'a> {
    kind: &'a str,
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetJsonOwned {
    kind: String,
    url: String,
    #[serde(default)]
    media_type: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

fn failure_detail(error: &Error) -> &'static str {
    let detail = match error {
        Error::Acquisition(detail) | Error::InvalidInput(detail) => *detail,
        Error::DestinationDenied => "redirect destination is not authorized",
        _ => "podcast refresh failed; previous snapshot retained",
    };
    if plain_text(detail, 256) {
        detail
    } else {
        "podcast refresh failed; previous snapshot retained"
    }
}

fn plain_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(unsafe_display)
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::MAX_RETAINED_EPISODES;
    use crate::{
        Error,
        podcast::{FeedCommit, document_sha256, parse},
        sources::{NetworkScope, RedirectPolicy},
        storage::{SCHEMA_VERSION, Store, captures::CapturePlan},
    };
    use std::fmt::Write;

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const PUBLIC: NetworkScope = NetworkScope::PublicInternet {};

    fn open() -> Result<(tempfile::TempDir, Store), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        Ok((directory, store))
    }

    fn document(items: &str) -> String {
        format!(
            r#"<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel>{items}</channel></rss>"#
        )
    }

    fn commit_at(
        store: &mut Store,
        id: &str,
        subscription: &str,
        xml: &str,
        at: i64,
    ) -> crate::Result<()> {
        if !store.begin_podcast_refresh_at(id, subscription, at)? {
            return Err(Error::RequestState);
        }
        let parsed = parse(xml, None)?;
        store.finish_podcast_refresh(
            id,
            &FeedCommit {
                sha256: document_sha256(xml.as_bytes()),
                parsed,
            },
        )
    }

    #[test]
    fn snapshot_keeps_omitted_episodes_and_a_failed_document() -> TestResult {
        let (_directory, mut store) = open()?;
        let source = crate::sources::HttpSource::new("Radio", "https://example.com/audio", PUBLIC)?;
        store.register_source("radio:v1", &source)?;
        store.create_capture("keep-me", &CapturePlan::new("radio:v1", 0, 60_000, 1_024)?)?;
        store.subscribe_podcast(
            "show:v1",
            "https://show.example/feed.xml?token=hidden",
            PUBLIC,
            RedirectPolicy::Deny,
        )?;
        let first = document(
            r#"<item><title>One</title><guid>ep-1</guid><enclosure url="https://cdn.example/a.mp3?token=hidden" type="audio/mpeg"/><podcast:transcript url="https://cdn.example/a.vtt" type="text/vtt" language="en"/><podcast:chapters url="https://cdn.example/a.json" type="application/json"/></item><item><title>Two</title><guid>ep-2</guid></item><item><title>Only a title</title></item><podcast:liveItem status="ended"/>"#,
        );
        commit_at(&mut store, "refresh:v1", "show:v1", &first, 5_000)?;
        let stored: String = store.connection.query_row(
            "SELECT enclosure_url FROM podcast_episodes WHERE guid = 'ep-1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(stored, "https://cdn.example/a.mp3?token=hidden");
        let transcript: String = store.connection.query_row(
            "SELECT assets_json FROM podcast_episodes WHERE guid = 'ep-1'",
            [],
            |row| row.get(0),
        )?;
        assert!(transcript.contains("https://cdn.example/a.vtt"));
        assert!(transcript.contains("https://cdn.example/a.json"));
        assert!(!format!("{:?}", store.podcast_episodes("show:v1", None, 16)?).contains("token"));
        let page = store.podcast_episodes("show:v1", None, 16)?;
        assert_eq!(page.len(), 2);
        assert!(page.iter().all(|episode| episode.in_latest));
        assert_eq!(
            store
                .podcast_snapshot("show:v1")?
                .ok_or("snapshot")?
                .live_count,
            1
        );
        let second = document(r"<item><guid>ep-2</guid><title>Two</title></item>");
        commit_at(&mut store, "refresh:v2", "show:v1", &second, 8_000)?;
        let kept = store.podcast_episodes("show:v1", None, 16)?;
        assert_eq!(kept.len(), 2);
        assert_eq!(kept.iter().filter(|episode| episode.in_latest).count(), 1);
        assert!(
            store
                .connection
                .execute("DELETE FROM podcast_episodes", [])
                .is_err()
        );
        if !store.begin_podcast_refresh_at("refresh:v3", "show:v1", 11_000)? {
            return Err("failed refresh did not start".into());
        }
        let Err(error) = parse("<rss version=\"2.0\"><channel>&nope;</channel></rss>", None) else {
            return Err("entity reference was accepted".into());
        };
        store.fail_podcast_refresh("refresh:v3", &error)?;
        assert_eq!(store.podcast_episodes("show:v1", None, 16)?.len(), 2);
        assert_eq!(
            store
                .podcast_snapshot("show:v1")?
                .ok_or("snapshot")?
                .refresh_id,
            "refresh:v2"
        );
        assert!(store.capture("keep-me")?.is_some());
        assert!(store.source("radio:v1")?.is_some());
        assert!(!store.begin_podcast_refresh_at("refresh:v2", "show:v1", 20_000)?);
        assert!(matches!(
            store.begin_podcast_refresh_at("refresh:v2", "other", 20_000),
            Err(Error::IdempotencyConflict)
        ));
        store.unsubscribe_podcast("show:v1")?;
        assert!(matches!(
            store.begin_podcast_refresh_at("refresh:v4", "show:v1", 30_000),
            Err(Error::InvalidInput(_))
        ));
        Ok(())
    }

    #[test]
    fn one_active_refresh_and_the_interval_are_exact() -> TestResult {
        let (_directory, mut store) = open()?;
        store.subscribe_podcast(
            "show:v1",
            "https://show.example/feed.xml",
            PUBLIC,
            RedirectPolicy::Deny,
        )?;
        assert!(store.begin_podcast_refresh_at("one", "show:v1", 1_000)?);
        assert!(matches!(
            store.begin_podcast_refresh_at("two", "show:v1", 1_500),
            Err(Error::InvalidInput(_))
        ));
        store.fail_podcast_refresh("one", &Error::Acquisition("feed size limit"))?;
        assert!(matches!(
            store.begin_podcast_refresh_at("two", "show:v1", 2_000),
            Err(Error::InvalidInput(_))
        ));
        assert!(store.begin_podcast_refresh_at("two", "show:v1", 3_000)?);
        Ok(())
    }

    #[test]
    fn retained_episode_cap_marks_the_snapshot_truncated() -> TestResult {
        let (_directory, mut store) = open()?;
        store.subscribe_podcast(
            "show:v1",
            "https://show.example/feed.xml",
            PUBLIC,
            RedirectPolicy::Deny,
        )?;
        let mut at = 10_000_i64;
        let mut start = 0_u32;
        while i64::from(start) < MAX_RETAINED_EPISODES {
            let mut items = String::new();
            for index in start..start + 500 {
                write!(items, "<item><guid>g{index}</guid></item>")?;
            }
            commit_at(
                &mut store,
                &format!("batch-{start}"),
                "show:v1",
                &document(&items),
                at,
            )?;
            start += 500;
            at += 3_000;
        }
        commit_at(
            &mut store,
            "overflow",
            "show:v1",
            &document("<item><guid>fresh</guid></item>"),
            at,
        )?;
        let snapshot = store.podcast_snapshot("show:v1")?.ok_or("snapshot")?;
        assert!(snapshot.truncated);
        assert_eq!(snapshot.committed_items, 0);
        let count: i64 =
            store
                .connection
                .query_row("SELECT count(*) FROM podcast_episodes", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(count, MAX_RETAINED_EPISODES);
        let present: i64 = store.connection.query_row(
            "SELECT count(*) FROM podcast_episodes WHERE guid = 'fresh'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(present, 0);
        Ok(())
    }

    #[test]
    fn migration_preserves_v12_and_rolls_back_conflicts() -> TestResult {
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
                include_str!("012-podcasts.sql"),
            ] {
                connection.execute_batch(sql)?;
            }
            connection.execute("UPDATE budgets SET limit_micros = 4242", [])?;
            if fail {
                connection.execute("CREATE TABLE podcast_episodes(existing TEXT)", [])?;
            }
            let opened = Store::open(&path);
            let version: u32 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if fail {
                assert!(opened.is_err());
                assert_eq!(version, 12);
                let snapshots: i64 = connection.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = 'podcast_snapshots'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(snapshots, 0);
            } else {
                let store = opened?;
                assert_eq!(version, SCHEMA_VERSION);
                assert_eq!(store.budget("global")?.limit().micros(), 4242);
                assert_eq!(
                    store.connection.query_row(
                        "SELECT count(*) FROM podcast_episodes",
                        [],
                        |row| row.get::<_, i64>(0)
                    )?,
                    0
                );
            }
        }
        Ok(())
    }

    #[test]
    fn episode_download_reserves_the_full_ceiling_before_any_connect() -> TestResult {
        let (_directory, mut store) = open()?;
        configure(&mut store)?;
        store.subscribe_podcast(
            "show:v1",
            "https://show.example/feed.xml",
            PUBLIC,
            RedirectPolicy::Deny,
        )?;
        let over = 512 * 1024 * 1024 + 1;
        commit_at(
            &mut store,
            "refresh:v1",
            "show:v1",
            &document(&format!(
                r#"<item><guid>big</guid><enclosure url="https://cdn.example/big.mp3" length="{over}" type="audio/mpeg"/></item>"#
            )),
            5_000,
        )?;
        let big = episode_id(&store, "big")?;
        assert!(matches!(
            store.begin_episode_download("episode:big", "show:v1", &big, "enc:big"),
            Err(Error::InvalidInput("enclosure length above 512 MiB"))
        ));
        assert_eq!(count(&store, "SELECT count(*) FROM source_revisions")?, 0);
        assert_eq!(count(&store, "SELECT count(*) FROM recordings")?, 0);
        assert_eq!(count(&store, "SELECT count(*) FROM podcast_downloads")?, 0);
        commit_at(
            &mut store,
            "refresh:v2",
            "show:v1",
            &document(
                r#"<item><guid>ep-1</guid><enclosure url="https://cdn.example/a.mp3?token=hidden" length="12" type="audio/mpeg"/></item><item><guid>local</guid><enclosure url="http://127.0.0.1/secret.mp3" length="12"/></item><item><guid>none</guid><title>No file</title></item>"#,
            ),
            8_000,
        )?;
        let missing = episode_id(&store, "none")?;
        assert!(matches!(
            store.begin_episode_download("episode:none", "show:v1", &missing, "enc:none"),
            Err(Error::InvalidInput("episode without an enclosure"))
        ));
        let private = episode_id(&store, "local")?;
        assert!(matches!(
            store.begin_episode_download("episode:local", "show:v1", &private, "enc:local"),
            Err(Error::DestinationDenied)
        ));
        assert_eq!(count(&store, "SELECT count(*) FROM recordings")?, 0);
        let episode = episode_id(&store, "ep-1")?;
        let started = store.begin_episode_download("episode:v1", "show:v1", &episode, "enc:v1")?;
        let crate::storage::podcast_feeds::EpisodeAdmission::Started(job) = started else {
            return Err("download did not reserve".into());
        };
        let record = store.recording("episode:v1")?;
        assert_eq!(
            record.profile,
            crate::storage::dvr::RecordingProfile::Episode
        );
        assert_eq!(record.source_revision, "enc:v1");
        assert_eq!(record.duration_seconds, 30 * 60);
        assert_eq!(record.maximum_bytes, 512 * 1024 * 1024);
        assert_eq!(record.charged_bytes, 512 * 1024 * 1024);
        assert_eq!(record.storage_state, "reserved");
        assert_eq!(store.dvr_status()?.reserved_bytes, 512 * 1024 * 1024);
        let source = store
            .source("enc:v1")?
            .ok_or("missing enclosure revision")?;
        assert_eq!(
            source.source.endpoint(),
            "https://cdn.example/a.mp3?token=hidden"
        );
        assert_eq!(source.source.network(), PUBLIC);
        assert_eq!(source.source.redirects(), RedirectPolicy::Deny);
        assert!(matches!(
            store.begin_episode_download("episode:v1", "show:v1", &episode, "enc:v1")?,
            crate::storage::podcast_feeds::EpisodeAdmission::Replay
        ));
        assert!(matches!(
            store.begin_episode_download("episode:v2", "show:v1", &episode, "enc:v2"),
            Err(Error::InvalidInput("episode enclosure already recorded"))
        ));
        assert_eq!(count(&store, "SELECT count(*) FROM recordings")?, 1);
        let mut unclean = sample_publication();
        unclean.end_reason = "byte_limit";
        assert!(store.publish_recording(&job.version, &unclean).is_err());
        assert_eq!(store.recording("episode:v1")?.storage_state, "reserved");
        assert!(store.recording("episode:v1")?.sha256.is_none());
        store.publish_recording(&job.version, &sample_publication())?;
        let published = store.recording("episode:v1")?;
        assert_eq!(published.storage_state, "retained");
        assert_eq!(published.end_reason.as_deref(), Some("end_of_body"));
        assert_eq!(published.charged_bytes, 12);
        assert_eq!(store.dvr_status()?.reserved_bytes, 0);
        store.unsubscribe_podcast("show:v1")?;
        let again = store.begin_episode_download("episode:v1", "show:v1", &episode, "enc:v1")?;
        assert!(matches!(
            again,
            crate::storage::podcast_feeds::EpisodeAdmission::Replay
        ));
        Ok(())
    }

    #[test]
    fn episode_revision_has_no_live_edge() -> TestResult {
        let (_directory, mut store) = open()?;
        configure(&mut store)?;
        store.subscribe_podcast(
            "show:v1",
            "https://show.example/feed.xml",
            PUBLIC,
            RedirectPolicy::Deny,
        )?;
        commit_at(
            &mut store,
            "refresh:v1",
            "show:v1",
            &document(
                r#"<item><guid>ep-1</guid><enclosure url="https://cdn.example/a.mp3" length="12" type="audio/mpeg"/></item>"#,
            ),
            5_000,
        )?;
        let episode = episode_id(&store, "ep-1")?;
        let started = store.begin_episode_download("episode:v1", "show:v1", &episode, "enc:v1")?;
        assert!(matches!(
            started,
            crate::storage::podcast_feeds::EpisodeAdmission::Started(_)
        ));
        assert!(matches!(
            store.begin_listen("live", "enc:v1"),
            Err(Error::InvalidInput("episode has no live edge"))
        ));
        assert!(matches!(store.listen("live"), Err(Error::NotFound)));
        assert_eq!(count(&store, "SELECT count(*) FROM listen_sessions")?, 0);
        let radio = crate::sources::HttpSource::new("Radio", "https://example.com/audio", PUBLIC)?;
        store.register_source("radio:v1", &radio)?;
        assert!(store.begin_listen("hear", "radio:v1")?);
        assert_eq!(store.listen("hear")?.state, "running");
        store.audit_listens()?;
        Ok(())
    }

    #[test]
    fn migration_preserves_v13_recordings_and_rolls_back_conflicts() -> TestResult {
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
                include_str!("012-podcasts.sql"),
                include_str!("013-podcast-feeds.sql"),
            ] {
                connection.execute_batch(sql)?;
            }
            connection.execute("UPDATE budgets SET limit_micros = 4242", [])?;
            connection.execute("INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms, redirect_policy) VALUES ('radio:v1', 'http_audio', 'Radio', 'https://example.com/audio', 'public_internet', 1, 'deny')", [])?;
            connection.execute("INSERT INTO capture_jobs(id, source_revision, starts_ms, ends_ms, maximum_bytes, state, revision, generation, created_ms, updated_ms) VALUES ('rec-1', 'radio:v1', 0, 60000, 1000, 'scheduled', 0, 0, 1, 1)", [])?;
            connection.execute("INSERT INTO capture_events(job_id, revision, generation, state, reason, recorded_ms) VALUES ('rec-1', 0, 0, 'scheduled', 'accepted', 1)", [])?;
            connection.execute("INSERT INTO recordings(id, object_key, duration_seconds, initial_retention, retention, storage_state, charged_bytes, metadata_requested) VALUES ('rec-1', '00112233445566778899aabbccddeeff', 60, 'temporary', 'temporary', 'reserved', 1000, 0)", [])?;
            if fail {
                connection.execute("CREATE TABLE podcast_downloads(existing TEXT)", [])?;
            }
            let opened = Store::open(&path);
            let version: u32 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if fail {
                assert!(opened.is_err());
                assert_eq!(version, 13);
                let profile: i64 = connection.query_row(
                    "SELECT count(*) FROM pragma_table_info('recordings') WHERE name = 'profile'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(profile, 0);
                let charged: i64 = connection.query_row(
                    "SELECT charged_bytes FROM recordings WHERE id = 'rec-1'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(charged, 1000);
            } else {
                let store = opened?;
                assert_eq!(version, SCHEMA_VERSION);
                assert_eq!(store.budget("global")?.limit().micros(), 4242);
                let record = store.recording("rec-1")?;
                assert_eq!(record.profile, crate::storage::dvr::RecordingProfile::Radio);
                assert_eq!(record.duration_seconds, 60);
                assert_eq!(record.maximum_bytes, 1000);
                assert_eq!(record.charged_bytes, 1000);
                let definition: String = store.connection.query_row(
                    "SELECT sql FROM sqlite_schema WHERE name = 'recording_observations'",
                    [],
                    |row| row.get(0),
                )?;
                assert!(definition.contains("recordings"));
                assert!(!definition.contains("recordings_v14"));
                assert_eq!(count(&store, "SELECT count(*) FROM podcast_downloads")?, 0);
            }
        }
        Ok(())
    }

    fn configure(store: &mut Store) -> TestResult {
        let executable = std::env::current_exe()?;
        let Some(path) = executable.to_str() else {
            return Err("decoder path is not Unicode".into());
        };
        store.configure_dvr(512 * 1024 * 1024, 64 * 1024 * 1024, 14, path)?;
        Ok(())
    }

    fn episode_id(store: &Store, guid: &str) -> Result<String, Box<dyn std::error::Error>> {
        Ok(store.connection.query_row(
            "SELECT episode_id FROM podcast_episodes WHERE guid = ?1",
            [guid],
            |row| row.get(0),
        )?)
    }

    fn count(store: &Store, sql: &str) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(store.connection.query_row(sql, [], |row| row.get(0))?)
    }

    fn sample_publication() -> crate::storage::dvr::Publication {
        crate::storage::dvr::Publication {
            bytes: 12,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![crate::sources::HttpHop {
                origin: "https://cdn.example".into(),
                peer: std::net::SocketAddr::from(([8, 8, 8, 8], 443)),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
            gap: None,
        }
    }
}
