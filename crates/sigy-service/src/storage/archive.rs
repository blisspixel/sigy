//! Bounded archive search over stored transcripts and translations. Reads only.
//!
//! Rows are visited in catalog key order: transcript ID, transcript revision, pass, cue
//! ordinal. That order follows the primary-key index, so each request reads at most its
//! row budget without sorting the catalog. It is not time order; each hit carries its
//! capture start. Every comparison is [`term_matches`], shared with monitor matches.

use std::time::{Duration, Instant};

use rusqlite::{Connection, Row, Statement, params};

use super::{
    Store,
    cue_media::{CueMedia, classify},
};
use crate::{
    Error, Result,
    archive::{
        ARCHIVE_MAX_LANGUAGES, ARCHIVE_MAX_UNIT_LABELS, ARCHIVE_PAGE_BYTES, ArchiveCursor,
        ArchiveField, ArchiveHit, ArchiveLanguage, ArchiveMedia, ArchivePage, ArchiveQuery,
        ArchiveRevisions, ArchiveStop,
    },
    monitor::term_matches,
};

/// Transcript revision rows fetched by one catalog query.
const HEADER_BATCH: u32 = 64;

const HEADERS: &str = "SELECT t.id, t.revision, t.kind, t.outcome, t.role, t.recording_id, t.media_sha256, t.analysis_id, t.analysis_revision, c.source_revision, c.starts_ms, EXISTS (SELECT 1 FROM transcripts n WHERE n.id = t.id AND n.revision > t.revision AND n.kind IN ('recognition', 'correction') AND n.outcome = 'text'), (SELECT max(tr.revision) FROM translations tr WHERE tr.transcript_id = t.id AND tr.transcript_revision = t.revision) FROM transcripts t JOIN capture_jobs c ON c.id = t.recording_id WHERE (t.id, t.revision) >= (?1, ?2) ORDER BY t.id, t.revision LIMIT ?3";

/// Pass 0: every cue of the revision beside its newest translation, if any.
const NEWEST_ROWS: &str = "SELECT q.ordinal, q.start_us, q.end_us, q.script, tc.english, tc.reason FROM transcript_cues q LEFT JOIN translation_cues tc ON tc.transcript_id = q.transcript_id AND tc.transcript_revision = q.revision AND tc.revision = ?3 AND tc.ordinal = q.ordinal WHERE q.transcript_id = ?1 AND q.revision = ?2 AND q.ordinal >= ?4 ORDER BY q.ordinal";

/// Pass N: the cues of one older translation revision beside their original script.
const OLDER_ROWS: &str = "SELECT q.ordinal, q.start_us, q.end_us, q.script, tc.english, tc.reason FROM translation_cues tc JOIN transcript_cues q ON q.transcript_id = tc.transcript_id AND q.revision = tc.transcript_revision AND q.ordinal = tc.ordinal WHERE tc.transcript_id = ?1 AND tc.transcript_revision = ?2 AND tc.revision = ?3 AND tc.ordinal >= ?4 ORDER BY tc.ordinal";

/// Labels of the newest revision of each evidence track on one analysis input.
const LABELS: &str = "SELECT e.id, e.revision, e.transcript_revision, json_extract(s.value, '$.start_us'), json_extract(s.value, '$.end_us'), json_extract(l.value, '$.tag'), json_extract(s.value, '$.route.capability') FROM language_evidence e, json_each(e.payload_json, '$.spans') s, json_each(s.value, '$.languages') l WHERE e.analysis_id = ?1 AND e.analysis_revision = ?2 AND e.revision = (SELECT max(x.revision) FROM language_evidence x WHERE x.id = e.id) ORDER BY e.id, s.key, l.key LIMIT ?3";

impl Store {
    /// Find a literal term in stored original-script cues and English translations.
    /// Nothing is written, queued or sent.
    /// # Errors
    /// Refuses an invalid query. Inconsistent stored rows are an integrity failure.
    pub fn archive_search(&self, query: ArchiveQuery) -> Result<ArchivePage> {
        let query = query.validate()?;
        let budget = Duration::from_millis(u64::from(
            query
                .deadline_ms
                .ok_or(Error::InvalidInput("archive deadline"))?,
        ));
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or(Error::InvalidInput("archive deadline"))?;
        search(&self.connection, query, deadline)
    }
}

/// Run a validated query until the catalog ends or a bound stops it.
pub(super) fn search(
    connection: &Connection,
    query: ArchiveQuery,
    deadline: Instant,
) -> Result<ArchivePage> {
    Scan::new(connection, query, deadline)?.run()
}

/// The first row not yet examined.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Position {
    id: String,
    revision: i64,
    pass: u32,
    ordinal: u32,
}

impl Position {
    fn cursor(&self) -> ArchiveCursor {
        ArchiveCursor {
            transcript_id: self.id.clone(),
            transcript_revision: self.revision,
            pass: self.pass,
            ordinal: self.ordinal,
        }
    }
}

struct Header {
    id: String,
    revision: i64,
    kind: String,
    outcome: String,
    role: String,
    recording_id: String,
    media_sha256: String,
    analysis_id: String,
    analysis_revision: i64,
    source: String,
    starts_ms: i64,
    stale: bool,
    newest_translation: Option<i64>,
}

fn header(row: &Row<'_>) -> rusqlite::Result<Header> {
    Ok(Header {
        id: row.get(0)?,
        revision: row.get(1)?,
        kind: row.get(2)?,
        outcome: row.get(3)?,
        role: row.get(4)?,
        recording_id: row.get(5)?,
        media_sha256: row.get(6)?,
        analysis_id: row.get(7)?,
        analysis_revision: row.get(8)?,
        source: row.get(9)?,
        starts_ms: row.get(10)?,
        stale: row.get(11)?,
        newest_translation: row.get(12)?,
    })
}

struct Cue {
    ordinal: u32,
    start_us: i64,
    end_us: i64,
    script: String,
    english: Option<String>,
    reason: Option<String>,
}

fn cue(row: &Row<'_>) -> Result<Cue> {
    let cue = Cue {
        ordinal: row.get(0)?,
        start_us: row.get(1)?,
        end_us: row.get(2)?,
        script: row.get(3)?,
        english: row.get(4)?,
        reason: row.get(5)?,
    };
    // Text revisions hold at most 256 cues, so the next ordinal stays a valid cursor.
    if cue.ordinal > 255 || cue.start_us < 0 || cue.end_us <= cue.start_us {
        return Err(Error::StorageIntegrity);
    }
    Ok(cue)
}

struct Label {
    evidence_id: String,
    evidence_revision: u32,
    transcript_revision: Option<i64>,
    start_us: i64,
    end_us: i64,
    tag: String,
    capability: String,
}

fn label(row: &Row<'_>) -> rusqlite::Result<Label> {
    Ok(Label {
        evidence_id: row.get(0)?,
        evidence_revision: row.get(1)?,
        transcript_revision: row.get(2)?,
        start_us: row.get(3)?,
        end_us: row.get(4)?,
        tag: row.get(5)?,
        capability: row.get(6)?,
    })
}

/// Stored labels for one transcript revision's analysis input, read once per unit.
struct Labels {
    rows: Vec<Label>,
    complete: bool,
}

impl Labels {
    fn overlapping(&self, cue: &Cue) -> impl Iterator<Item = &Label> {
        self.rows
            .iter()
            .filter(move |label| label.start_us < cue.end_us && label.end_us > cue.start_us)
    }
}

/// Statements prepared once per request and reused for every transcript revision.
struct Statements<'a> {
    headers: Statement<'a>,
    newest: Statement<'a>,
    older: Statement<'a>,
    labels: Statement<'a>,
}

impl<'a> Statements<'a> {
    fn prepare(connection: &'a Connection) -> Result<Self> {
        Ok(Self {
            headers: connection.prepare(HEADERS)?,
            newest: connection.prepare(NEWEST_ROWS)?,
            older: connection.prepare(OLDER_ROWS)?,
            labels: connection.prepare(LABELS)?,
        })
    }
}

struct Scan<'a> {
    connection: &'a Connection,
    query: ArchiveQuery,
    limit: usize,
    rows: u32,
    deadline: Instant,
    page: ArchivePage,
    bytes: usize,
    /// The resume position has moved past the request cursor.
    advanced: bool,
}

impl<'a> Scan<'a> {
    fn new(connection: &'a Connection, query: ArchiveQuery, deadline: Instant) -> Result<Self> {
        let limit = query
            .limit
            .and_then(|limit| usize::try_from(limit).ok())
            .ok_or(Error::InvalidInput("archive result limit"))?;
        let rows = query
            .scan_rows
            .ok_or(Error::InvalidInput("archive row limit"))?;
        let page = ArchivePage {
            query,
            hits: Vec::new(),
            transcripts_scanned: 0,
            rows_scanned: 0,
            language_rows_scanned: 0,
            stopped: None,
            next: None,
        };
        let bytes = reserved_bytes(&page)?;
        Ok(Self {
            query: page.query.clone(),
            connection,
            limit,
            rows,
            deadline,
            page,
            bytes,
            advanced: false,
        })
    }

    fn run(mut self) -> Result<ArchivePage> {
        let mut at = self.query.after.as_ref().map_or_else(
            || Position {
                id: String::new(),
                revision: 0,
                pass: 0,
                ordinal: 0,
            },
            |cursor| Position {
                id: cursor.transcript_id.clone(),
                revision: cursor.transcript_revision,
                pass: cursor.pass,
                ordinal: cursor.ordinal,
            },
        );
        let start = at.clone();
        let mut statements = Statements::prepare(self.connection)?;
        loop {
            if let Some(stop) = self.bound() {
                return self.finish(Some(stop), Some(&at));
            }
            let remaining = self.rows - self.page.rows_scanned;
            let headers = statements
                .headers
                .query_map(
                    params![at.id, at.revision, remaining.min(HEADER_BATCH)],
                    header,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if headers.is_empty() {
                return self.finish(None, None);
            }
            for header in headers {
                if let Some(stop) = self.bound() {
                    return self.finish(Some(stop), Some(&at));
                }
                self.page.rows_scanned += 1;
                self.page.transcripts_scanned += 1;
                let resumed = header.id == start.id && header.revision == start.revision;
                at = Position {
                    id: header.id.clone(),
                    revision: header.revision,
                    pass: if resumed { start.pass } else { 0 },
                    ordinal: if resumed { start.ordinal } else { 0 },
                };
                if self.eligible(&header)
                    && let Some(stop) = self.unit(&mut statements, &header, &mut at)?
                {
                    return self.finish(Some(stop), Some(&at));
                }
                at = Position {
                    id: header.id,
                    revision: header.revision + 1,
                    pass: 0,
                    ordinal: 0,
                };
                self.advanced = true;
            }
        }
    }

    /// A row or deadline bound that stops the page before the next row. The deadline
    /// applies only after the cursor has moved, so every request makes progress.
    fn bound(&self) -> Option<ArchiveStop> {
        if self.page.rows_scanned >= self.rows {
            return Some(ArchiveStop::Rows);
        }
        (self.advanced && Instant::now() >= self.deadline).then_some(ArchiveStop::Deadline)
    }

    fn eligible(&self, header: &Header) -> bool {
        let text = header.role == "original"
            && header.outcome == "text"
            && matches!(header.kind.as_str(), "recognition" | "correction");
        let revision = self.query.revisions == ArchiveRevisions::All || !header.stale;
        let source = self
            .query
            .source
            .as_ref()
            .is_none_or(|source| *source == header.source);
        let window = match (self.query.from_ms, self.query.to_ms) {
            (Some(from), Some(to)) => from <= header.starts_ms && header.starts_ms < to,
            _ => true,
        };
        text && revision && source && window
    }

    /// Pass 0, then each older translation revision when history and English are read.
    fn passes(&self, header: &Header) -> Result<Vec<u32>> {
        let mut passes = vec![0];
        if self.query.revisions == ArchiveRevisions::All
            && self.query.fields.english()
            && let Some(newest) = header.newest_translation
        {
            let newest = u32::try_from(newest).map_err(|_| Error::StorageIntegrity)?;
            if newest > 64 {
                return Err(Error::StorageIntegrity);
            }
            passes.extend(1..newest);
        }
        Ok(passes)
    }

    fn unit(
        &mut self,
        statements: &mut Statements<'_>,
        header: &Header,
        at: &mut Position,
    ) -> Result<Option<ArchiveStop>> {
        let mut labels = None;
        let first_pass = at.pass;
        for pass in self.passes(header)? {
            if pass < first_pass {
                continue;
            }
            if pass > first_pass {
                at.ordinal = 0;
            }
            at.pass = pass;
            let translation = if pass == 0 {
                header.newest_translation
            } else {
                Some(i64::from(pass))
            };
            let statement = if pass == 0 {
                &mut statements.newest
            } else {
                &mut statements.older
            };
            let mut rows =
                statement.query(params![header.id, header.revision, translation, at.ordinal])?;
            loop {
                if let Some(stop) = self.bound() {
                    return Ok(Some(stop));
                }
                let Some(row) = rows.next()? else {
                    break;
                };
                let cue = cue(row)?;
                at.ordinal = cue.ordinal;
                self.page.rows_scanned += 1;
                self.advanced = true;
                if let Some(field) = self.field(pass, &cue) {
                    if labels.is_none() {
                        labels = Some(self.labels(&mut statements.labels, header)?);
                    }
                    let unit_labels = labels.as_ref().ok_or(Error::StorageIntegrity)?;
                    if self.accepts(unit_labels, &cue) {
                        if self.page.hits.len() == self.limit {
                            return Ok(Some(ArchiveStop::Results));
                        }
                        let hit = self.hit(header, pass, field, cue, unit_labels)?;
                        if let Some(stop) = self.push(hit)? {
                            return Ok(Some(stop));
                        }
                    }
                }
                at.ordinal += 1;
            }
        }
        Ok(None)
    }

    /// Where the term is in this row, if anywhere. The original is preferred, as in
    /// monitor matches. An older translation pass skips cues whose original already hit.
    fn field(&self, pass: u32, cue: &Cue) -> Option<ArchiveField> {
        let fields = self.query.fields;
        if fields.original() && term_matches(&self.query.term, &cue.script) {
            return (pass == 0).then_some(ArchiveField::Original);
        }
        let english = fields.english()
            && cue
                .english
                .as_deref()
                .is_some_and(|text| term_matches(&self.query.term, text));
        english.then_some(ArchiveField::English)
    }

    fn accepts(&self, labels: &Labels, cue: &Cue) -> bool {
        self.query.language.as_ref().is_none_or(|filter| {
            labels
                .overlapping(cue)
                .any(|label| ArchiveQuery::language_accepts(filter, &label.tag))
        })
    }

    fn labels(&mut self, statement: &mut Statement<'_>, header: &Header) -> Result<Labels> {
        let limit =
            i64::try_from(ARCHIVE_MAX_UNIT_LABELS + 1).map_err(|_| Error::StorageIntegrity)?;
        let mut rows = statement
            .query_map(
                params![header.analysis_id, header.analysis_revision, limit],
                label,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let read = u32::try_from(rows.len()).map_err(|_| Error::StorageIntegrity)?;
        self.page.language_rows_scanned = self.page.language_rows_scanned.saturating_add(read);
        let complete = rows.len() <= ARCHIVE_MAX_UNIT_LABELS;
        rows.truncate(ARCHIVE_MAX_UNIT_LABELS);
        Ok(Labels { rows, complete })
    }

    fn hit(
        &self,
        header: &Header,
        pass: u32,
        field: ArchiveField,
        cue: Cue,
        labels: &Labels,
    ) -> Result<ArchiveHit> {
        let mut languages: Vec<ArchiveLanguage> = Vec::new();
        let mut more_languages = !labels.complete;
        for label in labels.overlapping(&cue) {
            let seen = languages.iter().any(|known| {
                known.tag == label.tag
                    && known.evidence_id == label.evidence_id
                    && known.evidence_revision == label.evidence_revision
            });
            if seen {
                continue;
            }
            if languages.len() == ARCHIVE_MAX_LANGUAGES {
                more_languages = true;
                break;
            }
            languages.push(ArchiveLanguage {
                tag: label.tag.clone(),
                evidence_id: label.evidence_id.clone(),
                evidence_revision: label.evidence_revision,
                transcript_revision: label.transcript_revision,
                capability: label.capability.clone(),
            });
        }
        let media = classify(
            self.connection,
            &header.recording_id,
            &header.media_sha256,
            cue.start_us,
            cue.end_us,
        )?;
        Ok(ArchiveHit {
            source: header.source.clone(),
            recording_id: header.recording_id.clone(),
            capture_start_ms: header.starts_ms,
            transcript_id: header.id.clone(),
            transcript_revision: header.revision,
            transcript_kind: header.kind.clone(),
            cue_ordinal: cue.ordinal,
            start_us: u64::try_from(cue.start_us).map_err(|_| Error::StorageIntegrity)?,
            end_us: u64::try_from(cue.end_us).map_err(|_| Error::StorageIntegrity)?,
            field,
            original: cue.script,
            translation_revision: if pass == 0 {
                header.newest_translation
            } else {
                Some(i64::from(pass))
            },
            english: cue.english,
            untranslated_reason: cue.reason,
            stale_transcript: header.stale,
            stale_translation: pass != 0,
            media: match media {
                CueMedia::Retained => ArchiveMedia::Retained,
                CueMedia::Released => ArchiveMedia::Released,
                CueMedia::Expired => ArchiveMedia::Expired,
                CueMedia::Missing => ArchiveMedia::Missing,
                CueMedia::Unavailable => ArchiveMedia::Unavailable,
            },
            languages,
            more_languages,
        })
    }

    /// Add a hit if it fits the serialized page bound. One hit always fits an empty page.
    fn push(&mut self, hit: ArchiveHit) -> Result<Option<ArchiveStop>> {
        let size = serde_json::to_vec(&hit)?.len() + usize::from(!self.page.hits.is_empty());
        if self.bytes + size > ARCHIVE_PAGE_BYTES {
            if self.page.hits.is_empty() {
                return Err(Error::StorageIntegrity);
            }
            return Ok(Some(ArchiveStop::PageBytes));
        }
        self.bytes += size;
        self.page.hits.push(hit);
        Ok(None)
    }

    fn finish(
        mut self,
        stopped: Option<ArchiveStop>,
        at: Option<&Position>,
    ) -> Result<ArchivePage> {
        self.page.stopped = stopped;
        self.page.next = at.map(Position::cursor);
        if serde_json::to_vec(&self.page)?.len() > ARCHIVE_PAGE_BYTES {
            return Err(Error::StorageIntegrity);
        }
        Ok(self.page)
    }
}

/// Serialized bytes of a page with no hits and the largest counters, stop and cursor.
fn reserved_bytes(page: &ArchivePage) -> Result<usize> {
    let worst = ArchivePage {
        query: page.query.clone(),
        hits: Vec::new(),
        transcripts_scanned: u32::MAX,
        rows_scanned: u32::MAX,
        language_rows_scanned: u32::MAX,
        stopped: Some(ArchiveStop::PageBytes),
        next: Some(ArchiveCursor {
            transcript_id: "n".repeat(128),
            transcript_revision: 65,
            pass: 64,
            ordinal: 256,
        }),
    };
    Ok(serde_json::to_vec(&worst)?.len())
}

#[cfg(test)]
mod tests;
