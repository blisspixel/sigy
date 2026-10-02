//! `analysis search`: a bounded literal search over stored originals and translations.
//! The service reads the catalog only. This module parses arguments and prints a page;
//! every stored string passes through the terminal sanitizer before it is printed.

use std::io::{self, Write};

use clap::{Args, ValueEnum};
use sigy_service::archive::{
    ArchiveCursor, ArchiveField, ArchiveFields, ArchiveHit, ArchiveMedia, ArchivePage,
    ArchiveQuery, ArchiveRevisions, ArchiveStop,
};
use sigy_service::control::AnalysisOperation;

use crate::explorer::{text::sanitize, utc_label};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Within {
    Original,
    English,
    Both,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Literal term, at most 200 characters. It matches as written, ignoring letter case only.
    #[arg(long, allow_hyphen_values = true)]
    term: String,
    /// Search the original script, the English translation, or both.
    #[arg(long = "in", value_enum, default_value_t = Within::Both)]
    within: Within,
    /// Also search older transcript and translation revisions. Each hit says whether it is stale.
    #[arg(long)]
    history: bool,
    /// Only recordings of this source revision.
    #[arg(long)]
    source: Option<String>,
    /// Earliest capture start, Unix milliseconds.
    #[arg(long, requires = "to_ms")]
    from_ms: Option<i64>,
    /// Capture start before this time (exclusive), Unix milliseconds.
    #[arg(long, requires = "from_ms")]
    to_ms: Option<i64>,
    /// Only cues that stored language evidence labels with this tag, such as fr or fr-CA.
    #[arg(long)]
    language: Option<String>,
    /// Most hits, 1 to 64. Defaults to 16.
    #[arg(long)]
    limit: Option<u32>,
    /// Most catalog rows read, 2 to 200000. Defaults to 20000.
    #[arg(long)]
    scan_rows: Option<u32>,
    /// Wall-time bound in milliseconds, 10 to 2000. Defaults to 1000.
    #[arg(long)]
    deadline_ms: Option<u32>,
    /// Continue from the cursor a previous page printed, with the same term and options.
    #[arg(long)]
    after: Option<String>,
}

impl SearchArgs {
    /// # Errors
    /// Fails on a malformed cursor. The service checks every other bound.
    pub fn operation(&self) -> Result<AnalysisOperation, Box<dyn std::error::Error>> {
        Ok(AnalysisOperation::Search {
            query: ArchiveQuery {
                term: self.term.clone(),
                fields: match self.within {
                    Within::Original => ArchiveFields::Original,
                    Within::English => ArchiveFields::English,
                    Within::Both => ArchiveFields::Both,
                },
                revisions: if self.history {
                    ArchiveRevisions::All
                } else {
                    ArchiveRevisions::Current
                },
                source: self.source.clone(),
                from_ms: self.from_ms,
                to_ms: self.to_ms,
                language: self.language.clone(),
                limit: self.limit,
                scan_rows: self.scan_rows,
                deadline_ms: self.deadline_ms,
                after: self
                    .after
                    .as_deref()
                    .map(ArchiveCursor::parse)
                    .transpose()?,
            },
        })
    }
}

fn scope(query: &ArchiveQuery) -> String {
    let mut parts = vec![
        match query.fields {
            ArchiveFields::Original => "original script",
            ArchiveFields::English => "English translations",
            ArchiveFields::Both => "original script and English translations",
        }
        .to_owned(),
        match query.revisions {
            ArchiveRevisions::Current => "newest revisions",
            ArchiveRevisions::All => "all stored revisions",
        }
        .to_owned(),
    ];
    if let Some(source) = &query.source {
        parts.push(format!("source {}", sanitize(source, 128)));
    }
    if let (Some(from), Some(to)) = (query.from_ms, query.to_ms) {
        parts.push(format!(
            "captures started {} to {}",
            utc_label(from),
            utc_label(to)
        ));
    }
    if let Some(language) = &query.language {
        parts.push(format!("language label {}", sanitize(language, 128)));
    }
    parts.join(", ")
}

/// Print one page. Stored text is sanitized; the page itself is not changed.
pub fn render(writer: &mut impl Write, page: &ArchivePage) -> io::Result<()> {
    let query = &page.query;
    writeln!(
        writer,
        "Archive search for \"{}\" in {} | {} hits | {} transcript revisions and {} rows read.",
        sanitize(&query.term, 200),
        scope(query),
        page.hits.len(),
        page.transcripts_scanned,
        page.rows_scanned
    )?;
    writeln!(
        writer,
        "The term matches as written, ignoring letter case only: no stemming, transliteration, accent folding or Unicode normalization. Recognized text and English are unreviewed machine output. A hit is a place to check, not a finding. Audio states come from catalog metadata; no file is opened."
    )?;
    for (index, hit) in page.hits.iter().enumerate() {
        render_hit(writer, index + 1, hit)?;
    }
    if page.hits.is_empty() {
        writeln!(writer, "No hits.")?;
    }
    let Some(stop) = page.stopped else {
        return writeln!(
            writer,
            "The scan reached the end of the catalog for this query."
        );
    };
    let reason = match stop {
        ArchiveStop::Results => format!(
            "More hits exist beyond the limit of {}.",
            query.limit.unwrap_or_default()
        ),
        ArchiveStop::PageBytes => {
            "More hits exist, but the next one would not fit in this page.".to_owned()
        }
        ArchiveStop::Rows => format!(
            "The budget of {} rows was spent before the end of the catalog.",
            query.scan_rows.unwrap_or_default()
        ),
        ArchiveStop::Deadline => format!(
            "The {} ms deadline passed before the end of the catalog.",
            query.deadline_ms.unwrap_or_default()
        ),
    };
    match &page.next {
        Some(next) => writeln!(
            writer,
            "{reason} Continue with the same term and options and --after {}",
            sanitize(&next.token(), 160)
        ),
        None => writeln!(writer, "{reason}"),
    }
}

fn render_hit(writer: &mut impl Write, number: usize, hit: &ArchiveHit) -> io::Result<()> {
    writeln!(
        writer,
        "{number}. {} | recording {} started {} | transcript {} revision {} ({}, {}) | cue {} at {} to {}",
        sanitize(&hit.source, 128),
        sanitize(&hit.recording_id, 128),
        utc_label(hit.capture_start_ms),
        sanitize(&hit.transcript_id, 128),
        hit.transcript_revision,
        sanitize(&hit.transcript_kind, 32),
        if hit.stale_transcript {
            "stale: a newer text revision exists"
        } else {
            "current"
        },
        hit.cue_ordinal,
        super::clock(hit.start_us),
        super::clock(hit.end_us)
    )?;
    writeln!(
        writer,
        "   Found in the {}. Audio: {}",
        match hit.field {
            ArchiveField::Original => "original",
            ArchiveField::English => "English",
        },
        match hit.media {
            ArchiveMedia::Retained => "retained.",
            ArchiveMedia::Released => "segment released; the text remains.",
            ArchiveMedia::Expired => "expired; the recording is deleted or being deleted.",
            ArchiveMedia::Missing =>
                "missing for this interval (outside published audio or in a gap).",
            ArchiveMedia::Unavailable => "unavailable; the recording is not verified retained.",
        }
    )?;
    writeln!(writer, "   Original: {}", sanitize(&hit.original, 4096))?;
    render_english(writer, hit)?;
    render_languages(writer, hit)
}

fn render_english(writer: &mut impl Write, hit: &ArchiveHit) -> io::Result<()> {
    let stale = if hit.stale_translation {
        ", stale: a newer translation exists"
    } else {
        ""
    };
    match (
        &hit.english,
        &hit.untranslated_reason,
        hit.translation_revision,
    ) {
        (Some(english), _, Some(revision)) => writeln!(
            writer,
            "   English (translation {revision}{stale}, machine output): {}",
            sanitize(english, 4096)
        ),
        (None, Some(reason), Some(revision)) => writeln!(
            writer,
            "   English: untranslated in translation {revision}{stale}: {}",
            sanitize(reason, 128)
        ),
        _ => writeln!(writer, "   English: no translation of this revision."),
    }
}

fn render_languages(writer: &mut impl Write, hit: &ArchiveHit) -> io::Result<()> {
    if hit.languages.is_empty() {
        return writeln!(writer, "   Language labels: none stored for this cue.");
    }
    let labels: Vec<String> = hit
        .languages
        .iter()
        .map(|label| {
            let binding = label.transcript_revision.map_or_else(
                || "unbound".to_owned(),
                |revision| format!("on transcript revision {revision}"),
            );
            format!(
                "{} from evidence {} revision {} {binding}, {}",
                sanitize(&label.tag, 128),
                sanitize(&label.evidence_id, 128),
                label.evidence_revision,
                sanitize(&label.capability, 32)
            )
        })
        .collect();
    writeln!(
        writer,
        "   Language labels: {}{}.",
        labels.join("; "),
        if hit.more_languages {
            "; more not shown"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigy_service::archive::ArchiveLanguage;

    fn hit() -> ArchiveHit {
        ArchiveHit {
            source: "radio:v1".into(),
            recording_id: "one".into(),
            capture_start_ms: 1_790_000_000_000,
            transcript_id: "one".into(),
            transcript_revision: 1,
            transcript_kind: "recognition".into(),
            cue_ordinal: 2,
            start_us: 1_500_000,
            end_us: 2_250_000,
            field: ArchiveField::Original,
            original: "سد\u{1b}[2J النهضة\u{202e}".into(),
            translation_revision: Some(2),
            english: Some("The \u{1b}]0;pwned\u{7}dam".into()),
            untranslated_reason: None,
            stale_transcript: true,
            stale_translation: true,
            media: ArchiveMedia::Released,
            languages: vec![ArchiveLanguage {
                tag: "ar\u{1b}[1m".into(),
                evidence_id: "lid-1".into(),
                evidence_revision: 1,
                transcript_revision: Some(1),
                capability: "unevaluated".into(),
            }],
            more_languages: true,
        }
    }

    fn page(hits: Vec<ArchiveHit>, stopped: Option<ArchiveStop>) -> ArchivePage {
        let mut query = ArchiveQuery::new("النهضة");
        query.limit = Some(16);
        query.scan_rows = Some(20_000);
        query.deadline_ms = Some(1_000);
        query.language = Some("ar".into());
        ArchivePage {
            query,
            hits,
            transcripts_scanned: 3,
            rows_scanned: 40,
            language_rows_scanned: 2,
            stopped,
            next: stopped.map(|_| ArchiveCursor {
                transcript_id: "one".into(),
                transcript_revision: 1,
                pass: 0,
                ordinal: 3,
            }),
        }
    }

    #[test]
    fn hostile_text_is_sanitized_and_original_stays_beside_english()
    -> Result<(), Box<dyn std::error::Error>> {
        let page = page(vec![hit()], Some(ArchiveStop::Results));
        let mut buffer = Vec::new();
        render(&mut buffer, &page)?;
        let text = String::from_utf8(buffer)?;
        for forbidden in ['\u{1b}', '\u{7}', '\u{202e}'] {
            assert!(!text.contains(forbidden), "{text}");
        }
        assert!(text.contains("   Original: سد[2J النهضة\n"), "{text}");
        assert!(text.contains(
            "   English (translation 2, stale: a newer translation exists, machine output): The ]0;pwneddam"
        ));
        assert!(text.contains("(recognition, stale: a newer text revision exists)"));
        assert!(text.contains("cue 2 at 00:01.500 to 00:02.250"));
        assert!(text.contains("Audio: segment released; the text remains."));
        assert!(text.contains(
            "Language labels: ar[1m from evidence lid-1 revision 1 on transcript revision 1, unevaluated; more not shown."
        ));
        assert!(text.contains("language label ar"));
        assert!(text.contains(
            "More hits exist beyond the limit of 16. Continue with the same term and options and --after one/1/0/3"
        ));
        assert!(
            text.contains("no stemming, transliteration, accent folding or Unicode normalization")
        );
        // The page itself is unchanged.
        assert_eq!(page.hits[0].original, "سد\u{1b}[2J النهضة\u{202e}");
        Ok(())
    }

    #[test]
    fn every_stop_reason_and_media_state_has_plain_words() -> Result<(), Box<dyn std::error::Error>>
    {
        for (stop, words) in [
            (
                None,
                "The scan reached the end of the catalog for this query.",
            ),
            (Some(ArchiveStop::PageBytes), "would not fit in this page"),
            (
                Some(ArchiveStop::Rows),
                "The budget of 20000 rows was spent",
            ),
            (Some(ArchiveStop::Deadline), "The 1000 ms deadline passed"),
        ] {
            let mut buffer = Vec::new();
            render(&mut buffer, &page(Vec::new(), stop))?;
            let text = String::from_utf8(buffer)?;
            assert!(text.contains(words), "{text}");
            assert!(text.contains("No hits."));
        }
        for (media, words) in [
            (ArchiveMedia::Retained, "Audio: retained."),
            (ArchiveMedia::Expired, "Audio: expired;"),
            (ArchiveMedia::Missing, "Audio: missing for this interval"),
            (ArchiveMedia::Unavailable, "Audio: unavailable;"),
        ] {
            let mut current = hit();
            current.media = media;
            current.stale_transcript = false;
            current.stale_translation = false;
            current.field = ArchiveField::English;
            current.english = None;
            current.untranslated_reason = Some("unsupported-language".into());
            current.languages.clear();
            let mut buffer = Vec::new();
            render(&mut buffer, &page(vec![current], None))?;
            let text = String::from_utf8(buffer)?;
            assert!(text.contains(words), "{text}");
            assert!(text.contains("(recognition, current)"));
            assert!(text.contains("Found in the English."));
            assert!(text.contains("English: untranslated in translation 2: unsupported-language"));
            assert!(text.contains("Language labels: none stored for this cue."));
        }
        let mut bare = hit();
        bare.translation_revision = None;
        bare.english = None;
        let mut buffer = Vec::new();
        render(&mut buffer, &page(vec![bare], None))?;
        assert!(String::from_utf8(buffer)?.contains("English: no translation of this revision."));
        Ok(())
    }

    #[test]
    fn arguments_map_to_one_read_only_query() -> Result<(), Box<dyn std::error::Error>> {
        let args = SearchArgs {
            term: "-feria".into(),
            within: Within::English,
            history: true,
            source: Some("radio:v1".into()),
            from_ms: Some(1),
            to_ms: Some(2),
            language: Some("fr".into()),
            limit: Some(3),
            scan_rows: Some(4),
            deadline_ms: Some(50),
            after: Some("one/1/0/3".into()),
        };
        let AnalysisOperation::Search { query } = args.operation()? else {
            return Err("search maps to a search operation".into());
        };
        assert_eq!(query.term, "-feria");
        assert_eq!(query.fields, ArchiveFields::English);
        assert_eq!(query.revisions, ArchiveRevisions::All);
        assert_eq!(
            query.after.as_ref().map(ArchiveCursor::token).as_deref(),
            Some("one/1/0/3")
        );
        let original = SearchArgs {
            within: Within::Original,
            history: false,
            after: Some("one/1/0".into()),
            ..args
        };
        assert!(original.operation().is_err());
        let mut fields = Vec::new();
        for within in [Within::Original, Within::Both] {
            let args = SearchArgs {
                term: "x".into(),
                within,
                history: false,
                source: None,
                from_ms: None,
                to_ms: None,
                language: None,
                limit: None,
                scan_rows: None,
                deadline_ms: None,
                after: None,
            };
            let AnalysisOperation::Search { query } = args.operation()? else {
                return Err("search maps to a search operation".into());
            };
            assert_eq!(query.revisions, ArchiveRevisions::Current);
            fields.push(query.fields);
        }
        assert_eq!(fields, [ArchiveFields::Original, ArchiveFields::Both]);
        Ok(())
    }
}
