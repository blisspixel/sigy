//! One frozen briefing generation per id. Classification stays off.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

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

    /// Read one generation. Coverage is the current read of its stored window.
    ///
    /// # Errors
    /// Returns not found when that briefing was never stored.
    pub(crate) fn briefing(&self, monitor: &str, id: &str) -> Result<BriefingPage> {
        validate_key(monitor, "monitor ID")?;
        validate_key(id, "briefing ID")?;
        let header = header(&self.connection, monitor, id)?.ok_or(Error::NotFound)?;
        let coverage = self.monitor_coverage(monitor, header.from_ms, header.to_ms)?;
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
    Ok(())
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
