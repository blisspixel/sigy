//! Bounded RSS 2.0 item extraction. No DTD, external entity, or `XInclude` resolution.

use std::collections::HashSet;

use super::{
    date::parse_pub_date,
    identity::{EpisodeIdentity, resolve_http_url},
};
use crate::{Error, Result, sources::unsafe_display};

const MAX_DEPTH: u32 = 32;
const MAX_ITEMS: usize = 500;
const MAX_TRANSCRIPTS: usize = 8;
const MAX_CHAPTERS: usize = 4;
const PODCAST_NAMESPACE: &str = "https://podcastindex.org/namespace/1.0";
const XINCLUDE_NAMESPACE: &str = "http://www.w3.org/2001/XInclude";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetRef {
    pub url: String,
    pub media_type: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedEpisode {
    pub identity: EpisodeIdentity,
    pub title: Option<String>,
    pub published_ms: Option<i64>,
    pub enclosure_url: Option<String>,
    pub enclosure_type: Option<String>,
    pub enclosure_length: Option<i64>,
    pub transcripts: Vec<AssetRef>,
    pub chapters: Vec<AssetRef>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedFeed {
    pub episodes: Vec<ParsedEpisode>,
    pub truncated: bool,
    pub live_count: u32,
    pub skipped: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rss,
    Channel,
    Item,
    Live,
    Title,
    Guid,
    PubDate,
    Enclosure,
    Transcript,
    Chapters,
    Other,
}

#[derive(Clone, Copy)]
enum Capture {
    Title,
    Guid,
    Published,
}

struct Namespace {
    depth: u32,
    prefix: String,
    uri: String,
}

struct Open {
    raw: String,
    kind: Kind,
}

#[derive(Default)]
struct ItemBuild {
    title: String,
    guid: String,
    published: String,
    enclosure_url: Option<String>,
    enclosure_type: Option<String>,
    enclosure_length: Option<i64>,
    transcripts: Vec<AssetRef>,
    chapters: Vec<AssetRef>,
    title_overflow: bool,
}

struct Parser<'a> {
    input: &'a str,
    index: usize,
    depth: u32,
    namespaces: Vec<Namespace>,
    stack: Vec<Open>,
    base: Option<&'a reqwest::Url>,
    feed: ParsedFeed,
    item: Option<ItemBuild>,
    seen: HashSet<String>,
    capture: Option<(Capture, u32)>,
    saw_root: bool,
}

/// Parses one RSS 2.0 document. Relative references use `base`, never `xml:base`.
/// # Errors
/// Rejects a DTD, an external or undeclared entity, `XInclude`, excess depth, and
/// a document that is not RSS 2.0.
pub(crate) fn parse(document: &str, base: Option<&reqwest::Url>) -> Result<ParsedFeed> {
    if document.len() > 8 * 1024 * 1024 || document.as_bytes().contains(&0) {
        return Err(Error::InvalidInput("feed document exceeded parser limits"));
    }
    let mut parser = Parser {
        input: document,
        index: 0,
        depth: 0,
        namespaces: Vec::new(),
        stack: Vec::new(),
        base,
        feed: ParsedFeed::default(),
        item: None,
        seen: HashSet::new(),
        capture: None,
        saw_root: false,
    };
    parser.document()
}

impl Parser<'_> {
    fn document(&mut self) -> Result<ParsedFeed> {
        if self.input.starts_with('\u{feff}') {
            self.index = '\u{feff}'.len_utf8();
        }
        self.skip_space();
        if self.starts_with("<?xml") {
            self.declaration()?;
        }
        while self.index < self.input.len() {
            self.skip_space();
            if self.index >= self.input.len() {
                break;
            }
            if self.starts_with("<![CDATA[") {
                self.consume_cdata()?;
            } else if self.starts_with("<?") {
                self.skip_pi()?;
            } else if self.starts_with("<!--") {
                self.skip_comment()?;
            } else if self.starts_with("<") {
                self.element()?;
            } else if self.saw_root && self.stack.is_empty() {
                return Err(Error::InvalidInput("feed document is not RSS 2.0"));
            } else {
                self.consume_text()?;
            }
        }
        if !self.saw_root || !self.stack.is_empty() {
            return Err(Error::InvalidInput("feed document is not RSS 2.0"));
        }
        Ok(std::mem::take(&mut self.feed))
    }

    fn declaration(&mut self) -> Result<()> {
        let end = self.input[self.index..]
            .find("?>")
            .ok_or(Error::InvalidInput(
                "feed document rejected an XML construct",
            ))?;
        let body = &self.input[self.index + 5..self.index + end];
        self.index += end + 2;
        if body.contains("<!") {
            return Err(Error::InvalidInput("feed document contains a DTD"));
        }
        for token in body.split_whitespace() {
            let Some((name, raw)) = token.split_once('=') else {
                continue;
            };
            let value = raw.trim_matches(['"', '\'']);
            if name == "encoding"
                && !value.eq_ignore_ascii_case("utf-8")
                && !value.eq_ignore_ascii_case("us-ascii")
            {
                return Err(Error::InvalidInput("feed document is not UTF-8"));
            }
        }
        Ok(())
    }

    fn element(&mut self) -> Result<()> {
        if self.saw_root && self.stack.is_empty() {
            return Err(Error::InvalidInput("feed document is not RSS 2.0"));
        }
        if self.starts_with("<!") {
            return self.reject_declaration();
        }
        if !self.starts_with("<") {
            return Err(Error::InvalidInput("feed document is not RSS 2.0"));
        }
        self.index += 1;
        let closing = self.consume('/');
        let name = self.read_name()?;
        if closing {
            self.skip_space();
            self.require(">")?;
            return self.close(&name);
        }
        let (attributes, empty) = self.attributes()?;
        self.open(name, &attributes, empty)
    }

    fn open(&mut self, raw: String, attributes: &[(String, String)], empty: bool) -> Result<()> {
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or(Error::InvalidInput("feed document exceeds parser depth"))?;
        if self.depth > MAX_DEPTH {
            return Err(Error::InvalidInput("feed document exceeds parser depth"));
        }
        self.push_namespaces(attributes)?;
        let (uri, local) = self.resolve(&raw)?;
        if uri == XINCLUDE_NAMESPACE || raw == "xi:include" {
            return Err(Error::InvalidInput("feed document contains XInclude"));
        }
        let mut kind = classify(&uri, &local);
        if kind == Kind::Live {
            self.feed.live_count = self
                .feed
                .live_count
                .checked_add(1)
                .ok_or(Error::InvalidInput("feed document exceeded parser limits"))?;
        }
        if self.inside(Kind::Live) && kind != Kind::Live {
            kind = Kind::Other;
        }
        if kind == Kind::Item && (self.item.is_some() || !self.inside(Kind::Channel)) {
            kind = Kind::Other;
        }
        self.prepare(kind, attributes)?;
        self.stack.push(Open { raw, kind });
        if empty {
            let raw = self
                .stack
                .last()
                .map(|open| open.raw.clone())
                .ok_or(Error::InvalidInput("feed document is not RSS 2.0"))?;
            self.close(&raw)?;
        }
        Ok(())
    }

    fn prepare(&mut self, kind: Kind, attributes: &[(String, String)]) -> Result<()> {
        match kind {
            Kind::Rss => {
                if self.saw_root
                    || attributes
                        .iter()
                        .any(|(name, value)| name == "version" && value != "2.0")
                    || !attributes.iter().any(|(name, _)| name == "version")
                {
                    return Err(Error::InvalidInput("feed document is not RSS 2.0"));
                }
                self.saw_root = true;
            }
            Kind::Item if self.inside(Kind::Channel) && self.item.is_none() => {
                self.item = Some(ItemBuild::default());
            }
            Kind::Title | Kind::Guid | Kind::PubDate if self.item.is_some() => {
                self.capture = Some((
                    match kind {
                        Kind::Title => Capture::Title,
                        Kind::Guid => Capture::Guid,
                        _ => Capture::Published,
                    },
                    self.depth,
                ));
            }
            Kind::Enclosure if self.item.is_some() => self.enclosure(attributes),
            Kind::Transcript if self.item.is_some() => self.asset(attributes, true),
            Kind::Chapters if self.item.is_some() => self.asset(attributes, false),
            _ => {}
        }
        Ok(())
    }

    fn enclosure(&mut self, attributes: &[(String, String)]) {
        let Some(item) = self.item.as_mut() else {
            return;
        };
        if item.enclosure_url.is_some() {
            return;
        }
        let Some(url) = attr(attributes, "url").and_then(|url| resolve_http_url(url, self.base))
        else {
            return;
        };
        item.enclosure_url = Some(url);
        item.enclosure_type = attr(attributes, "type").and_then(|value| bounded(value, 128));
        item.enclosure_length = attr(attributes, "length").and_then(parse_length);
    }

    fn asset(&mut self, attributes: &[(String, String)], transcript: bool) {
        let Some(url) = attr(attributes, "url").and_then(|url| resolve_http_url(url, self.base))
        else {
            return;
        };
        let Some(item) = self.item.as_mut() else {
            return;
        };
        let (list, limit) = if transcript {
            (&mut item.transcripts, MAX_TRANSCRIPTS)
        } else {
            (&mut item.chapters, MAX_CHAPTERS)
        };
        if list.len() >= limit {
            self.feed.truncated = true;
            return;
        }
        list.push(AssetRef {
            url,
            media_type: attr(attributes, "type").and_then(|value| bounded(value, 128)),
            language: attr(attributes, "language").and_then(|value| bounded(value, 32)),
        });
    }

    fn close(&mut self, raw: &str) -> Result<()> {
        let top = self
            .stack
            .pop()
            .ok_or(Error::InvalidInput("feed document is not RSS 2.0"))?;
        if top.raw != raw {
            return Err(Error::InvalidInput("feed document is not RSS 2.0"));
        }
        if top.kind == Kind::Item && self.item.is_some() {
            self.finish_item()?;
        }
        if self.capture.is_some_and(|(_, depth)| depth == self.depth) {
            self.capture = None;
        }
        while self
            .namespaces
            .last()
            .is_some_and(|item| item.depth == self.depth)
        {
            self.namespaces.pop();
        }
        self.depth = self.depth.saturating_sub(1);
        Ok(())
    }

    fn finish_item(&mut self) -> Result<()> {
        let item = self
            .item
            .take()
            .ok_or(Error::InvalidInput("feed document is not RSS 2.0"))?;
        let (title, title_truncated) = clean_title(&item.title, item.title_overflow)?;
        if title_truncated {
            self.feed.truncated = true;
        }
        let guid = clean_guid(&item.guid)?;
        let published_ms = if item.published.trim().is_empty() {
            None
        } else {
            parse_pub_date(item.published.trim())
        };
        let identity = if let Some(guid) = guid {
            EpisodeIdentity::PublisherGuid(guid)
        } else if let (Some(url), Some(published_ms)) = (item.enclosure_url.clone(), published_ms) {
            EpisodeIdentity::Derived { url, published_ms }
        } else {
            self.feed.skipped = self
                .feed
                .skipped
                .checked_add(1)
                .ok_or(Error::InvalidInput("feed document exceeded parser limits"))?;
            return Ok(());
        };
        let key = identity.key();
        if !self.seen.insert(key) {
            return Ok(());
        }
        if self.feed.episodes.len() >= MAX_ITEMS {
            self.feed.truncated = true;
            return Ok(());
        }
        self.feed.episodes.push(ParsedEpisode {
            identity,
            title,
            published_ms,
            enclosure_url: item.enclosure_url,
            enclosure_type: item.enclosure_type,
            enclosure_length: item.enclosure_length,
            transcripts: item.transcripts,
            chapters: item.chapters,
        });
        Ok(())
    }

    fn text_until(&mut self, end: char) -> Result<()> {
        let start = self.index;
        while self.index < self.input.len() && !self.input[self.index..].starts_with(end) {
            self.index += self.input[self.index..]
                .chars()
                .next()
                .map_or(1, char::len_utf8);
        }
        if self.capture.is_some() {
            let text = decode_entities(&self.input[start..self.index])?;
            self.append_capture(&text)?;
        } else {
            reject_entities(&self.input[start..self.index])?;
        }
        Ok(())
    }

    fn append_capture(&mut self, text: &str) -> Result<()> {
        let Some((which, _)) = self.capture else {
            return Ok(());
        };
        let Some(item) = self.item.as_mut() else {
            return Ok(());
        };
        let (buffer, limit) = match which {
            Capture::Title => (&mut item.title, 4_096),
            Capture::Guid => (&mut item.guid, 2_048),
            Capture::Published => (&mut item.published, 128),
        };
        if buffer.len().saturating_add(text.len()) > limit {
            return match which {
                Capture::Title => {
                    item.title_overflow = true;
                    self.feed.truncated = true;
                    self.capture = None;
                    Ok(())
                }
                Capture::Guid => Err(Error::InvalidInput("feed document exceeded parser limits")),
                Capture::Published => {
                    item.published.clear();
                    self.capture = None;
                    Ok(())
                }
            };
        }
        buffer.push_str(text);
        Ok(())
    }

    fn attributes(&mut self) -> Result<(Vec<(String, String)>, bool)> {
        let mut attributes = Vec::new();
        loop {
            self.skip_space();
            if self.consume('/') {
                self.require(">")?;
                return Ok((attributes, true));
            }
            if self.consume('>') {
                return Ok((attributes, false));
            }
            if attributes.len() == 64 {
                return Err(Error::InvalidInput("feed document exceeded parser limits"));
            }
            let name = self.read_name()?;
            self.skip_space();
            self.require("=")?;
            self.skip_space();
            let quote = self
                .input
                .as_bytes()
                .get(self.index)
                .copied()
                .filter(|byte| matches!(byte, b'"' | b'\''))
                .ok_or(Error::InvalidInput(
                    "feed document rejected an XML construct",
                ))?;
            self.index += 1;
            let start = self.index;
            while self.index < self.input.len() && self.input.as_bytes()[self.index] != quote {
                if self.input.as_bytes()[self.index] == b'<' {
                    return Err(Error::InvalidInput(
                        "feed document rejected an XML construct",
                    ));
                }
                self.index += 1;
            }
            if self.index - start > 4_096 || self.index >= self.input.len() {
                return Err(Error::InvalidInput("feed document exceeded parser limits"));
            }
            let value = decode_entities(&self.input[start..self.index])?;
            self.index += 1;
            if attributes.iter().any(|(existing, _)| existing == &name) {
                return Err(Error::InvalidInput(
                    "feed document rejected an XML construct",
                ));
            }
            attributes.push((name, value));
        }
    }

    fn push_namespaces(&mut self, attributes: &[(String, String)]) -> Result<()> {
        for (name, value) in attributes {
            let prefix = if name == "xmlns" {
                ""
            } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                prefix
            } else {
                continue;
            };
            if prefix.len() > 64 || value.len() > 256 {
                return Err(Error::InvalidInput("feed document exceeded parser limits"));
            }
            self.namespaces.push(Namespace {
                depth: self.depth,
                prefix: prefix.to_owned(),
                uri: value.clone(),
            });
        }
        Ok(())
    }

    fn resolve(&self, qname: &str) -> Result<(String, String)> {
        let (prefix, local) = qname
            .split_once(':')
            .map_or(("", qname), |(prefix, local)| (prefix, local));
        if local.is_empty() || local.contains(':') || prefix.len() > 64 || local.len() > 64 {
            return Err(Error::InvalidInput(
                "feed document rejected an XML construct",
            ));
        }
        let uri = self.lookup(prefix);
        if !prefix.is_empty() && uri.is_none() {
            return Err(Error::InvalidInput(
                "feed document rejected an XML construct",
            ));
        }
        Ok((uri.unwrap_or_default(), local.to_owned()))
    }

    fn lookup(&self, prefix: &str) -> Option<String> {
        self.namespaces
            .iter()
            .rev()
            .find(|item| item.prefix == prefix)
            .map(|item| item.uri.clone())
    }

    fn inside(&self, kind: Kind) -> bool {
        self.stack.iter().any(|open| open.kind == kind)
    }

    fn reject_declaration(&self) -> Result<()> {
        let rest = self.input[self.index..].to_ascii_lowercase();
        if rest.starts_with("<!doctype") || rest.starts_with("<!entity") {
            return Err(Error::InvalidInput("feed document contains a DTD"));
        }
        Err(Error::InvalidInput(
            "feed document rejected an XML construct",
        ))
    }

    fn skip_comment(&mut self) -> Result<()> {
        let rest = &self.input[self.index + 4..];
        let end = rest.find("-->").ok_or(Error::InvalidInput(
            "feed document rejected an XML construct",
        ))?;
        if rest[..end].contains("--") {
            return Err(Error::InvalidInput(
                "feed document rejected an XML construct",
            ));
        }
        self.index += 4 + end + 3;
        Ok(())
    }

    fn skip_pi(&mut self) -> Result<()> {
        if self.starts_with("<?xml") {
            return Err(Error::InvalidInput("feed document is not RSS 2.0"));
        }
        let end = self.input[self.index..]
            .find("?>")
            .ok_or(Error::InvalidInput(
                "feed document rejected an XML construct",
            ))?;
        self.index += end + 2;
        Ok(())
    }

    fn read_name(&mut self) -> Result<String> {
        let start = self.index;
        let mut chars = self.input[self.index..].chars();
        let Some(first) = chars.next() else {
            return Err(Error::InvalidInput("feed document is not RSS 2.0"));
        };
        if !first.is_ascii_alphabetic() && first != '_' {
            return Err(Error::InvalidInput(
                "feed document rejected an XML construct",
            ));
        }
        self.index += first.len_utf8();
        while let Some(next) = self.input[self.index..].chars().next() {
            if next.is_ascii_alphanumeric() || matches!(next, '_' | '-' | '.' | ':') {
                self.index += next.len_utf8();
            } else {
                break;
            }
        }
        if self.index - start > 128 {
            return Err(Error::InvalidInput("feed document exceeded parser limits"));
        }
        Ok(self.input[start..self.index].to_owned())
    }

    fn consume_cdata(&mut self) -> Result<()> {
        let rest = &self.input[self.index + 9..];
        let end = rest.find("]]>").ok_or(Error::InvalidInput(
            "feed document rejected an XML construct",
        ))?;
        if self.capture.is_some() {
            self.append_capture(&rest[..end])?;
        }
        self.index += 9 + end + 3;
        Ok(())
    }

    fn consume_text(&mut self) -> Result<()> {
        self.text_until('<')
    }

    fn skip_space(&mut self) {
        while self.input[self.index..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_whitespace())
        {
            self.index += 1;
        }
    }

    fn starts_with(&self, value: &str) -> bool {
        self.input[self.index..].starts_with(value)
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.input[self.index..].starts_with(expected) {
            self.index += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn require(&mut self, value: &str) -> Result<()> {
        if self.starts_with(value) {
            self.index += value.len();
            Ok(())
        } else {
            Err(Error::InvalidInput(
                "feed document rejected an XML construct",
            ))
        }
    }
}

fn classify(uri: &str, local: &str) -> Kind {
    if uri == PODCAST_NAMESPACE {
        return match local {
            "liveItem" => Kind::Live,
            "transcript" => Kind::Transcript,
            "chapters" => Kind::Chapters,
            _ => Kind::Other,
        };
    }
    if !uri.is_empty() {
        return Kind::Other;
    }
    match local {
        "rss" => Kind::Rss,
        "channel" => Kind::Channel,
        "item" => Kind::Item,
        "title" => Kind::Title,
        "guid" => Kind::Guid,
        "pubDate" => Kind::PubDate,
        "enclosure" => Kind::Enclosure,
        _ => Kind::Other,
    }
}

fn attr<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn bounded(value: &str, maximum: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.chars().any(unsafe_display) {
        None
    } else {
        Some(value.to_owned())
    }
}

fn parse_length(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn clean_title(raw: &str, overflow: bool) -> Result<(Option<String>, bool)> {
    let mut text = String::new();
    for character in raw.chars() {
        if matches!(character, '\t' | '\n' | '\r') {
            text.push(' ');
        } else if unsafe_display(character) {
            return Err(Error::InvalidInput(
                "feed document rejected an XML construct",
            ));
        } else {
            text.push(character);
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok((None, overflow));
    }
    if trimmed.len() <= 512 {
        return Ok((Some(trimmed.to_owned()), overflow));
    }
    let mut end = 512;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    Ok((Some(trimmed[..end].to_owned()), true))
}

fn clean_guid(raw: &str) -> Result<Option<String>> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 2_048 || value.chars().any(unsafe_display) {
        return Err(Error::InvalidInput("feed document exceeded parser limits"));
    }
    Ok(Some(value.to_owned()))
}

fn reject_entities(input: &str) -> Result<()> {
    let _ = decode_entities(input)?;
    Ok(())
}

fn decode_entities(input: &str) -> Result<String> {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        let token = rest[start + 1..]
            .split(';')
            .next()
            .ok_or(Error::InvalidInput(
                "feed document rejected an XML construct",
            ))?;
        if token.len() > 16 || !rest[start + 1..].contains(';') {
            return Err(Error::InvalidInput(
                "feed document rejected an XML construct",
            ));
        }
        output.push(decode_reference(token)?);
        rest = &rest[start + 1 + token.len() + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

fn decode_reference(token: &str) -> Result<char> {
    match token {
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "amp" => Ok('&'),
        "quot" => Ok('"'),
        "apos" => Ok('\''),
        _ if token.starts_with('#') => decode_numeric(&token[1..]),
        _ => Err(Error::InvalidInput(
            "feed document contains an external entity",
        )),
    }
}

fn decode_numeric(token: &str) -> Result<char> {
    let (digits, radix) =
        if let Some(hex) = token.strip_prefix('x').or_else(|| token.strip_prefix('X')) {
            (hex, 16)
        } else {
            (token, 10)
        };
    if digits.is_empty() || digits.len() > 6 {
        return Err(Error::InvalidInput(
            "feed document rejected an XML construct",
        ));
    }
    let value = u32::from_str_radix(digits, radix)
        .map_err(|_| Error::InvalidInput("feed document rejected an XML construct"))?;
    let character = char::from_u32(value)
        .filter(|character| xml_char(*character))
        .ok_or(Error::InvalidInput(
            "feed document rejected an XML construct",
        ))?;
    Ok(character)
}

fn xml_char(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\r' | ' '..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::{MAX_DEPTH, parse};
    use crate::Error;
    use std::fmt::Write;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn document(items: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel>{items}</channel></rss>"#
        )
    }

    #[test]
    fn items_keep_guid_identity_assets_and_skip_titles() -> TestResult {
        let feed = parse(
            &document(
                r#"<item><title>Same</title><guid isPermaLink="false">ep-1</guid><pubDate>Thu, 01 Jan 1970 00:00:00 GMT</pubDate><enclosure url="https://cdn.example/a.mp3" length="3" type="audio/mpeg"/><podcast:transcript url="https://cdn.example/a.vtt" type="text/vtt" language="en"/><podcast:chapters url="https://cdn.example/a.json" type="application/json"/></item><item><title>Same</title><guid>ep-2</guid></item><item><title>Same</title><guid>ep-1</guid><title>Later</title></item><item><title>Untitled</title></item><item><title>Derived</title><pubDate>01 Jan 1970 00:00:00 GMT</pubDate><enclosure url="HTTPS://CDN.Example/b.mp3"/></item><podcast:liveItem status="live"><enclosure url="https://cdn.example/live.mp3"/><podcast:transcript url="https://cdn.example/live.vtt" type="text/vtt"/></podcast:liveItem>"#,
            ),
            None,
        )?;
        assert_eq!(feed.episodes.len(), 3);
        assert!(!feed.truncated);
        assert_eq!(feed.live_count, 1);
        assert_eq!(feed.skipped, 1);
        assert_eq!(feed.episodes[0].title.as_deref(), Some("Same"));
        assert_eq!(feed.episodes[0].transcripts.len(), 1);
        assert_eq!(feed.episodes[0].chapters.len(), 1);
        assert_eq!(feed.episodes[0].published_ms, Some(0));
        assert!(matches!(
            &feed.episodes[0].identity,
            super::EpisodeIdentity::PublisherGuid(guid) if guid == "ep-1"
        ));
        assert!(matches!(
            &feed.episodes[2].identity,
            super::EpisodeIdentity::Derived { url, published_ms }
                if url == "https://cdn.example/b.mp3" && *published_ms == 0
        ));
        assert_ne!(
            feed.episodes[0].identity.key(),
            feed.episodes[1].identity.key()
        );
        Ok(())
    }

    #[test]
    fn hostile_constructs_fail_closed() {
        let samples = [
            (
                r#"<!DOCTYPE rss [<!ENTITY xxe SYSTEM "file:///secret">]><rss version="2.0"><channel/></rss>"#,
                "DTD",
            ),
            (
                r#"<rss version="2.0"><channel><item><guid>&secret;</guid></item></channel></rss>"#,
                "external entity",
            ),
            (
                r#"<rss version="2.0" xmlns:xi="http://www.w3.org/2001/XInclude"><channel><xi:include href="http://evil.example/x"/></channel></rss>"#,
                "XInclude",
            ),
            (
                r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>No</title></feed>"#,
                "not RSS 2.0",
            ),
        ];
        for (xml, detail) in samples {
            match parse(xml, None) {
                Err(error) => assert!(error.to_string().contains(detail), "{error}"),
                Ok(_) => panic!("{detail} was accepted"),
            }
        }
        let mut deep = String::from(r#"<rss version="2.0"><channel>"#);
        for _ in 0..30 {
            deep.push_str("<a>");
        }
        for _ in 0..30 {
            deep.push_str("</a>");
        }
        deep.push_str("</channel></rss>");
        assert!(parse(&deep, None).is_ok(), "depth {MAX_DEPTH} is allowed");
        deep.insert_str(r#"<rss version="2.0"><channel>"#.len(), "<a>");
        deep.insert_str(deep.len() - "</channel></rss>".len(), "</a>");
        assert!(matches!(
            parse(&deep, None),
            Err(Error::InvalidInput(detail)) if detail.contains("depth")
        ));
    }

    #[test]
    fn further_items_are_counted_as_truncated() -> TestResult {
        let mut items = String::new();
        for index in 0..501 {
            write!(items, "<item><guid>g{index}</guid></item>")?;
        }
        let feed = parse(&document(&items), None)?;
        assert_eq!(feed.episodes.len(), 500);
        assert!(feed.truncated);
        Ok(())
    }

    #[test]
    fn relative_references_use_the_fetched_document_not_xml_base() -> TestResult {
        let base = reqwest::Url::parse("http://fixture.example/show/feed.xml")?;
        let feed = parse(
            &document(
                r#"<item xml:base="http://evil.example/"><guid>ep</guid><enclosure url="../audio/1.mp3"/><podcast:transcript url="/notes.vtt" type="text/vtt"/></item>"#,
            ),
            Some(&base),
        )?;
        assert_eq!(
            feed.episodes[0].enclosure_url.as_deref(),
            Some("http://fixture.example/audio/1.mp3")
        );
        assert_eq!(
            feed.episodes[0].transcripts[0].url,
            "http://fixture.example/notes.vtt"
        );
        Ok(())
    }
}
