//! Bounded literal search over stored original-script cues and their English translations.
//!
//! A search reads the catalog only. It creates no job, finding, briefing, ledger event or
//! network request. Each cue is compared with [`crate::monitor::term_matches`], the same
//! case-insensitive literal containment that monitor matches use, so one term finds the
//! same cues in both places. A hit is a place to check, not a finding. Recognized text
//! and translations are unreviewed machine output.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, monitor::MAX_TERM_CHARS, storage::validate_key};

/// Hits returned when the request names no limit.
pub const ARCHIVE_DEFAULT_RESULTS: u32 = 16;
/// Most hits one request returns.
pub const ARCHIVE_MAX_RESULTS: u32 = 64;
/// Rows read when the request names no budget: transcript revisions and cue rows.
pub const ARCHIVE_DEFAULT_ROWS: u32 = 20_000;
/// The smallest row budget. Two rows always advance the cursor.
pub const ARCHIVE_MIN_ROWS: u32 = 2;
/// Most rows one request reads.
pub const ARCHIVE_MAX_ROWS: u32 = 200_000;
/// Wall-time bound when the request names none.
pub const ARCHIVE_DEFAULT_DEADLINE_MS: u32 = 1_000;
/// The shortest deadline a request may ask for.
pub const ARCHIVE_MIN_DEADLINE_MS: u32 = 10;
/// The longest deadline. The catalog actor serves other requests after it.
pub const ARCHIVE_MAX_DEADLINE_MS: u32 = 2_000;
/// Serialized page bound, including JSON escaping. It leaves room for the snapshot
/// envelope inside the 64 KiB agent tool output.
pub const ARCHIVE_PAGE_BYTES: usize = 57_344;
/// Language labels reported on one hit.
pub const ARCHIVE_MAX_LANGUAGES: usize = 8;
/// Language label rows read for one transcript revision.
pub const ARCHIVE_MAX_UNIT_LABELS: usize = 1_024;

/// Which stored text the term is compared with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFields {
    /// The original-script cue only, like a monitor term in a language other than `en` or
    /// `und`.
    Original,
    /// The English translation only.
    English,
    /// The original first, then the English, like a monitor term in `en` or `und`.
    #[default]
    Both,
}

impl ArchiveFields {
    #[must_use]
    pub const fn original(self) -> bool {
        matches!(self, Self::Original | Self::Both)
    }

    #[must_use]
    pub const fn english(self) -> bool {
        matches!(self, Self::English | Self::Both)
    }
}

/// Which revisions are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveRevisions {
    /// The newest text revision of each transcript and its newest translation.
    #[default]
    Current,
    /// Every stored text revision and every translation revision, each labeled.
    All,
}

/// The first position a continued search reads. Rows are ordered by transcript ID,
/// transcript revision, pass and cue ordinal. Pass 0 reads the cues of that revision with
/// its newest translation; pass N reads the older translation revision N.
/// It travels as the text `ID/REVISION/PASS/ORDINAL` in JSON and on the command line, so a
/// page's `next` is passed back unchanged as `after`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ArchiveCursor {
    pub transcript_id: String,
    /// One past the newest stored revision is allowed: it continues at the next transcript.
    pub transcript_revision: i64,
    pub pass: u32,
    pub ordinal: u32,
}

impl ArchiveCursor {
    fn validate(&self) -> Result<()> {
        validate_key(&self.transcript_id, "archive cursor")?;
        if !(1..=65).contains(&self.transcript_revision) || self.pass > 64 || self.ordinal > 256 {
            return Err(Error::InvalidInput("archive cursor"));
        }
        Ok(())
    }

    /// The text form used by the command line: `ID/REVISION/PASS/ORDINAL`.
    #[must_use]
    pub fn token(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.transcript_id, self.transcript_revision, self.pass, self.ordinal
        )
    }

    /// Parse the command-line form. Transcript IDs never contain `/`.
    /// # Errors
    /// Refuses a malformed token or one outside the cursor bounds.
    pub fn parse(token: &str) -> Result<Self> {
        let invalid = || Error::InvalidInput("archive cursor");
        let mut parts = token.rsplitn(4, '/');
        let ordinal = parts.next().ok_or_else(invalid)?;
        let pass = parts.next().ok_or_else(invalid)?;
        let revision = parts.next().ok_or_else(invalid)?;
        let id = parts.next().ok_or_else(invalid)?;
        // At most three ASCII digits and no sign, so every value fits each target type.
        let number = |value: &str| -> Result<u16> {
            if value.is_empty() || value.len() > 3 || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            value.parse::<u16>().map_err(|_| invalid())
        };
        let cursor = Self {
            transcript_id: id.to_owned(),
            transcript_revision: i64::from(number(revision)?),
            pass: u32::from(number(pass)?),
            ordinal: u32::from(number(ordinal)?),
        };
        cursor.validate()?;
        Ok(cursor)
    }
}

impl TryFrom<String> for ArchiveCursor {
    type Error = Error;

    fn try_from(token: String) -> Result<Self> {
        Self::parse(&token)
    }
}

impl From<ArchiveCursor> for String {
    fn from(cursor: ArchiveCursor) -> Self {
        cursor.token()
    }
}

/// One literal search. Omitted bounds take their defaults; the page echoes the bounds used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveQuery {
    /// Compared as written, ignoring letter case only.
    pub term: String,
    #[serde(default)]
    pub fields: ArchiveFields,
    #[serde(default)]
    pub revisions: ArchiveRevisions,
    /// Only recordings of this source revision.
    #[serde(default)]
    pub source: Option<String>,
    /// Half-open window on capture start times, Unix milliseconds. Both or neither.
    #[serde(default)]
    pub from_ms: Option<i64>,
    #[serde(default)]
    pub to_ms: Option<i64>,
    /// Only cues that a stored language label overlaps. A primary subtag such as `fr`
    /// also accepts `fr-CA`.
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub scan_rows: Option<u32>,
    #[serde(default)]
    pub deadline_ms: Option<u32>,
    #[serde(default)]
    pub after: Option<ArchiveCursor>,
}

impl ArchiveQuery {
    /// A current-revision search over both texts with default bounds.
    #[must_use]
    pub fn new(term: &str) -> Self {
        Self {
            term: term.to_owned(),
            fields: ArchiveFields::Both,
            revisions: ArchiveRevisions::Current,
            source: None,
            from_ms: None,
            to_ms: None,
            language: None,
            limit: None,
            scan_rows: None,
            deadline_ms: None,
            after: None,
        }
    }

    /// Check every bound and fill omitted ones, so the page states exactly what ran.
    /// # Errors
    /// Refuses an empty, padded, oversized or control-bearing term, a malformed source,
    /// window, language tag or cursor, and bounds outside their ranges.
    pub fn validate(mut self) -> Result<Self> {
        let count = self.term.chars().count();
        if count == 0
            || count > MAX_TERM_CHARS
            || self.term.chars().any(char::is_control)
            || self.term.trim() != self.term
        {
            return Err(Error::InvalidInput("archive term"));
        }
        if let Some(source) = &self.source {
            validate_key(source, "source revision")?;
        }
        match (self.from_ms, self.to_ms) {
            (None, None) => {}
            (Some(from), Some(to)) if from >= 0 && to > from => {}
            _ => return Err(Error::InvalidInput("archive window")),
        }
        if let Some(language) = &self.language {
            self.language = Some(crate::languages::normalize_tag(language)?);
        }
        let limit = self.limit.unwrap_or(ARCHIVE_DEFAULT_RESULTS);
        let rows = self.scan_rows.unwrap_or(ARCHIVE_DEFAULT_ROWS);
        let deadline = self.deadline_ms.unwrap_or(ARCHIVE_DEFAULT_DEADLINE_MS);
        if !(1..=ARCHIVE_MAX_RESULTS).contains(&limit) {
            return Err(Error::InvalidInput("archive result limit"));
        }
        if !(ARCHIVE_MIN_ROWS..=ARCHIVE_MAX_ROWS).contains(&rows) {
            return Err(Error::InvalidInput("archive row limit"));
        }
        if !(ARCHIVE_MIN_DEADLINE_MS..=ARCHIVE_MAX_DEADLINE_MS).contains(&deadline) {
            return Err(Error::InvalidInput("archive deadline"));
        }
        if let Some(cursor) = &self.after {
            cursor.validate()?;
        }
        self.limit = Some(limit);
        self.scan_rows = Some(rows);
        self.deadline_ms = Some(deadline);
        Ok(self)
    }

    /// Whether a stored language tag satisfies the language filter. An exact tag matches
    /// ignoring ASCII case. A filter with no subtags also matches that primary language.
    #[must_use]
    pub fn language_accepts(filter: &str, tag: &str) -> bool {
        if tag.eq_ignore_ascii_case(filter) {
            return true;
        }
        !filter.contains('-')
            && tag
                .split('-')
                .next()
                .is_some_and(|primary| primary.eq_ignore_ascii_case(filter))
    }
}

/// Where the term was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveField {
    Original,
    English,
}

/// What the catalog says about the cue's audio when the page was read. It is a metadata
/// statement: the file is not opened or hash-verified, and nothing is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveMedia {
    /// The recording is retained with the transcript's checksum, one published interval
    /// covers the cue, no gap overlaps it, and its segment is not released.
    Retained,
    /// The recording is retained, but the segment that held the cue was released.
    Released,
    /// The recording is deleting or deleted.
    Expired,
    /// The cue lies outside every published interval or overlaps a gap.
    Missing,
    /// The interval is covered but the recording is not verified retained, for example a
    /// checksum mismatch.
    Unavailable,
}

/// One stored language label that overlaps the cue. Labels are evidence, not quality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveLanguage {
    pub tag: String,
    pub evidence_id: String,
    pub evidence_revision: u32,
    /// The transcript revision the evidence is bound to, if any. An older revision than
    /// the hit's means the evidence was produced before a correction.
    pub transcript_revision: Option<i64>,
    /// The route capability stored with the span, such as `unevaluated`.
    pub capability: String,
}

/// One cue that contains the term, with everything needed to go back to the audio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveHit {
    pub source: String,
    pub recording_id: String,
    pub capture_start_ms: i64,
    pub transcript_id: String,
    pub transcript_revision: i64,
    /// `recognition` or `correction`.
    pub transcript_kind: String,
    pub cue_ordinal: u32,
    /// Media clock of the cue in the pinned recording, half-open.
    pub start_us: u64,
    pub end_us: u64,
    pub field: ArchiveField,
    pub original: String,
    pub translation_revision: Option<i64>,
    pub english: Option<String>,
    pub untranslated_reason: Option<String>,
    /// A newer text revision of this transcript exists.
    pub stale_transcript: bool,
    /// A newer translation of this transcript revision exists.
    pub stale_translation: bool,
    pub media: ArchiveMedia,
    pub languages: Vec<ArchiveLanguage>,
    /// More overlapping labels were stored than one hit reports.
    pub more_languages: bool,
}

/// Why a page ended before the end of the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveStop {
    /// Another hit exists beyond the result limit.
    Results,
    /// Another hit exists but would not fit the serialized page bound.
    PageBytes,
    /// The row budget was spent.
    Rows,
    /// The wall-time bound passed.
    Deadline,
}

/// One bounded page. `stopped` is absent only when every row after the cursor was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivePage {
    /// The validated query with every bound filled in.
    pub query: ArchiveQuery,
    pub hits: Vec<ArchiveHit>,
    /// Transcript revision rows read, including those the filters skipped.
    pub transcripts_scanned: u32,
    /// Transcript revision rows plus cue rows read. Never above the row budget.
    pub rows_scanned: u32,
    /// Language label rows read, bounded per transcript revision.
    pub language_rows_scanned: u32,
    pub stopped: Option<ArchiveStop>,
    /// Pass this as `after` with the same query to continue.
    pub next: Option<ArchiveCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_fill_bounds_and_refuse_hostile_or_oversized_input() -> Result<()> {
        let query = ArchiveQuery::new("feria").validate()?;
        assert_eq!(query.limit, Some(ARCHIVE_DEFAULT_RESULTS));
        assert_eq!(query.scan_rows, Some(ARCHIVE_DEFAULT_ROWS));
        assert_eq!(query.deadline_ms, Some(ARCHIVE_DEFAULT_DEADLINE_MS));
        assert_eq!(query.fields, ArchiveFields::Both);
        assert_eq!(query.revisions, ArchiveRevisions::Current);
        let longest = "\u{0939}".repeat(MAX_TERM_CHARS);
        assert!(ArchiveQuery::new(&longest).validate().is_ok());
        for term in [
            String::new(),
            " feria".into(),
            "feria ".into(),
            "fe\u{1b}[31mria".into(),
            "fe\nria".into(),
            "a".repeat(MAX_TERM_CHARS + 1),
        ] {
            assert!(
                matches!(
                    ArchiveQuery::new(&term).validate(),
                    Err(Error::InvalidInput("archive term"))
                ),
                "{term:?}"
            );
        }
        let mut query = ArchiveQuery::new("x");
        query.from_ms = Some(10);
        assert!(query.clone().validate().is_err());
        query.to_ms = Some(10);
        assert!(query.clone().validate().is_err());
        query.to_ms = Some(11);
        assert!(query.clone().validate().is_ok());
        query.from_ms = Some(-1);
        assert!(query.validate().is_err());
        for (limit, rows, deadline) in [
            (Some(0), None, None),
            (Some(ARCHIVE_MAX_RESULTS + 1), None, None),
            (None, Some(1), None),
            (None, Some(ARCHIVE_MAX_ROWS + 1), None),
            (None, None, Some(9)),
            (None, None, Some(ARCHIVE_MAX_DEADLINE_MS + 1)),
        ] {
            let mut query = ArchiveQuery::new("x");
            query.limit = limit;
            query.scan_rows = rows;
            query.deadline_ms = deadline;
            assert!(query.validate().is_err());
        }
        let mut query = ArchiveQuery::new("x");
        query.source = Some("radio/v1".into());
        assert!(query.validate().is_err());
        let mut query = ArchiveQuery::new("x");
        query.language = Some("FR-ca".into());
        assert_eq!(query.validate()?.language.as_deref(), Some("fr-CA"));
        let mut query = ArchiveQuery::new("x");
        query.language = Some("fr_CA".into());
        assert!(query.validate().is_err());
        Ok(())
    }

    #[test]
    fn cursors_round_trip_and_refuse_out_of_range_positions() -> Result<()> {
        let cursor = ArchiveCursor {
            transcript_id: "pin:a.b-c_d".into(),
            transcript_revision: 65,
            pass: 64,
            ordinal: 256,
        };
        assert_eq!(cursor.token(), "pin:a.b-c_d/65/64/256");
        assert_eq!(ArchiveCursor::parse(&cursor.token())?, cursor);
        let wire = serde_json::to_string(&cursor)?;
        assert_eq!(wire, "\"pin:a.b-c_d/65/64/256\"");
        assert_eq!(serde_json::from_str::<ArchiveCursor>(&wire)?, cursor);
        assert!(serde_json::from_str::<ArchiveCursor>("\"pin/0/0/0\"").is_err());
        assert!(
            serde_json::from_str::<ArchiveCursor>(
                r#"{"transcript_id":"pin","transcript_revision":1,"pass":0,"ordinal":0}"#
            )
            .is_err()
        );
        for token in [
            "",
            "pin",
            "pin/1/0",
            "/1/0/0",
            "pin/0/0/0",
            "pin/66/0/0",
            "pin/1/65/0",
            "pin/1/0/257",
            "pin/1/0/-1",
            "pin/1/0/0001",
            "pin/1/+0/0",
            "a/b/1/0/0",
            "pin\u{1b}/1/0/0",
        ] {
            assert!(ArchiveCursor::parse(token).is_err(), "{token:?}");
        }
        let mut query = ArchiveQuery::new("x");
        query.after = Some(ArchiveCursor {
            transcript_id: "pin".into(),
            transcript_revision: 0,
            pass: 0,
            ordinal: 0,
        });
        assert!(query.validate().is_err());
        Ok(())
    }

    #[test]
    fn language_filters_compare_tags_or_primary_subtags() {
        assert!(ArchiveQuery::language_accepts("fr", "fr"));
        assert!(ArchiveQuery::language_accepts("fr", "fr-CA"));
        assert!(ArchiveQuery::language_accepts("fr-CA", "fr-ca"));
        assert!(!ArchiveQuery::language_accepts("fr-CA", "fr"));
        assert!(!ArchiveQuery::language_accepts("fr", "frr"));
        assert!(ArchiveQuery::language_accepts("nv", "nv"));
        assert!(ArchiveQuery::language_accepts("tlh", "tlh-Latn"));
        assert!(!ArchiveQuery::language_accepts("tlh", "i-klingon"));
        assert!(ArchiveFields::Both.original() && ArchiveFields::Both.english());
        assert!(!ArchiveFields::Original.english() && !ArchiveFields::English.original());
    }
}
