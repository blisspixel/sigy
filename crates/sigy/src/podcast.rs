use std::{
    io::{self, Write},
    net::IpAddr,
};

use clap::Subcommand;
use sigy_service::{
    control::{Operation, PodcastOperation, PodcastPage},
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
