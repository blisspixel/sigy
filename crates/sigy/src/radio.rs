use crate::style::{Ink, Tone, tone_for_state};
use clap::{Args, Subcommand, ValueEnum};
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
    #[arg(long, default_value = "", hide_default_value = true)]
    name: String,
    /// Country name or two-letter code. Ambiguous names require an explicit code.
    #[arg(long, default_value = "", hide_default_value = true)]
    country: String,
    /// Whole directory language label, for example french. Not detected speech.
    #[arg(long, default_value = "", hide_default_value = true)]
    language: String,
    /// Whole directory tag, for example news.
    #[arg(long, default_value = "", hide_default_value = true)]
    tag: String,
    /// Keep only entries whose most recent directory check succeeded.
    #[arg(long)]
    healthy: bool,
}

impl Filters {
    fn filter(&self) -> sigy_service::Result<StationFilter> {
        Ok(StationFilter {
            name: self.name.clone(),
            country: sigy_service::discovery::countries::resolve(&self.country)?,
            language: self.language.clone(),
            tag: self.tag.clone(),
            healthy_only: self.healthy,
        })
    }
}

#[derive(Debug, Subcommand)]
pub enum RadioCommand {
    /// Browse the bundled worldwide country reference, without opening a library or network.
    Countries {
        /// Country name substring or two-letter code. Blank lists the full reference.
        #[arg(default_value = "")]
        query: String,
        /// Display locale: en, fr, es, ar, hi, zh, pt, sw. Others explicitly fall back to en.
        #[arg(long, default_value = "en")]
        locale: String,
        /// Continue a country page with the same query, locale and reference.
        #[arg(long)]
        after: Option<String>,
        /// Print the unchanged legal notices for the bundled reference data.
        #[arg(long)]
        licenses: bool,
    },
    /// Refresh one bounded directory page in the background. Never contacts station streams.
    Refresh {
        /// New request ID, for example news-001. Reusing an ID never sends another request.
        id: String,
        #[command(flatten)]
        filters: Filters,
        /// Stations to request, 1 to 500.
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
        limit: u32,
        /// Directory rows to skip before this page, 0 to 100000.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=100_000))]
        offset: u32,
        /// Explicit mirror origin. Omit to discover public Radio Browser mirrors.
        #[arg(long)]
        mirror: Option<String>,
        /// Explicit exact-IP grant for this mirror only, not its station entries.
        #[arg(long, requires = "mirror")]
        pin_address: Option<IpAddr>,
    },
    /// Inspect one refresh request and its accepted/skipped counts.
    RefreshStatus {
        /// The ID given to radio refresh.
        id: String,
    },
    /// Inspect local cache size and the most recent refresh.
    Status,
    /// Search cached stations without making network requests.
    Search {
        #[command(flatten)]
        filters: Filters,
        /// Only stations explicitly saved as favorites. Combines with all other filters.
        #[arg(long)]
        favorites: bool,
        /// id preserves UUID order; name uses the pinned Unicode name key with UUID ties.
        #[arg(long, value_enum, default_value_t = StationOrder::Id)]
        order: StationOrder,
        /// Stations per page, 1 to 16.
        #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u32).range(1..=16))]
        limit: u32,
        /// With id: station UUID. With name: opaque cursor, same filters and limit; changed cache requires restart without --after.
        #[arg(long, value_parser = search_cursor)]
        after: Option<String>,
    },
    /// Inspect cached station metadata. Does not tune or record.
    Show {
        /// Station UUID from radio search.
        id: String,
    },
    /// Inspect exact historical registrations and their recording metadata, without tuning.
    Linked {
        /// Canonical station UUID. Historical registrations remain readable after cache removal.
        id: String,
    },
    /// Save a cached station as a favorite without tuning or recording.
    Favorite {
        /// Station UUID from radio search.
        id: String,
    },
    /// Remove a favorite without deleting the station, source or recordings.
    Unfavorite {
        /// Station UUID from radio search.
        id: String,
    },
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
    ClickStatus {
        /// The ID given to radio click.
        id: String,
    },
    /// Register a selected station as an immutable public-internet source revision.
    Add {
        /// Station UUID from radio search.
        id: String,
        /// New source revision key, for example station:v1. Record it with record start --source.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StationOrder {
    Id,
    Name,
}

fn search_cursor(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > sigy_service::discovery::ordered::MAX_CURSOR_BYTES {
        return Err("station continuation exceeds its bounded cursor size".into());
    }
    Ok(value.into())
}

#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Store one bounded page and an interval of 1 to 168 hours. This does not fetch.
    Set {
        /// Policy name. Saving the same name again revises it.
        id: String,
        #[command(flatten)]
        filters: Filters,
        /// Stations to request per attempt, 1 to 500.
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
        limit: u32,
        /// Directory rows to skip before this page, 0 to 100000.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=100_000))]
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
    Show {
        /// Policy name. Omit to list every saved policy.
        id: Option<String>,
    },
    /// Remove a saved policy. Cached stations, favorites, and refresh history stay.
    Clear {
        /// Policy name.
        id: String,
    },
}

impl RadioCommand {
    pub fn operation(&self) -> sigy_service::Result<Operation> {
        Ok(match self {
            Self::Countries { .. } => {
                return Err(sigy_service::Error::InvalidInput(
                    "country reference is an offline client operation",
                ));
            }
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
                    filter: filters.filter()?,
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
                order,
            } => match order {
                StationOrder::Id => DirectoryOperation::Search {
                    filter: filters.filter()?,
                    favorites_only: *favorites,
                    after: after.clone(),
                    limit: *limit,
                },
                StationOrder::Name => DirectoryOperation::SearchOrdered {
                    filter: filters.filter()?,
                    favorites_only: *favorites,
                    after: after.clone(),
                    limit: *limit,
                },
            },
            Self::Show { id } => DirectoryOperation::Show { id: id.clone() },
            Self::Linked { id } => DirectoryOperation::Linked {
                id: id.clone(),
                catalog: None,
            },
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
        .into())
    }
}

fn policy_operation(command: &PolicyCommand) -> sigy_service::Result<Operation> {
    Ok(match command {
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
                filter: filters.filter()?,
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
    .into())
}

pub fn offline(command: &RadioCommand, json: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let RadioCommand::Countries {
        query,
        locale,
        after,
        licenses,
    } = command
    else {
        return Ok(false);
    };
    if *licenses {
        let mut writer = io::stdout().lock();
        if json {
            serde_json::to_writer(
                &mut writer,
                &serde_json::json!({"reference_version": sigy_service::discovery::countries::VERSION, "licenses": sigy_service::discovery::countries::LICENSES}),
            )?;
            writeln!(writer)?;
        } else {
            write!(writer, "{}", sigy_service::discovery::countries::LICENSES)?;
        }
        return Ok(true);
    }
    let page = sigy_service::discovery::countries::page(query, locale, after.as_deref())?;
    let mut writer = io::stdout().lock();
    if json {
        serde_json::to_writer(&mut writer, &page)?;
        writeln!(writer)?;
    } else {
        write_countries(&mut writer, &page)?;
    }
    Ok(true)
}

fn write_countries(
    writer: &mut impl Write,
    page: &sigy_service::discovery::countries::Page,
) -> io::Result<()> {
    writeln!(
        writer,
        "Countries and territories: {} | locale {}{} | {} candidates",
        page.reference_version,
        page.display_locale,
        if page.locale_fallback {
            " (requested locale unavailable; English fallback)"
        } else {
            ""
        },
        page.total_candidates
    )?;
    for entry in &page.entries {
        writeln!(
            writer,
            "{}: {}{}",
            entry.code,
            clean(&entry.name),
            if entry.listed {
                ""
            } else {
                " (raw provider code; absent from reference)"
            }
        )?;
        if let (Some(alias), Some(locale)) = (&entry.matched_alias, &entry.matched_locale) {
            writeln!(
                writer,
                "  Matched alias ({}): {}",
                clean(locale),
                clean(alias)
            )?;
        }
    }
    writeln!(
        writer,
        "Select an explicit --country CODE. Station availability is unknown until a cache query; this reference supplies no station counts."
    )?;
    if let Some(after) = &page.next_after {
        writeln!(
            writer,
            "Continue with the same query and locale: --after {}",
            clean(after)
        )?;
    }
    Ok(())
}

pub fn render(writer: &mut impl Write, view: &Snapshot, ink: Ink) -> io::Result<()> {
    if let Some(page) = &view.linked_station_context {
        writeln!(
            writer,
            "Linked station {} | catalog {} revision {}",
            clean(&page.station_id),
            clean(&page.catalog.namespace),
            page.catalog.revision
        )?;
        for line in crate::explorer::context::linked_lines(page) {
            writeln!(writer, "{line}")?;
        }
    }
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
                "Cache needs a newer page. Favorites and unseen stations stay. Next: sigy radio refresh NEW_ID --limit 100, with a new ID for each fetch"
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
    if let Some(page) = &view.ordered_station_page {
        write_ordered_stations(writer, page, ink)?;
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
            "No cached matches. Check your filters, or fetch another directory page with sigy radio refresh NEW_ID. Language and tag filters match whole directory labels, ignoring case."
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

fn write_ordered_stations(
    writer: &mut impl Write,
    page: &sigy_service::control::OrderedStationPage,
    ink: Ink,
) -> io::Result<()> {
    writeln!(
        writer,
        "Name order, UUID ties | partial local cache revision {}.",
        page.catalog.revision
    )?;
    let legacy = sigy_service::control::StationPage {
        entries: page.entries.clone(),
        favorite_ids: page.favorite_ids.clone(),
        next_after: None,
    };
    write_stations(writer, &legacy, ink)?;
    if let Some(cursor) = &page.next_after {
        writeln!(
            writer,
            "Continue with --order name, the same filters and --limit, and --after {}",
            crate::explorer::text::sanitize(
                cursor,
                sigy_service::discovery::ordered::MAX_CURSOR_BYTES
            )
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

    #[tokio::test]
    async fn country_command_executes_without_opening_the_selected_library() -> TestResult {
        use clap::Parser;
        let temporary = tempfile::tempdir()?;
        let missing = temporary.path().join("never-initialized");
        let mut cli = crate::Cli::try_parse_from([
            std::ffi::OsStr::new("sigy"),
            std::ffi::OsStr::new("--data-dir"),
            missing.as_os_str(),
            std::ffi::OsStr::new("--json"),
            std::ffi::OsStr::new("radio"),
            std::ffi::OsStr::new("countries"),
            std::ffi::OsStr::new("Canada"),
        ])?;
        crate::init::resolve_directory(&mut cli)?;
        Box::pin(crate::run(&cli)).await?;
        assert!(!missing.exists());
        let mut notices =
            crate::Cli::try_parse_from(["sigy", "--json", "radio", "countries", "--licenses"])?;
        crate::init::resolve_directory(&mut notices)?;
        assert!(notices.data_dir.is_none());
        Box::pin(crate::run(&notices)).await?;
        Ok(())
    }

    #[test]
    fn offline_country_cli_and_filter_names_share_the_bounded_resolver() -> TestResult {
        use clap::Parser;
        let mut cli =
            crate::Cli::try_parse_from(["sigy", "radio", "countries", "Congo", "--locale", "fr"])?;
        crate::init::resolve_directory(&mut cli)?;
        assert!(cli.data_dir.is_none());
        let crate::Command::Radio {
            command:
                RadioCommand::Countries {
                    query,
                    locale,
                    after,
                    ..
                },
        } = cli.command
        else {
            return Err("country command".into());
        };
        let page = sigy_service::discovery::countries::page(&query, &locale, after.as_deref())?;
        let mut output = Vec::new();
        write_countries(&mut output, &page)?;
        let text = String::from_utf8(output)?;
        assert!(text.contains("2 candidates"));
        assert!(text.contains("CD:") && text.contains("CG:"));
        assert!(text.contains("Station availability is unknown"));
        let cli = crate::Cli::try_parse_from(["sigy", "radio", "search", "--country", "Canada"])?;
        let crate::Command::Radio { command } = cli.command else {
            return Err("radio command".into());
        };
        assert!(
            matches!(command.operation()?, Operation::Radio { command: DirectoryOperation::Search { filter, limit: 16, .. } } if filter.country == "CA")
        );
        let cli = crate::Cli::try_parse_from(["sigy", "radio", "search", "--country", "Congo"])?;
        let crate::Command::Radio { command } = cli.command else {
            return Err("radio command".into());
        };
        assert!(command.operation().is_err());
        Ok(())
    }

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
    fn explicit_name_order_keeps_legacy_id_default_and_bounds_opaque_input() -> TestResult {
        use clap::Parser;
        for (order, ordered) in [(None, false), (Some("id"), false), (Some("name"), true)] {
            let mut words = vec![
                "sigy",
                "radio",
                "search",
                "--after",
                "00000000-0000-4000-8000-000000000001",
            ];
            if let Some(order) = order {
                words.extend(["--order", order]);
            }
            let cli = crate::Cli::try_parse_from(words)?;
            let crate::Command::Radio { command } = cli.command else {
                return Err("radio command".into());
            };
            assert_eq!(
                matches!(
                    command.operation()?,
                    Operation::Radio {
                        command: DirectoryOperation::SearchOrdered { .. }
                    }
                ),
                ordered
            );
        }
        let cursor = "a".repeat(sigy_service::discovery::ordered::MAX_CURSOR_BYTES);
        assert!(
            crate::Cli::try_parse_from([
                "sigy", "radio", "search", "--order", "name", "--after", &cursor
            ])
            .is_ok()
        );
        assert!(
            crate::Cli::try_parse_from([
                "sigy",
                "radio",
                "search",
                "--order",
                "name",
                "--after",
                &(cursor + "a")
            ])
            .is_err()
        );
        assert!(
            crate::Cli::try_parse_from(["sigy", "radio", "search", "--order", "popularity"])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn ordered_output_and_json_preserve_the_complete_bounded_cursor() -> TestResult {
        let cursor = "af".repeat(4096);
        let page = sigy_service::control::OrderedStationPage {
            entries: Vec::new(),
            favorite_ids: Vec::new(),
            next_after: Some(cursor.clone()),
            catalog: sigy_service::control::DirectoryCatalog {
                namespace: "a".repeat(32),
                revision: 7,
                comparison: sigy_service::discovery::ordered::COMPARISON.into(),
            },
        };
        let mut bytes = Vec::new();
        write_ordered_stations(&mut bytes, &page, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains(&cursor));
        assert!(text.contains("--order name"));
        assert!(text.contains("No cached matches"));
        let restored: sigy_service::control::OrderedStationPage =
            serde_json::from_str(&serde_json::to_string(&page)?)?;
        assert_eq!(restored.next_after.as_deref(), Some(cursor.as_str()));
        Ok(())
    }

    #[test]
    fn page_bounds_are_refused_by_the_parser_before_any_request() {
        use clap::Parser;
        let parse = |words: &[&str]| crate::Cli::try_parse_from(words);
        assert!(parse(&["sigy", "radio", "search", "--limit", "16"]).is_ok());
        for words in [
            &["sigy", "radio", "search", "--limit", "0"][..],
            &["sigy", "radio", "search", "--limit", "17"],
            &["sigy", "radio", "refresh", "a", "--limit", "501"],
            &["sigy", "radio", "refresh", "a", "--offset", "100001"],
            &["sigy", "radio", "policy", "set", "a", "--limit", "0"],
        ] {
            let error = parse(words).err().map(|error| error.to_string());
            assert!(
                error
                    .as_deref()
                    .is_some_and(|text| text.contains("is not in")),
                "{words:?}: {error:?}"
            );
        }
        assert!(parse(&["sigy", "radio", "refresh", "a", "--limit", "500"]).is_ok());
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
