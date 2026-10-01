use crate::style::{Ink, Tone, tone_for_state};
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
    /// Save one directory page for the service to refresh. An open client does not run it.
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Store one bounded page and an interval of 1 to 168 hours. This does not fetch.
    Set {
        id: String,
        #[command(flatten)]
        filters: Filters,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Explicit mirror origin. Omit to discover public Radio Browser mirrors when the slot is due.
        #[arg(long)]
        mirror: Option<String>,
        /// Explicit exact-IP grant for this mirror only, not its station entries.
        #[arg(long, requires = "mirror")]
        pin_address: Option<IpAddr>,
        /// Hours between attempts. The first attempt waits this long.
        #[arg(long, default_value_t = 24, value_parser = 1..=168)]
        every_hours: i64,
    },
    /// Show one saved policy, or every saved policy when the id is omitted.
    Show { id: Option<String> },
    /// Remove a saved policy. Cached stations, favorites, and refresh history stay.
    Clear { id: String },
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
            Self::Policy { command } => return policy_operation(command),
        }
        .into()
    }
}

fn policy_operation(command: &PolicyCommand) -> Operation {
    match command {
        PolicyCommand::Set {
            id,
            filters,
            limit,
            offset,
            mirror,
            pin_address,
            every_hours,
        } => DirectoryOperation::SetPolicy {
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
            interval_ms: *every_hours * 3_600_000,
        },
        PolicyCommand::Show { id } => DirectoryOperation::ShowPolicy { id: id.clone() },
        PolicyCommand::Clear { id } => DirectoryOperation::ClearPolicy { id: id.clone() },
    }
    .into()
}

pub fn render(writer: &mut impl Write, view: &Snapshot, ink: Ink) -> io::Result<()> {
    if let Some(status) = &view.directory {
        writeln!(
            writer,
            "Radio Browser cache: {} / {} stations | favorites: {} | stale: {}. Partial observations, not the complete world catalog.",
            status.cached_stations,
            status.maximum_stations,
            status.favorite_stations,
            ink.tint(
                if status.stale_stations > 0 {
                    Tone::Warn
                } else {
                    Tone::Plain
                },
                &status.stale_stations.to_string(),
            )
        )?;
        if status.cached_stations == 0 || status.stale_stations > 0 {
            writeln!(
                writer,
                "Cache needs a newer page. Favorites and unseen stations stay. Next: radio refresh NEW_ID --limit 100"
            )?;
        } else if let Some(newest) = status.newest_observed_ms {
            writeln!(writer, "Newest directory observation: {}.", age(newest))?;
        }
        if let Some(refresh) = view
            .directory_refresh
            .as_ref()
            .or(status.latest_refresh.as_ref())
        {
            writeln!(
                writer,
                "Refresh {}: {} | accepted {} | skipped {} | started {}",
                clean(&refresh.id),
                ink.tint(tone_for_state(&refresh.state), &clean(&refresh.state)),
                refresh.accepted,
                refresh.skipped,
                age(refresh.started_ms)
            )?;
            if let Some(failure) = &refresh.failure {
                writeln!(
                    writer,
                    "{}",
                    ink.tint(Tone::Fail, &format!("Reason: {}", clean(failure)))
                )?;
            }
        }
    }
    if let Some(click) = &view.directory_click {
        writeln!(
            writer,
            "Click {}: {} | station {} | acknowledged {}",
            clean(&click.id),
            ink.tint(tone_for_state(&click.state), &clean(&click.state)),
            clean(&click.station_id),
            if click.acknowledged { "yes" } else { "no" }
        )?;
        if let Some(failure) = &click.failure {
            writeln!(
                writer,
                "{}",
                ink.tint(Tone::Fail, &format!("Reason: {}", clean(failure)))
            )?;
        }
    }
    if let Some(page) = &view.directory_policy
        && (page.listed || page.disposition.is_some())
    {
        write_policies(writer, page, ink)?;
    }
    if let Some(page) = &view.station_page {
        write_stations(writer, page, ink)?;
    }
    Ok(())
}

fn write_policies(
    writer: &mut impl Write,
    page: &sigy_service::control::DirectoryPolicyPage,
    ink: Ink,
) -> io::Result<()> {
    let disposition = match page.disposition {
        Some(sigy_service::control::PolicyDisposition::Created) => {
            "Policy saved. The service refreshes it when the interval has elapsed."
        }
        Some(sigy_service::control::PolicyDisposition::Unchanged) => "Policy unchanged.",
        Some(sigy_service::control::PolicyDisposition::Revised) => {
            "Policy revised. The next interval starts now."
        }
        None => "Saved directory policies. An open client does not refresh them.",
    };
    writeln!(writer, "{disposition}")?;
    if page.policies.is_empty() {
        writeln!(writer, "No saved directory policy.")?;
    }
    for policy in &page.policies {
        let hours = policy.interval_ms / 3_600_000;
        writeln!(
            writer,
            "{}: every {hours} h | revision {} | {}",
            clean(&policy.id),
            policy.revision,
            ink.tint(Tone::Warn, &next_due(policy.next_due_ms))
        )?;
        if let Some(refresh) = &policy.last_refresh {
            writeln!(
                writer,
                "  Last attempt {}: {}.",
                clean(&refresh.id),
                ink.tint(tone_for_state(&refresh.state), &clean(&refresh.state))
            )?;
        }
    }
    Ok(())
}

fn next_due(next_due_ms: i64) -> String {
    let Some(now_ms) = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|now| i64::try_from(now.as_millis()).ok())
    else {
        return "due time unknown".into();
    };
    if next_due_ms <= now_ms {
        return "due".into();
    }
    let minutes = (next_due_ms - now_ms) / 60_000;
    if minutes < 60 {
        format!("due in {minutes}m")
    } else {
        format!("due in {}h", minutes / 60)
    }
}

fn write_stations(
    writer: &mut impl Write,
    page: &sigy_service::control::StationPage,
    ink: Ink,
) -> io::Result<()> {
    for station in &page.entries {
        let favorite = if page.favorite_ids.contains(&station.id) {
            ink.tint(Tone::Warn, " [favorite]")
        } else {
            String::new()
        };
        writeln!(
            writer,
            "{}: {}{favorite} | {} | {} | bitrate {} | directory languages: {}",
            clean(&station.id),
            clean(&station.name),
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
            clean(&station.stream_origin)
        )?;
        if station.hls {
            writeln!(
                writer,
                "  HLS stream: register a source revision, then use record hls. Use --live for a live media playlist; master variants require explicit acceptance."
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
        writeln!(
            writer,
            "Continue with the same filters and --after {}",
            clean(after)
        )?;
    }
    Ok(())
}

fn known(value: &str) -> String {
    let text = clean(value);
    if text.is_empty() {
        "unknown".into()
    } else {
        text
    }
}

fn clean(value: &str) -> String {
    crate::explorer::text::sanitize(value, 1024)
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

#[cfg(test)]
mod tests {
    use super::*;
    use sigy_service::{
        control::{PolicyDisposition, StationPage},
        discovery::Station,
        storage::discovery::{DirectoryStatus, RefreshStatus},
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn snapshot() -> Result<Snapshot, serde_json::Error> {
        serde_json::from_value(
            serde_json::json!({"schema_version":39,"sqlite_version":"fixture",
            "provider_dispatch_available":false,"budgets":[],"captures":{"dispatch_available":false,
                "scheduled":0,"active":0,"interrupted":0,"terminal":0}}),
        )
    }

    fn refresh() -> RefreshStatus {
        RefreshStatus {
            id: "refresh\u{1b}[2J".into(),
            request: RefreshRequest {
                filter: StationFilter::default(),
                limit: 1,
                offset: 0,
                mirror: None,
                network: NetworkScope::PublicInternet {},
            },
            state: "failed\u{7}".into(),
            started_ms: -1,
            completed_ms: Some(0),
            accepted: 1,
            skipped: 2,
            mirror_origin: None,
            failure: Some("unavailable\u{1b}]52;c;payload\u{7}\nforged".into()),
        }
    }

    #[test]
    fn station_output_sanitizes_observations_and_points_hls_to_explicit_recording() -> TestResult {
        let station: Station =
            serde_json::from_value(serde_json::json!({"provider":"radio_browser",
            "id":"station\u{1b}[2J","name":"أخبار\u{1b}]52;c;payload\u{7}","country":"",
            "state":"", "languages":["français\r\nforged","Diné bizaad"], "language_codes":[],
            "tags":["news\u{1b}[31m"], "codec":"", "bitrate_kbps":0,"hls":true,"last_check_ok":null,
            "latitude":null,"longitude":null,"stream_origin":"https://radio.invalid\u{7}",
            "observed_ms":-1,"refresh_id":"refresh"}))?;
        let mut page = StationPage {
            favorite_ids: vec![station.id.clone()],
            entries: vec![station],
            next_after: Some("station\u{1b}[2J".into()),
        };
        let mut bytes = Vec::new();
        write_stations(&mut bytes, &page, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("أخبار"));
        assert!(text.contains("français"));
        assert!(text.contains("Diné bizaad"));
        assert!(text.contains("[favorite] | unknown | unknown | bitrate unknown"));
        assert!(text.contains("record hls"));
        assert!(text.contains("--live"));
        assert!(text.contains("master variants require explicit acceptance"));
        assert!(!text.contains("does not support this format"));
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        assert_eq!(text.lines().count(), 4);
        assert!(page.entries[0].name.contains('\u{1b}'));
        page.entries[0].bitrate_kbps = 128;
        page.entries[0].hls = false;
        page.favorite_ids.clear();
        bytes = Vec::new();
        write_stations(&mut bytes, &page, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("128 kbps"));
        assert!(!text.contains("HLS stream"));
        assert!(!text.contains("[favorite]"));
        page.entries.clear();
        bytes = Vec::new();
        write_stations(&mut bytes, &page, Ink::stdout(true))?;
        assert!(String::from_utf8(bytes)?.contains("No cached matches"));
        Ok(())
    }

    #[test]
    fn refresh_click_and_policy_evidence_stay_bounded_and_terminal_safe() -> TestResult {
        let mut view = snapshot()?;
        view.directory = Some(DirectoryStatus {
            cached_stations: 1,
            favorite_stations: 1,
            maximum_stations: 10_000,
            oldest_observed_ms: Some(0),
            newest_observed_ms: Some(0),
            stale_stations: 1,
            latest_refresh: Some(refresh()),
        });
        view.directory_click = Some(sigy_service::storage::ClickStatus {
            id: "click\u{1b}[2J".into(),
            station_id: "station\u{7}".into(),
            state: "future-state\u{1b}[31m".into(),
            started_ms: 0,
            completed_ms: None,
            mirror_origin: None,
            acknowledged: false,
            failure: Some("denied\nforged".into()),
        });
        view.directory_policy = Some(serde_json::from_value(serde_json::json!({
            "policies":[{"id":"policy\u{1b}[2J", "interval_ms":3_600_000,"revision":2,
                "updated_ms":0,"next_due_ms":0,"request":refresh().request,"last_refresh":refresh()}],
            "disposition":"created","listed":true
        }))?);
        let mut bytes = Vec::new();
        render(&mut bytes, &view, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("Partial observations, not the complete world catalog"));
        assert!(text.contains("Favorites and unseen stations stay"));
        assert!(text.contains("accepted 1 | skipped 2"));
        assert!(text.contains("acknowledged no"));
        assert!(text.contains("every 1 h | revision 2 | due"));
        assert!(text.contains("Last attempt refresh"));
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        let directory = view.directory.as_mut().ok_or("directory")?;
        directory.stale_stations = 0;
        directory.latest_refresh = None;
        view.directory_refresh = Some(refresh());
        view.directory_click.as_mut().ok_or("click")?.acknowledged = true;
        let page = view.directory_policy.as_mut().ok_or("policy")?;
        page.policies.clear();
        for disposition in [
            None,
            Some(PolicyDisposition::Revised),
            Some(PolicyDisposition::Unchanged),
        ] {
            page.disposition = disposition;
            write_policies(&mut Vec::new(), page, Ink::stdout(true))?;
        }
        bytes = Vec::new();
        render(&mut bytes, &view, Ink::stdout(true))?;
        assert!(String::from_utf8(bytes)?.contains("acknowledged yes"));
        Ok(())
    }

    #[test]
    fn metadata_clock_and_empty_labels_are_truthful() -> TestResult {
        let now = i64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_millis(),
        )?;
        assert_eq!(known("\u{7}"), "unknown");
        assert_eq!(next_due(-1), "due");
        assert!(next_due(now + 120_000).starts_with("due in "));
        assert!(next_due(now + 7_200_000).ends_with('h'));
        assert_eq!(age(-1), "unknown age (clock mismatch)");
        assert_eq!(age(i64::MAX), "unknown age (clock mismatch)");
        for (duration, suffix) in [
            (0, "s ago"),
            (120_000, "m ago"),
            (7_200_000, "h ago"),
            (172_800_000, "d ago"),
        ] {
            assert!(age(now - duration).ends_with(suffix));
        }
        Ok(())
    }
}
