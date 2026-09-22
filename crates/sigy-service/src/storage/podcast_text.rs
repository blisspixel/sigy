//! One explicit publisher transcript or chapter document. Not a recording.

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

use super::{Store, now_ms, validate_key};
use crate::{
    Error, Result,
    podcast::{PublisherCue, TextKind, normalize_text_type},
    sources::HttpSource,
};

const MAX_TEXT_FETCHES: i64 = 256;

#[derive(Clone, Debug)]
pub(crate) struct TextFetch {
    pub id: String,
    pub subscription_id: String,
    pub episode_id: String,
    pub kind: String,
    pub asset_index: u32,
    pub state: String,
    pub origin: Option<String>,
    pub media_type: Option<String>,
    pub language_hint: Option<String>,
    pub document_sha256: Option<String>,
    pub cues: Vec<PublisherCue>,
    pub failure: Option<String>,
}

pub(crate) enum TextAdmission {
    Replay,
    Fetch {
        source: HttpSource,
        media_type: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct TextDocument {
    pub origin: String,
    pub sha256: String,
    pub cues: Vec<PublisherCue>,
}

impl Store {
    pub(crate) fn begin_publisher_text(
        &mut self,
        id: &str,
        subscription_id: &str,
        episode_id: &str,
        kind: TextKind,
        index: u32,
    ) -> Result<TextAdmission> {
        validate_key(id, "publisher text ID")?;
        validate_key(subscription_id, "podcast subscription ID")?;
        if !hex64(episode_id) || index > kind_limit(kind) {
            return Err(Error::InvalidInput("publisher text target"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let admission = begin_in(&tx, id, subscription_id, episode_id, kind, index)?;
        tx.commit()?;
        Ok(admission)
    }

    pub(crate) fn finish_publisher_text(
        &mut self,
        id: &str,
        document: &TextDocument,
    ) -> Result<()> {
        let cues = serde_json::to_string(&document.cues)?;
        if cues.len() > 200_000 {
            return Err(Error::InvalidInput("publisher text cue limit"));
        }
        let tx = self.connection.transaction()?;
        let changed = tx.execute(
            "UPDATE podcast_text_fetches SET state = 'completed', completed_ms = ?2, origin = ?3, document_sha256 = ?4, cues_json = ?5 WHERE id = ?1 AND state = 'running'",
            params![id, now_ms()?, document.origin, document.sha256, cues],
        )?;
        if changed != 1 {
            return Err(Error::PodcastIntegrity);
        }
        let (subscription, episode, kind, index): (String, String, String, i64) = tx.query_row(
            "SELECT subscription_id, episode_id, kind, asset_index FROM podcast_text_fetches WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        tx.execute(
            "INSERT INTO podcast_text_snapshots(subscription_id, episode_id, kind, asset_index, fetch_id) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(subscription_id, episode_id, kind, asset_index) DO UPDATE SET fetch_id = excluded.fetch_id",
            params![subscription, episode, kind, index, id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn fail_publisher_text(&mut self, id: &str, error: &Error) -> Result<()> {
        let detail = error.to_string();
        let detail = if (1..=256).contains(&detail.len())
            && !detail.chars().any(crate::sources::unsafe_display)
        {
            detail
        } else {
            "publisher text failed; previous text retained".to_owned()
        };
        if self.connection.execute(
            "UPDATE podcast_text_fetches SET state = 'failed', completed_ms = ?2, failure = ?3 WHERE id = ?1 AND state = 'running'",
            params![id, now_ms()?, detail],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    pub(crate) fn running_publisher_fetches(&self) -> Result<i64> {
        Ok(self.connection.query_row(
            "SELECT count(*) FROM podcast_text_fetches WHERE state = 'running'",
            [],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn recover_publisher_text(&mut self) -> Result<()> {
        self.connection.execute(
            "UPDATE podcast_text_fetches SET state = 'interrupted', failure = 'service stopped before publisher text completed' WHERE state = 'running'",
            [],
        )?;
        Ok(())
    }

    pub(crate) fn audit_publisher_text(&self) -> Result<()> {
        let mut fetches = self
            .connection
            .prepare("SELECT id FROM podcast_text_fetches")?;
        let ids = fetches
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(fetches);
        for id in ids {
            let fetch = self.publisher_text(&id)?;
            let completed = fetch.state == "completed";
            if completed
                && (fetch.cues.is_empty()
                    || fetch.origin.is_none()
                    || fetch.document_sha256.is_none())
            {
                return Err(Error::PodcastIntegrity);
            }
            if !completed && !fetch.cues.is_empty() {
                return Err(Error::PodcastIntegrity);
            }
        }
        let broken: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM podcast_text_snapshots s LEFT JOIN podcast_text_fetches f ON f.id = s.fetch_id WHERE f.id IS NULL OR f.state != 'completed' OR f.subscription_id != s.subscription_id OR f.episode_id != s.episode_id OR f.kind != s.kind OR f.asset_index != s.asset_index)",
            [],
            |row| row.get(0),
        )?;
        if broken {
            return Err(Error::PodcastIntegrity);
        }
        Ok(())
    }

    pub(crate) fn publisher_text(&self, id: &str) -> Result<TextFetch> {
        validate_key(id, "publisher text ID")?;
        let row = self
            .connection
            .query_row(
                "SELECT subscription_id, episode_id, kind, asset_index, state, origin, media_type, language_hint, document_sha256, cues_json, failure FROM podcast_text_fetches WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let cues = match row.9.as_deref() {
            Some(json) => serde_json::from_str(json).map_err(|_| Error::PodcastIntegrity)?,
            None => Vec::new(),
        };
        Ok(TextFetch {
            id: id.to_owned(),
            subscription_id: row.0,
            episode_id: row.1,
            kind: row.2,
            asset_index: u32::try_from(row.3).map_err(|_| Error::PodcastIntegrity)?,
            state: row.4,
            origin: row.5,
            media_type: row.6,
            language_hint: row.7,
            document_sha256: row.8,
            cues,
            failure: row.10,
        })
    }
}

fn begin_in(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    subscription_id: &str,
    episode_id: &str,
    kind: TextKind,
    index: u32,
) -> Result<TextAdmission> {
    if let Some(existing) = existing_fetch(tx, id)? {
        if existing.0 != subscription_id
            || existing.1 != episode_id
            || existing.2 != kind.as_str()
            || existing.3 != i64::from(index)
        {
            return Err(Error::IdempotencyConflict);
        }
        return Ok(TextAdmission::Replay);
    }
    let running: i64 = tx.query_row(
        "SELECT count(*) FROM podcast_text_fetches WHERE state = 'running'",
        [],
        |row| row.get(0),
    )?;
    let count: i64 = tx.query_row("SELECT count(*) FROM podcast_text_fetches", [], |row| {
        row.get(0)
    })?;
    if running != 0 || count >= MAX_TEXT_FETCHES {
        return Err(Error::InvalidInput("publisher text capacity reached"));
    }
    let subscription =
        super::podcasts::read_subscription(tx, subscription_id)?.ok_or(Error::NotFound)?;
    let asset = select_asset(tx, subscription_id, episode_id, kind, index)?;
    let media_type = asset
        .media_type
        .as_deref()
        .and_then(|raw| normalize_text_type(kind, raw))
        .ok_or(Error::InvalidInput("publisher text type"))?;
    let source = HttpSource::new("Publisher text", &asset.url, subscription.network)
        .map_err(|_| Error::InvalidInput("publisher text URL"))?
        .with_redirects(subscription.redirects)?;
    tx.execute(
        "INSERT INTO podcast_text_fetches(id, subscription_id, episode_id, kind, asset_index, state, started_ms, media_type, language_hint) VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7, ?8)",
        params![id, subscription_id, episode_id, kind.as_str(), index, now_ms()?, media_type, asset.language],
    )?;
    Ok(TextAdmission::Fetch { source, media_type })
}

fn existing_fetch(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
) -> Result<Option<(String, String, String, i64)>> {
    tx.query_row(
        "SELECT subscription_id, episode_id, kind, asset_index FROM podcast_text_fetches WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .optional()
    .map_err(Into::into)
}

fn select_asset(
    tx: &rusqlite::Transaction<'_>,
    subscription_id: &str,
    episode_id: &str,
    kind: TextKind,
    index: u32,
) -> Result<StoredAsset> {
    let json: String = tx
        .query_row(
            "SELECT assets_json FROM podcast_episodes WHERE episode_id = ?1 AND subscription_id = ?2",
            params![episode_id, subscription_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    let assets: Vec<StoredAsset> =
        serde_json::from_str(&json).map_err(|_| Error::PodcastIntegrity)?;
    assets
        .into_iter()
        .filter(|asset| asset.kind == kind.as_str())
        .nth(usize::try_from(index).map_err(|_| Error::InvalidInput("publisher text target"))?)
        .ok_or(Error::NotFound)
}

fn kind_limit(kind: TextKind) -> u32 {
    match kind {
        TextKind::Transcript => 7,
        TextKind::Chapters => 3,
    }
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Deserialize)]
struct StoredAsset {
    kind: String,
    url: String,
    media_type: Option<String>,
    language: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::super::Store;
    use crate::{
        Error,
        sources::{NetworkScope, RedirectPolicy},
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn recovery_releases_the_single_running_publisher_fetch() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        store.subscribe_podcast(
            "show:v1",
            "https://show.example/feed.xml",
            NetworkScope::PublicInternet {},
            RedirectPolicy::Deny,
        )?;
        let episode = "a".repeat(64);
        insert(
            &store,
            "text:v1",
            &episode,
            "transcript",
            0,
            "running",
            None,
        )?;
        store.recover_publisher_text()?;
        let fetch = store.publisher_text("text:v1")?;
        if fetch.state != "interrupted"
            || fetch.failure.as_deref() != Some("service stopped before publisher text completed")
            || !fetch.cues.is_empty()
            || fetch.document_sha256.is_some()
        {
            return Err("running publisher text was not interrupted".into());
        }
        if !matches!(
            store.fail_publisher_text("text:v1", &Error::Acquisition("late")),
            Err(Error::RequestState)
        ) {
            return Err("an interrupted fetch accepted a late failure".into());
        }
        insert(
            &store,
            "text:v2",
            &episode,
            "transcript",
            0,
            "running",
            None,
        )?;
        if insert(
            &store,
            "text:v3",
            &episode,
            "transcript",
            0,
            "running",
            None,
        )
        .is_ok()
        {
            return Err("a second running publisher fetch was admitted".into());
        }
        insert(
            &store,
            "text:v4",
            &episode,
            "chapters",
            3,
            "failed",
            Some("kept"),
        )?;
        if insert(
            &store,
            "text:v5",
            &episode,
            "chapters",
            4,
            "failed",
            Some("rejected"),
        )
        .is_ok()
        {
            return Err("chapter index 4 was stored".into());
        }
        store.audit_podcasts()?;
        Ok(())
    }

    fn insert(
        store: &Store,
        id: &str,
        episode: &str,
        kind: &str,
        index: i64,
        state: &str,
        failure: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let completed = if state == "running" {
            None
        } else {
            Some(2_i64)
        };
        store.connection.execute(
            "INSERT INTO podcast_text_fetches(id, subscription_id, episode_id, kind, asset_index, state, started_ms, completed_ms, media_type, failure) VALUES (?1, 'show:v1', ?2, ?3, ?4, ?5, 1, ?6, 'text/vtt', ?7)",
            rusqlite::params![id, episode, kind, index, state, completed, failure],
        )?;
        Ok(())
    }
}
