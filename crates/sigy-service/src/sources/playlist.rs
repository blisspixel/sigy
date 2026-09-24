//! One playlist document. Entry targets are authorized, never fetched.

use std::{collections::BTreeMap, fmt};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    HttpSource, RedirectPolicy,
    http::{HttpAcquirer, PlaylistKind},
};
use crate::{Error, Result};

pub(crate) const MAX_PLAYLIST_ENTRIES: usize = 32;

/// What one resolved candidate is. An HLS variant or rendition is still only a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    /// An M3U or PLS entry.
    Entry,
    /// An `#EXT-X-STREAM-INF` variant of an HLS master playlist.
    HlsVariant,
    /// An `#EXT-X-MEDIA` audio rendition with its own URI.
    HlsAudio,
}

impl CandidateKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::HlsVariant => "hls_variant",
            Self::HlsAudio => "hls_audio",
        }
    }

    /// # Errors
    /// Rejects an unknown stored kind.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "entry" => Ok(Self::Entry),
            "hls_variant" => Ok(Self::HlsVariant),
            "hls_audio" => Ok(Self::HlsAudio),
            _ => Err(Error::SourceIntegrity),
        }
    }
}

pub(crate) struct ResolvedEntry {
    pub endpoint: String,
    pub origin: String,
    pub kind: CandidateKind,
    /// Declared peak bits per second of an HLS variant. Publisher text, not a measurement.
    pub bandwidth: Option<u64>,
    /// Declared RFC 6381 codecs of an HLS variant. Publisher text, not a measurement.
    pub codecs: Option<String>,
    /// Whether the declared codecs are all audio. None when the playlist does not say.
    pub audio_only: Option<bool>,
}

impl ResolvedEntry {
    fn plain(source: &HttpSource) -> Self {
        Self {
            endpoint: source.endpoint().to_owned(),
            origin: source.origin(),
            kind: CandidateKind::Entry,
            bandwidth: None,
            codecs: None,
            audio_only: None,
        }
    }
}

pub(crate) struct ResolvedPlaylist {
    pub document_sha256: String,
    pub final_origin: String,
    pub entries: Vec<ResolvedEntry>,
}

impl fmt::Debug for ResolvedPlaylist {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedPlaylist")
            .field("final_origin", &self.final_origin)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// Reads one document and resolves its entries. Does not request those entries.
/// An HLS master playlist resolves to its variants; no variant is fetched or chosen.
/// # Errors
/// Returns acquisition, parser, or destination-policy errors. An HLS media playlist fails.
pub(crate) async fn resolve(
    acquirer: &HttpAcquirer,
    parent: &HttpSource,
) -> Result<ResolvedPlaylist> {
    let document = acquirer.playlist_document(parent).await?;
    let entries = parse_playlist(parent, &document.final_url, document.kind, &document.bytes)?;
    Ok(ResolvedPlaylist {
        document_sha256: document_hash(&document.bytes),
        final_origin: document.final_url.origin().ascii_serialization(),
        entries,
    })
}

fn document_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

pub(crate) fn parse_playlist(
    parent: &HttpSource,
    final_url: &Url,
    kind: PlaylistKind,
    body: &[u8],
) -> Result<Vec<ResolvedEntry>> {
    let lines = playlist_lines(body)?;
    if lines.iter().any(|line| hls_marker(line)) {
        return match kind {
            PlaylistKind::M3u => super::hls::master_candidates(parent, final_url, &lines),
            PlaylistKind::Pls => Err(Error::InvalidInput("HLS playlist is not accepted")),
        };
    }
    match kind {
        PlaylistKind::M3u => parse_m3u(parent, final_url, &lines),
        PlaylistKind::Pls => parse_pls(parent, final_url, &lines),
    }
}

pub(crate) fn entry_matches_policy(
    parent: &HttpSource,
    final_origin: &str,
    candidate: &HttpSource,
) -> Result<()> {
    let candidate_https = candidate.endpoint().starts_with("https://");
    if (parent.endpoint().starts_with("https://") || final_origin.starts_with("https://"))
        && !candidate_https
    {
        return Err(Error::DestinationDenied);
    }
    match parent.redirects() {
        RedirectPolicy::Deny | RedirectPolicy::SameOrigin => {
            if candidate.origin() != final_origin || candidate.origin() != parent.origin() {
                return Err(Error::DestinationDenied);
            }
        }
        RedirectPolicy::Public => (),
    }
    Ok(())
}

pub(crate) fn authorize_entry(
    parent: &HttpSource,
    final_url: &Url,
    reference: &str,
) -> Result<HttpSource> {
    if reference.is_empty()
        || reference.len() > 2048
        || reference.contains('\\')
        || reference
            .chars()
            .any(|character| character.is_whitespace() || super::unsafe_display(character))
    {
        return Err(Error::InvalidInput("playlist entry URL"));
    }
    let url = final_url
        .join(reference)
        .map_err(|_| Error::InvalidInput("playlist entry URL"))?;
    let candidate = HttpSource::new(parent.name(), url.as_str(), parent.network())
        .map_err(|error| match error {
            Error::DestinationDenied => Error::DestinationDenied,
            _ => Error::InvalidInput("playlist entry URL"),
        })?
        .with_redirects(parent.redirects())?;
    entry_matches_policy(
        parent,
        &final_url.origin().ascii_serialization(),
        &candidate,
    )?;
    Ok(candidate)
}

fn parse_m3u(parent: &HttpSource, final_url: &Url, lines: &[String]) -> Result<Vec<ResolvedEntry>> {
    let mut entries = Vec::new();
    for line in lines {
        if m3u_comment(line) {
            continue;
        }
        if line.starts_with('#') {
            return Err(Error::InvalidInput("unsupported playlist directive"));
        }
        push_entry(&mut entries, parent, final_url, line)?;
    }
    if entries.is_empty() {
        return Err(Error::InvalidInput("playlist has no entries"));
    }
    Ok(entries)
}

fn parse_pls(parent: &HttpSource, final_url: &Url, lines: &[String]) -> Result<Vec<ResolvedEntry>> {
    let mut lines = lines.iter();
    if !lines
        .next()
        .is_some_and(|line| line.eq_ignore_ascii_case("[playlist]"))
    {
        return Err(Error::InvalidInput("invalid playlist document"));
    }
    let mut files = BTreeMap::new();
    let mut declared = None;
    for line in lines {
        if line.starts_with('[') {
            return Err(Error::InvalidInput("unsupported playlist directive"));
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(Error::InvalidInput("invalid playlist document"));
        };
        let key = key.trim();
        let value = value.trim();
        if let Some(number) = numbered_key(key, "file")? {
            if value.is_empty() || files.insert(number, value.to_owned()).is_some() {
                return Err(Error::InvalidInput("invalid playlist document"));
            }
        } else if numbered_key(key, "title")?.is_some() || numbered_key(key, "length")?.is_some() {
            // Titles are untrusted display text and are not source authority.
        } else if key.eq_ignore_ascii_case("numberofentries") {
            let count = value
                .parse::<u32>()
                .map_err(|_| Error::InvalidInput("invalid playlist document"))?;
            if usize::try_from(count).is_ok_and(|count| count > MAX_PLAYLIST_ENTRIES) {
                return Err(Error::InvalidInput("playlist entry limit"));
            }
            if declared.replace(count).is_some() {
                return Err(Error::InvalidInput("invalid playlist document"));
            }
        } else if key.eq_ignore_ascii_case("version") {
            if value != "1" && value != "2" {
                return Err(Error::InvalidInput("invalid playlist document"));
            }
        } else {
            return Err(Error::InvalidInput("unsupported playlist directive"));
        }
    }
    let count = u32::try_from(files.len()).unwrap_or(u32::MAX);
    if files.is_empty()
        || files.len() > MAX_PLAYLIST_ENTRIES
        || declared.is_some_and(|declared| declared != count)
        || !(1..=count).all(|number| files.contains_key(&number))
    {
        return Err(Error::InvalidInput(if files.len() > MAX_PLAYLIST_ENTRIES {
            "playlist entry limit"
        } else if files.is_empty() {
            "playlist has no entries"
        } else {
            "invalid playlist document"
        }));
    }
    let mut entries = Vec::new();
    for number in 1..=count {
        let reference = files
            .get(&number)
            .ok_or(Error::InvalidInput("invalid playlist document"))?;
        push_entry(&mut entries, parent, final_url, reference)?;
    }
    Ok(entries)
}

fn push_entry(
    entries: &mut Vec<ResolvedEntry>,
    parent: &HttpSource,
    final_url: &Url,
    reference: &str,
) -> Result<()> {
    if entries.len() == MAX_PLAYLIST_ENTRIES {
        return Err(Error::InvalidInput("playlist entry limit"));
    }
    let source = authorize_entry(parent, final_url, reference)?;
    entries.push(ResolvedEntry::plain(&source));
    Ok(())
}

pub(crate) fn playlist_lines(body: &[u8]) -> Result<Vec<String>> {
    let body = body.strip_prefix("\u{feff}".as_bytes()).unwrap_or(body);
    let text =
        std::str::from_utf8(body).map_err(|_| Error::InvalidInput("invalid playlist document"))?;
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if line.len() > 4096 || line.chars().any(super::unsafe_display) {
            return Err(Error::InvalidInput("invalid playlist document"));
        }
        lines.push(line.to_owned());
    }
    Ok(lines)
}

fn hls_marker(line: &str) -> bool {
    let tag = line.split(':').next().unwrap_or(line);
    tag.get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("#EXT-X-"))
}

fn m3u_comment(line: &str) -> bool {
    line.eq_ignore_ascii_case("#EXTM3U")
        || has_ascii_prefix(line, "#EXTINF:")
        || has_ascii_prefix(line, "#PLAYLIST:")
        || has_ascii_prefix(line, "#EXTGRP:")
        || has_ascii_prefix(line, "#EXTALB:")
        || has_ascii_prefix(line, "#EXTART:")
        || has_ascii_prefix(line, "#EXTGENRE:")
}

pub(crate) fn has_ascii_prefix(line: &str, prefix: &str) -> bool {
    line.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn numbered_key(key: &str, prefix: &str) -> Result<Option<u32>> {
    if !has_ascii_prefix(key, prefix) {
        return Ok(None);
    }
    let Some(rest) = key.get(prefix.len()..) else {
        return Ok(None);
    };
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    if rest.starts_with('0') {
        return Err(Error::InvalidInput("invalid playlist document"));
    }
    let number = rest
        .parse::<u32>()
        .map_err(|_| Error::InvalidInput("invalid playlist document"))?;
    if number > u32::try_from(MAX_PLAYLIST_ENTRIES).unwrap_or(u32::MAX) {
        return Err(Error::InvalidInput("playlist entry limit"));
    }
    Ok(Some(number))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{NetworkScope, RedirectPolicy};

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn parent(url: &str, policy: RedirectPolicy) -> Result<HttpSource> {
        HttpSource::new("Station", url, NetworkScope::PublicInternet {})?.with_redirects(policy)
    }

    fn pinned(url: &str) -> Result<HttpSource> {
        HttpSource::new(
            "Station",
            url,
            NetworkScope::PinnedAddress {
                address: std::net::Ipv4Addr::LOCALHOST.into(),
            },
        )
    }

    #[test]
    fn relative_entries_use_the_final_url_and_parent_policy() -> TestResult {
        let source = parent(
            "https://radio.example/start.m3u",
            RedirectPolicy::SameOrigin,
        )?;
        let final_url = Url::parse("https://radio.example/moved/list.m3u")?;
        let entries = parse_playlist(
            &source,
            &final_url,
            PlaylistKind::M3u,
            b"#EXTM3U\n#EXTINF:-1,Hidden\nclip.mp3\n",
        )?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].endpoint, "https://radio.example/moved/clip.mp3");
        assert_eq!(entries[0].origin, "https://radio.example");
        let denied = parse_playlist(
            &source,
            &final_url,
            PlaylistKind::M3u,
            b"https://other.example/audio\n",
        );
        assert!(matches!(denied, Err(Error::DestinationDenied)));
        let downgrade = parse_playlist(
            &source,
            &final_url,
            PlaylistKind::M3u,
            b"http://radio.example/moved/clip.mp3\n",
        );
        assert!(matches!(downgrade, Err(Error::DestinationDenied)));
        Ok(())
    }

    #[test]
    fn public_entries_may_change_origin_without_downgrade_or_private_targets() -> TestResult {
        let source = parent("https://radio.example/list.m3u", RedirectPolicy::Public)?;
        let final_url = Url::parse("https://cdn.example/list.m3u")?;
        let entries = parse_playlist(
            &source,
            &final_url,
            PlaylistKind::M3u,
            b"https://audio.example/stream\n",
        )?;
        assert_eq!(entries[0].origin, "https://audio.example");
        assert!(
            parse_playlist(
                &source,
                &final_url,
                PlaylistKind::M3u,
                b"http://audio.example/stream\n",
            )
            .is_err()
        );
        assert!(matches!(
            parse_playlist(
                &source,
                &final_url,
                PlaylistKind::M3u,
                b"https://127.0.0.1/stream\n",
            ),
            Err(Error::DestinationDenied)
        ));
        Ok(())
    }

    #[test]
    fn hls_markers_fail_before_entries_are_returned() -> TestResult {
        let source = pinned("http://127.0.0.1:9/list.m3u")?;
        let final_url = Url::parse("http://127.0.0.1:9/list.m3u")?;
        let error = parse_playlist(
            &source,
            &final_url,
            PlaylistKind::M3u,
            b"#EXTM3U\n#EXT-X-TARGETDURATION:10\nhttp://127.0.0.1:9/secret-segment\n",
        );
        assert!(matches!(
            error,
            Err(Error::InvalidInput(
                "HLS media playlist is recorded with record hls"
            ))
        ));
        assert!(
            parse_playlist(
                &source,
                &final_url,
                PlaylistKind::M3u,
                b"#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://127.0.0.1:9/master\n",
            )
            .is_err()
        );
        assert!(matches!(
            parse_playlist(
                &source,
                &final_url,
                PlaylistKind::Pls,
                b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nmaster\n",
            ),
            Err(Error::InvalidInput("HLS playlist is not accepted"))
        ));
        Ok(())
    }

    #[test]
    fn limits_directives_and_pls_order_are_closed() -> TestResult {
        let source = pinned("http://127.0.0.1:9/secret/list.m3u?token=hidden")?;
        let final_url = Url::parse("http://127.0.0.1:9/secret/list.m3u?token=hidden")?;
        let relative = parse_playlist(&source, &final_url, PlaylistKind::M3u, b"live/main\n")?;
        assert_eq!(relative[0].endpoint, "http://127.0.0.1:9/secret/live/main");
        assert_eq!(relative[0].origin, "http://127.0.0.1:9");
        let mut many = String::new();
        for _ in 0..33 {
            many.push_str("clip\n");
        }
        assert!(matches!(
            parse_playlist(&source, &final_url, PlaylistKind::M3u, many.as_bytes()),
            Err(Error::InvalidInput("playlist entry limit"))
        ));
        assert!(
            parse_playlist(
                &source,
                &final_url,
                PlaylistKind::M3u,
                b"#EXTVLCOPT:network-caching=1\nclip\n"
            )
            .is_err()
        );
        let pls = parse_playlist(
            &source,
            &final_url,
            PlaylistKind::Pls,
            b"[playlist]\nNumberOfEntries=2\nFile2=second\nTitle2=Hidden\nFile1=first\nVersion=2\n",
        )?;
        assert_eq!(pls[0].endpoint, "http://127.0.0.1:9/secret/first");
        assert_eq!(pls[1].endpoint, "http://127.0.0.1:9/secret/second");
        assert!(
            parse_playlist(&source, &final_url, PlaylistKind::Pls, b"#EXTM3U\nclip\n").is_err()
        );
        Ok(())
    }
}
