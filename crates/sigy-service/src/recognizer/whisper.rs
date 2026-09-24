//! Untrusted whisper.cpp JSON output, mapped onto the pinned media clock.

use serde::Deserialize;

use super::SAMPLE_RATE;
use crate::recognition::{LocalAsrInput, MAX_ASR_CUES, MAX_ASR_TEXT_BYTES, RecognitionCue};

#[derive(Deserialize)]
struct WhisperJson {
    #[serde(default)]
    result: Option<WhisperResult>,
    transcription: Vec<WhisperSegment>,
}

#[derive(Deserialize)]
struct WhisperResult {
    #[serde(default)]
    language: Option<String>,
}

/// One recognizer run's output: cues on the media clock and its block language label.
#[derive(Debug)]
pub(crate) struct WhisperOutput {
    pub cues: Vec<RecognitionCue>,
    /// The recognizer's own code, bounded and unmapped. `None` when absent or malformed.
    pub language: Option<String>,
}

#[derive(Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
}

#[derive(Deserialize)]
struct WhisperOffsets {
    from: i64,
    to: i64,
}

/// Map untrusted recognizer JSON onto the pinned media clock.
///
/// Offsets are milliseconds from the decoded interval start. Surrounding whitespace is
/// formatting and is trimmed; a segment with no remaining text is not a cue. A segment
/// end past the pinned interval end is bounded to that end, because the recognizer
/// rounds to its own frame grid. Every other inconsistency rejects the whole output.
pub(crate) fn parse_whisper_json(
    json: &[u8],
    input: &LocalAsrInput,
    sample_count: u64,
) -> std::result::Result<WhisperOutput, &'static str> {
    let parsed: WhisperJson = serde_json::from_slice(json).map_err(|_| "malformed")?;
    if parsed.transcription.len() > MAX_ASR_CUES * 4 {
        return Err("too many segments");
    }
    let decoded_end = input.start_us + sample_count * 1_000_000 / u64::from(SAMPLE_RATE);
    let mut cues = Vec::new();
    let mut previous_end = input.start_us;
    let mut text_bytes = 0_usize;
    for segment in parsed.transcription {
        let script = segment.text.trim();
        if script.is_empty() {
            continue;
        }
        let (Ok(from), Ok(to)) = (
            u64::try_from(segment.offsets.from),
            u64::try_from(segment.offsets.to),
        ) else {
            return Err("negative offset");
        };
        if to <= from {
            return Err("empty segment");
        }
        let start = from
            .checked_mul(1_000)
            .and_then(|value| value.checked_add(input.start_us))
            .ok_or("offset range")?;
        let end = to
            .checked_mul(1_000)
            .and_then(|value| value.checked_add(input.start_us))
            .ok_or("offset range")?
            .min(input.end_us);
        if start >= decoded_end || start >= end || start < previous_end {
            return Err("segment outside decoded audio or out of order");
        }
        if script.len() > 4096 || script.chars().any(|c| c == '\0') {
            return Err("segment text");
        }
        text_bytes += script.len();
        if cues.len() == MAX_ASR_CUES || text_bytes > MAX_ASR_TEXT_BYTES {
            return Err("output limit");
        }
        cues.push(RecognitionCue {
            ordinal: u32::try_from(cues.len()).map_err(|_| "output limit")?,
            start_us: start,
            end_us: end,
            script: script.to_owned(),
        });
        previous_end = end;
    }
    let language = parsed
        .result
        .and_then(|result| result.language)
        .filter(|code| {
            (2..=8).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_lowercase())
        });
    Ok(WhisperOutput { cues, language })
}

/// Map the recognizer's language code to a BCP 47 tag. The original code is kept separately.
/// whisper.cpp uses ISO 639-1 where one exists; `jw` is its non-standard code for Javanese.
pub(crate) fn language_tag(code: &str) -> String {
    match code {
        "jw" => "jv".to_owned(),
        other => other.to_owned(),
    }
}
