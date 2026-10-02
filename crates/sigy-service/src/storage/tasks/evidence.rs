//! Read-only reconciliation from a task's exact collection and processing receipts.
//! It stores nothing, admits nothing and never selects an unrelated same-source recording.

use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, validate_key};
use crate::{
    Error, Result,
    monitor::{MATCH_PAGE, MonitorTerm, searches_english, term_matches},
    task::{
        TaskCitation,
        collection::{TaskCaptureSpec, TaskCollectionCapture, TaskCollectionView},
        evidence::{
            EVIDENCE_TEMPLATE, TaskEvidenceEntry, TaskEvidenceStage, TaskEvidenceView, TaskOutcome,
        },
        processing::{TaskProcessingStep, TaskProcessingView},
    },
};

const ACTIVE_JOBS: [&str; 3] = ["queued", "running", "cancelling"];
const TERMINAL_CAPTURES: [&str; 3] = ["failed", "interrupted", "cancelled"];

fn micros(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::StorageIntegrity)
}

fn stage(step: &TaskProcessingStep) -> TaskEvidenceStage {
    TaskEvidenceStage {
        decision: step.decision.clone(),
        reason: step.reason.clone(),
        job_id: step.job_id.clone(),
        job_state: step.job_state.clone(),
    }
}

/// The exact transcript revision one recognition job published.
struct Heard {
    transcript_id: String,
    revision: i64,
    outcome: String,
    covered_us: u64,
}

fn heard(connection: &Connection, job: &str) -> Result<Option<Heard>> {
    let row: Option<(String, i64, String)> = connection
        .query_row(
            "SELECT id, revision, outcome FROM transcripts WHERE job_id = ?1 AND kind = 'recognition'",
            [job],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((transcript_id, revision, outcome)) = row else {
        return Ok(None);
    };
    let covered: i64 = connection.query_row(
        "SELECT coalesce(sum(end_us - start_us), 0) FROM transcript_coverage WHERE transcript_id = ?1 AND revision = ?2",
        params![transcript_id, revision],
        |row| row.get(0),
    )?;
    Ok(Some(Heard {
        transcript_id,
        revision,
        outcome,
        covered_us: micros(covered)?,
    }))
}

/// Revision, cue count and translated count published by one translation job.
fn translated(connection: &Connection, job: &str) -> Result<Option<(i64, u32, u32)>> {
    Ok(connection
        .query_row(
            "SELECT revision, cue_count, translated_count FROM translations WHERE job_id = ?1",
            [job],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

struct Matching<'a> {
    terms: &'a [MonitorTerm],
    citations: Vec<TaskCitation>,
    more: bool,
}

impl Matching<'_> {
    /// Cite each cue of the exact revision once when any frozen literal term matches.
    fn scan(
        &mut self,
        connection: &Connection,
        source: &str,
        recording: &str,
        heard: &Heard,
        translation: Option<i64>,
    ) -> Result<()> {
        let mut statement = connection.prepare(
            "SELECT c.ordinal, c.start_us, c.end_us, c.script, e.english FROM transcript_cues c LEFT JOIN translation_cues e ON e.transcript_id = c.transcript_id AND e.transcript_revision = c.revision AND e.revision = ?3 AND e.ordinal = c.ordinal AND e.state = 'translated' WHERE c.transcript_id = ?1 AND c.revision = ?2 ORDER BY c.ordinal",
        )?;
        let cues = statement
            .query_map(
                params![heard.transcript_id, heard.revision, translation],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (ordinal, start, end, script, english) in cues {
            let matched = self.terms.iter().any(|term| {
                term_matches(&term.text, &script)
                    || (searches_english(&term.language)
                        && english
                            .as_ref()
                            .is_some_and(|text| term_matches(&term.text, text)))
            });
            if !matched {
                continue;
            }
            if self.citations.len() == MATCH_PAGE {
                self.more = true;
                return Ok(());
            }
            self.citations.push(TaskCitation {
                source: source.to_owned(),
                recording_id: recording.to_owned(),
                transcript_id: heard.transcript_id.clone(),
                transcript_revision: heard.revision,
                translation_revision: translation,
                cue_ordinal: ordinal,
                start_us: micros(start)?,
                end_us: micros(end)?,
            });
        }
        Ok(())
    }
}

/// Current authority for future task effects; holds are permanent except a clock hold.
struct Authority<'a> {
    collection: &'a TaskCollectionView,
    processing: Option<&'a TaskProcessingView>,
}

impl Authority<'_> {
    fn collection_hold(&self) -> Option<&'static str> {
        if self.collection.cancelled {
            Some("collection-cancelled")
        } else if !self.collection.scope_current {
            Some("scope-changed")
        } else {
            None
        }
    }

    fn processing_hold(&self) -> Option<&'static str> {
        match self.processing {
            None => Some("processing-not-granted"),
            Some(view) if view.cancelled => Some("processing-cancelled"),
            Some(view) if !view.scope_current => Some("scope-changed"),
            Some(_) => None,
        }
    }
}

struct Draft {
    reasons: Vec<String>,
    pending: bool,
}

impl Draft {
    fn reason(&mut self, reason: impl Into<String>) {
        self.reasons.push(reason.into());
    }

    /// Work that can still advance is pending; a permanent hold becomes a reason.
    fn wait(&mut self, hold: Option<&'static str>) {
        match hold {
            Some(reason) => self.reason(reason),
            None => self.pending = true,
        }
    }
}

impl Store {
    /// Reconcile this task's collection through processing to literal evidence.
    /// # Errors
    /// Refuses a missing task or invalid stored task, collection or processing records.
    pub fn task_evidence(&self, id: &str) -> Result<Option<TaskEvidenceView>> {
        validate_key(id, "task ID")?;
        let task = self.task(id)?;
        let Some(collection) = self.task_collection(id)? else {
            return Ok(None);
        };
        let processing = self.task_processing(id)?;
        let terms = self
            .monitor_version(&task.spec.monitor_id, task.spec.monitor_version)?
            .spec
            .terms;
        let authority = Authority {
            collection: &collection,
            processing: processing.as_ref(),
        };
        let mut matching = Matching {
            terms: &terms,
            citations: Vec::new(),
            more: false,
        };
        let mut entries = Vec::with_capacity(collection.captures.len());
        for (ordinal, (planned, capture)) in collection
            .spec
            .captures
            .iter()
            .zip(&collection.captures)
            .enumerate()
        {
            let ordinal = u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?;
            entries.push(self.evidence_entry(
                &authority,
                ordinal,
                planned,
                capture,
                &mut matching,
            )?);
        }
        let mut reasons: Vec<String> = Vec::new();
        for reason in entries.iter().flat_map(|entry| &entry.reasons) {
            if !reasons.contains(reason) {
                reasons.push(reason.clone());
            }
        }
        if matching.more {
            reasons.push("citations-truncated".into());
        }
        let total = |value: fn(&TaskEvidenceEntry) -> u64| -> Result<u64> {
            entries
                .iter()
                .try_fold(0_u64, |sum, entry| sum.checked_add(value(entry)))
                .ok_or(Error::StorageIntegrity)
        };
        let outcome = if entries.iter().any(|entry| entry.pending) {
            TaskOutcome::Pending
        } else if !reasons.is_empty() {
            TaskOutcome::Partial
        } else if matching.citations.is_empty() {
            TaskOutcome::NoLiteralMatch
        } else {
            TaskOutcome::Cited
        };
        Ok(Some(TaskEvidenceView {
            id: id.to_owned(),
            template: EVIDENCE_TEMPLATE.into(),
            monitor_id: task.spec.monitor_id,
            monitor_version: task.spec.monitor_version,
            outcome,
            reasons,
            planned_us: total(|entry| entry.planned_us)?,
            recorded_us: total(|entry| entry.recorded_us)?,
            uncovered_us: total(|entry| entry.uncovered_us)?,
            recognized_us: total(|entry| entry.recognized_us)?,
            unprocessed_us: total(|entry| entry.unprocessed_us)?,
            entries,
            citations: matching.citations,
            more: matching.more,
            paid_allowance_usd: "0.000000".into(),
        }))
    }

    fn evidence_entry(
        &self,
        authority: &Authority<'_>,
        ordinal: u32,
        planned: &TaskCaptureSpec,
        capture: &TaskCollectionCapture,
        matching: &mut Matching<'_>,
    ) -> Result<TaskEvidenceEntry> {
        let mut draft = Draft {
            reasons: Vec::new(),
            pending: false,
        };
        let (capture_state, storage_state, decoded) = match &capture.recording_id {
            Some(recording) => self.connection.query_row(
                "SELECT c.state, r.storage_state, r.decoded_microseconds FROM capture_jobs c LEFT JOIN recordings r ON r.id = c.id WHERE c.id = ?1",
                [recording],
                |row| Ok((Some(row.get::<_, String>(0)?), row.get::<_, Option<String>>(1)?, row.get::<_, Option<i64>>(2)?)),
            )?,
            None => (None, None, None),
        };
        match (capture.state.as_str(), capture_state.as_deref()) {
            ("waiting", _) => draft.wait(authority.collection_hold()),
            ("missed", _) => draft.reason("capture-missed"),
            (_, Some("completed")) => {}
            (_, Some(state)) if TERMINAL_CAPTURES.contains(&state) => {
                draft.reason(format!("capture-{state}"));
            }
            _ => draft.pending = true,
        }
        let completed = capture_state.as_deref() == Some("completed");
        let planned_us = u64::from(planned.duration_seconds) * 1_000_000;
        let recorded_us = match decoded {
            Some(value) if completed => micros(value)?,
            _ => 0,
        };
        let mut entry = TaskEvidenceEntry {
            ordinal,
            source_revision: planned.source_revision.clone(),
            planned_start_ms: planned.start_ms,
            planned_us,
            schedule_state: capture.state.clone(),
            recording_id: capture.recording_id.clone(),
            capture_state,
            storage_state,
            recorded_us,
            uncovered_us: planned_us.saturating_sub(recorded_us),
            recognized_us: 0,
            unprocessed_us: recorded_us,
            recognition: None,
            transcript_revision: None,
            transcript_outcome: None,
            translation: None,
            translation_revision: None,
            cues: 0,
            translated_cues: 0,
            reasons: Vec::new(),
            pending: false,
        };
        if completed {
            self.processed(authority, &mut entry, &mut draft, matching)?;
        }
        if !draft.pending && entry.uncovered_us > 0 {
            draft.reason("uncovered-time");
        }
        if !draft.pending && entry.unprocessed_us > 0 {
            draft.reason("unprocessed-time");
        }
        entry.reasons = draft.reasons;
        entry.pending = draft.pending;
        Ok(entry)
    }

    fn processed(
        &self,
        authority: &Authority<'_>,
        entry: &mut TaskEvidenceEntry,
        draft: &mut Draft,
        matching: &mut Matching<'_>,
    ) -> Result<()> {
        let steps = authority
            .processing
            .map(|view| view.steps.as_slice())
            .unwrap_or_default();
        let find = |stage: &str| {
            steps
                .iter()
                .find(|step| step.ordinal == entry.ordinal && step.stage == stage)
        };
        let Some(recognition) = find("recognition") else {
            draft.wait(authority.processing_hold());
            return Ok(());
        };
        entry.recognition = Some(stage(recognition));
        let (Some(job), Some(state)) = (&recognition.job_id, &recognition.job_state) else {
            draft.reason("recognition-skipped");
            return Ok(());
        };
        if ACTIVE_JOBS.contains(&state.as_str()) {
            draft.pending = true;
            return Ok(());
        }
        if state != "succeeded" {
            draft.reason(format!("recognition-{state}"));
            return Ok(());
        }
        let Some(heard) = heard(&self.connection, job)? else {
            draft.reason("no-transcript");
            return Ok(());
        };
        entry.transcript_revision = Some(heard.revision);
        entry.transcript_outcome = Some(heard.outcome.clone());
        entry.recognized_us = heard.covered_us.min(entry.recorded_us);
        entry.unprocessed_us = entry.recorded_us - entry.recognized_us;
        if heard.outcome != "text" {
            draft.reason("no-recognized-text");
            return Ok(());
        }
        let translation = match find("translation") {
            None => {
                draft.wait(authority.processing_hold());
                None
            }
            Some(step) => {
                entry.translation = Some(stage(step));
                self.translated_entry(step, entry, draft)?
            }
        };
        let recording = entry
            .recording_id
            .as_deref()
            .ok_or(Error::StorageIntegrity)?;
        matching.scan(
            &self.connection,
            &entry.source_revision,
            recording,
            &heard,
            translation,
        )
    }

    /// The translation revision this task's job published, or why there is none yet.
    fn translated_entry(
        &self,
        step: &TaskProcessingStep,
        entry: &mut TaskEvidenceEntry,
        draft: &mut Draft,
    ) -> Result<Option<i64>> {
        let (Some(job), Some(state)) = (&step.job_id, &step.job_state) else {
            draft.reason("translation-skipped");
            return Ok(None);
        };
        if ACTIVE_JOBS.contains(&state.as_str()) {
            draft.pending = true;
            return Ok(None);
        }
        if state != "succeeded" {
            draft.reason(format!("translation-{state}"));
            return Ok(None);
        }
        let Some((revision, cues, done)) = translated(&self.connection, job)? else {
            draft.reason("no-translation");
            return Ok(None);
        };
        entry.translation_revision = Some(revision);
        entry.cues = cues;
        entry.translated_cues = done;
        if done < cues {
            draft.reason("untranslated-cues");
        }
        Ok(Some(revision))
    }
}
