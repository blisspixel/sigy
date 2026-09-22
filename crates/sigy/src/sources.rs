use std::{
    io::{self, Write},
    net::IpAddr,
};

use clap::Subcommand;
use sigy_service::{
    control::{Operation, PlaylistOperation, PlaylistView, SourcePage},
    sources::{NetworkScope, RedirectPolicy},
};

#[derive(Subcommand)]
pub enum SourceCommand {
    /// Store an immutable HTTP audio configuration. Does not connect or record.
    Add {
        /// Unique revision key, for example station:v1. Reuse cannot change it.
        revision_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        url: String,
        /// Explicitly pin this revision to one public, private or loopback IP.
        #[arg(long)]
        pin_address: Option<IpAddr>,
        /// Redirect scope: deny, same-origin, or public (at most three hops).
        #[arg(long, default_value = "deny")]
        redirects: RedirectPolicy,
    },
    /// List a bounded page of registered revisions, with URL paths omitted.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 16)]
        limit: u32,
    },
    /// Inspect a revision's origin and network grant. URL paths are omitted.
    Show { revision_id: String },
    /// Resolve one playlist document or accept one stored entry.
    Playlist {
        #[command(subcommand)]
        command: PlaylistCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlaylistCommand {
    /// Read one playlist through the service. Does not open entry URLs or play audio.
    Resolve {
        /// Unique request ID. Exact replay never fetches again.
        id: String,
        /// Parent source revision that names the playlist document.
        #[arg(long)]
        revision: String,
    },
    /// Inspect a playlist request. Paths and queries are omitted.
    Status { id: String },
    /// Register one resolved entry as an audio revision. Performs no network I/O.
    Accept {
        id: String,
        /// Zero-based index in the resolved list.
        #[arg(long)]
        index: u32,
        /// New immutable source revision key.
        #[arg(long)]
        revision: String,
        /// Display name for the new revision. Playlist titles are not copied.
        #[arg(long)]
        name: String,
    },
}

impl std::fmt::Debug for SourceCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.operation(), f)
    }
}

impl SourceCommand {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Add {
                revision_id,
                name,
                url,
                pin_address,
                redirects,
            } => Operation::RegisterSource {
                revision_id: revision_id.clone(),
                name: name.clone(),
                url: url.clone(),
                network: pin_address.map_or(NetworkScope::PublicInternet {}, |address| {
                    NetworkScope::PinnedAddress { address }
                }),
                redirects: *redirects,
            },
            Self::List { after, limit } => Operation::ListSources {
                after: after.clone(),
                limit: *limit,
            },
            Self::Show { revision_id } => Operation::ShowSource {
                revision_id: revision_id.clone(),
            },
            Self::Playlist { command } => command.operation(),
        }
    }
}

impl PlaylistCommand {
    fn operation(&self) -> Operation {
        match self {
            Self::Resolve { id, revision } => PlaylistOperation::Resolve {
                id: id.clone(),
                revision_id: revision.clone(),
            }
            .into(),
            Self::Status { id } => PlaylistOperation::Status { id: id.clone() }.into(),
            Self::Accept {
                id,
                index,
                revision,
                name,
            } => PlaylistOperation::Accept {
                id: id.clone(),
                index: *index,
                revision_id: revision.clone(),
                name: name.clone(),
            }
            .into(),
        }
    }
}

pub fn render(writer: &mut impl Write, page: &SourcePage) -> io::Result<()> {
    if let Some(created) = page.newly_created {
        writeln!(
            writer,
            "{} No connection or recording started.",
            if created {
                "Source revision registered."
            } else {
                "Source revision already registered."
            }
        )?;
    }
    for entry in &page.entries {
        let policy = match entry.network {
            NetworkScope::PublicInternet {} => "public internet".into(),
            NetworkScope::PinnedAddress { address } => format!("pinned address {address}"),
        };
        writeln!(
            writer,
            "{}: {} | {} | {} | redirects: {}",
            entry.revision_id, entry.name, entry.origin, policy, entry.redirects
        )?;
    }
    if page.entries.is_empty() {
        writeln!(writer, "No source revisions on this page.")?;
    }
    if let Some(after) = &page.next_after {
        writeln!(writer, "Continue with source list --after {after}")?;
    }
    Ok(())
}

pub fn render_playlist(writer: &mut impl Write, playlist: &PlaylistView) -> io::Result<()> {
    writeln!(
        writer,
        "Playlist {}: {} | parent {} | {} entries",
        playlist.id,
        playlist.state,
        playlist.parent_revision,
        playlist.entries.len()
    )?;
    writeln!(writer, "Started at {} ms", playlist.started_ms)?;
    if let Some(completed) = playlist.completed_ms {
        writeln!(writer, "Completed at {completed} ms")?;
    }
    if let Some(origin) = &playlist.final_origin {
        writeln!(writer, "Document origin: {origin}")?;
    }
    if let Some(hash) = &playlist.document_sha256 {
        writeln!(writer, "Document hash: {hash}")?;
    }
    if let Some(failure) = &playlist.failure {
        writeln!(writer, "Reason: {failure}")?;
    }
    for entry in &playlist.entries {
        writeln!(writer, "  {}: {}", entry.index, entry.origin)?;
    }
    for acceptance in &playlist.acceptances {
        writeln!(
            writer,
            "Accepted index {} as {}",
            acceptance.index, acceptance.child_revision
        )?;
    }
    Ok(())
}
