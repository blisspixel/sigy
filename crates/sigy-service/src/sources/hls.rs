//! Finite HLS media playlists. Master playlists and segment URLs never reach the decoder.

use reqwest::Url;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::{
    HttpSource,
    http::{AcquisitionLimits, AudioContentType, HttpAcquirer, TransferEnd, TransferReceipt},
    playlist::{authorize_entry, has_ascii_prefix, playlist_lines},
};
use crate::{Error, Result};

const MAX_SEGMENTS: usize = 32;

pub(crate) async fn record<W: AsyncWrite + Unpin>(
    acquirer: &HttpAcquirer,
    source: &HttpSource,
    limits: AcquisitionLimits,
    sink: &mut W,
    stop: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<TransferReceipt> {
    let document = acquirer.playlist_document(source).await?;
    let segments = media_segments(source, &document.final_url, &document.bytes)?;
    let started = tokio::time::Instant::now();
    let mut total = 0_u64;
    let mut kind = None;
    let mut peer = None;
    let mut route = Vec::new();
    for segment in segments {
        if *stop.borrow() {
            return stopped(sink, total, kind, peer, route).await;
        }
        if started.elapsed() >= limits.duration() {
            return timed_out(sink, total, kind, peer, route).await;
        }
        let remaining_bytes = limits.bytes().saturating_sub(total);
        if remaining_bytes == 0 {
            sink.flush().await?;
            return receipt_from(total, TransferEnd::ByteLimit, kind, peer, route);
        }
        let remaining_time = limits.duration().saturating_sub(started.elapsed());
        let receipt = acquirer
            .record(
                &segment,
                AcquisitionLimits::new(remaining_bytes, remaining_time)?,
                sink,
                stop,
            )
            .await?;
        if kind.is_some_and(|seen: AudioContentType| seen != receipt.declared_content_type) {
            return Err(Error::Acquisition("HLS segments use different audio types"));
        }
        kind = Some(receipt.declared_content_type);
        peer = Some(receipt.peer);
        if route.is_empty() {
            route = receipt.route;
        }
        total = total.saturating_add(receipt.bytes);
        if receipt.end == TransferEnd::UserStop {
            return receipt_from(total, TransferEnd::UserStop, kind, peer, route);
        }
    }
    receipt_from(total, TransferEnd::EndOfBody, kind, peer, route)
}

pub(crate) fn media_segments(
    parent: &HttpSource,
    final_url: &Url,
    body: &[u8],
) -> Result<Vec<HttpSource>> {
    let lines = playlist_lines(body)?;
    if !lines
        .iter()
        .any(|line| line.eq_ignore_ascii_case("#EXTM3U"))
    {
        return Err(Error::InvalidInput("HLS playlist is not accepted"));
    }
    if lines.iter().any(|line| master_tag(line)) {
        return Err(Error::InvalidInput("HLS master playlist is not accepted"));
    }
    if lines.iter().any(|line| rejected_tag(line)) {
        return Err(Error::InvalidInput("unsupported HLS playlist"));
    }
    if !lines
        .iter()
        .any(|line| line.eq_ignore_ascii_case("#EXT-X-ENDLIST"))
    {
        return Err(Error::InvalidInput(
            "HLS live playlist is not a finite recording",
        ));
    }
    let mut segments = Vec::new();
    for line in &lines {
        if line.starts_with('#') {
            if !allowed_tag(line) {
                return Err(Error::InvalidInput("unsupported HLS playlist"));
            }
            continue;
        }
        if segments.len() == MAX_SEGMENTS {
            return Err(Error::InvalidInput("playlist entry limit"));
        }
        segments.push(authorize_entry(parent, final_url, line)?);
    }
    if segments.is_empty() {
        return Err(Error::InvalidInput("playlist has no entries"));
    }
    Ok(segments)
}

fn allowed_tag(line: &str) -> bool {
    line.eq_ignore_ascii_case("#EXTM3U")
        || has_ascii_prefix(line, "#EXTINF:")
        || line.eq_ignore_ascii_case("#EXT-X-ENDLIST")
        || line.eq_ignore_ascii_case("#EXT-X-INDEPENDENT-SEGMENTS")
        || has_ascii_prefix(line, "#EXT-X-VERSION:")
        || has_ascii_prefix(line, "#EXT-X-TARGETDURATION:")
        || has_ascii_prefix(line, "#EXT-X-MEDIA-SEQUENCE:")
        || has_ascii_prefix(line, "#EXT-X-PLAYLIST-TYPE:")
}

fn master_tag(line: &str) -> bool {
    has_ascii_prefix(line, "#EXT-X-STREAM-INF:")
        || has_ascii_prefix(line, "#EXT-X-I-FRAME-STREAM-INF:")
        || has_ascii_prefix(line, "#EXT-X-MEDIA:")
        || has_ascii_prefix(line, "#EXT-X-SESSION-DATA:")
        || has_ascii_prefix(line, "#EXT-X-SESSION-KEY:")
}

fn rejected_tag(line: &str) -> bool {
    has_ascii_prefix(line, "#EXT-X-KEY:")
        || has_ascii_prefix(line, "#EXT-X-MAP:")
        || has_ascii_prefix(line, "#EXT-X-BYTERANGE:")
        || line.eq_ignore_ascii_case("#EXT-X-DISCONTINUITY")
        || has_ascii_prefix(line, "#EXT-X-PART:")
}

async fn stopped<W: AsyncWrite + Unpin>(
    sink: &mut W,
    total: u64,
    kind: Option<AudioContentType>,
    peer: Option<std::net::SocketAddr>,
    route: Vec<super::HttpHop>,
) -> Result<TransferReceipt> {
    if total == 0 {
        return Err(Error::Acquisition("stopped before receiving audio"));
    }
    sink.flush().await?;
    receipt_from(total, TransferEnd::UserStop, kind, peer, route)
}

async fn timed_out<W: AsyncWrite + Unpin>(
    sink: &mut W,
    total: u64,
    kind: Option<AudioContentType>,
    peer: Option<std::net::SocketAddr>,
    route: Vec<super::HttpHop>,
) -> Result<TransferReceipt> {
    if total == 0 {
        return Err(Error::Acquisition("empty timed recording"));
    }
    sink.flush().await?;
    receipt_from(total, TransferEnd::DurationLimit, kind, peer, route)
}

fn receipt_from(
    bytes: u64,
    end: TransferEnd,
    kind: Option<AudioContentType>,
    peer: Option<std::net::SocketAddr>,
    route: Vec<super::HttpHop>,
) -> Result<TransferReceipt> {
    Ok(TransferReceipt {
        bytes,
        end,
        declared_content_type: kind.ok_or(Error::Acquisition("HLS playlist produced no audio"))?,
        peer: peer.ok_or(Error::Acquisition("HLS playlist produced no audio"))?,
        route,
        observations: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::{MAX_SEGMENTS, media_segments};
    use crate::{
        Error,
        sources::{HttpSource, NetworkScope, RedirectPolicy},
    };
    use reqwest::Url;
    use std::fmt::Write;

    fn parent() -> Result<HttpSource, Error> {
        HttpSource::new(
            "Playlist",
            "http://127.0.0.1:9/media.m3u8",
            NetworkScope::PinnedAddress {
                address: "127.0.0.1"
                    .parse()
                    .map_err(|_| Error::InvalidInput("address"))?,
            },
        )?
        .with_redirects(RedirectPolicy::Deny)
    }

    #[test]
    fn master_playlist_fails_before_variant_authorization() -> Result<(), Error> {
        let source = parent()?;
        let url =
            Url::parse("http://127.0.0.1:9/master.m3u8").map_err(|_| Error::InvalidInput("url"))?;
        let error = media_segments(
            &source,
            &url,
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://secret.example/variant.m3u8\n",
        );
        assert!(matches!(
            error,
            Err(Error::InvalidInput("HLS master playlist is not accepted"))
        ));
        Ok(())
    }

    #[test]
    fn media_playlist_keeps_bounded_same_origin_segments() -> Result<(), Error> {
        let source = parent()?;
        let url =
            Url::parse("http://127.0.0.1:9/media.m3u8").map_err(|_| Error::InvalidInput("url"))?;
        let segments = media_segments(
            &source,
            &url,
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nseg0\n#EXTINF:1,\nseg1\n#EXT-X-ENDLIST\n",
        )?;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].endpoint(), "http://127.0.0.1:9/seg0");
        assert_eq!(segments[1].endpoint(), "http://127.0.0.1:9/seg1");
        let allowed = media_segments(
            &source,
            &url,
            b"#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:4\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXTINF:1,title\nseg0\n#EXT-X-ENDLIST\n",
        )?;
        assert_eq!(allowed.len(), 1);
        Ok(())
    }

    #[test]
    fn media_playlist_rejects_live_and_unsupported_documents() -> Result<(), Error> {
        let source = parent()?;
        let url =
            Url::parse("http://127.0.0.1:9/media.m3u8").map_err(|_| Error::InvalidInput("url"))?;
        let cases: &[(&[u8], &str)] = &[
            (b"seg0\n", "HLS playlist is not accepted"),
            (
                b"#EXTM3U\n#EXTINF:1,\nhttp://secret.example/live\n",
                "HLS live playlist is not a finite recording",
            ),
            (
                b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
                "unsupported HLS playlist",
            ),
            (
                b"#EXTM3U\n#EXT-X-MAP:URI=\"init\"\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
                "unsupported HLS playlist",
            ),
            (
                b"#EXTM3U\n#EXTINF:1,\n#EXT-X-BYTERANGE:1\nseg0\n#EXT-X-ENDLIST\n",
                "unsupported HLS playlist",
            ),
            (
                b"#EXTM3U\n#EXT-X-DISCONTINUITY\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
                "unsupported HLS playlist",
            ),
            (
                b"#EXTM3U\n#EXT-X-PART:DURATION=0.1,URI=\"part\"\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
                "unsupported HLS playlist",
            ),
            (
                b"#EXTM3U\n#EXT-X-ENDLIST\n",
                "playlist has no entries",
            ),
            (
                b"#EXTM3U\n#EXTINF\xC3\xA9\nseg0\n#EXT-X-ENDLIST\n",
                "unsupported HLS playlist",
            ),
        ];
        for (body, message) in cases {
            let error = media_segments(&source, &url, body);
            assert!(
                matches!(error, Err(Error::InvalidInput(text)) if text == *message),
                "{body:?} -> {error:?}"
            );
        }
        let foreign = media_segments(
            &source,
            &url,
            b"#EXTM3U\n#EXTINF:1,\nhttp://secret.example/seg0\n#EXT-X-ENDLIST\n",
        );
        assert!(matches!(foreign, Err(Error::DestinationDenied)));
        let mut bounded = String::from("#EXTM3U\n");
        for index in 0..=MAX_SEGMENTS {
            bounded.push_str("#EXTINF:1,\n");
            if writeln!(bounded, "seg{index}").is_err() {
                return Err(Error::InvalidInput("segment name"));
            }
        }
        bounded.push_str("#EXT-X-ENDLIST\n");
        let overflow = media_segments(&source, &url, bounded.as_bytes());
        assert!(matches!(
            overflow,
            Err(Error::InvalidInput("playlist entry limit"))
        ));
        Ok(())
    }
}
