use super::LocalAsrJob;
use super::{Connection, Error, OptionalExtension, Result, Store, params, validate_key};
use crate::recognition::{MAX_ASR_CUES, RecognitionOutput};
use crate::recognition::{
    RecognitionCoverage, RecognitionCue, TRANSCRIPT_PAGE_BYTES, TRANSCRIPT_PAGE_ITEMS,
    TranscriptCuePage, TranscriptRevisionPage, TranscriptSummary,
};

const SUMMARY: &str = "SELECT t.id, t.revision, t.analysis_revision, t.recording_id, t.media_sha256, t.kind, t.outcome, t.parent_revision, t.profile, t.profile_sha256, t.job_id, t.job_generation, (SELECT count(*) FROM transcript_cues c WHERE c.transcript_id = t.id AND c.revision = t.revision), coalesce(t.text_bytes, 0), t.created_ms FROM transcripts t JOIN analysis_decisions d ON d.transcript_id = t.id AND d.transcript_revision = t.revision";

impl Store {
    /// The newest transcript revision for one analysis input, or zero.
    /// # Errors
    /// Refuses a malformed ID.
    pub fn latest_transcript_revision(&self, id: &str) -> Result<i64> {
        validate_key(id, "transcript ID")?;
        Ok(self.connection.query_row(
            "SELECT coalesce(max(revision), 0) FROM transcripts WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?)
    }

    /// Read at most sixteen immutable revisions after an exact revision cursor.
    /// Historical reads do not require retained audio or a current input revision.
    /// # Errors
    /// Refuses malformed cursors/IDs, invalid storage or an oversized page.
    pub fn transcript_revisions(
        &self,
        id: &str,
        after_revision: i64,
    ) -> Result<TranscriptRevisionPage> {
        validate_read(id, after_revision, true)?;
        let mut statement = self.connection.prepare(&format!(
            "{SUMMARY} WHERE t.id = ?1 AND t.revision > ?2 ORDER BY t.revision LIMIT 17"
        ))?;
        let mut revisions = statement
            .query_map(params![id, after_revision], summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = revisions.len() > TRANSCRIPT_PAGE_ITEMS;
        revisions.truncate(TRANSCRIPT_PAGE_ITEMS);
        let next_after_revision = more
            .then(|| revisions.last().map(|row| row.revision))
            .flatten();
        let page = TranscriptRevisionPage {
            revisions,
            next_after_revision,
        };
        if serde_json::to_vec(&page)?.len() > TRANSCRIPT_PAGE_BYTES {
            return Err(Error::StorageIntegrity);
        }
        Ok(page)
    }

    /// Read one exact revision, including its coverage, with bounded original-script cues.
    /// Both the sixteen-item and 64 KiB serialized JSON bounds apply, including escaping.
    /// # Errors
    /// Refuses missing revisions, invalid cursors, inconsistent storage or oversized metadata.
    pub fn transcript_cues_page(
        &self,
        id: &str,
        revision: i64,
        after_ordinal: Option<u32>,
    ) -> Result<TranscriptCuePage> {
        validate_read(id, revision, false)?;
        if after_ordinal.is_some_and(|value| value > 1_000_000) {
            return Err(Error::InvalidInput("transcript cursor"));
        }
        let transcript = self
            .connection
            .query_row(
                &format!("{SUMMARY} WHERE t.id = ?1 AND t.revision = ?2"),
                params![id, revision],
                summary,
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let mut page = TranscriptCuePage {
            transcript,
            coverage: coverage(&self.connection, id, revision)?,
            cues: Vec::new(),
            next_after_ordinal: None,
        };
        if serde_json::to_vec(&page)?.len() > TRANSCRIPT_PAGE_BYTES {
            return Err(Error::StorageIntegrity);
        }
        let mut statement = self.connection.prepare("SELECT ordinal, start_us, end_us, script FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 AND ordinal > ?3 ORDER BY ordinal LIMIT 17")?;
        let rows = statement.query_map(
            params![id, revision, after_ordinal.map_or(-1, i64::from)],
            cue,
        )?;
        let mut more = false;
        for row in rows {
            let candidate = row?;
            if page.cues.len() == TRANSCRIPT_PAGE_ITEMS {
                more = true;
                break;
            }
            let ordinal = candidate.ordinal;
            page.cues.push(candidate);
            // Reserve cursor bytes while sizing even the final candidate.
            page.next_after_ordinal = Some(ordinal);
            if serde_json::to_vec(&page)?.len() > TRANSCRIPT_PAGE_BYTES {
                page.cues.pop();
                if page.cues.is_empty() {
                    return Err(Error::StorageIntegrity);
                }
                more = true;
                break;
            }
        }
        let last = page.cues.last().map(|cue| cue.ordinal);
        // Legacy cue ordinals can be sparse. Only an observed further row grants a cursor.
        page.next_after_ordinal = last.filter(|_| more);
        Ok(page)
    }
}

fn validate_read(id: &str, revision: i64, zero: bool) -> Result<()> {
    validate_key(id, "transcript ID")?;
    if !(i64::from(!zero)..=64).contains(&revision) {
        return Err(Error::InvalidInput("transcript revision"));
    }
    Ok(())
}

fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptSummary> {
    Ok(TranscriptSummary {
        id: row.get(0)?,
        revision: row.get(1)?,
        analysis_revision: row.get(2)?,
        recording_id: row.get(3)?,
        media_sha256: row.get(4)?,
        kind: row.get(5)?,
        outcome: row.get(6)?,
        parent_revision: row.get(7)?,
        profile: row.get(8)?,
        profile_sha256: row.get(9)?,
        job_id: row.get(10)?,
        job_generation: row.get(11)?,
        cue_count: row.get(12)?,
        text_bytes: row.get::<_, u32>(13)?.into(),
        created_ms: row.get(14)?,
        amount_usd: "0.000000".into(),
        wording: "uncertain".into(),
    })
}

fn cue(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecognitionCue> {
    Ok(RecognitionCue {
        ordinal: row.get(0)?,
        start_us: unsigned(row, 1)?,
        end_us: unsigned(row, 2)?,
        script: row.get(3)?,
    })
}

fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn coverage(
    connection: &Connection,
    id: &str,
    revision: i64,
) -> Result<Option<RecognitionCoverage>> {
    Ok(connection.query_row(
        "SELECT interval_ordinal, start_us, end_us, source_sha256, decoded_sha256, sample_rate, sample_count FROM transcript_coverage WHERE transcript_id = ?1 AND revision = ?2",
        params![id, revision], |row| Ok(RecognitionCoverage { interval_ordinal: row.get(0)?, start_us: unsigned(row, 1)?, end_us: unsigned(row, 2)?, source_sha256: row.get(3)?, decoded_sha256: row.get(4)?, sample_rate: row.get(5)?, sample_count: unsigned(row, 6)? }),
    ).optional()?)
}

pub(super) fn output(connection: &Connection, job: &LocalAsrJob) -> Result<RecognitionOutput> {
    let id = &job.request.analysis_id;
    let revision = job.request.parent_revision + 1;
    let coverage = coverage(connection, id, revision)?.ok_or(Error::StorageIntegrity)?;
    let mut statement = connection.prepare("SELECT ordinal, start_us, end_us, script FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 ORDER BY ordinal LIMIT 257")?;
    let cues = statement
        .query_map(params![id, revision], cue)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if cues.len() > MAX_ASR_CUES {
        return Err(Error::StorageIntegrity);
    }
    Ok(RecognitionOutput {
        profile_sha256: job.request.profile_sha256.clone(),
        manifest_sha256: job.manifest_sha256.clone(),
        coverage,
        cues,
    })
}
