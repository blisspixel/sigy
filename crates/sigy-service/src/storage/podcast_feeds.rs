//! RSS snapshot and episode rows. Omission does not delete an episode.

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{Store, now_ms, validate_key};
use crate::{
    Error, Result,
    podcast::{AssetRef, EpisodeIdentity, FeedCommit, ParsedEpisode},
    sources::unsafe_display,
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
}
