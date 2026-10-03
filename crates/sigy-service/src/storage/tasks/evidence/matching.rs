//! Streaming comparison with separate row, text and citation bounds.

use rusqlite::{Connection, Row, params, types::ValueRef};

use super::{Heard, MonitorTerm, QueryWork, micros};
use crate::{
    Error, Result,
    monitor::{MATCH_PAGE, searches_english, term_matches},
    task::TaskCitation,
};

const VALUE_BYTES: usize = 4_096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::storage) struct Bounds {
    pub rows: usize,
    pub text_bytes: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            rows: 4_096,
            text_bytes: 4 * 1_024 * 1_024,
        }
    }
}

pub(super) struct Matching<'a> {
    terms: &'a [MonitorTerm],
    work: &'a QueryWork<'a>,
    bounds: Bounds,
    rows: usize,
    text_bytes: usize,
    pub citations: Vec<TaskCitation>,
    pub more: bool,
    stop: Option<&'static str>,
}

impl<'a> Matching<'a> {
    pub fn new(terms: &'a [MonitorTerm], bounds: Bounds, work: &'a QueryWork<'a>) -> Self {
        Self {
            terms,
            work,
            bounds,
            rows: 0,
            text_bytes: 0,
            citations: Vec::new(),
            more: false,
            stop: None,
        }
    }

    pub const fn stop_reason(&self) -> Option<&'static str> {
        self.stop
    }

    fn stop(&mut self, reason: &'static str) {
        self.more = true;
        self.stop = Some(reason);
    }

    fn available(&mut self) -> bool {
        if self.work.exhausted() {
            self.stop("evidence-work-truncated");
        } else if self.rows >= self.bounds.rows {
            self.stop("evidence-rows-truncated");
        } else {
            return true;
        }
        false
    }

    /// Each compared cue is counted once, including one lookahead match at the hit cap.
    pub fn scan(
        &mut self,
        connection: &Connection,
        source: &str,
        recording: &str,
        heard: &Heard,
        translation: Option<i64>,
    ) -> Result<()> {
        if self.more || !self.available() {
            return Ok(());
        }
        let mut statement = connection.prepare(
            "SELECT c.ordinal, c.start_us, c.end_us, CASE WHEN typeof(c.script) = 'text' AND length(CAST(c.script AS BLOB)) BETWEEN 1 AND 4096 THEN c.script END, CASE WHEN e.english IS NULL OR (typeof(e.english) = 'text' AND length(CAST(e.english AS BLOB)) BETWEEN 1 AND 4096) THEN e.english END, typeof(e.english), length(CAST(e.english AS BLOB)) FROM transcript_cues c LEFT JOIN translation_cues e ON e.transcript_id = c.transcript_id AND e.transcript_revision = c.revision AND e.revision = ?3 AND e.ordinal = c.ordinal AND e.state = 'translated' WHERE c.transcript_id = ?1 AND c.revision = ?2 ORDER BY c.ordinal",
        )?;
        let mut rows =
            statement.query(params![heard.transcript_id, heard.revision, translation])?;
        loop {
            if !self.available() {
                break;
            }
            let Some(row) = rows.next()? else {
                break;
            };
            self.rows += 1;
            // Borrow SQLite's values and validate both before allocating comparison strings.
            let script = text(row, 3, false)?.ok_or(Error::StorageIntegrity)?;
            let english_type = row
                .get_ref(5)?
                .as_str()
                .map_err(|_| Error::StorageIntegrity)?;
            if english_type != "null" {
                let length: i64 = row.get(6)?;
                if english_type != "text" || !(1..=4096).contains(&length) {
                    return Err(Error::StorageIntegrity);
                }
            }
            let english = text(row, 4, true)?;
            let size = script
                .len()
                .checked_add(english.map_or(0, str::len))
                .ok_or(Error::StorageIntegrity)?;
            let total = self
                .text_bytes
                .checked_add(size)
                .ok_or(Error::StorageIntegrity)?;
            if total > self.bounds.text_bytes {
                self.stop("evidence-text-truncated");
                break;
            }
            self.text_bytes = total;
            let matched = self.terms.iter().any(|term| {
                term_matches(&term.text, script)
                    || (searches_english(&term.language)
                        && english.is_some_and(|value| term_matches(&term.text, value)))
            });
            if !matched {
                continue;
            }
            if self.citations.len() == MATCH_PAGE {
                self.stop("citations-truncated");
                break;
            }
            let ordinal: u32 = row.get(0)?;
            let start = micros(row.get(1)?)?;
            let end = micros(row.get(2)?)?;
            if ordinal > 255 || end <= start {
                return Err(Error::StorageIntegrity);
            }
            self.citations.push(TaskCitation {
                source: source.into(),
                recording_id: recording.into(),
                transcript_id: heard.transcript_id.clone(),
                transcript_revision: heard.revision,
                translation_revision: translation,
                cue_ordinal: ordinal,
                start_us: start,
                end_us: end,
            });
        }
        Ok(())
    }
}

fn text<'a>(row: &'a Row<'_>, index: usize, nullable: bool) -> Result<Option<&'a str>> {
    match row.get_ref(index)? {
        ValueRef::Null if nullable => Ok(None),
        ValueRef::Text(value) if value.len() <= VALUE_BYTES => std::str::from_utf8(value)
            .map(Some)
            .map_err(|_| Error::StorageIntegrity),
        _ => Err(Error::StorageIntegrity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_types_encoding_and_oversize_values_refuse_before_comparison() -> Result<()> {
        let connection = Connection::open_in_memory()?;
        for sql in [
            "SELECT 1",
            "SELECT x'6162'",
            "SELECT CAST(x'ff' AS TEXT)",
            "SELECT printf('%4097s','x')",
            "SELECT NULL",
        ] {
            let mut statement = connection.prepare(sql)?;
            let mut rows = statement.query([])?;
            let row = rows.next()?.ok_or(Error::StorageIntegrity)?;
            assert!(matches!(text(row, 0, false), Err(Error::StorageIntegrity)));
        }
        let mut statement = connection.prepare("SELECT NULL, printf('%4096s','x')")?;
        let mut rows = statement.query([])?;
        let row = rows.next()?.ok_or(Error::StorageIntegrity)?;
        assert_eq!(text(row, 0, true)?, None);
        assert_eq!(text(row, 1, false)?.map(str::len), Some(4096));
        Ok(())
    }
}
