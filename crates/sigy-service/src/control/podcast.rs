//! Podcast subscription and RSS snapshot commands. Refresh writes metadata only.

use serde::{Deserialize, Serialize};

use super::{Operation, Snapshot};
use crate::{
    Error, Result,
    sources::{NetworkScope, RedirectPolicy},
    storage::{
        Store,
        podcast_feeds::PodcastEpisodeRecord,
        podcasts::{PodcastPolls, PodcastSubscription},
    },
};

pub use crate::storage::podcast_feeds::PodcastIdentityKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PodcastOperation {
    Subscribe {
        id: String,
        url: String,
        network: NetworkScope,
        #[serde(default)]
        redirects: RedirectPolicy,
    },
    Unsubscribe {
        id: String,
    },
    List {
        after: Option<String>,
        limit: u32,
    },
    Show {
        id: String,
    },
    /// Fetch one RSS document for an active subscription. Replay does not fetch again.
    Refresh {
        id: String,
        subscription_id: String,
    },
    RefreshStatus {
        id: String,
    },
    /// List stored episodes. This does not contact the feed.
    Episodes {
        subscription_id: String,
        after: Option<String>,
        limit: u32,
    },
}

/// Ordinary subscription view. The feed path and query stay in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodcastView {
    pub id: String,
    pub origin: String,
    pub network: NetworkScope,
    pub redirects: RedirectPolicy,
    pub polls: PodcastPolls,
    pub created_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodcastPage {
    pub entries: Vec<PodcastView>,
    pub next_after: Option<String>,
    pub newly_created: Option<bool>,
    pub newly_stopped: Option<bool>,
}

/// One refresh, the last good snapshot, and a page of episodes. URLs are omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodcastFeedView {
    pub refresh: Option<PodcastRefreshView>,
    pub snapshot: Option<PodcastSnapshotView>,
    pub episodes: Vec<PodcastEpisodeView>,
    pub next_after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodcastRefreshView {
    pub id: String,
    pub subscription_id: String,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub committed_items: u32,
    pub truncated: bool,
    pub live_count: u32,
    pub skipped_items: u32,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodcastSnapshotView {
    pub refresh_id: String,
    pub observed_ms: i64,
    pub committed_items: u32,
    pub truncated: bool,
    pub live_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodcastEpisodeView {
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

pub(super) fn apply(store: &mut Store, command: PodcastOperation) -> Result<Snapshot> {
    if matches!(command, PodcastOperation::Refresh { .. }) {
        return Err(Error::ServiceRequired);
    }
    if let Some(feed) = feed_view(store, &command)? {
        let mut view = super::snapshot(store)?;
        view.podcast_feed = Some(feed);
        return Ok(view);
    }
    let page = match command {
        PodcastOperation::Subscribe {
            id,
            url,
            network,
            redirects,
        } => {
            let admission = store.subscribe_podcast(&id, &url, network, redirects)?;
            page_of(
                vec![admission.subscription],
                None,
                Some(admission.newly_created),
                None,
            )
        }
        PodcastOperation::Unsubscribe { id } => {
            let stopped = store.unsubscribe_podcast(&id)?;
            page_of(
                vec![stopped.subscription],
                None,
                None,
                Some(stopped.newly_stopped),
            )
        }
        PodcastOperation::Show { id } => {
            let subscription = store.podcast_subscription(&id)?.ok_or(Error::NotFound)?;
            page_of(vec![subscription], None, None, None)
        }
        PodcastOperation::Refresh { .. }
        | PodcastOperation::RefreshStatus { .. }
        | PodcastOperation::Episodes { .. } => return Err(Error::InvalidInput("podcast command")),
        PodcastOperation::List { after, limit } => {
            let entries = store.podcast_subscriptions(after.as_deref(), limit)?;
            let next_after = if let Some(last) = entries.last()
                && !store.podcast_subscriptions(Some(&last.id), 1)?.is_empty()
            {
                Some(last.id.clone())
            } else {
                None
            };
            page_of(entries, next_after, None, None)
        }
    };
    let mut view = super::snapshot(store)?;
    view.podcast_page = Some(page);
    Ok(view)
}

fn page_of(
    entries: Vec<PodcastSubscription>,
    next_after: Option<String>,
    newly_created: Option<bool>,
    newly_stopped: Option<bool>,
) -> PodcastPage {
    PodcastPage {
        entries: entries.into_iter().map(view_of).collect(),
        next_after,
        newly_created,
        newly_stopped,
    }
}

fn feed_view(store: &Store, command: &PodcastOperation) -> Result<Option<PodcastFeedView>> {
    match command {
        PodcastOperation::RefreshStatus { id } => {
            let refresh = store.podcast_refresh(id)?;
            let snapshot = store.podcast_snapshot(&refresh.subscription_id)?;
            Ok(Some(PodcastFeedView {
                refresh: Some(refresh_view(refresh)),
                snapshot: snapshot.map(snapshot_view),
                episodes: Vec::new(),
                next_after: None,
            }))
        }
        PodcastOperation::Episodes {
            subscription_id,
            after,
            limit,
        } => {
            let entries = store.podcast_episodes(subscription_id, after.as_deref(), *limit)?;
            let next_after = if let Some(last) = entries.last()
                && !store
                    .podcast_episodes(subscription_id, Some(&last.id), 1)?
                    .is_empty()
            {
                Some(last.id.clone())
            } else {
                None
            };
            Ok(Some(PodcastFeedView {
                refresh: None,
                snapshot: store.podcast_snapshot(subscription_id)?.map(snapshot_view),
                episodes: entries.into_iter().map(episode_view).collect(),
                next_after,
            }))
        }
        _ => Ok(None),
    }
}

fn refresh_view(
    refresh: crate::storage::podcast_feeds::PodcastRefreshStatus,
) -> PodcastRefreshView {
    PodcastRefreshView {
        id: refresh.id,
        subscription_id: refresh.subscription_id,
        state: refresh.state,
        started_ms: refresh.started_ms,
        completed_ms: refresh.completed_ms,
        committed_items: refresh.committed_items,
        truncated: refresh.truncated,
        live_count: refresh.live_count,
        skipped_items: refresh.skipped_items,
        failure: refresh.failure,
    }
}

fn snapshot_view(
    snapshot: crate::storage::podcast_feeds::PodcastSnapshotRecord,
) -> PodcastSnapshotView {
    PodcastSnapshotView {
        refresh_id: snapshot.refresh_id,
        observed_ms: snapshot.observed_ms,
        committed_items: snapshot.committed_items,
        truncated: snapshot.truncated,
        live_count: snapshot.live_count,
    }
}

fn episode_view(episode: PodcastEpisodeRecord) -> PodcastEpisodeView {
    PodcastEpisodeView {
        id: episode.id,
        identity: episode.identity,
        title: episode.title,
        published_ms: episode.published_ms,
        enclosure: episode.enclosure,
        enclosure_type: episode.enclosure_type,
        transcripts: episode.transcripts,
        chapters: episode.chapters,
        in_latest: episode.in_latest,
    }
}

fn view_of(subscription: PodcastSubscription) -> PodcastView {
    PodcastView {
        id: subscription.id,
        origin: subscription.origin,
        network: subscription.network,
        redirects: subscription.redirects,
        polls: subscription.polls,
        created_ms: subscription.created_ms,
    }
}

impl From<PodcastOperation> for Operation {
    fn from(command: PodcastOperation) -> Self {
        Self::Podcast { command }
    }
}
