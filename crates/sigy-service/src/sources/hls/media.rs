//! One HLS media playlist document. Segment references are parsed, not authorized or fetched.

use super::attributes::{decimal_integer, duration_us, invalid};
use crate::{
    Error, Result,
    sources::playlist::{has_ascii_prefix, playlist_lines},
};

/// A finite playlist keeps the original 32-segment bound.
pub(crate) const MAX_FINITE_SEGMENTS: usize = 32;
/// A live window is also bounded by the 64 KiB document limit.
pub(crate) const MAX_LIVE_WINDOW: usize = 1024;
/// A live target duration above one minute is not a radio stream this recorder accepts.
pub(crate) const MAX_LIVE_TARGET_SECONDS: u64 = 60;
const MAX_TARGET_SECONDS: u64 = 3600;
const MAX_SEGMENT_US: u64 = 3600 * 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Finite,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaSegment {
    /// The media sequence number. Each number is fetched at most once.
    pub sequence: u64,
    /// The discontinuity sequence number this segment belongs to.
    pub epoch: u64,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaPlaylist {
    pub target_seconds: Option<u64>,
    pub media_sequence: u64,
    pub ended: bool,
    pub segments: Vec<MediaSegment>,
}

#[derive(Clone, Copy, Default)]
struct Header {
    target_seconds: Option<u64>,
    media_sequence: Option<u64>,
    discontinuity_sequence: Option<u64>,
    ended: bool,
}

/// Parses one media playlist. A master playlist, encryption, byte ranges, maps and
/// low-latency parts fail closed. A discontinuity is accepted only in live mode.
/// # Errors
/// Returns an input error for malformed, unsupported, or oversized documents.
pub(crate) fn parse(body: &[u8], mode: Mode) -> Result<MediaPlaylist> {
    let lines = playlist_lines(body)?;
    if !lines
        .first()
        .is_some_and(|line| line.eq_ignore_ascii_case("#EXTM3U"))
    {
        return Err(Error::InvalidInput("HLS playlist is not accepted"));
    }
    if lines.iter().any(|line| master_tag(line)) {
        return Err(Error::InvalidInput(
            "HLS master playlist; resolve it and accept one variant",
        ));
    }
    if lines.iter().any(|line| rejected_tag(line)) {
        return Err(invalid());
    }
    let limit = match mode {
        Mode::Finite => MAX_FINITE_SEGMENTS,
        Mode::Live => MAX_LIVE_WINDOW,
    };
    let mut header = Header::default();
    let mut pending: Option<bool> = None;
    let mut discontinuity = false;
    let mut epoch_offset = 0_u64;
    let mut references = Vec::new();
    for line in lines.iter().skip(1) {
        if comment(line) {
            continue;
        }
        if !line.starts_with('#') {
            let Some(marked) = pending.take() else {
                return Err(invalid());
            };
            if references.len() == limit {
                return Err(Error::InvalidInput("playlist entry limit"));
            }
            if marked {
                epoch_offset = epoch_offset.checked_add(1).ok_or_else(invalid)?;
            }
            references.push((epoch_offset, line.clone()));
            continue;
        }
        if let Some(value) = tag_value(line, "#EXTINF:") {
            if pending.is_some() {
                return Err(invalid());
            }
            segment_duration(value)?;
            pending = Some(std::mem::take(&mut discontinuity));
        } else if line.eq_ignore_ascii_case("#EXT-X-DISCONTINUITY") {
            // The tag applies to the next segment and may follow its EXTINF.
            match (mode, pending) {
                (Mode::Live, Some(false)) => pending = Some(true),
                (Mode::Live, None) if !discontinuity => discontinuity = true,
                _ => return Err(invalid()),
            }
        } else {
            header_tag(
                &mut header,
                line,
                references.is_empty() && pending.is_none(),
            )?;
        }
    }
    if pending.is_some() || discontinuity {
        return Err(invalid());
    }
    finish(header, references, mode)
}

fn finish(header: Header, references: Vec<(u64, String)>, mode: Mode) -> Result<MediaPlaylist> {
    match mode {
        Mode::Finite if !header.ended => {
            return Err(Error::InvalidInput(
                "HLS live playlist requires record hls --live",
            ));
        }
        Mode::Live
            if header
                .target_seconds
                .is_none_or(|seconds| seconds > MAX_LIVE_TARGET_SECONDS) =>
        {
            return Err(Error::InvalidInput(
                "HLS live playlist needs a target duration of 1 to 60 seconds",
            ));
        }
        _ => (),
    }
    if references.is_empty() && mode == Mode::Finite {
        return Err(Error::InvalidInput("playlist has no entries"));
    }
    let media_sequence = header.media_sequence.unwrap_or(0);
    let base_epoch = header.discontinuity_sequence.unwrap_or(0);
    let mut segments = Vec::with_capacity(references.len());
    for (index, (offset, reference)) in references.into_iter().enumerate() {
        let index = u64::try_from(index).map_err(|_| invalid())?;
        segments.push(MediaSegment {
            sequence: media_sequence.checked_add(index).ok_or_else(invalid)?,
            epoch: base_epoch.checked_add(offset).ok_or_else(invalid)?,
            reference,
        });
    }
    Ok(MediaPlaylist {
        target_seconds: header.target_seconds,
        media_sequence,
        ended: header.ended,
        segments,
    })
}

fn header_tag(header: &mut Header, line: &str, before_segments: bool) -> Result<()> {
    if let Some(value) = tag_value(line, "#EXT-X-TARGETDURATION:") {
        let seconds = decimal_integer(value)?;
        if !before_segments
            || !(1..=MAX_TARGET_SECONDS).contains(&seconds)
            || header.target_seconds.replace(seconds).is_some()
        {
            return Err(invalid());
        }
    } else if let Some(value) = tag_value(line, "#EXT-X-MEDIA-SEQUENCE:") {
        if !before_segments
            || header
                .media_sequence
                .replace(decimal_integer(value)?)
                .is_some()
        {
            return Err(invalid());
        }
    } else if let Some(value) = tag_value(line, "#EXT-X-DISCONTINUITY-SEQUENCE:") {
        if !before_segments
            || header
                .discontinuity_sequence
                .replace(decimal_integer(value)?)
                .is_some()
        {
            return Err(invalid());
        }
    } else if line.eq_ignore_ascii_case("#EXT-X-ENDLIST") {
        if header.ended {
            return Err(invalid());
        }
        header.ended = true;
    } else if !ignored_tag(line) {
        return Err(invalid());
    }
    Ok(())
}

fn segment_duration(value: &str) -> Result<()> {
    let duration = value
        .split_once(',')
        .map_or(value, |(duration, _)| duration);
    let micros = duration_us(duration.trim())?;
    if micros == 0 || micros > MAX_SEGMENT_US {
        return Err(invalid());
    }
    Ok(())
}

/// Informational tags that do not change which bytes are fetched.
fn ignored_tag(line: &str) -> bool {
    line.eq_ignore_ascii_case("#EXT-X-INDEPENDENT-SEGMENTS")
        || has_ascii_prefix(line, "#EXT-X-VERSION:")
        || has_ascii_prefix(line, "#EXT-X-PLAYLIST-TYPE:")
        || has_ascii_prefix(line, "#EXT-X-PROGRAM-DATE-TIME:")
        || has_ascii_prefix(line, "#EXT-X-DATERANGE:")
        || has_ascii_prefix(line, "#EXT-X-ALLOW-CACHE:")
        || has_ascii_prefix(line, "#EXT-X-START:")
        || has_ascii_prefix(line, "#EXT-X-SERVER-CONTROL:")
        || has_ascii_prefix(line, "#EXT-X-BITRATE:")
}

pub(super) fn master_tag(line: &str) -> bool {
    has_ascii_prefix(line, "#EXT-X-STREAM-INF:")
        || has_ascii_prefix(line, "#EXT-X-I-FRAME-STREAM-INF:")
        || has_ascii_prefix(line, "#EXT-X-MEDIA:")
        || has_ascii_prefix(line, "#EXT-X-SESSION-DATA:")
        || has_ascii_prefix(line, "#EXT-X-SESSION-KEY:")
}

/// Encryption, byte ranges, fMP4 maps, variable substitution, gaps, and low-latency parts.
fn rejected_tag(line: &str) -> bool {
    has_ascii_prefix(line, "#EXT-X-KEY:")
        || has_ascii_prefix(line, "#EXT-X-MAP:")
        || has_ascii_prefix(line, "#EXT-X-BYTERANGE:")
        || has_ascii_prefix(line, "#EXT-X-DEFINE:")
        || line.eq_ignore_ascii_case("#EXT-X-GAP")
        || has_ascii_prefix(line, "#EXT-X-PART:")
        || has_ascii_prefix(line, "#EXT-X-PART-INF:")
        || has_ascii_prefix(line, "#EXT-X-PRELOAD-HINT:")
        || has_ascii_prefix(line, "#EXT-X-RENDITION-REPORT:")
        || has_ascii_prefix(line, "#EXT-X-SKIP:")
}

/// A line beginning with `#` but not `#EXT` is a comment.
pub(super) fn comment(line: &str) -> bool {
    line.starts_with('#') && !has_ascii_prefix(line, "#EXT")
}

pub(super) fn tag_value<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    if has_ascii_prefix(line, prefix) {
        line.get(prefix.len()..)
    } else {
        None
    }
}
