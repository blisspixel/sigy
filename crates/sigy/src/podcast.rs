use std::{
    io::{self, Write},
    net::IpAddr,
};

use clap::Subcommand;
use sigy_service::{
    control::{Operation, PodcastFeedView, PodcastIdentityKind, PodcastOperation, PodcastPage},
    sources::{NetworkScope, RedirectPolicy},
};

#[derive(Subcommand)]
pub enum PodcastCommand {
    /// Store one feed subscription. Does not resolve DNS, download, or record.
    Subscribe {
        /// Subscription key. Reuse cannot change the URL, scope, pin, or redirects.
        id: String,
        #[arg(long)]
        url: String,
        /// Explicitly pin this subscription to one public, private, or loopback IP.
        #[arg(long)]
        pin_address: Option<IpAddr>,
        /// Redirect scope: deny, same-origin, or public (at most three hops).
        #[arg(long, default_value = "deny")]
        redirects: RedirectPolicy,
    },
    /// Stop future polls. The subscription and every other record stay in place.
    Unsubscribe { id: String },
    /// List a bounded page of subscriptions, with feed paths omitted.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 16)]
        limit: u32,
    },
    /// Inspect one subscription's origin and network grant. Feed paths are omitted.
    Show { id: String },
    /// Refresh one RSS 2.0 document. Does not download enclosures, transcripts, or chapters.
    Refresh {
        /// Subscription to refresh. A stopped subscription is not polled.
        subscription: String,
        /// Unique request ID. Exact replay never sends another request.
        #[arg(long)]
        id: String,
    },
    /// Inspect one feed refresh. Paths and queries are omitted.
    RefreshStatus { id: String },
    /// List stored episodes without contacting the feed. URLs are omitted.
    Episodes {
        subscription: String,
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 16)]
        limit: u32,
    },
    /// Download one enclosure. Reserves 512 MiB and 30 minutes before connecting.
    Download {
        /// Subscription that stored the episode.
        subscription: String,
        /// Episode id from `podcast episodes`. Titles are not accepted.
        #[arg(long)]
        episode: String,
        /// Recording id. Exact replay does not download again.
        #[arg(long)]
        id: String,
        /// Immutable audio revision key for this enclosure URL.
        #[arg(long)]
        revision: String,
    },
}

impl std::fmt::Debug for PodcastCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.operation(), f)
    }
}

impl PodcastCommand {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Subscribe {
                id,
                url,
                pin_address,
                redirects,
            } => PodcastOperation::Subscribe {
                id: id.clone(),
                url: url.clone(),
                network: pin_address.map_or(NetworkScope::PublicInternet {}, |address| {
                    NetworkScope::PinnedAddress { address }
                }),
                redirects: *redirects,
            }
            .into(),
            Self::Unsubscribe { id } => PodcastOperation::Unsubscribe { id: id.clone() }.into(),
            Self::List { after, limit } => PodcastOperation::List {
                after: after.clone(),
                limit: *limit,
            }
            .into(),
            Self::Show { id } => PodcastOperation::Show { id: id.clone() }.into(),
            Self::Refresh { subscription, id } => PodcastOperation::Refresh {
                id: id.clone(),
                subscription_id: subscription.clone(),
            }
            .into(),
            Self::RefreshStatus { id } => PodcastOperation::RefreshStatus { id: id.clone() }.into(),
            Self::Episodes {
                subscription,
                after,
                limit,
            } => PodcastOperation::Episodes {
                subscription_id: subscription.clone(),
                after: after.clone(),
                limit: *limit,
            }
            .into(),
            Self::Download {
                subscription,
                episode,
                id,
                revision,
            } => PodcastOperation::Download {
                id: id.clone(),
                subscription_id: subscription.clone(),
                episode_id: episode.clone(),
                revision_id: revision.clone(),
            }
            .into(),
        }
    }
}

pub fn render(writer: &mut impl Write, page: &PodcastPage) -> io::Result<()> {
    if let Some(created) = page.newly_created {
        writeln!(
            writer,
            "{}",
            if created {
                "Subscribed. No DNS lookup and no capture started."
            } else {
                "Subscription already stored. No DNS lookup and no capture started."
            }
        )?;
    }
    if let Some(stopped) = page.newly_stopped {
        writeln!(
            writer,
            "{}",
            if stopped {
                "Unsubscribed. Future polls are stopped. Nothing was deleted."
            } else {
                "Subscription was already stopped. Nothing was deleted."
            }
        )?;
    }
    for entry in &page.entries {
        let policy = match entry.network {
            NetworkScope::PublicInternet {} => "public internet".to_owned(),
            NetworkScope::PinnedAddress { address } => format!("pinned address {address}"),
        };
        writeln!(
            writer,
            "{}: {} | {} | redirects: {} | polls: {}",
            entry.id, entry.origin, policy, entry.redirects, entry.polls
        )?;
    }
    if page.entries.is_empty() {
        writeln!(writer, "No podcast subscriptions on this page.")?;
    }
    if let Some(after) = &page.next_after {
        writeln!(writer, "Continue with podcast list --after {after}")?;
    }
    Ok(())
}

pub fn render_feed(writer: &mut impl Write, feed: &PodcastFeedView) -> io::Result<()> {
    if let Some(refresh) = &feed.refresh {
        writeln!(
            writer,
            "Feed refresh {}: {} | items {} | truncated {} | live {} | skipped {}",
            refresh.id,
            refresh.state,
            refresh.committed_items,
            if refresh.truncated { "yes" } else { "no" },
            refresh.live_count,
            refresh.skipped_items
        )?;
        if let Some(failure) = &refresh.failure {
            writeln!(writer, "Reason: {failure}")?;
        }
    }
    if let Some(snapshot) = &feed.snapshot {
        writeln!(
            writer,
            "Latest snapshot {} | items {} | truncated {} | live {}",
            snapshot.refresh_id,
            snapshot.committed_items,
            if snapshot.truncated { "yes" } else { "no" },
            snapshot.live_count
        )?;
    } else if feed.refresh.is_none() {
        writeln!(writer, "No feed snapshot yet.")?;
    }
    for episode in &feed.episodes {
        let identity = match episode.identity {
            PodcastIdentityKind::PublisherGuid => "publisher guid",
            PodcastIdentityKind::DerivedEnclosure => "derived enclosure",
        };
        let title = episode.title.as_deref().unwrap_or("no title");
        writeln!(
            writer,
            "{}: {identity} | {title} | enclosure {} | transcripts {} | chapters {} | latest {}",
            episode.id,
            if episode.enclosure { "yes" } else { "no" },
            episode.transcripts,
            episode.chapters,
            if episode.in_latest { "yes" } else { "no" }
        )?;
    }
    if feed.episodes.is_empty() && feed.refresh.is_none() {
        writeln!(writer, "No episodes on this page.")?;
    }
    if let Some(after) = &feed.next_after {
        writeln!(writer, "Continue with podcast episodes --after {after}")?;
    }
    Ok(())
}
