//! Read-only coverage counts and passage matches for one monitor over one window.
//!
//! Nothing here writes a row, starts work or contacts a network. Counts come from the
//! existing capture, pin, transcript and translation records of the sources the monitor
//! follows now.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, active_sources, latest_version};
use crate::{
    Error, Result,
    monitor::{
        MATCH_PAGE, MAX_WINDOW_CAPTURES, MonitorCoverage, MonitorMatches, PassageMatch,
        ScheduleCoverage, SourceCoverage, check_window, searches_english, term_matches,
    },
    storage::validate_key,
};

/// Most transcripts one match request reads.
const MAX_SCANNED_TRANSCRIPTS: u32 = 512;

struct Capture {
    id: String,
    starts_ms: i64,
    decoded_us: Option<u64>,
}

fn micros(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::StorageIntegrity)
}

/// Captures of one source that started in the window, oldest first, and whether more exist.
fn captures(
    connection: &Connection,
    source: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<(Vec<Capture>, bool)> {
    let mut statement = connection.prepare(
        "SELECT c.id, c.starts_ms, r.decoded_microseconds FROM capture_jobs c LEFT JOIN recordings r ON r.id = c.id WHERE c.source_revision = ?1 AND c.starts_ms >= ?2 AND c.starts_ms < ?3 ORDER BY c.starts_ms, c.id LIMIT ?4",
    )?;
    let limit = i64::try_from(MAX_WINDOW_CAPTURES).map_err(|_| Error::StorageIntegrity)? + 1;
    let raw = statement
        .query_map(params![source, from_ms, to_ms, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rows = raw
        .into_iter()
        .map(|(id, starts_ms, decoded)| {
            Ok(Capture {
                id,
                starts_ms,
                decoded_us: decoded.map(micros).transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let more = rows.len() > MAX_WINDOW_CAPTURES;
    rows.truncate(MAX_WINDOW_CAPTURES);
    Ok((rows, more))
}

/// The latest published pin of a recording and its latest recognition revision, if any.
struct Recognized {
    transcript_id: String,
    revision: i64,
    outcome: String,
    cue_count: u32,
    covered_us: u64,
}

fn latest_pin(connection: &Connection, recording: &str) -> Result<Option<(String, i64)>> {
    Ok(connection
        .query_row(
            "SELECT id, revision FROM analysis_inputs WHERE recording_id = ?1 AND state = 'published' ORDER BY created_ms DESC, id DESC, revision DESC LIMIT 1",
            [recording],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

fn recognized(connection: &Connection, pin: &(String, i64)) -> Result<Option<Recognized>> {
    let row: Option<(i64, String, u32)> = connection
        .query_row(
            "SELECT revision, outcome, cue_count FROM transcripts WHERE analysis_id = ?1 AND analysis_revision = ?2 AND kind = 'recognition' ORDER BY revision DESC LIMIT 1",
            params![pin.0, pin.1],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((revision, outcome, cue_count)) = row else {
        return Ok(None);
    };
    let covered: i64 = connection.query_row(
        "SELECT coalesce(sum(end_us - start_us), 0) FROM transcript_coverage WHERE transcript_id = ?1 AND revision = ?2",
        params![pin.0, revision],
        |row| row.get(0),
    )?;
    Ok(Some(Recognized {
        transcript_id: pin.0.clone(),
        revision,
        outcome,
        cue_count,
        covered_us: micros(covered)?,
    }))
}

fn latest_translation(connection: &Connection, transcript: &Recognized) -> Result<Option<i64>> {
    Ok(connection.query_row(
        "SELECT max(revision) FROM translations WHERE transcript_id = ?1 AND transcript_revision = ?2",
        params![transcript.transcript_id, transcript.revision],
        |row| row.get(0),
    )?)
}

fn count_translation(
    connection: &Connection,
    transcript: &Recognized,
    coverage: &mut SourceCoverage,
    reasons: &mut BTreeMap<String, u32>,
) -> Result<()> {
    let Some(revision) = latest_translation(connection, transcript)? else {
        coverage.cues_without_translation += transcript.cue_count;
        return Ok(());
    };
    let mut statement = connection.prepare(
        "SELECT state, reason, count(*) FROM translation_cues WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3 GROUP BY state, reason",
    )?;
    let rows = statement
        .query_map(
            params![transcript.transcript_id, transcript.revision, revision],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, u32>(2)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (state, reason, count) in rows {
        if state == "translated" {
            coverage.translated_cues += count;
        } else {
            coverage.untranslated_cues += count;
            *reasons
                .entry(reason.unwrap_or_else(|| "unspecified".into()))
                .or_default() += count;
        }
    }
    Ok(())
}

fn source_coverage(
    connection: &Connection,
    source: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<SourceCoverage> {
    let (rows, truncated) = captures(connection, source, from_ms, to_ms)?;
    let mut coverage = SourceCoverage {
        source: source.to_owned(),
        captures: u32::try_from(rows.len()).map_err(|_| Error::StorageIntegrity)?,
        truncated,
        ..SourceCoverage::default()
    };
    let mut reasons = BTreeMap::new();
    for capture in &rows {
        let Some(decoded_us) = capture.decoded_us else {
            continue;
        };
        coverage.published += 1;
        coverage.recorded_us += decoded_us;
        let (gaps, gap_us): (u32, i64) = connection.query_row(
            "SELECT count(*), coalesce(sum(end_us - start_us), 0) FROM recording_gaps WHERE recording_id = ?1",
            [&capture.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        coverage.gaps += gaps;
        coverage.gap_us += micros(gap_us)?;
        let Some(pin) = latest_pin(connection, &capture.id)? else {
            continue;
        };
        coverage.pinned += 1;
        let Some(transcript) = recognized(connection, &pin)? else {
            continue;
        };
        if transcript.outcome == "text" {
            coverage.transcribed += 1;
            coverage.transcribed_us += transcript.covered_us;
            count_translation(connection, &transcript, &mut coverage, &mut reasons)?;
        } else {
            coverage.no_text += 1;
            coverage.no_text_us += transcript.covered_us;
        }
    }
    coverage.untranslated_reasons = reasons.into_iter().collect();
    Ok(coverage)
}

fn schedule_coverage(
    connection: &Connection,
    schedule: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<ScheduleCoverage> {
    let mut statement = connection.prepare(
        "SELECT state, miss_reason, count(*) FROM schedule_occurrences WHERE rule_id = ?1 AND coalesce(start_ms, transition_ms) >= ?2 AND coalesce(start_ms, transition_ms) < ?3 GROUP BY state, miss_reason",
    )?;
    let rows = statement
        .query_map(params![schedule, from_ms, to_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, u32>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut coverage = ScheduleCoverage {
        schedule: schedule.to_owned(),
        ..ScheduleCoverage::default()
    };
    for (state, reason, count) in rows {
        match (state.as_str(), reason.as_deref()) {
            ("admitted", _) => coverage.admitted += count,
            ("waiting", _) => coverage.waiting += count,
            ("missed", Some("spring_forward")) => coverage.missed_spring_forward += count,
            ("missed", _) => coverage.missed_elapsed += count,
            _ => return Err(Error::StorageIntegrity),
        }
    }
    Ok(coverage)
}

fn english_cues(
    connection: &Connection,
    transcript: &Recognized,
) -> Result<(Option<i64>, BTreeMap<u32, String>)> {
    let Some(revision) = latest_translation(connection, transcript)? else {
        return Ok((None, BTreeMap::new()));
    };
    let mut statement = connection.prepare(
        "SELECT ordinal, english FROM translation_cues WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3 AND state = 'translated'",
    )?;
    let rows = statement
        .query_map(
            params![transcript.transcript_id, transcript.revision, revision],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
        )?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    Ok((Some(revision), rows))
}

struct Cue {
    ordinal: u32,
    start_us: u64,
    end_us: u64,
    script: String,
}

fn cues(connection: &Connection, transcript: &Recognized) -> Result<Vec<Cue>> {
    let mut statement = connection.prepare(
        "SELECT ordinal, start_us, end_us, script FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 ORDER BY ordinal",
    )?;
    let rows = statement
        .query_map(
            params![transcript.transcript_id, transcript.revision],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(ordinal, start, end, script)| {
            Ok(Cue {
                ordinal,
                start_us: micros(start)?,
                end_us: micros(end)?,
                script,
            })
        })
        .collect()
}

impl Store {
    /// Stage counts for every source the monitor follows now and every saved schedule of
    /// its latest version, over captures that started in `[from_ms, to_ms)`.
    /// # Errors
    /// Refuses a missing monitor or an invalid window.
    pub fn monitor_coverage(&self, id: &str, from_ms: i64, to_ms: i64) -> Result<MonitorCoverage> {
        validate_key(id, "monitor ID")?;
        check_window(from_ms, to_ms)?;
        let version = latest_version(&self.connection, id)?.ok_or(Error::NotFound)?;
        let sources = active_sources(&self.connection, &version)?
            .iter()
            .map(|source| source_coverage(&self.connection, source, from_ms, to_ms))
            .collect::<Result<Vec<_>>>()?;
        let schedules = version
            .spec
            .schedules
            .iter()
            .map(|schedule| schedule_coverage(&self.connection, schedule, from_ms, to_ms))
            .collect::<Result<Vec<_>>>()?;
        Ok(MonitorCoverage {
            id: id.to_owned(),
            version: version.version,
            from_ms,
            to_ms,
            daily_audio_seconds: version.spec.daily_audio_seconds,
            sources,
            schedules,
        })
    }

    /// Literal term matches in the latest recognition revision (and its latest English
    /// translation) of each capture the monitor's current sources started in the window.
    /// # Errors
    /// Refuses a missing monitor or an invalid window.
    pub fn monitor_matches(&self, id: &str, from_ms: i64, to_ms: i64) -> Result<MonitorMatches> {
        validate_key(id, "monitor ID")?;
        check_window(from_ms, to_ms)?;
        let version = latest_version(&self.connection, id)?.ok_or(Error::NotFound)?;
        let terms = &version.spec.terms;
        let mut result = MonitorMatches {
            id: id.to_owned(),
            version: version.version,
            from_ms,
            to_ms,
            transcripts_scanned: 0,
            matches: Vec::new(),
            more: false,
        };
        'sources: for source in active_sources(&self.connection, &version)? {
            let (rows, truncated) = captures(&self.connection, &source, from_ms, to_ms)?;
            result.more |= truncated;
            for capture in rows.iter().filter(|capture| capture.decoded_us.is_some()) {
                let Some(pin) = latest_pin(&self.connection, &capture.id)? else {
                    continue;
                };
                let Some(transcript) = recognized(&self.connection, &pin)? else {
                    continue;
                };
                if transcript.outcome != "text" {
                    continue;
                }
                if result.transcripts_scanned == MAX_SCANNED_TRANSCRIPTS {
                    result.more = true;
                    break 'sources;
                }
                result.transcripts_scanned += 1;
                let (translation_revision, english) = english_cues(&self.connection, &transcript)?;
                for cue in cues(&self.connection, &transcript)? {
                    let translated = english.get(&cue.ordinal);
                    for term in terms {
                        let field = if term_matches(&term.text, &cue.script) {
                            "original"
                        } else if searches_english(&term.language)
                            && translated.is_some_and(|text| term_matches(&term.text, text))
                        {
                            "english"
                        } else {
                            continue;
                        };
                        if result.matches.len() == MATCH_PAGE {
                            result.more = true;
                            break 'sources;
                        }
                        result.matches.push(PassageMatch {
                            source: source.clone(),
                            recording_id: capture.id.clone(),
                            capture_start_ms: capture.starts_ms,
                            transcript_id: transcript.transcript_id.clone(),
                            transcript_revision: transcript.revision,
                            cue_ordinal: cue.ordinal,
                            start_us: cue.start_us,
                            end_us: cue.end_us,
                            term_language: term.language.clone(),
                            term: term.text.clone(),
                            field: field.into(),
                            original: cue.script.clone(),
                            translation_revision,
                            english: translated.cloned(),
                        });
                    }
                }
            }
        }
        Ok(result)
    }
}
