use std::{
    io::{self, Write},
    net::IpAddr,
};

use crate::explorer::text::sanitize;
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
    /// Fetch one stored transcript or chapter document. Does not change the recording quota.
    Text {
        subscription: String,
        #[arg(long)]
        episode: String,
        /// `transcript` or `chapters`.
        #[arg(long)]
        kind: String,
        /// Zero-based index among that episode's stored assets of this kind.
        #[arg(long)]
        index: u32,
        /// Request id. Reuse does not fetch again.
        #[arg(long)]
        id: String,
    },
    /// Show one fetched publisher document. Cue times are not media time.
    TextShow { id: String },
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
            Self::Text {
                subscription,
                episode,
                kind,
                index,
                id,
            } => PodcastOperation::Text {
                id: id.clone(),
                subscription_id: subscription.clone(),
                episode_id: episode.clone(),
                kind: kind.clone(),
                index: *index,
            }
            .into(),
            Self::TextShow { id } => PodcastOperation::TextShow { id: id.clone() }.into(),
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
            sanitize(&entry.id, 256),
            sanitize(&entry.origin, 512),
            policy,
            entry.redirects,
            entry.polls
        )?;
    }
    if page.entries.is_empty() {
        writeln!(writer, "No podcast subscriptions on this page.")?;
    }
    if let Some(after) = &page.next_after {
        writeln!(
            writer,
            "Continue with podcast list --after {}",
            sanitize(after, 256)
        )?;
    }
    Ok(())
}

pub fn render_text(
    writer: &mut impl Write,
    text: &sigy_service::control::PublisherTextView,
) -> io::Result<()> {
    writeln!(
        writer,
        "Publisher text {}: {} | {} | alignment {} | attribution {}",
        sanitize(&text.id, 256),
        sanitize(&text.state, 64),
        sanitize(&text.kind, 64),
        sanitize(&text.alignment, 64),
        sanitize(&text.attribution, 64)
    )?;
    if let Some(failure) = &text.failure {
        writeln!(writer, "Reason: {}", sanitize(failure, 512))?;
    }
    for cue in &text.cues {
        writeln!(
            writer,
            "{} ms: {}",
            cue.publisher_start_ms,
            sanitize(&cue.text, 4096)
        )?;
    }
    Ok(())
}

pub fn render_feed(writer: &mut impl Write, feed: &PodcastFeedView) -> io::Result<()> {
    if let Some(refresh) = &feed.refresh {
        writeln!(
            writer,
            "Feed refresh {}: {} | items {} | truncated {} | live {} | skipped {}",
            sanitize(&refresh.id, 256),
            sanitize(&refresh.state, 64),
            refresh.committed_items,
            if refresh.truncated { "yes" } else { "no" },
            refresh.live_count,
            refresh.skipped_items
        )?;
        if let Some(failure) = &refresh.failure {
            writeln!(writer, "Reason: {}", sanitize(failure, 512))?;
        }
    }
    if let Some(snapshot) = &feed.snapshot {
        writeln!(
            writer,
            "Latest snapshot {} | items {} | truncated {} | live {}",
            sanitize(&snapshot.refresh_id, 256),
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
            "{}: {identity} | {} | enclosure {} | transcripts {} | chapters {} | latest {}",
            sanitize(&episode.id, 256),
            sanitize(title, 512),
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
        writeln!(
            writer,
            "Continue with podcast episodes --after {}",
            sanitize(after, 256)
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use sigy_service::control::{PodcastView, PublisherTextView};
    use sigy_service::storage::podcasts::PodcastPolls;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Parser)]
    struct Only {
        #[command(subcommand)]
        command: PodcastCommand,
    }

    fn assert_safe(text: &str) {
        assert!(
            !text
                .chars()
                .any(|character| character.is_control() && character != '\n')
        );
        assert!(!text.contains('\u{202e}'));
        assert!(!text.contains('\u{2066}'));
    }

    #[test]
    fn subscription_authority_is_explicit_and_debug_omits_the_feed_url() -> TestResult {
        let url = "https://example.test/private/feed.xml?secret=hidden";
        let default = Only::try_parse_from(["sigy", "subscribe", "feed:v1", "--url", url])?;
        let debug = format!("{:?}", default.command);
        assert!(!debug.contains("hidden"));
        assert!(!debug.contains("private"));
        match default.command.operation() {
            Operation::Podcast {
                command:
                    PodcastOperation::Subscribe {
                        id,
                        url: actual,
                        network,
                        redirects,
                    },
            } => {
                assert_eq!(id, "feed:v1");
                assert_eq!(actual, url);
                assert!(matches!(network, NetworkScope::PublicInternet {}));
                assert_eq!(redirects, RedirectPolicy::Deny);
            }
            _ => return Err("subscription changed operation".into()),
        }
        let pinned = Only::try_parse_from([
            "sigy",
            "subscribe",
            "feed:v1",
            "--url",
            url,
            "--pin-address",
            "127.0.0.1",
            "--redirects",
            "same-origin",
        ])?;
        match pinned.command.operation() {
            Operation::Podcast {
                command:
                    PodcastOperation::Subscribe {
                        network, redirects, ..
                    },
            } => {
                assert!(matches!(network, NetworkScope::PinnedAddress { address }
                    if address == std::net::Ipv4Addr::LOCALHOST));
                assert_eq!(redirects, RedirectPolicy::SameOrigin);
            }
            _ => return Err("pinned subscription changed operation".into()),
        }
        assert!(
            Only::try_parse_from([
                "sigy",
                "subscribe",
                "feed:v1",
                "--url",
                url,
                "--pin-address",
                "not-an-address",
            ])
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn publisher_fetch_and_download_preserve_stored_identities_and_read_commands() -> TestResult {
        let download = Only::try_parse_from([
            "sigy",
            "download",
            "feed:v1",
            "--episode",
            "episode:v2",
            "--id",
            "record:v3",
            "--revision",
            "audio:v4",
        ])?;
        let operation = serde_json::to_value(download.command.operation())?;
        assert_eq!(operation["command"]["action"], "download");
        assert_eq!(operation["command"]["subscription_id"], "feed:v1");
        assert_eq!(operation["command"]["episode_id"], "episode:v2");
        assert_eq!(operation["command"]["id"], "record:v3");
        assert_eq!(operation["command"]["revision_id"], "audio:v4");
        let text = Only::try_parse_from([
            "sigy",
            "text",
            "feed:v1",
            "--episode",
            "episode:v2",
            "--kind",
            "transcript",
            "--index",
            "2",
            "--id",
            "text:v3",
        ])?;
        let operation = serde_json::to_value(text.command.operation())?;
        assert_eq!(operation["command"]["action"], "text");
        assert_eq!(operation["command"]["episode_id"], "episode:v2");
        assert_eq!(operation["command"]["index"], 2);
        assert_eq!(operation["command"]["id"], "text:v3");
        for (command, action) in [
            ("show", "show"),
            ("text-show", "text_show"),
            ("refresh-status", "refresh_status"),
        ] {
            let parsed = Only::try_parse_from(["sigy", command, "saved:v1"])?;
            let operation = serde_json::to_value(parsed.command.operation())?;
            assert_eq!(operation["command"]["action"], action);
            assert_eq!(operation["command"]["id"], "saved:v1");
        }
        Ok(())
    }

    fn publisher_text() -> Result<PublisherTextView, serde_json::Error> {
        serde_json::from_value(serde_json::json!({
            "id": "text:v1", "subscription_id": "feed:v1", "episode_id": "episode:v1",
            "kind": "transcript", "asset_index": 0, "state": "completed",
            "origin": "https://example.test", "media_type": "text/vtt", "language_hint": "ar",
            "document_sha256": "ab".repeat(32), "alignment": "unverified\u{2066}",
            "attribution": "publisher\u{1b}[2J",
            "cues": [{ "publisher_start_ms": 1234, "publisher_end_ms": 5678, "speaker": "speaker",
                "text": "تقارير 中文 Diné\u{1b}]52;c;Zm9v\u{7}\n\u{202e}hidden" }],
            "failure": "failure\u{1b}[31m\rforged line"
        }))
    }

    #[test]
    fn publisher_display_sanitizes_scripts_and_reasons_without_changing_json() -> TestResult {
        let page = publisher_text()?;
        let original = serde_json::to_value(&page)?;
        let mut output = Vec::new();
        render_text(&mut output, &page)?;
        let rendered = String::from_utf8(output)?;
        assert_safe(&rendered);
        assert!(rendered.contains("alignment unverified | attribution publisher"));
        assert!(rendered.contains("1234 ms: تقارير 中文 Diné"));
        assert!(rendered.contains("Reason: failure"));
        assert!(!rendered.contains("media time"));
        assert!(!rendered.contains("machine-recognized"));
        assert_eq!(serde_json::to_value(&page)?, original);
        assert!(render_text(&mut io::Cursor::new(&mut [0_u8; 1][..]), &page).is_err());
        Ok(())
    }

    fn feed() -> Result<PodcastFeedView, serde_json::Error> {
        serde_json::from_value(serde_json::json!({
            "refresh": { "id": "failed:v2", "subscription_id": "feed:v1", "state": "failed",
                "started_ms": 2, "completed_ms": 3, "committed_items": 0, "truncated": false,
                "live_count": 0, "skipped_items": 1, "failure": "bad document\u{1b}[2J\nforged" },
            "snapshot": { "refresh_id": "good:v1", "observed_ms": 1, "committed_items": 2,
                "truncated": true, "live_count": 1 },
            "episodes": [
                { "id": "episode:v1", "identity": "publisher_guid",
                    "title": "عنوان 中文\u{1b}]0;rename\u{7}\u{202e}", "published_ms": null,
                    "enclosure": true, "enclosure_type": "audio/mpeg", "transcripts": 1,
                    "chapters": 2, "in_latest": true },
                { "id": "episode:v2", "identity": "derived_enclosure", "title": null,
                    "published_ms": null, "enclosure": false, "enclosure_type": null,
                    "transcripts": 0, "chapters": 0, "in_latest": false }
            ], "next_after": "episode:v2"
        }))
    }

    #[test]
    fn failed_feed_keeps_prior_snapshot_and_safe_titles_without_changing_evidence() -> TestResult {
        let page = feed()?;
        let original = serde_json::to_value(&page)?;
        let mut output = Vec::new();
        render_feed(&mut output, &page)?;
        let text = String::from_utf8(output)?;
        assert_safe(&text);
        assert!(text.contains("Feed refresh failed:v2: failed"));
        assert!(text.contains("Latest snapshot good:v1 | items 2 | truncated yes | live 1"));
        assert!(text.contains("publisher guid | عنوان 中文"));
        assert!(text.contains("derived enclosure | no title | enclosure no"));
        assert!(text.contains("Continue with podcast episodes --after episode:v2"));
        assert_eq!(serde_json::to_value(&page)?, original);
        assert!(render_feed(&mut io::Cursor::new(&mut [0_u8; 1][..]), &page).is_err());
        let empty = PodcastFeedView {
            refresh: None,
            snapshot: None,
            episodes: vec![],
            next_after: None,
        };
        let mut output = Vec::new();
        render_feed(&mut output, &empty)?;
        let text = String::from_utf8(output)?;
        assert!(text.contains("No feed snapshot yet."));
        assert!(text.contains("No episodes on this page."));
        Ok(())
    }

    #[test]
    fn subscription_status_distinguishes_idempotence_and_keeps_grants_visible() -> TestResult {
        for changed in [true, false] {
            let page = PodcastPage {
                entries: vec![PodcastView {
                    id: "feed:v1\u{1b}[2J".into(),
                    origin: "https://example.test\u{202e}".into(),
                    network: NetworkScope::PinnedAddress {
                        address: std::net::Ipv4Addr::LOCALHOST.into(),
                    },
                    redirects: RedirectPolicy::Deny,
                    polls: PodcastPolls::Stopped,
                    created_ms: 0,
                }],
                next_after: Some("feed:v1\nforged".into()),
                newly_created: Some(changed),
                newly_stopped: Some(changed),
            };
            let original = serde_json::to_value(&page)?;
            let mut output = Vec::new();
            render(&mut output, &page)?;
            let text = String::from_utf8(output)?;
            assert_safe(&text);
            assert!(text.contains("No DNS lookup and no capture started."));
            assert!(text.contains("Nothing was deleted."));
            assert!(text.contains("pinned address 127.0.0.1 | redirects: deny | polls: stopped"));
            assert_eq!(text.contains("already stored"), !changed);
            assert_eq!(text.contains("already stopped"), !changed);
            assert_eq!(serde_json::to_value(&page)?, original);
            assert!(render(&mut io::Cursor::new(&mut [0_u8; 1][..]), &page).is_err());
        }
        Ok(())
    }
}
