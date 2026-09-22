//! Publisher transcript and chapter documents. Cue times are not media time.

use crate::{Error, Result, sources::unsafe_display};

const MAX_CUES: usize = 1_000;
const MAX_CUE_TEXT: usize = 180;
const MAX_DOCUMENT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextKind {
    Transcript,
    Chapters,
}

impl TextKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Transcript => "transcript",
            Self::Chapters => "chapters",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "transcript" => Ok(Self::Transcript),
            "chapters" => Ok(Self::Chapters),
            _ => Err(Error::InvalidInput("publisher text kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PublisherCue {
    pub publisher_start_ms: i64,
    pub publisher_end_ms: Option<i64>,
    pub speaker: Option<String>,
    pub text: String,
}

pub(crate) fn normalize_text_type(kind: TextKind, raw: &str) -> Option<String> {
    let mut parts = raw.split(';');
    let mime = parts.next()?.trim().to_ascii_lowercase();
    if !accepted_type(kind, &mime) {
        return None;
    }
    for parameter in parts {
        let parameter = parameter.trim();
        if parameter.is_empty() {
            continue;
        }
        let (name, value) = parameter.split_once('=')?;
        if name.trim().eq_ignore_ascii_case("charset") {
            let charset = value.trim().trim_matches('"');
            if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("us-ascii") {
                return None;
            }
        }
    }
    Some(mime)
}

pub(crate) fn accepted_type(kind: TextKind, media_type: &str) -> bool {
    matches!(
        (kind, media_type),
        (
            TextKind::Transcript,
            "text/vtt" | "application/x-subrip" | "application/srt" | "application/json"
        ) | (
            TextKind::Chapters,
            "application/json" | "application/json+chapters"
        )
    )
}

/// Parses one accepted publisher document. URLs inside the document are ignored.
/// # Errors
/// Rejects an unsupported type, hostile text, or a document over the cue limit.
pub(crate) fn parse(kind: TextKind, media_type: &str, bytes: &[u8]) -> Result<Vec<PublisherCue>> {
    if !accepted_type(kind, media_type) || bytes.len() > MAX_DOCUMENT || bytes.contains(&0) {
        return Err(Error::InvalidInput("publisher text type"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::InvalidInput("publisher text is not UTF-8"))?;
    let cues = match (kind, media_type) {
        (TextKind::Transcript, "text/vtt") => parse_vtt(text)?,
        (TextKind::Transcript, "application/x-subrip" | "application/srt") => parse_srt(text)?,
        (TextKind::Transcript, "application/json") => parse_json_transcript(text)?,
        (TextKind::Chapters, _) => parse_json_chapters(text)?,
        _ => return Err(Error::InvalidInput("publisher text type")),
    };
    if cues.is_empty() || cues.len() > MAX_CUES {
        return Err(Error::InvalidInput("publisher text cue limit"));
    }
    Ok(cues)
}

fn parse_vtt(text: &str) -> Result<Vec<PublisherCue>> {
    let body = text.trim_start_matches('\u{feff}');
    let mut lines = body.lines();
    let header = lines.next().unwrap_or("").trim();
    if !header.starts_with("WEBVTT") {
        return Err(Error::InvalidInput("publisher text is not WebVTT"));
    }
    cues_from_blocks(body, true)
}

fn parse_srt(text: &str) -> Result<Vec<PublisherCue>> {
    cues_from_blocks(text, false)
}

fn cues_from_blocks(text: &str, vtt: bool) -> Result<Vec<PublisherCue>> {
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        if cues.len() > MAX_CUES {
            return Err(Error::InvalidInput("publisher text cue limit"));
        }
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if lines.is_empty() || lines[0].starts_with("WEBVTT") || lines[0].starts_with("NOTE") {
            continue;
        }
        let timing = lines.iter().position(|line| line.contains("-->"));
        let Some(timing) = timing else {
            continue;
        };
        let (start, end) = timing_line(lines[timing], vtt)?;
        let payload: Vec<&str> = lines[timing + 1..].to_vec();
        if payload.is_empty() {
            continue;
        }
        let (speaker, text) = if vtt {
            voice(&payload)
        } else {
            speaker_prefix(&payload)
        };
        cues.push(cue(start, Some(end), speaker, &text)?);
    }
    Ok(cues)
}

fn timing_line(line: &str, vtt: bool) -> Result<(i64, i64)> {
    let (start, rest) = line
        .split_once("-->")
        .ok_or(Error::InvalidInput("publisher cue time"))?;
    let end = rest.split_whitespace().next().unwrap_or("");
    let start = clock(start.trim(), vtt)?;
    let end = clock(end, vtt)?;
    if end < start {
        return Err(Error::InvalidInput("publisher cue time"));
    }
    Ok((start, end))
}

fn clock(value: &str, vtt: bool) -> Result<i64> {
    let value = value.trim();
    let (main, fraction) = value
        .split_once(if vtt { '.' } else { ',' })
        .unwrap_or((value, "0"));
    let mut parts = main.split(':').map(str::parse::<i64>);
    let (hours, minutes, seconds) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(minutes)), Some(Ok(seconds)), None, None) => (0, minutes, seconds),
        (Some(Ok(hours)), Some(Ok(minutes)), Some(Ok(seconds)), None) => (hours, minutes, seconds),
        _ => return Err(Error::InvalidInput("publisher cue time")),
    };
    if !(0..60).contains(&minutes) || !(0..60).contains(&seconds) || !(0..100).contains(&hours) {
        return Err(Error::InvalidInput("publisher cue time"));
    }
    let millis = if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidInput("publisher cue time"));
    } else {
        let padded = format!("{fraction:0<3}");
        padded.parse::<i64>().unwrap_or(0)
    };
    Ok(hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + millis)
}

fn voice(lines: &[&str]) -> (Option<String>, String) {
    let joined = lines.join(" ");
    let Some(rest) = joined.strip_prefix("<v ") else {
        return (None, strip_tags(&joined));
    };
    let Some((name, body)) = rest.split_once('>') else {
        return (None, strip_tags(&joined));
    };
    (clean_label(name), strip_tags(body))
}

fn speaker_prefix(lines: &[&str]) -> (Option<String>, String) {
    let joined = lines.join(" ");
    let Some((name, body)) = joined.split_once(": ") else {
        return (None, joined);
    };
    if name.len() <= 32 && name.chars().any(char::is_alphabetic) && !name.contains(':') {
        (clean_label(name), body.to_owned())
    } else {
        (None, joined)
    }
}

fn strip_tags(value: &str) -> String {
    let mut output = String::new();
    let mut tag = false;
    for character in value.chars() {
        if character == '<' {
            tag = true;
        } else if character == '>' {
            tag = false;
        } else if !tag {
            output.push(character);
        }
    }
    output
}

fn parse_json_transcript(text: &str) -> Result<Vec<PublisherCue>> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| Error::InvalidInput("publisher JSON"))?;
    let object = value
        .as_object()
        .ok_or(Error::InvalidInput("publisher JSON"))?;
    require_version(object.get("version"))?;
    if object.contains_key("chapters") {
        return Err(Error::InvalidInput("publisher JSON is chapters"));
    }
    let segments = object
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .ok_or(Error::InvalidInput("publisher JSON"))?;
    let mut cues = Vec::new();
    for segment in segments {
        if cues.len() >= MAX_CUES {
            return Err(Error::InvalidInput("publisher text cue limit"));
        }
        let segment = segment
            .as_object()
            .ok_or(Error::InvalidInput("publisher JSON"))?;
        let start = seconds(segment.get("startTime"))?;
        let end = optional_seconds(segment.get("endTime"))?;
        let body = segment
            .get("body")
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::InvalidInput("publisher JSON"))?;
        let speaker = segment
            .get("speaker")
            .and_then(serde_json::Value::as_str)
            .and_then(clean_label);
        cues.push(cue(start, end, speaker, body)?);
    }
    Ok(cues)
}

fn parse_json_chapters(text: &str) -> Result<Vec<PublisherCue>> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| Error::InvalidInput("publisher JSON"))?;
    let object = value
        .as_object()
        .ok_or(Error::InvalidInput("publisher JSON"))?;
    require_version(object.get("version"))?;
    if object.contains_key("segments") {
        return Err(Error::InvalidInput("publisher JSON is a transcript"));
    }
    let chapters = object
        .get("chapters")
        .and_then(serde_json::Value::as_array)
        .ok_or(Error::InvalidInput("publisher JSON"))?;
    let mut cues = Vec::new();
    for chapter in chapters {
        if cues.len() >= MAX_CUES {
            return Err(Error::InvalidInput("publisher text cue limit"));
        }
        let chapter = chapter
            .as_object()
            .ok_or(Error::InvalidInput("publisher JSON"))?;
        let start = seconds(chapter.get("startTime"))?;
        let end = optional_seconds(chapter.get("endTime"))?;
        let title = chapter
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        cues.push(cue(start, end, None, title)?);
    }
    Ok(cues)
}

fn require_version(value: Option<&serde_json::Value>) -> Result<()> {
    let version = value
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::InvalidInput("publisher JSON version"))?;
    if (1..=16).contains(&version.len())
        && version
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        Ok(())
    } else {
        Err(Error::InvalidInput("publisher JSON version"))
    }
}

fn optional_seconds(value: Option<&serde_json::Value>) -> Result<Option<i64>> {
    if value.is_none() {
        return Ok(None);
    }
    seconds(value).map(Some)
}

fn seconds(value: Option<&serde_json::Value>) -> Result<i64> {
    let seconds = value
        .ok_or(Error::InvalidInput("publisher cue time"))?
        .as_f64()
        .filter(|seconds| seconds.is_finite() && (0.0..=86_400.0).contains(seconds))
        .ok_or(Error::InvalidInput("publisher cue time"))?;
    let millis = (seconds * 1_000.0).round();
    if !millis.is_finite() || !(0.0..=86_400_000.0).contains(&millis) {
        return Err(Error::InvalidInput("publisher cue time"));
    }
    format!("{millis:.0}")
        .parse::<i64>()
        .map_err(|_| Error::InvalidInput("publisher cue time"))
}

fn cue(start: i64, end: Option<i64>, speaker: Option<String>, text: &str) -> Result<PublisherCue> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().any(unsafe_display) || text.len() > MAX_CUE_TEXT {
        return Err(Error::InvalidInput("publisher cue text"));
    }
    if end.is_some_and(|end| end < start) {
        return Err(Error::InvalidInput("publisher cue time"));
    }
    Ok(PublisherCue {
        publisher_start_ms: start,
        publisher_end_ms: end,
        speaker,
        text,
    })
}

fn clean_label(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 32 || value.chars().any(unsafe_display) {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{TextKind, normalize_text_type, parse};

    #[test]
    fn vtt_srt_and_json_keep_publisher_times() -> Result<(), String> {
        let vtt = b"WEBVTT\n\n00:00:01.000 --> 00:00:04.000\n<v Ada>Hello there\n";
        let cues =
            parse(TextKind::Transcript, "text/vtt", vtt).map_err(|error| error.to_string())?;
        if cues[0].publisher_start_ms != 1_000
            || cues[0].speaker.as_deref() != Some("Ada")
            || cues[0].text != "Hello there"
        {
            return Err(format!("{cues:?}"));
        }
        let srt = b"1\n00:00:01,500 --> 00:00:02,000\nSam: Hi\n";
        let cues = parse(TextKind::Transcript, "application/srt", srt)
            .map_err(|error| error.to_string())?;
        if cues[0].publisher_start_ms != 1_500 || cues[0].text != "Hi" {
            return Err(format!("{cues:?}"));
        }
        let json = br#"{"version":"1.0.0","segments":[{"speaker":"Ada","startTime":0.5,"endTime":1.25,"body":"Hello"}]}"#;
        let cues = parse(TextKind::Transcript, "application/json", json)
            .map_err(|error| error.to_string())?;
        if cues[0].publisher_start_ms != 500 || cues[0].publisher_end_ms != Some(1_250) {
            return Err(format!("{cues:?}"));
        }
        let chapters = br#"{"version":"1.2.0","chapters":[{"startTime":0,"title":"Intro","img":"https://example.com/secret.png"}]}"#;
        let cues = parse(TextKind::Chapters, "application/json+chapters", chapters)
            .map_err(|error| error.to_string())?;
        if cues[0].text != "Intro" || parse(TextKind::Transcript, "text/html", b"<p>Hi</p>").is_ok()
        {
            return Err(format!("{cues:?}"));
        }
        Ok(())
    }

    #[test]
    fn charset_parameter_keeps_the_media_type() -> Result<(), String> {
        if normalize_text_type(TextKind::Transcript, "Text/VTT; charset=\"utf-8\"").as_deref()
            != Some("text/vtt")
            || normalize_text_type(TextKind::Chapters, "application/json+chapters").as_deref()
                != Some("application/json+chapters")
            || normalize_text_type(TextKind::Transcript, "text/html").is_some()
            || normalize_text_type(TextKind::Transcript, "text/vtt; charset=utf-16").is_some()
        {
            return Err("publisher type normalization rejected a stored type".to_owned());
        }
        Ok(())
    }
}
