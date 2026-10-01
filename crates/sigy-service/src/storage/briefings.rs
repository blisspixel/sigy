//! One frozen briefing generation per id. Coverage is copied at publish. Classification stays off.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

mod task;
pub(super) use task::write_task_briefing;

use super::{Store, validate_key};
use crate::{
    Error, Result,
    monitor::{BriefingMember, BriefingPage, MonitorCoverage, check_window},
};

const MEMBER_SQL: &str = "SELECT m.finding_id, m.group_ordinal, c.script, tc.english, tc.reason, EXISTS (SELECT 1 FROM transcripts AS n WHERE n.id = f.transcript_id AND n.revision > f.transcript_revision AND n.kind IN ('recognition', 'correction') AND n.outcome = 'text'), EXISTS (SELECT 1 FROM translations AS tr WHERE tr.transcript_id = f.transcript_id AND tr.transcript_revision = f.transcript_revision AND tr.revision > f.translation_revision) FROM monitor_briefing_members AS m JOIN monitor_findings AS f ON f.monitor_id = m.monitor_id AND f.id = m.finding_id JOIN transcript_cues AS c ON c.transcript_id = f.transcript_id AND c.revision = f.transcript_revision AND c.ordinal = f.cue_ordinal JOIN translation_cues AS tc ON tc.transcript_id = f.transcript_id AND tc.transcript_revision = f.transcript_revision AND tc.revision = f.translation_revision AND tc.ordinal = f.cue_ordinal WHERE m.monitor_id = ?1 AND m.briefing_id = ?2 ORDER BY m.group_ordinal, m.finding_id";

struct Request<'a> {
    monitor: &'a str,
    id: &'a str,
    from_ms: i64,
    to_ms: i64,
    now: i64,
}

struct Header {
    generation: i64,
    from_ms: i64,
    to_ms: i64,
    monitor_version: i64,
    classification: String,
    corroboration: i64,
}

struct Cited {
    id: String,
    key: String,
}

impl Store {
    /// Store one generation for this id, or return it when the same window is repeated.
    ///
    /// # Errors
    /// A missing monitor is not found. The same id with another window conflicts and writes
    /// nothing. The service clock must be at least zero.
    pub(crate) fn publish_briefing(
        &mut self,
        monitor: &str,
        id: &str,
        from_ms: i64,
        to_ms: i64,
        now: i64,
    ) -> Result<BriefingPage> {
        let request = Request {
            monitor,
            id,
            from_ms,
            to_ms,
            now,
        };
        validate_briefing(&request)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_briefing(&tx, &request)?;
        tx.commit()?;
        self.briefing(monitor, id)
    }

    /// Read one generation. Coverage is the copy stored with that generation.
    ///
    /// # Errors
    /// Returns not found when that briefing was never stored.
    pub(crate) fn briefing(&self, monitor: &str, id: &str) -> Result<BriefingPage> {
        validate_key(monitor, "monitor ID")?;
        validate_key(id, "briefing ID")?;
        let header = header(&self.connection, monitor, id)?.ok_or(Error::NotFound)?;
        let coverage = load_snapshot(&self.connection, monitor, id, header.from_ms, header.to_ms)?;
        assemble(&self.connection, monitor, id, &header, coverage)
    }

    pub(crate) fn audit_briefings(&self) -> Result<()> {
        let invalid: bool =
            self.connection
                .query_row(include_str!("briefings_audit.sql"), [], |row| row.get(0))?;
        if invalid {
            return Err(Error::CatalogIntegrity);
        }
        Ok(())
    }
}

fn validate_briefing(request: &Request<'_>) -> Result<()> {
    validate_key(request.monitor, "monitor ID")?;
    validate_key(request.id, "briefing ID")?;
    if request.now < 0 {
        return Err(Error::InvalidInput("briefing clock"));
    }
    check_window(request.from_ms, request.to_ms)
}

fn write_briefing(connection: &Connection, request: &Request<'_>) -> Result<()> {
    if let Some(header) = header(connection, request.monitor, request.id)? {
        return if header.from_ms == request.from_ms && header.to_ms == request.to_ms {
            Ok(())
        } else {
            Err(Error::Analysis("briefing-conflict"))
        };
    }
    if !monitor_exists(connection, request.monitor)? {
        return Err(Error::NotFound);
    }
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM monitor_briefings WHERE monitor_id = ?1",
        [request.monitor],
        |row| row.get(0),
    )?;
    if count >= 1024 {
        return Err(Error::Analysis("briefing-limit"));
    }
    let version = latest_version(connection, request.monitor)?;
    let cited = citations(connection, request.monitor)?;
    let groups = group_ordinals(&cited);
    let generation = count + 1;
    let distinct = groups.iter().max().map_or(0, |ordinal| ordinal + 1);
    let corroboration = i64::try_from(distinct).map_err(|_| Error::StorageIntegrity)?;
    insert_header(connection, request, generation, version, corroboration)?;
    insert_members(connection, request, &cited, &groups)?;
    let coverage = super::monitors::coverage::coverage_at(
        connection,
        request.monitor,
        request.from_ms,
        request.to_ms,
    )?;
    store_snapshot(connection, request.monitor, request.id, &coverage)?;
    Ok(())
}

pub(super) fn migrate_039(connection: &Connection) -> Result<()> {
    connection.execute_batch(include_str!("039-briefing-coverage.sql"))?;
    backfill_coverage(connection)
}

fn monitor_exists(connection: &Connection, monitor: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM monitors WHERE id = ?1)",
        [monitor],
        |row| row.get(0),
    )?)
}

fn latest_version(connection: &Connection, monitor: &str) -> Result<i64> {
    connection
        .query_row(
            "SELECT version FROM monitor_versions WHERE monitor_id = ?1 ORDER BY version DESC LIMIT 1",
            [monitor],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)
}

fn citations(connection: &Connection, monitor: &str) -> Result<Vec<Cited>> {
    let mut statement = connection.prepare(
        "SELECT f.id, c.script FROM monitor_findings AS f JOIN transcript_cues AS c ON c.transcript_id = f.transcript_id AND c.revision = f.transcript_revision AND c.ordinal = f.cue_ordinal WHERE f.monitor_id = ?1 ORDER BY f.id",
    )?;
    let rows = statement.query_map([monitor], |row| {
        Ok(Cited {
            id: row.get(0)?,
            key: repetition_key(&row.get::<_, String>(1)?),
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn group_ordinals(cited: &[Cited]) -> Vec<usize> {
    let mut order: Vec<&str> = Vec::new();
    cited
        .iter()
        .map(|item| {
            if let Some(found) = order.iter().position(|key| *key == item.key.as_str()) {
                found
            } else {
                order.push(item.key.as_str());
                order.len() - 1
            }
        })
        .collect()
}

fn repetition_key(script: &str) -> String {
    let mut folded = String::new();
    let mut spaced = false;
    for character in script.chars() {
        if character.is_whitespace() {
            spaced = !folded.is_empty();
            continue;
        }
        if spaced {
            folded.push(' ');
            spaced = false;
        }
        folded.extend(character.to_lowercase());
    }
    folded
}

fn insert_header(
    connection: &Connection,
    request: &Request<'_>,
    generation: i64,
    version: i64,
    corroboration: i64,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO monitor_briefings(monitor_id, id, generation, from_ms, to_ms, monitor_version, classification, corroboration, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'off', ?7, ?8)",
            params![
                request.monitor,
                request.id,
                generation,
                request.from_ms,
                request.to_ms,
                version,
                corroboration,
                request.now
            ],
        )
        .map_err(briefing_error)?;
    Ok(())
}

fn insert_members(
    connection: &Connection,
    request: &Request<'_>,
    cited: &[Cited],
    groups: &[usize],
) -> Result<()> {
    for (item, ordinal) in cited.iter().zip(groups) {
        let ordinal = i64::try_from(*ordinal).map_err(|_| Error::StorageIntegrity)?;
        connection
            .execute(
                "INSERT INTO monitor_briefing_members(monitor_id, briefing_id, finding_id, group_ordinal) VALUES (?1, ?2, ?3, ?4)",
                params![request.monitor, request.id, item.id, ordinal],
            )
            .map_err(briefing_error)?;
    }
    Ok(())
}

fn briefing_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    if message.contains("briefing limit") {
        Error::Analysis("briefing-limit")
    } else if message.contains("briefing generation") {
        Error::StorageIntegrity
    } else {
        Error::Database(error)
    }
}

fn header(connection: &Connection, monitor: &str, id: &str) -> Result<Option<Header>> {
    connection
        .query_row(
            "SELECT generation, from_ms, to_ms, monitor_version, classification, corroboration FROM monitor_briefings WHERE monitor_id = ?1 AND id = ?2",
            params![monitor, id],
            |row| {
                Ok(Header {
                    generation: row.get(0)?,
                    from_ms: row.get(1)?,
                    to_ms: row.get(2)?,
                    monitor_version: row.get(3)?,
                    classification: row.get(4)?,
                    corroboration: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Error::Database)
}

fn assemble(
    connection: &Connection,
    monitor: &str,
    id: &str,
    header: &Header,
    coverage: MonitorCoverage,
) -> Result<BriefingPage> {
    Ok(BriefingPage {
        monitor_id: monitor.to_owned(),
        id: id.to_owned(),
        generation: number(header.generation)?,
        from_ms: header.from_ms,
        to_ms: header.to_ms,
        monitor_version: number(header.monitor_version)?,
        classification: header.classification.clone(),
        corroboration: number(header.corroboration)?,
        coverage,
        members: members(connection, monitor, id)?,
    })
}

fn members(connection: &Connection, monitor: &str, id: &str) -> Result<Vec<BriefingMember>> {
    let mut statement = connection.prepare(MEMBER_SQL)?;
    let rows = statement.query_map(params![monitor, id], member_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn member_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BriefingMember> {
    let stale_transcript: bool = row.get(5)?;
    let stale_translation: bool = row.get(6)?;
    Ok(BriefingMember {
        finding_id: row.get(0)?,
        group_ordinal: row.get(1)?,
        original_script: row.get(2)?,
        english: row.get(3)?,
        untranslated_reason: row.get(4)?,
        stale_transcript: stale_transcript.then_some(true),
        stale_translation: stale_translation.then_some(true),
    })
}

fn number(value: i64) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::StorageIntegrity)
}

fn wide(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::StorageIntegrity)
}

fn ordinal(value: usize, limit: usize) -> Result<i64> {
    if value >= limit {
        return Err(Error::StorageIntegrity);
    }
    i64::try_from(value).map_err(|_| Error::StorageIntegrity)
}

fn bit(value: i64) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::StorageIntegrity),
    }
}

fn backfill_coverage(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT monitor_id, id, from_ms, to_ms FROM monitor_briefings AS b WHERE NOT EXISTS (SELECT 1 FROM monitor_briefing_coverage AS c WHERE c.monitor_id = b.monitor_id AND c.briefing_id = b.id)",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let pending = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    for (monitor, id, from_ms, to_ms) in pending {
        let coverage =
            super::monitors::coverage::coverage_at(connection, &monitor, from_ms, to_ms)?;
        store_snapshot(connection, &monitor, &id, &coverage)?;
    }
    Ok(())
}

fn store_snapshot(
    connection: &Connection,
    monitor: &str,
    id: &str,
    coverage: &MonitorCoverage,
) -> Result<()> {
    connection.execute(
        "INSERT INTO monitor_briefing_coverage(monitor_id, briefing_id, monitor_version, daily_audio_seconds) VALUES (?1, ?2, ?3, ?4)",
        params![
            monitor,
            id,
            i64::from(coverage.version),
            i64::from(coverage.daily_audio_seconds)
        ],
    )?;
    for (index, source) in coverage.sources.iter().enumerate() {
        insert_source(connection, monitor, id, index, source)?;
    }
    for (index, schedule) in coverage.schedules.iter().enumerate() {
        insert_schedule(connection, monitor, id, index, schedule)?;
    }
    Ok(())
}

fn insert_source(
    connection: &Connection,
    monitor: &str,
    id: &str,
    index: usize,
    source: &crate::monitor::SourceCoverage,
) -> Result<()> {
    let ordinal = ordinal(index, 32)?;
    connection.execute(
        "INSERT INTO monitor_briefing_sources(monitor_id, briefing_id, ordinal, source, captures, published, recorded_us, gaps, gap_us, pinned, transcribed, transcribed_us, no_text, no_text_us, translated_cues, untranslated_cues, cues_without_translation, truncated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            monitor,
            id,
            ordinal,
            source.source,
            i64::from(source.captures),
            i64::from(source.published),
            i64::try_from(source.recorded_us).map_err(|_| Error::StorageIntegrity)?,
            i64::from(source.gaps),
            i64::try_from(source.gap_us).map_err(|_| Error::StorageIntegrity)?,
            i64::from(source.pinned),
            i64::from(source.transcribed),
            i64::try_from(source.transcribed_us).map_err(|_| Error::StorageIntegrity)?,
            i64::from(source.no_text),
            i64::try_from(source.no_text_us).map_err(|_| Error::StorageIntegrity)?,
            i64::from(source.translated_cues),
            i64::from(source.untranslated_cues),
            i64::from(source.cues_without_translation),
            i64::from(source.truncated)
        ],
    )?;
    for (index, (reason, count)) in source.untranslated_reasons.iter().enumerate() {
        insert_reason(connection, monitor, id, ordinal, index, reason, *count)?;
    }
    Ok(())
}

fn insert_reason(
    connection: &Connection,
    monitor: &str,
    id: &str,
    source_ordinal: i64,
    index: usize,
    reason: &str,
    count: u32,
) -> Result<()> {
    if reason.is_empty() || reason.len() > 1024 || count == 0 {
        return Err(Error::StorageIntegrity);
    }
    connection.execute(
        "INSERT INTO monitor_briefing_reasons(monitor_id, briefing_id, source_ordinal, ordinal, reason, count) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            monitor,
            id,
            source_ordinal,
            ordinal(index, 256)?,
            reason,
            i64::from(count)
        ],
    )?;
    Ok(())
}

fn insert_schedule(
    connection: &Connection,
    monitor: &str,
    id: &str,
    index: usize,
    schedule: &crate::monitor::ScheduleCoverage,
) -> Result<()> {
    connection.execute(
        "INSERT INTO monitor_briefing_schedules(monitor_id, briefing_id, ordinal, schedule, admitted, missed_elapsed, missed_spring_forward, waiting) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            monitor,
            id,
            ordinal(index, 32)?,
            schedule.schedule,
            i64::from(schedule.admitted),
            i64::from(schedule.missed_elapsed),
            i64::from(schedule.missed_spring_forward),
            i64::from(schedule.waiting)
        ],
    )?;
    Ok(())
}

fn load_snapshot(
    connection: &Connection,
    monitor: &str,
    id: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<MonitorCoverage> {
    let row = connection
        .query_row(
            "SELECT monitor_version, daily_audio_seconds FROM monitor_briefing_coverage WHERE monitor_id = ?1 AND briefing_id = ?2",
            params![monitor, id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((version, daily)) = row else {
        return Err(Error::StorageIntegrity);
    };
    Ok(MonitorCoverage {
        id: monitor.to_owned(),
        version: number(version)?,
        from_ms,
        to_ms,
        daily_audio_seconds: number(daily)?,
        sources: load_sources(connection, monitor, id)?,
        schedules: load_schedules(connection, monitor, id)?,
    })
}

struct StoredSource {
    ordinal: i64,
    source: String,
    captures: i64,
    published: i64,
    recorded_us: i64,
    gaps: i64,
    gap_us: i64,
    pinned: i64,
    transcribed: i64,
    transcribed_us: i64,
    no_text: i64,
    no_text_us: i64,
    translated: i64,
    untranslated: i64,
    missing: i64,
    truncated: i64,
}

fn load_sources(
    connection: &Connection,
    monitor: &str,
    id: &str,
) -> Result<Vec<crate::monitor::SourceCoverage>> {
    let mut statement = connection.prepare(
        "SELECT ordinal, source, captures, published, recorded_us, gaps, gap_us, pinned, transcribed, transcribed_us, no_text, no_text_us, translated_cues, untranslated_cues, cues_without_translation, truncated FROM monitor_briefing_sources WHERE monitor_id = ?1 AND briefing_id = ?2 ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![monitor, id], stored_source)?;
    let rows = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    rows.into_iter()
        .map(|row| source_from(connection, monitor, id, row))
        .collect()
}

fn stored_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredSource> {
    Ok(StoredSource {
        ordinal: row.get(0)?,
        source: row.get(1)?,
        captures: row.get(2)?,
        published: row.get(3)?,
        recorded_us: row.get(4)?,
        gaps: row.get(5)?,
        gap_us: row.get(6)?,
        pinned: row.get(7)?,
        transcribed: row.get(8)?,
        transcribed_us: row.get(9)?,
        no_text: row.get(10)?,
        no_text_us: row.get(11)?,
        translated: row.get(12)?,
        untranslated: row.get(13)?,
        missing: row.get(14)?,
        truncated: row.get(15)?,
    })
}

fn source_from(
    connection: &Connection,
    monitor: &str,
    id: &str,
    row: StoredSource,
) -> Result<crate::monitor::SourceCoverage> {
    Ok(crate::monitor::SourceCoverage {
        source: row.source,
        captures: number(row.captures)?,
        published: number(row.published)?,
        recorded_us: wide(row.recorded_us)?,
        gaps: number(row.gaps)?,
        gap_us: wide(row.gap_us)?,
        pinned: number(row.pinned)?,
        transcribed: number(row.transcribed)?,
        transcribed_us: wide(row.transcribed_us)?,
        no_text: number(row.no_text)?,
        no_text_us: wide(row.no_text_us)?,
        translated_cues: number(row.translated)?,
        untranslated_cues: number(row.untranslated)?,
        cues_without_translation: number(row.missing)?,
        truncated: bit(row.truncated)?,
        untranslated_reasons: load_reasons(connection, monitor, id, row.ordinal)?,
    })
}

fn load_reasons(
    connection: &Connection,
    monitor: &str,
    id: &str,
    source_ordinal: i64,
) -> Result<Vec<(String, u32)>> {
    let mut statement = connection.prepare(
        "SELECT reason, count FROM monitor_briefing_reasons WHERE monitor_id = ?1 AND briefing_id = ?2 AND source_ordinal = ?3 ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![monitor, id, source_ordinal], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.map(|row| -> Result<_> {
        let (reason, count) = row.map_err(Error::Database)?;
        Ok((reason, number(count)?))
    })
    .collect()
}

fn load_schedules(
    connection: &Connection,
    monitor: &str,
    id: &str,
) -> Result<Vec<crate::monitor::ScheduleCoverage>> {
    let mut statement = connection.prepare(
        "SELECT schedule, admitted, missed_elapsed, missed_spring_forward, waiting FROM monitor_briefing_schedules WHERE monitor_id = ?1 AND briefing_id = ?2 ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![monitor, id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    rows.map(|row| -> Result<_> {
        let (schedule, admitted, missed_elapsed, missed_spring_forward, waiting) =
            row.map_err(Error::Database)?;
        Ok(crate::monitor::ScheduleCoverage {
            schedule,
            admitted: number(admitted)?,
            missed_elapsed: number(missed_elapsed)?,
            missed_spring_forward: number(missed_spring_forward)?,
            waiting: number(waiting)?,
        })
    })
    .collect()
}

#[cfg(test)]
mod repetition_tests {
    use super::repetition_key;

    #[test]
    fn repetition_folds_case_and_space() {
        assert_eq!(
            repetition_key("Una  Feria\tMundial"),
            repetition_key("una feria mundial")
        );
        assert_ne!(
            repetition_key("una feria local"),
            repetition_key("una feria mundial")
        );
    }

    #[test]
    fn equal_keys_share_one_group() {
        let cited = [
            super::Cited {
                id: "copy".into(),
                key: "una feria mundial".into(),
            },
            super::Cited {
                id: "local".into(),
                key: "una feria local".into(),
            },
            super::Cited {
                id: "world".into(),
                key: "una feria mundial".into(),
            },
        ];
        assert_eq!(super::group_ordinals(&cited), [0, 1, 0]);
    }
}
