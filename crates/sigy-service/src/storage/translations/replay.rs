//! Exact successful settlement replay, independent of the newest translation revision.

use super::{
    Connection, Error, OptionalExtension, Result, TranslatedCue, TranslationJob,
    TranslationOutcome, TranslationResult, TranslationWork, cues_valid, params,
};

pub(super) fn validate_success(
    connection: &Connection,
    work: &TranslationWork,
    outcome: &TranslationOutcome,
    job: &TranslationJob,
) -> Result<()> {
    let TranslationResult::Succeeded(cues) = outcome.result() else {
        return Err(Error::IdempotencyConflict);
    };
    validate_succeeded_cues(connection, work, cues, job)
}

pub(super) fn validate_succeeded_cues(
    connection: &Connection,
    work: &TranslationWork,
    cues: &[TranslatedCue],
    job: &TranslationJob,
) -> Result<()> {
    if !cues_valid(work, cues) {
        return Err(Error::IdempotencyConflict);
    }
    let request = &job.request;
    let count = i64::try_from(cues.len()).map_err(|_| Error::StorageIntegrity)?;
    let header: Option<(i64, bool)> = connection.query_row(
        "SELECT revision, transcript_id = ?2 AND transcript_revision = ?3 AND job_generation = ?4 AND profile = ?5 AND profile_sha256 = ?6 AND target = 'en' AND cue_count = ?7 AND translated_count = (SELECT count(*) FROM translation_cues c WHERE c.transcript_id = t.transcript_id AND c.transcript_revision = t.transcript_revision AND c.revision = t.revision AND c.state = 'translated') AND amount_micros = 0 AND created_ms = ?8 FROM translations t WHERE job_id = ?1",
        params![request.id, request.transcript_id, request.transcript_revision, job.generation, request.profile, request.profile_sha256, count, job.finished_ms],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    let Some((revision, true)) = header else {
        return Err(Error::StorageIntegrity);
    };
    // One lookahead row detects extra membership without an unbounded result allocation.
    let mut statement = connection.prepare(
        "SELECT ordinal, state, english, reason FROM translation_cues WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3 ORDER BY ordinal LIMIT ?4",
    )?;
    let mut rows = statement.query(params![
        request.transcript_id,
        request.transcript_revision,
        revision,
        count + 1
    ])?;
    for expected in cues {
        let row = rows.next()?.ok_or(Error::StorageIntegrity)?;
        let stored = TranslatedCue {
            ordinal: row.get(0)?,
            state: row.get(1)?,
            english: row.get(2)?,
            reason: row.get(3)?,
        };
        if &stored != expected {
            return Err(Error::IdempotencyConflict);
        }
    }
    if rows.next()?.is_some() {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
