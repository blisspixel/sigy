//! One RSS 2.0 document. Feed text does not grant a fetch, a recording, or playback.

mod date;
mod identity;
mod rss;
mod text;

use crate::{Error, Result, sources::HttpSource};
pub(crate) use identity::{EpisodeIdentity, document_sha256, normalize_http_url};
pub(crate) use rss::{AssetRef, ParsedEpisode, ParsedFeed, parse};
pub(crate) use text::{PublisherCue, TextKind, normalize_text_type};

/// Bytes already limited and decoded, plus the items safe to commit.
#[derive(Debug)]
pub(crate) struct FeedCommit {
    pub sha256: String,
    pub parsed: ParsedFeed,
}

/// Reads one feed on the shared acquirer and parses it. No enclosure request.
/// # Errors
/// Fails closed on transport, compression, or document rejection. The caller
/// keeps the previous snapshot.
pub(crate) async fn fetch(
    acquirer: &crate::sources::http::HttpAcquirer,
    source: &HttpSource,
) -> Result<FeedCommit> {
    let body = acquirer.feed_document(source).await?;
    let document = std::str::from_utf8(&body.document)
        .map_err(|_| Error::InvalidInput("feed document is not UTF-8"))?;
    let parsed = parse(document, Some(&body.final_url))?;
    Ok(FeedCommit {
        sha256: document_sha256(&body.document),
        parsed,
    })
}

/// Reads and parses one stored publisher asset. Nested URLs are not requested.
/// # Errors
/// Fails closed on transport or document rejection. The caller keeps prior text.
pub(crate) async fn fetch_text(
    acquirer: &crate::sources::http::HttpAcquirer,
    source: &HttpSource,
    kind: TextKind,
    media_type: &str,
) -> Result<crate::storage::podcast_text::TextDocument> {
    let (bytes, final_url, response_type) = acquirer.publisher_text(source).await?;
    if response_type != media_type {
        return Err(Error::InvalidInput("publisher text type"));
    }
    let cues = text::parse(kind, &response_type, &bytes)?;
    Ok(crate::storage::podcast_text::TextDocument {
        origin: final_url.origin().ascii_serialization(),
        sha256: document_sha256(&bytes),
        cues,
    })
}
