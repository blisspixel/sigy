//! Live HLS media playlists. Each media sequence number is fetched at most once, in order.
//!
//! The capture ends at the first skipped sequence, discontinuity, failed or stalled
//! reload, failed segment, or media type change. Only whole segments are kept.

use std::{io::SeekFrom, time::Duration};

use reqwest::Url;
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::watch,
    time::Instant,
};

use super::{
    media::{self, MediaPlaylist, MediaSegment, Mode},
    receipt_from,
};
use crate::{
    Error, Result,
    sources::{
        HttpHop, HttpSource,
        http::{AcquisitionLimits, AudioContentType, HttpAcquirer, TransferEnd, TransferReceipt},
        playlist::authorize_entry,
    },
};

/// Start this many segments before the live edge, as RFC 8216 advises.
pub(crate) const LIVE_START_SEGMENTS: usize = 3;
/// No two playlist reloads start closer together than this.
pub(crate) const MIN_RELOAD_INTERVAL: Duration = Duration::from_secs(1);
/// A playlist that adds no segment for this many target durations has stalled.
const STALL_TARGETS: u32 = 3;

/// Why a live capture ended before its time or byte ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveGap {
    /// The next media sequence number expired from the window before it was fetched.
    SequenceSkip,
    /// The next segment starts a new discontinuity sequence.
    Discontinuity,
    /// A reload failed, was not a usable media playlist, or stopped advancing.
    ReloadFailure,
    /// A segment request failed after audio was received.
    SegmentFailure,
    /// A segment declared a different media type.
    CodecChange,
}

#[derive(Debug)]
pub(crate) struct LiveOutcome {
    pub receipt: TransferReceipt,
    pub gap: Option<LiveGap>,
}

struct Window {
    playlist: MediaPlaylist,
    final_url: Url,
}

struct Session<'a> {
    acquirer: &'a HttpAcquirer,
    source: &'a HttpSource,
    limits: AcquisitionLimits,
    deadline: Instant,
    committed: u64,
    kind: Option<AudioContentType>,
    peer: Option<std::net::SocketAddr>,
    route: Vec<HttpHop>,
    next: u64,
    epoch: Option<u64>,
    progressed: Instant,
}

enum Step {
    Continue { appended: bool },
    End(TransferEnd, Option<LiveGap>),
}

/// Records a live media playlist until the time or byte ceiling, a stop, or a gap.
/// # Errors
/// Fails when the first playlist is unusable or no whole segment was received.
pub(crate) async fn record(
    acquirer: &HttpAcquirer,
    source: &HttpSource,
    limits: AcquisitionLimits,
    file: &mut tokio::fs::File,
    stop: &mut watch::Receiver<bool>,
) -> Result<LiveOutcome> {
    let started = Instant::now();
    let document = acquirer.playlist_document(source).await?;
    let playlist = media::parse(&document.bytes, Mode::Live)?;
    if playlist.ended {
        return Err(Error::InvalidInput(
            "HLS playlist has ended; record it without --live",
        ));
    }
    let first = playlist.segments.len().saturating_sub(LIVE_START_SEGMENTS);
    let mut session = Session {
        acquirer,
        source,
        limits,
        deadline: started + limits.duration(),
        committed: 0,
        kind: None,
        peer: None,
        route: Vec::new(),
        next: playlist
            .segments
            .get(first)
            .map_or(playlist.media_sequence, |segment| segment.sequence),
        epoch: None,
        progressed: started,
    };
    let mut window = Window {
        playlist,
        final_url: document.final_url,
    };
    let mut reloaded = started;
    loop {
        let appended = match session.drain(&window, file, stop).await? {
            Step::Continue { appended } => appended,
            Step::End(end, gap) => return session.finish(file, end, gap).await,
        };
        if window.playlist.ended {
            return session.finish(file, TransferEnd::EndOfBody, None).await;
        }
        let target = Duration::from_secs(window.playlist.target_seconds.unwrap_or(1));
        if session.committed > 0 && session.progressed.elapsed() > target * STALL_TARGETS {
            return session
                .finish(file, TransferEnd::EndOfBody, Some(LiveGap::ReloadFailure))
                .await;
        }
        let interval = if appended { target } else { target / 2 };
        let next_reload = reloaded + interval.max(MIN_RELOAD_INTERVAL);
        if let Some(end) = session.wait(next_reload, stop).await {
            return session.finish(file, end, None).await;
        }
        reloaded = Instant::now();
        match session.reload(stop).await {
            Ok(Ok(next)) => window = next,
            Ok(Err(end)) => return session.finish(file, end, None).await,
            Err(_) => {
                return session
                    .finish(file, TransferEnd::EndOfBody, Some(LiveGap::ReloadFailure))
                    .await;
            }
        }
    }
}

impl Session<'_> {
    async fn drain(
        &mut self,
        window: &Window,
        file: &mut tokio::fs::File,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<Step> {
        let available = window
            .playlist
            .segments
            .first()
            .map_or(window.playlist.media_sequence, |segment| segment.sequence);
        if available > self.next {
            if self.committed > 0 {
                return Ok(Step::End(
                    TransferEnd::EndOfBody,
                    Some(LiveGap::SequenceSkip),
                ));
            }
            // Nothing is recorded yet, so an expired start is not a hole in the capture.
            self.next = available;
        }
        let mut appended = false;
        for segment in &window.playlist.segments {
            if segment.sequence < self.next {
                continue;
            }
            if self.epoch.is_some_and(|epoch| epoch != segment.epoch) {
                return Ok(Step::End(
                    TransferEnd::EndOfBody,
                    Some(LiveGap::Discontinuity),
                ));
            }
            if let Some(end) = self.limit_reached(stop) {
                return Ok(Step::End(end, None));
            }
            match self.fetch(segment, &window.final_url, file, stop).await? {
                Step::Continue { .. } => appended = true,
                ended @ Step::End(..) => return Ok(ended),
            }
            self.next = segment
                .sequence
                .checked_add(1)
                .ok_or(Error::InvalidInput("unsupported HLS playlist"))?;
            self.epoch = Some(segment.epoch);
        }
        Ok(Step::Continue { appended })
    }

    async fn fetch(
        &mut self,
        segment: &MediaSegment,
        final_url: &Url,
        file: &mut tokio::fs::File,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<Step> {
        let before = self.committed;
        let remaining_time = self.deadline.saturating_duration_since(Instant::now());
        if remaining_time.is_zero() {
            return Ok(Step::End(TransferEnd::DurationLimit, None));
        }
        let remaining =
            AcquisitionLimits::new(self.limits.bytes().saturating_sub(before), remaining_time)?;
        let result = match authorize_entry(self.source, final_url, &segment.reference) {
            Ok(target) => {
                self.acquirer
                    .record_segment(&target, remaining, file, stop)
                    .await
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(receipt) if receipt.end == TransferEnd::EndOfBody => {
                if self
                    .kind
                    .is_some_and(|kind| kind != receipt.declared_content_type)
                {
                    truncate(file, before).await?;
                    return Ok(Step::End(
                        TransferEnd::EndOfBody,
                        Some(LiveGap::CodecChange),
                    ));
                }
                self.kind = Some(receipt.declared_content_type);
                self.peer = Some(receipt.peer);
                if self.route.is_empty() {
                    self.route = receipt.route;
                }
                self.committed = before
                    .checked_add(receipt.bytes)
                    .ok_or(Error::StorageIntegrity)?;
                self.progressed = Instant::now();
                Ok(Step::Continue { appended: true })
            }
            Ok(receipt) => {
                // Only whole segments are published. The partial one is removed.
                truncate(file, before).await?;
                Ok(Step::End(receipt.end, None))
            }
            Err(error) => {
                truncate(file, before).await?;
                if let Some(end) = self.limit_reached(stop) {
                    return Ok(Step::End(end, None));
                }
                if before == 0 {
                    return Err(error);
                }
                Ok(Step::End(
                    TransferEnd::EndOfBody,
                    Some(LiveGap::SegmentFailure),
                ))
            }
        }
    }

    fn limit_reached(&self, stop: &watch::Receiver<bool>) -> Option<TransferEnd> {
        if *stop.borrow() {
            Some(TransferEnd::UserStop)
        } else if Instant::now() >= self.deadline {
            Some(TransferEnd::DurationLimit)
        } else if self.committed >= self.limits.bytes() {
            Some(TransferEnd::ByteLimit)
        } else {
            None
        }
    }

    /// Waits for the next reload. Returns an end when the capture stops or times out first.
    async fn wait(
        &self,
        next_reload: Instant,
        stop: &mut watch::Receiver<bool>,
    ) -> Option<TransferEnd> {
        if let Some(end) = self.limit_reached(stop) {
            return Some(end);
        }
        tokio::select! {
            biased;
            () = async { if !*stop.borrow() { let _ = stop.changed().await; } } => {
                Some(TransferEnd::UserStop)
            }
            () = tokio::time::sleep_until(self.deadline) => Some(TransferEnd::DurationLimit),
            () = tokio::time::sleep_until(next_reload) => None,
        }
    }

    /// One bounded reload of the parent playlist through its own redirect policy.
    async fn reload(
        &self,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<std::result::Result<Window, TransferEnd>> {
        let reading = async {
            let document = self.acquirer.playlist_document(self.source).await?;
            let playlist = media::parse(&document.bytes, Mode::Live)?;
            Ok::<_, Error>(Window {
                playlist,
                final_url: document.final_url,
            })
        };
        tokio::select! {
            biased;
            () = async { if !*stop.borrow() { let _ = stop.changed().await; } } => {
                Ok(Err(TransferEnd::UserStop))
            }
            () = tokio::time::sleep_until(self.deadline) => Ok(Err(TransferEnd::DurationLimit)),
            window = reading => window.map(Ok),
        }
    }

    async fn finish(
        self,
        file: &mut tokio::fs::File,
        end: TransferEnd,
        gap: Option<LiveGap>,
    ) -> Result<LiveOutcome> {
        if self.committed == 0 {
            return Err(Error::Acquisition(
                "HLS live capture ended before a whole segment",
            ));
        }
        file.flush().await?;
        Ok(LiveOutcome {
            receipt: receipt_from(self.committed, end, self.kind, self.peer, self.route)?,
            gap,
        })
    }
}

async fn truncate(file: &mut tokio::fs::File, length: u64) -> Result<()> {
    file.flush().await?;
    file.set_len(length).await?;
    file.seek(SeekFrom::Start(length)).await?;
    Ok(())
}
