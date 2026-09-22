//! Bounded metadata reads through the same destination and transport policy.

use super::{AcquisitionLimits, HttpAcquirer};
use crate::{Error, Result, sources::HttpSource};
use std::{fmt, io::Read, time::Duration};
use tokio::time::{Instant, timeout, timeout_at};

/// One decoded feed body. Callers parse it and do not fetch URLs inside it.
pub(crate) struct FeedDocument {
    pub document: Vec<u8>,
    pub final_url: reqwest::Url,
}

impl fmt::Debug for FeedDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FeedDocument")
            .field("bytes", &self.document.len())
            .field("origin", &self.final_url.origin().ascii_serialization())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FeedEncoding {
    Identity,
    Gzip,
    Deflate,
}

pub(crate) const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const DOCUMENT_DEADLINE: Duration = Duration::from_secs(8);
pub(crate) const MAX_PLAYLIST_BYTES: usize = 64 * 1024;
const MAX_ENCODED_FEED: usize = 2 * 1024 * 1024;
const MAX_FEED_DOCUMENT: usize = 8 * 1024 * 1024;
const MAX_FEED_EXPANSION: usize = 16;
const PLAYLIST_ACCEPT: &str = "audio/x-mpegurl, audio/mpegurl, application/x-mpegurl, application/vnd.apple.mpegurl, audio/x-scpls";
const FEED_ACCEPT: &str = "application/rss+xml, application/xml, text/xml";

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
                .open(
                    source,
                    limits,
                    deadline,
                    "application/json",
                    super::AcceptEncoding::Identity,
                    crate::sources::icy::MetadataPolicy::Off,
                )
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
            let (mut response, _) = self
                .open(
                    source,
                    limits,
                    deadline,
                    PLAYLIST_ACCEPT,
                    super::AcceptEncoding::Identity,
                    crate::sources::icy::MetadataPolicy::Off,
                )
                .await?;
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

    /// Reads one RSS document. The decoded bytes are not a recording or a grant.
    /// # Errors
    /// Fails closed on capacity, type, compression, size, or deadline.
    pub(crate) async fn feed_document(&self, source: &HttpSource) -> Result<FeedDocument> {
        let _slot = self
            .attempts
            .try_acquire()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let limits = AcquisitionLimits::new(MAX_FEED_DOCUMENT as u64, DOCUMENT_DEADLINE)?;
        let deadline = Instant::now() + limits.duration;
        timeout_at(deadline, async {
            let (mut response, _) = self
                .open(
                    source,
                    limits,
                    deadline,
                    FEED_ACCEPT,
                    super::AcceptEncoding::Feed,
                    crate::sources::icy::MetadataPolicy::Off,
                )
                .await?;
            if response.headers().contains_key("icy-metaint") {
                return Err(Error::Acquisition(
                    "interleaved ICY metadata is unsupported",
                ));
            }
            let encoding = feed_encoding(response.headers())?;
            feed_content_type(response.headers())?;
            let wire_cap = match encoding {
                FeedEncoding::Identity => MAX_FEED_DOCUMENT,
                FeedEncoding::Gzip | FeedEncoding::Deflate => MAX_ENCODED_FEED,
            };
            if response
                .content_length()
                .is_some_and(|bytes| bytes > wire_cap as u64)
            {
                return Err(Error::Acquisition("feed size limit"));
            }
            let final_url = response.url().clone();
            let mut wire = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::Acquisition("feed body interrupted"))?
            {
                if chunk.len() > wire_cap.saturating_sub(wire.len()) {
                    return Err(Error::Acquisition("feed size limit"));
                }
                wire.extend_from_slice(&chunk);
            }
            if wire.is_empty() {
                return Err(Error::Acquisition("empty feed document"));
            }
            let document = decode_feed(&wire, encoding)?;
            Ok(FeedDocument {
                document,
                final_url,
            })
        })
        .await
        .map_err(|_| Error::Acquisition("feed deadline"))?
    }
}

fn feed_encoding(headers: &reqwest::header::HeaderMap) -> Result<FeedEncoding> {
    let values = headers.get_all(reqwest::header::CONTENT_ENCODING);
    let count = values.iter().count();
    if count == 0 {
        return Ok(FeedEncoding::Identity);
    }
    if count != 1 {
        return Err(Error::Acquisition("encoded feed is unsupported"));
    }
    let value = values
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .ok_or(Error::Acquisition("encoded feed is unsupported"))?;
    if value.contains(',') || value.contains(';') {
        return Err(Error::Acquisition("encoded feed is unsupported"));
    }
    match value.trim().to_ascii_lowercase().as_str() {
        "identity" => Ok(FeedEncoding::Identity),
        "gzip" | "x-gzip" => Ok(FeedEncoding::Gzip),
        "deflate" => Ok(FeedEncoding::Deflate),
        _ => Err(Error::Acquisition("encoded feed is unsupported")),
    }
}

fn feed_content_type(headers: &reqwest::header::HeaderMap) -> Result<()> {
    let types = headers.get_all(reqwest::header::CONTENT_TYPE);
    if types.iter().count() != 1 {
        return Err(Error::Acquisition("expected an RSS document"));
    }
    let raw = types
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .ok_or(Error::Acquisition("expected an RSS document"))?;
    let mut parts = raw.split(';');
    let mime = parts.next().map_or("", str::trim).to_ascii_lowercase();
    if !matches!(
        mime.as_str(),
        "application/rss+xml" | "application/xml" | "text/xml"
    ) {
        return Err(Error::Acquisition("expected an RSS document"));
    }
    for parameter in parts {
        let Some((name, value)) = parameter.trim().split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("charset") {
            let charset = value.trim().trim_matches('"');
            if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("us-ascii") {
                return Err(Error::Acquisition("feed document is not UTF-8"));
            }
        }
    }
    Ok(())
}

fn decode_feed(wire: &[u8], encoding: FeedEncoding) -> Result<Vec<u8>> {
    let document = match encoding {
        FeedEncoding::Identity => {
            if wire.len() > MAX_FEED_DOCUMENT {
                return Err(Error::Acquisition("feed size limit"));
            }
            wire.to_vec()
        }
        FeedEncoding::Gzip => {
            let mut decoder = flate2::bufread::GzDecoder::new(wire);
            let document = inflate(&mut decoder, wire.len())?;
            if !decoder.get_ref().is_empty() {
                return Err(Error::Acquisition("feed compression is invalid"));
            }
            document
        }
        FeedEncoding::Deflate => {
            let mut decoder = flate2::bufread::ZlibDecoder::new(wire);
            let document = inflate(&mut decoder, wire.len())?;
            if !decoder.get_ref().is_empty() {
                return Err(Error::Acquisition("feed compression is invalid"));
            }
            document
        }
    };
    if document.is_empty() || document.len() > MAX_FEED_DOCUMENT {
        return Err(Error::Acquisition("feed size limit"));
    }
    Ok(document)
}

fn inflate<R: Read>(mut decoder: R, compressed: usize) -> Result<Vec<u8>> {
    let cap = compressed
        .saturating_mul(MAX_FEED_EXPANSION)
        .min(MAX_FEED_DOCUMENT);
    if cap == 0 {
        return Err(Error::Acquisition("feed expansion limit"));
    }
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8_192];
    loop {
        let read = match decoder.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => return Err(Error::Acquisition("feed compression is invalid")),
        };
        if output.len().saturating_add(read) > cap {
            return Err(Error::Acquisition("feed expansion limit"));
        }
        output.extend_from_slice(&buffer[..read]);
    }
    if output.is_empty() {
        return Err(Error::Acquisition("empty feed document"));
    }
    Ok(output)
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

#[cfg(test)]
mod tests {
    use super::{FeedEncoding, decode_feed};
    use crate::{
        Error,
        sources::{HttpSource, NetworkScope, http::HttpAcquirer},
    };
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn gzip(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes)?;
        encoder.finish()
    }

    #[test]
    fn expansion_and_trailing_bytes_fail_closed() -> crate::Result<()> {
        let bomb = gzip(&vec![0_u8; 4_096]).map_err(|_| Error::Acquisition("gzip"))?;
        assert!(matches!(
            decode_feed(&bomb, FeedEncoding::Gzip),
            Err(Error::Acquisition("feed expansion limit"))
        ));
        let mut trailing = gzip(b"<rss/>").map_err(|_| Error::Acquisition("gzip"))?;
        trailing.push(b'x');
        assert!(matches!(
            decode_feed(&trailing, FeedEncoding::Gzip),
            Err(Error::Acquisition("feed compression is invalid"))
        ));
        let xml = b"<?xml version=\"1.0\"?><rss version=\"2.0\"/>";
        let encoded = gzip(xml).map_err(|_| Error::Acquisition("gzip"))?;
        assert_eq!(decode_feed(&encoded, FeedEncoding::Gzip)?, xml);
        Ok(())
    }

    #[tokio::test]
    async fn feed_document_decodes_one_gzip_body() -> crate::Result<()> {
        let xml = b"<rss version=\"2.0\"><channel/></rss>";
        let body = gzip(xml).map_err(|_| Error::Acquisition("gzip"))?;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml; charset=utf-8\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let mut request = Vec::new();
            let Ok(Ok((mut socket, _))) =
                tokio::time::timeout(Duration::from_secs(2), listener.accept()).await
            else {
                return request;
            };
            while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                match tokio::time::timeout(Duration::from_secs(2), socket.read_u8()).await {
                    Ok(Ok(byte)) => request.push(byte),
                    _ => break,
                }
            }
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.write_all(&body).await;
            request
        });
        let source = HttpSource::new(
            "Feed",
            &format!(
                "http://fixture.invalid:{}/feed.xml?token=hidden",
                address.port()
            ),
            NetworkScope::PinnedAddress {
                address: address.ip(),
            },
        )?;
        let document = HttpAcquirer::default().feed_document(&source).await?;
        assert_eq!(document.document, xml);
        let received = server.await.map_err(|_| Error::Acquisition("server"))?;
        let request = String::from_utf8_lossy(&received);
        assert!(
            request
                .to_ascii_lowercase()
                .contains("accept-encoding: gzip, deflate, identity")
        );
        assert!(request.contains("GET /feed.xml?token=hidden"));
        Ok(())
    }
}
