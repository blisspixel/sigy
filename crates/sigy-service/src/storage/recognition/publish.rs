use rusqlite::TransactionBehavior;

use super::{
    Connection, Error, LocalAsrJob, LocalAsrWork, Result, Store, current_input, find_job, manifest,
    params, reads, sql_integer, validation,
};
use crate::recognition::{LocalAsrOutcome, ReapedLocalAsr, RecognitionOutput};

impl Store {
    /// Atomically finalize a drained worker's validated result and zero USD decision.
    /// No production path can construct the required cleanup capability yet.
    /// Cancellation wins over a queued result. Storage errors retain the active lease.
    /// # Errors
    /// Refuses stale capabilities, conflicting successful replays or a failed transaction.
    pub(crate) fn finish_local_asr(
        &mut self,
        work: &LocalAsrWork,
        completion: &ReapedLocalAsr,
        now: i64,
    ) -> Result<LocalAsrJob> {
        if !completion.matches(work) {
            return Err(Error::Analysis("stale-worker"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = find_job(&tx, &work.job.request.id)?.ok_or(Error::NotFound)?;
        if job.request != work.job.request
            || job.generation != work.job.generation
            || job.manifest_sha256 != work.job.manifest_sha256
            || job.expected_bytes != work.input.byte_length
            || job.recording_id != work.input.recording_id
            || manifest(&work.input)? != job.manifest_sha256
        {
            return Err(Error::Analysis("stale-worker"));
        }
        if !matches!(job.state.as_str(), "running" | "cancelling") {
            if job.state == "succeeded" {
                let stored = reads::output(&tx, &job)?;
                if completion.outcome() != &LocalAsrOutcome::Succeeded(stored) {
                    return Err(Error::IdempotencyConflict);
                }
            }
            return Ok(job);
        }
        let (state, reason) = terminal_outcome(&tx, work, &job, completion.outcome())?;
        let finished = now.max(job.created_ms);
        if state == "succeeded" {
            let LocalAsrOutcome::Succeeded(output) = completion.outcome() else {
                return Err(Error::StorageIntegrity);
            };
            insert_result(&tx, work, output, finished)?;
        }
        let changed = tx.execute(
            "UPDATE analysis_jobs SET state = ?3, reason = ?4, finished_ms = ?5 WHERE id = ?1 AND generation = ?2 AND state IN ('running', 'cancelling')",
            params![job.request.id, job.generation, state, reason, finished],
        )?;
        if changed != 1 {
            return Err(Error::StorageIntegrity);
        }
        tx.commit()?;
        self.local_asr_job(&job.request.id)
    }
}

fn terminal_outcome(
    connection: &Connection,
    work: &LocalAsrWork,
    job: &LocalAsrJob,
    outcome: &LocalAsrOutcome,
) -> Result<(&'static str, Option<&'static str>)> {
    if job.state == "cancelling" || matches!(outcome, LocalAsrOutcome::Cancelled) {
        return Ok(("cancelled", Some("cancelled")));
    }
    if let LocalAsrOutcome::Failed(failure) = outcome {
        return Ok(("failed", Some(failure.reason())));
    }
    let LocalAsrOutcome::Succeeded(output) = outcome else {
        return Err(Error::StorageIntegrity);
    };
    if !validation::output_valid(work, output) {
        return Ok(("failed", Some("invalid-worker-result")));
    }
    if !current_input(connection, work)? {
        return Ok(("failed", Some("input-no-longer-current")));
    }
    let parent: i64 = connection.query_row(
        "SELECT coalesce(max(revision), 0) FROM transcripts WHERE id = ?1",
        [&job.request.analysis_id],
        |row| row.get(0),
    )?;
    if parent != job.request.parent_revision {
        return Ok(("failed", Some("transcript-parent-conflict")));
    }
    Ok(("succeeded", None))
}

fn insert_result(
    connection: &Connection,
    work: &LocalAsrWork,
    output: &RecognitionOutput,
    now: i64,
) -> Result<()> {
    let request = &work.job.request;
    let revision = request.parent_revision + 1;
    let parent = (request.parent_revision != 0).then_some(request.parent_revision);
    let outcome = if output.cues.is_empty() {
        "no_text"
    } else {
        "text"
    };
    let bytes = output
        .cues
        .iter()
        .map(|cue| cue.script.len())
        .sum::<usize>();
    let count = i64::try_from(output.cues.len()).map_err(|_| Error::StorageIntegrity)?;
    let bytes = i64::try_from(bytes).map_err(|_| Error::StorageIntegrity)?;
    connection.execute(
        "INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, kind, outcome, parent_revision, job_id, job_generation, profile_sha256, cue_count, text_bytes, state, created_ms) VALUES (?1, ?2, ?1, ?3, ?4, ?5, 'original', ?6, 'recognition', ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'published', ?14)",
        params![request.analysis_id, revision, request.analysis_revision, work.input.recording_id, work.input.media_sha256, request.profile, outcome, parent, request.id, work.job.generation, request.profile_sha256, count, bytes, now],
    )?;
    for cue in &output.cues {
        connection.execute(
            "INSERT INTO transcript_cues(transcript_id, revision, ordinal, start_us, end_us, script, wording) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'uncertain')",
            params![request.analysis_id, revision, cue.ordinal, sql_integer(cue.start_us)?, sql_integer(cue.end_us)?, cue.script],
        )?;
    }
    let coverage = &output.coverage;
    connection.execute(
        "INSERT INTO transcript_coverage(transcript_id, revision, interval_ordinal, start_us, end_us, source_sha256, decoded_sha256, sample_rate, sample_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![request.analysis_id, revision, coverage.interval_ordinal, sql_integer(coverage.start_us)?, sql_integer(coverage.end_us)?, coverage.source_sha256, coverage.decoded_sha256, coverage.sample_rate, sql_integer(coverage.sample_count)?],
    )?;
    connection.execute(
        "INSERT INTO analysis_decisions(transcript_id, transcript_revision, amount_micros, request_id, created_ms) VALUES (?1, ?2, 0, NULL, ?3)",
        params![request.analysis_id, revision, now],
    )?;
    Ok(())
}
