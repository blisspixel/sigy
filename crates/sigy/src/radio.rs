use clap::{Args, Subcommand};
use sigy_service::{
    control::{DirectoryOperation, Operation, Snapshot},
    discovery::{ClickRequest, RefreshRequest, StationFilter},
    sources::{NetworkScope, RedirectPolicy},
};
use std::{
    io::{self, Write},
    net::IpAddr,
    time::SystemTime,
};

#[derive(Debug, Clone, Args)]
pub struct Filters {
    /// Station name substring. Local search uses Unicode lowercase matching.
    #[arg(long, default_value = "")]
    name: String,
    /// Two-letter country code, independent of spoken language.
    #[arg(long, default_value = "")]
    country: String,
    /// Exact directory language label, for example french. Not detected speech.
    #[arg(long, default_value = "")]
    language: String,
    /// Exact directory tag, for example news.
    #[arg(long, default_value = "")]
    tag: String,
    /// Keep only entries whose most recent directory check succeeded.
    #[arg(long)]
    healthy: bool,
}

impl Filters {
    fn filter(&self) -> StationFilter {
        StationFilter {
            name: self.name.clone(),
            country: self.country.to_ascii_uppercase(),
            language: self.language.clone(),
            tag: self.tag.clone(),
            healthy_only: self.healthy,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum RadioCommand {
    /// Refresh one bounded directory page in the background. Never contacts station streams.
    Refresh {
        /// Unique request ID. Exact replay never sends another request.
        id: String,
        #[command(flatten)]
        filters: Filters,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Explicit mirror origin. Omit to discover public Radio Browser mirrors.
        #[arg(long)]
        mirror: Option<String>,
        /// Explicit exact-IP grant for this mirror only, not its station entries.
        #[arg(long, requires = "mirror")]
        pin_address: Option<IpAddr>,
    },
    /// Inspect one refresh request and its accepted/skipped counts.
    RefreshStatus { id: String },
    /// Inspect local cache size and the most recent refresh.
    Status,
    /// Search cached stations without making network requests.
    Search {
        #[command(flatten)]
        filters: Filters,
        /// Only stations explicitly saved as favorites. Combines with all other filters.
        #[arg(long)]
        favorites: bool,
        #[arg(long, default_value_t = 16)]
        limit: u32,
        #[arg(long)]
        after: Option<String>,
    },
    /// Inspect cached station metadata. Does not tune or record.
    Show { id: String },
    /// Save a cached station as a favorite without tuning or recording.
    Favorite { id: String },
    /// Remove a favorite without deleting the station, source or recordings.
    Unfavorite { id: String },
    /// Report one directory click. This does not play, record, or vote.
    Click {
        /// Unique request ID. Reuse does not send the click again.
        id: String,
        /// Cached station UUID.
        #[arg(long)]
        station: String,
        /// Explicit mirror origin. Omit to choose one public mirror.
        #[arg(long)]
        mirror: Option<String>,
        /// Explicit exact-IP grant for this mirror only.
        #[arg(long, requires = "mirror")]
        pin_address: Option<IpAddr>,
    },
    /// Inspect one click request. The provider stream URL is not shown.
    ClickStatus { id: String },
    /// Register a selected station as an immutable public-internet source revision.
    Add {
        id: String,
        #[arg(long)]
        revision: String,
        /// Redirect scope: deny, same-origin, or public (at most three hops).
        #[arg(long, default_value = "deny")]
        redirects: RedirectPolicy,
    },
}

impl RadioCommand {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Refresh {
                id,
                filters,
                limit,
                offset,
                mirror,
                pin_address,
            } => DirectoryOperation::Refresh {
                id: id.clone(),
                request: RefreshRequest {
                    filter: filters.filter(),
                    limit: *limit,
                    offset: *offset,
                    mirror: mirror.clone(),
                    network: pin_address.map_or(NetworkScope::PublicInternet {}, |address| {
                        NetworkScope::PinnedAddress { address }
                    }),
                },
            },
            Self::RefreshStatus { id } => DirectoryOperation::RefreshStatus { id: id.clone() },
            Self::Status => DirectoryOperation::Status {},
            Self::Search {
                filters,
                favorites,
                limit,
                after,
            } => DirectoryOperation::Search {
                filter: filters.filter(),
                favorites_only: *favorites,
                after: after.clone(),
                limit: *limit,
            },
            Self::Show { id } => DirectoryOperation::Show { id: id.clone() },
            Self::Favorite { id } => DirectoryOperation::SetFavorite {
                id: id.clone(),
                favorite: true,
            },
            Self::Unfavorite { id } => DirectoryOperation::SetFavorite {
                id: id.clone(),
                favorite: false,
            },
            Self::Click {
                id,
                station,
                mirror,
                pin_address,
            } => DirectoryOperation::Click {
                id: id.clone(),
                request: ClickRequest {
                    station_id: station.clone(),
                    mirror: mirror.clone(),
                    network: pin_address.map_or(NetworkScope::PublicInternet {}, |address| {
                        NetworkScope::PinnedAddress { address }
                    }),
                },
            },
            Self::ClickStatus { id } => DirectoryOperation::ClickStatus { id: id.clone() },
            Self::Add {
                id,
                revision,
                redirects,
            } => DirectoryOperation::Add {
                id: id.clone(),
                revision_id: revision.clone(),
                redirects: *redirects,
            },
        }
        .into()
    }
}

pub fn render(writer: &mut impl Write, view: &Snapshot) -> io::Result<()> {
    if let Some(status) = &view.directory {
        writeln!(
            writer,
            "Radio Browser cache: {} / {} stations | favorites: {}. Partial observations, not the complete world catalog.",
            status.cached_stations, status.maximum_stations, status.favorite_stations
        )?;
        if let Some(refresh) = view
            .directory_refresh
            .as_ref()
            .or(status.latest_refresh.as_ref())
        {
            writeln!(
                writer,
                "Refresh {}: {} | accepted {} | skipped {} | started {}",
                refresh.id,
                refresh.state,
                refresh.accepted,
                refresh.skipped,
                age(refresh.started_ms)
            )?;
            if let Some(failure) = &refresh.failure {
                writeln!(writer, "Reason: {failure}")?;
            }
        }
    }
    if let Some(click) = &view.directory_click {
        writeln!(
            writer,
            "Click {}: {} | station {} | acknowledged {}",
            click.id,
            click.state,
            click.station_id,
            if click.acknowledged { "yes" } else { "no" }
        )?;
        if let Some(failure) = &click.failure {
            writeln!(writer, "Reason: {failure}")?;
        }
    }
    if let Some(page) = &view.station_page {
        for station in &page.entries {
            let favorite = if page.favorite_ids.contains(&station.id) {
                " [favorite]"
            } else {
                ""
            };
            writeln!(
                writer,
                "{}: {}{favorite} | {} | {} | bitrate {} | directory languages: {}",
                station.id,
                station.name,
                known(&station.country),
                known(&station.codec),
                if station.bitrate_kbps == 0 {
                    "unknown".into()
                } else {
                    format!("{} kbps", station.bitrate_kbps)
                },
                known(&station.languages.join(", "))
            )?;
            writeln!(
                writer,
                "  Tags: {} | observed {} | origin {}",
                known(&station.tags.join(", ")),
                age(station.observed_ms),
                station.stream_origin
            )?;
            if station.hls {
                writeln!(
                    writer,
                    "  HLS stream: current recording adapter does not support this format."
                )?;
            }
        }
        if page.entries.is_empty() {
            writeln!(
                writer,
                "No cached matches. Check your filters, save favorites with radio favorite, or fetch observations with radio refresh."
            )?;
        }
        if let Some(after) = &page.next_after {
            writeln!(writer, "Continue with the same filters and --after {after}")?;
        }
    }
    Ok(())
}

fn known(value: &str) -> &str {
    if value.is_empty() { "unknown" } else { value }
}

fn age(observed_ms: i64) -> String {
    let Some(elapsed_ms) = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|now| {
            u128::try_from(observed_ms)
                .ok()
                .and_then(|then| now.as_millis().checked_sub(then))
        })
    else {
        return "unknown age (clock mismatch)".into();
    };
    let seconds = elapsed_ms / 1000;
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}
