//! An HLS master playlist lists candidates. No variant or rendition is fetched or chosen.

use reqwest::Url;

use super::{
    attributes::{Attributes, invalid},
    media::{comment, master_tag, tag_value},
};
use crate::{
    Error, Result,
    sources::{
        HttpSource,
        playlist::{
            CandidateKind, MAX_PLAYLIST_ENTRIES, ResolvedEntry, authorize_entry, has_ascii_prefix,
        },
    },
};

const MAX_BANDWIDTH: u64 = 10_000_000_000;
const MAX_CODECS: usize = 256;

struct Variant {
    bandwidth: u64,
    codecs: Option<String>,
    audio_only: Option<bool>,
}

/// Lists the variants and audio renditions of one master playlist.
/// A media playlist is not a candidate list and fails here.
/// # Errors
/// Returns input or destination-policy errors. Nothing is fetched.
pub(crate) fn candidates(
    parent: &HttpSource,
    final_url: &Url,
    lines: &[String],
) -> Result<Vec<ResolvedEntry>> {
    if !lines
        .first()
        .is_some_and(|line| line.eq_ignore_ascii_case("#EXTM3U"))
    {
        return Err(Error::InvalidInput("HLS playlist is not accepted"));
    }
    if !lines.iter().any(|line| master_tag(line)) {
        return Err(Error::InvalidInput(
            "HLS media playlist is recorded with record hls",
        ));
    }
    let mut entries = Vec::new();
    let mut pending: Option<Variant> = None;
    for line in lines.iter().skip(1) {
        if comment(line) {
            continue;
        }
        if let Some(variant) = pending.take() {
            if line.starts_with('#') {
                return Err(invalid());
            }
            push(
                &mut entries,
                parent,
                final_url,
                line,
                CandidateKind::HlsVariant,
                &variant,
            )?;
        } else if let Some(value) = tag_value(line, "#EXT-X-STREAM-INF:") {
            pending = Some(variant(value)?);
        } else if let Some(value) = tag_value(line, "#EXT-X-MEDIA:") {
            if let Some(reference) = audio_rendition(value)? {
                let rendition = Variant {
                    bandwidth: 0,
                    codecs: None,
                    audio_only: Some(true),
                };
                push(
                    &mut entries,
                    parent,
                    final_url,
                    reference,
                    CandidateKind::HlsAudio,
                    &rendition,
                )?;
            }
        } else if !ignored_master_tag(line) {
            return Err(invalid());
        }
    }
    if pending.is_some() {
        return Err(invalid());
    }
    if entries.is_empty() {
        return Err(Error::InvalidInput("playlist has no entries"));
    }
    Ok(entries)
}

fn push(
    entries: &mut Vec<ResolvedEntry>,
    parent: &HttpSource,
    final_url: &Url,
    reference: &str,
    kind: CandidateKind,
    variant: &Variant,
) -> Result<()> {
    if entries.len() == MAX_PLAYLIST_ENTRIES {
        return Err(Error::InvalidInput("playlist entry limit"));
    }
    let source = authorize_entry(parent, final_url, reference)?;
    entries.push(ResolvedEntry {
        endpoint: source.endpoint().to_owned(),
        origin: source.origin(),
        kind,
        bandwidth: (variant.bandwidth > 0).then_some(variant.bandwidth),
        codecs: variant.codecs.clone(),
        audio_only: variant.audio_only,
    });
    Ok(())
}

fn variant(value: &str) -> Result<Variant> {
    // Some broadcasters repeat CODECS. It is a publisher claim that grants nothing, so a
    // repeated value becomes unknown instead of rejecting the whole master playlist.
    let attributes = Attributes::parse_allowing_repeated(value, &["CODECS"])?;
    let bandwidth = attributes.integer("BANDWIDTH")?.ok_or_else(invalid)?;
    if !(1..=MAX_BANDWIDTH).contains(&bandwidth) {
        return Err(invalid());
    }
    let codecs = attributes.quoted_unless_repeated("CODECS")?;
    if let Some(codecs) = codecs
        && !valid_codecs(codecs)
    {
        return Err(invalid());
    }
    let has_video = attributes.contains("RESOLUTION") || attributes.contains("VIDEO");
    let audio_only = match codecs {
        _ if has_video => Some(false),
        Some(codecs) => Some(codecs.split(',').all(|codec| audio_codec(codec.trim()))),
        None => None,
    };
    Ok(Variant {
        bandwidth,
        codecs: codecs.map(str::to_owned),
        audio_only,
    })
}

/// An audio rendition with its own URI. Other rendition types are listed nowhere.
fn audio_rendition(value: &str) -> Result<Option<&str>> {
    let attributes = Attributes::parse(value)?;
    let kind = attributes.plain("TYPE")?.ok_or_else(invalid)?;
    let uri = attributes.quoted("URI")?;
    Ok(if kind == "AUDIO" { uri } else { None })
}

fn valid_codecs(codecs: &str) -> bool {
    !codecs.is_empty()
        && codecs.len() <= MAX_CODECS
        && codecs.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b',' | b'-' | b'_' | b' ' | b'+')
        })
}

fn audio_codec(codec: &str) -> bool {
    [
        "mp4a.", "ac-3", "ec-3", "ac-4", "opus", "flac", "fLaC", "mp3",
    ]
    .iter()
    .any(|prefix| codec.starts_with(prefix))
}

fn ignored_master_tag(line: &str) -> bool {
    line.eq_ignore_ascii_case("#EXT-X-INDEPENDENT-SEGMENTS")
        || has_ascii_prefix(line, "#EXT-X-VERSION:")
        || has_ascii_prefix(line, "#EXT-X-I-FRAME-STREAM-INF:")
        || has_ascii_prefix(line, "#EXT-X-SESSION-DATA:")
        || has_ascii_prefix(line, "#EXT-X-START:")
        || has_ascii_prefix(line, "#EXT-X-CONTENT-STEERING:")
}
