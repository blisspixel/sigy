//! Local podcast subscription commands. No fetch and no capture.

use serde::{Deserialize, Serialize};

use super::{Operation, Snapshot};
use crate::{
    Error, Result,
    sources::{NetworkScope, RedirectPolicy},
    storage::{
        Store,
        podcasts::{PodcastPolls, PodcastSubscription},
    },
};

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

pub(super) fn apply(store: &mut Store, command: PodcastOperation) -> Result<Snapshot> {
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
