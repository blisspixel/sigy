//! Bounded metadata reads through the same destination and transport policy.

use super::{AcquisitionLimits, HttpAcquirer};
use crate::{Error, Result, sources::HttpSource};
use std::{fmt, time::Duration};
use tokio::time::{Instant, timeout, timeout_at};

pub(crate) const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const DOCUMENT_DEADLINE: Duration = Duration::from_secs(8);
pub(crate) const MAX_PLAYLIST_BYTES: usize = 64 * 1024;
const PLAYLIST_ACCEPT: &str = "audio/x-mpegurl, audio/mpegurl, application/x-mpegurl, application/vnd.apple.mpegurl, audio/x-scpls";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaylistKind {
    M3u,
    Pls,
}

pub(crate) struct PlaylistDocument {
    pub bytes: Vec<u8>,
    pub final_url: reqwest::Url,
    pub kind: PlaylistKind,
}

impl fmt::Debug for PlaylistDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlaylistDocument")
            .field("bytes", &self.bytes.len())
            .field("origin", &self.final_url.origin().ascii_serialization())
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl HttpAcquirer {
    pub(crate) async fn json_document(&self, source: &HttpSource) -> Result<Vec<u8>> {
        self.read_json(source, MAX_DOCUMENT_BYTES).await
    }

    /// A small JSON acknowledgement. The caller must not treat the body as a source.
    pub(crate) async fn acknowledgement(&self, source: &HttpSource) -> Result<Vec<u8>> {
        self.read_json(source, 8 * 1024).await
    }

    async fn read_json(&self, source: &HttpSource, maximum: usize) -> Result<Vec<u8>> {
        let _slot = self
            .attempts
            .try_acquire()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let limits = AcquisitionLimits::new(maximum as u64, DOCUMENT_DEADLINE)?;
        let deadline = Instant::now() + limits.duration;
        timeout_at(deadline, async {
            let (mut response, _) = self
                .open(source, limits, deadline, "application/json")
                .await?;
            for encoding in response
                .headers()
                .get_all(reqwest::header::CONTENT_ENCODING)
            {
                if !encoding.as_bytes().eq_ignore_ascii_case(b"identity") {
                    return Err(Error::Acquisition("encoded metadata is unsupported"));
                }
            }
            let types = response.headers().get_all(reqwest::header::CONTENT_TYPE);
            if types.iter().count() != 1
                || !types
                    .iter()
                    .next()
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.split(';').next())
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
            {
                return Err(Error::Acquisition("expected JSON metadata"));
            }
            if response
                .content_length()
                .is_some_and(|bytes| bytes > maximum as u64)
            {
                return Err(Error::Acquisition("metadata size limit"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::Acquisition("metadata body interrupted"))?
            {
                if chunk.len() > maximum - bytes.len() {
                    return Err(Error::Acquisition("metadata size limit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| Error::Acquisition("metadata deadline"))?
    }

    pub(crate) async fn service_hosts(&self, name: &str) -> Result<Vec<String>> {
        let _slot = self
            .dns
            .try_acquire()
            .map_err(|_| Error::Acquisition("DNS capacity reached"))?;
        timeout(Duration::from_secs(5), async {
            let resolver = hickory_resolver::Resolver::builder_tokio()
                .and_then(hickory_resolver::ResolverBuilder::build)
                .map_err(|_| Error::Acquisition("DNS configuration"))?;
            let lookup = resolver
                .srv_lookup(name)
                .await
                .map_err(|_| Error::Acquisition("directory mirror discovery"))?;
            let mut hosts = Vec::new();
            for record in lookup.answers().iter().take(17) {
                if let hickory_resolver::proto::rr::RData::SRV(srv) = &record.data {
                    hosts.push(srv.target.to_utf8().trim_end_matches('.').to_lowercase());
                }
            }
            if hosts.is_empty() || hosts.len() > 16 {
                return Err(Error::Acquisition("directory mirror count"));
            }
            Ok(hosts)
        })
        .await
        .map_err(|_| Error::Acquisition("directory DNS deadline"))?
    }

    /// Reads one playlist document. Callers resolve entries without fetching them.
    /// # Errors
    /// Uses the source redirect policy and fails closed on capacity, type, size, or deadline.
    pub(crate) async fn playlist_document(&self, source: &HttpSource) -> Result<PlaylistDocument> {
        let _slot = self
            .attempts
            .try_acquire()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let limits = AcquisitionLimits::new(MAX_PLAYLIST_BYTES as u64, DOCUMENT_DEADLINE)?;
        let deadline = Instant::now() + limits.duration;
        timeout_at(deadline, async {
            let (mut response, _) = self.open(source, limits, deadline, PLAYLIST_ACCEPT).await?;
            let kind = playlist_kind(response.headers())?;
            if response
                .content_length()
                .is_some_and(|bytes| bytes > MAX_PLAYLIST_BYTES as u64)
            {
                return Err(Error::Acquisition("playlist size limit"));
            }
            let final_url = response.url().clone();
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::Acquisition("playlist body interrupted"))?
            {
                if chunk.len() > MAX_PLAYLIST_BYTES.saturating_sub(bytes.len()) {
                    return Err(Error::Acquisition("playlist size limit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes.is_empty() {
                return Err(Error::Acquisition("empty playlist"));
            }
            Ok(PlaylistDocument {
                bytes,
                final_url,
                kind,
            })
        })
        .await
        .map_err(|_| Error::Acquisition("playlist deadline"))?
    }
}

fn playlist_kind(headers: &reqwest::header::HeaderMap) -> Result<PlaylistKind> {
    for encoding in headers.get_all(reqwest::header::CONTENT_ENCODING) {
        if !encoding.as_bytes().eq_ignore_ascii_case(b"identity") {
            return Err(Error::Acquisition("encoded playlist is unsupported"));
        }
    }
    if headers.contains_key("icy-metaint") {
        return Err(Error::Acquisition(
            "interleaved ICY metadata is unsupported",
        ));
    }
    let types = headers.get_all(reqwest::header::CONTENT_TYPE);
    if types.iter().count() != 1 {
        return Err(Error::Acquisition("missing or ambiguous playlist type"));
    }
    let mime = types
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .ok_or(Error::Acquisition("invalid playlist content type"))?;
    match mime.to_ascii_lowercase().as_str() {
        "audio/x-mpegurl"
        | "audio/mpegurl"
        | "application/x-mpegurl"
        | "application/vnd.apple.mpegurl" => Ok(PlaylistKind::M3u),
        "audio/x-scpls" => Ok(PlaylistKind::Pls),
        _ => Err(Error::Acquisition("unsupported playlist content type")),
    }
}
