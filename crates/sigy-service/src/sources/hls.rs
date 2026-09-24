//! HLS playlists. Master playlists list candidates; media segment URLs never reach the decoder.

mod attributes;
pub(crate) mod live;
mod master;
pub(crate) mod media;
#[cfg(test)]
mod tests;

pub(crate) use master::candidates as master_candidates;

use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::{
    HttpSource,
    http::{AcquisitionLimits, AudioContentType, HttpAcquirer, TransferEnd, TransferReceipt},
    playlist::authorize_entry,
};
use crate::{Error, Result};

/// Records one finite media playlist. Every segment is authorized before the first fetch.
/// # Errors
/// Fails closed on master, live, or unsupported playlists and on transport errors.
pub(crate) async fn record<W: AsyncWrite + Unpin>(
    acquirer: &HttpAcquirer,
    source: &HttpSource,
    limits: AcquisitionLimits,
    sink: &mut W,
    stop: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<TransferReceipt> {
    let document = acquirer.playlist_document(source).await?;
    let playlist = media::parse(&document.bytes, media::Mode::Finite)?;
    let segments = playlist
        .segments
        .iter()
        .map(|segment| authorize_entry(source, &document.final_url, &segment.reference))
        .collect::<Result<Vec<_>>>()?;
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
            .record_segment(
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
        if receipt.end != TransferEnd::EndOfBody {
            return receipt_from(total, receipt.end, kind, peer, route);
        }
    }
    receipt_from(total, TransferEnd::EndOfBody, kind, peer, route)
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
