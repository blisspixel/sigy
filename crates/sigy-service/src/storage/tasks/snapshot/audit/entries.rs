//! Every entry is bound to immutable results, even without a literal citation.

use crate::{
    Error, Result,
    task::{
        evidence::{TaskEvidenceEntry, TaskEvidenceStage},
        processing::TaskProcessingStep,
        snapshot::{TaskEvidenceSnapshot, TaskJobObservation},
    },
};
use rusqlite::{Connection, OptionalExtension, params};

fn stage(step: &TaskProcessingStep) -> TaskEvidenceStage {
    TaskEvidenceStage {
        decision: step.decision.clone(),
        reason: step.reason.clone(),
        job_id: step.job_id.clone(),
        job_state: step.job_state.clone(),
    }
}

fn job<'a>(
    snapshot: &'a TaskEvidenceSnapshot,
    ordinal: u32,
    stage: &str,
) -> Result<&'a TaskJobObservation> {
    snapshot
        .jobs
        .iter()
        .find(|j| j.ordinal == ordinal && j.stage == stage)
        .ok_or(Error::StorageIntegrity)
}

pub(super) fn validate(connection: &Connection, snapshot: &TaskEvidenceSnapshot) -> Result<()> {
    for entry in &snapshot.evidence.entries {
        let media = snapshot.media.iter().find(|m| m.ordinal == entry.ordinal);
        let completed = entry.capture_state.as_deref() == Some("completed");
        let recorded = if completed {
            media.and_then(|m| m.decoded_us).unwrap_or(0)
        } else {
            0
        };
        if entry.recorded_us != recorded
            || entry.pending != pending(snapshot, entry)
            || entry.recognized_us > recorded
            || entry.unprocessed_us != recorded - entry.recognized_us
            || entry.uncovered_us != entry.planned_us.saturating_sub(recorded)
            || entry.schedule_state
                != snapshot
                    .collection
                    .captures
                    .get(entry.ordinal as usize)
                    .ok_or(Error::StorageIntegrity)?
                    .state
        {
            return Err(Error::StorageIntegrity);
        }
        if completed {
            if media.is_none_or(|m| {
                m.decoded_us.is_none() || m.media_sha256.is_none() || m.media_bytes.is_none()
            }) {
                return Err(Error::StorageIntegrity);
            }
            let valid:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM capture_jobs WHERE id=?1 AND state='completed' AND updated_ms<=?2)",params![entry.recording_id,snapshot.observed_ms],|r|r.get(0))?;
            if !valid {
                return Err(Error::StorageIntegrity);
            }
        }
        validate_results(connection, snapshot, entry, completed)?;
    }
    Ok(())
}

fn pending(snapshot: &TaskEvidenceSnapshot, entry: &TaskEvidenceEntry) -> bool {
    if entry.schedule_state == "waiting" {
        return !snapshot.collection.cancelled && snapshot.collection.scope_current;
    }
    if entry.schedule_state == "missed" {
        return false;
    }
    match entry.capture_state.as_deref() {
        Some("failed" | "interrupted" | "cancelled") => return false,
        Some("completed") => {}
        _ => return true,
    }
    let available = snapshot.processing.as_ref().is_some_and(|p| {
        !p.cancelled
            && p.scope_current
            && p.hold_reason.as_deref() != Some("task-interest-withdrawn")
    });
    let Some(recognition) = &entry.recognition else {
        return available;
    };
    if matches!(
        recognition.job_state.as_deref(),
        Some("queued" | "running" | "cancelling")
    ) {
        return true;
    }
    if recognition.job_state.as_deref() != Some("succeeded")
        || entry.transcript_outcome.as_deref() != Some("text")
    {
        return false;
    }
    let Some(translation) = &entry.translation else {
        return available;
    };
    matches!(
        translation.job_state.as_deref(),
        Some("queued" | "running" | "cancelling")
    )
}

fn validate_results(
    connection: &Connection,
    snapshot: &TaskEvidenceSnapshot,
    entry: &TaskEvidenceEntry,
    completed: bool,
) -> Result<()> {
    let steps = snapshot
        .processing
        .as_ref()
        .map(|p| p.steps.as_slice())
        .unwrap_or_default();
    let recognition = steps
        .iter()
        .find(|s| s.ordinal == entry.ordinal && s.stage == "recognition")
        .filter(|_| completed);
    if entry.recognition != recognition.map(stage) {
        return Err(Error::StorageIntegrity);
    }
    let succeeded = entry
        .recognition
        .as_ref()
        .is_some_and(|s| s.job_state.as_deref() == Some("succeeded"));
    if !succeeded {
        if entry.transcript_revision.is_some()
            || entry.transcript_outcome.is_some()
            || entry.recognized_us != 0
            || entry.translation.is_some()
            || entry.translation_revision.is_some()
            || entry.cues != 0
            || entry.translated_cues != 0
        {
            return Err(Error::StorageIntegrity);
        }
        return require_pending(entry);
    }
    let recognized = job(snapshot, entry.ordinal, "recognition")?;
    let result:Option<(String,i64,String,i64,i64)>=connection.query_row(
        "SELECT id,revision,outcome,created_ms,(SELECT coalesce(sum(end_us-start_us),0) FROM transcript_coverage WHERE transcript_id=t.id AND revision=t.revision) FROM transcripts t WHERE job_id=?1 AND job_generation=?2 AND recording_id=?3 AND analysis_id=?4 AND analysis_revision=?5 AND profile_sha256=?6 AND kind='recognition' AND media_sha256=?7",
        params![recognized.job_id,recognized.observed_generation,entry.recording_id,recognized.input_id,recognized.input_revision,recognized.profile_sha256,snapshot.media.iter().find(|m|m.ordinal==entry.ordinal).and_then(|m|m.media_sha256.as_ref())],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).optional()?;
    let Some((transcript, revision, outcome, created, covered)) = result else {
        // A successful job without a result remains explicit missing evidence.
        if entry.transcript_revision.is_some()
            || entry.transcript_outcome.is_some()
            || entry.recognized_us != 0
            || entry.translation.is_some()
            || entry.translation_revision.is_some()
            || entry.cues != 0
            || entry.translated_cues != 0
        {
            return Err(Error::StorageIntegrity);
        }
        return Ok(());
    };
    let coverage = u64::try_from(covered).map_err(|_| Error::StorageIntegrity)?;
    if created > snapshot.observed_ms
        || entry.transcript_revision != Some(revision)
        || entry.transcript_outcome.as_ref() != Some(&outcome)
        || entry.recognized_us != coverage.min(entry.recorded_us)
    {
        return Err(Error::StorageIntegrity);
    }
    let translation = steps
        .iter()
        .find(|s| s.ordinal == entry.ordinal && s.stage == "translation")
        .filter(|_| outcome == "text");
    if entry.translation != translation.map(stage) {
        return Err(Error::StorageIntegrity);
    }
    validate_translation(connection, snapshot, entry, &transcript, revision)
}

fn validate_translation(
    connection: &Connection,
    snapshot: &TaskEvidenceSnapshot,
    entry: &TaskEvidenceEntry,
    transcript: &str,
    revision: i64,
) -> Result<()> {
    if entry
        .translation
        .as_ref()
        .is_some_and(|s| s.job_state.as_deref() == Some("succeeded"))
    {
        let translated = job(snapshot, entry.ordinal, "translation")?;
        let result:Option<(i64,u32,u32,i64)>=connection.query_row(
            "SELECT revision,cue_count,translated_count,created_ms FROM translations WHERE job_id=?1 AND job_generation=?2 AND transcript_id=?3 AND transcript_revision=?4 AND profile_sha256=?5 AND target='en'",
            params![translated.job_id,translated.observed_generation,transcript,revision,translated.profile_sha256],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional()?;
        match result {
            Some((revision, cues, done, created))
                if created <= snapshot.observed_ms
                    && entry.translation_revision == Some(revision)
                    && entry.cues == cues
                    && entry.translated_cues == done => {}
            None if entry.translation_revision.is_none()
                && entry.cues == 0
                && entry.translated_cues == 0 => {}
            _ => return Err(Error::StorageIntegrity),
        }
    } else if entry.translation_revision.is_some() || entry.cues != 0 || entry.translated_cues != 0
    {
        return Err(Error::StorageIntegrity);
    }
    require_pending(entry)
}

fn require_pending(entry: &TaskEvidenceEntry) -> Result<()> {
    if !entry.pending
        && [entry.recognition.as_ref(), entry.translation.as_ref()]
            .into_iter()
            .flatten()
            .any(|s| {
                matches!(
                    s.job_state.as_deref(),
                    Some("queued" | "running" | "cancelling")
                )
            })
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
