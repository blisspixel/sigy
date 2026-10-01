//! Evidence pointers stay bound to immutable revisions, never to the latest text.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};

use super::Store;
use super::audit::CheckpointContext;
use crate::{
    Error, Result,
    monitor::{searches_english, term_matches},
    task::{TaskCheckpoint, TaskCitation},
};

type CitationRow = (String, String, i64, i64, i64, String);

impl Store {
    pub(super) fn validate_task_citation(
        &self,
        context: &CheckpointContext,
        cite: &TaskCitation,
    ) -> Result<()> {
        let spec = &context.spec;
        let row: CitationRow = self.connection.query_row(
            "SELECT t.recording_id, c.source_revision, c.starts_ms, cue.start_us, cue.end_us, cue.script FROM transcripts t JOIN capture_jobs c ON c.id = t.recording_id JOIN transcript_cues cue ON cue.transcript_id = t.id AND cue.revision = t.revision WHERE t.id = ?1 AND t.revision = ?2 AND cue.ordinal = ?3 AND t.outcome = 'text' AND t.kind IN ('recognition', 'correction')",
            params![cite.transcript_id, cite.transcript_revision, cite.cue_ordinal],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        ).optional()?.ok_or(Error::StorageIntegrity)?;
        let (recording, source, capture_start, start, end, script) = row;
        if recording != cite.recording_id
            || source != cite.source
            || capture_start < spec.from_ms
            || capture_start >= spec.to_ms
            || u64::try_from(start).ok() != Some(cite.start_us)
            || u64::try_from(end).ok() != Some(cite.end_us)
            || start >= end
        {
            return Err(Error::StorageIntegrity);
        }
        let english = if let Some(revision) = cite.translation_revision {
            let exists: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM translations WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3)",
                params![cite.transcript_id, cite.transcript_revision, revision], |row| row.get(0),
            )?;
            if !exists {
                return Err(Error::StorageIntegrity);
            }
            self.connection.query_row(
                "SELECT english FROM translation_cues WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3 AND ordinal = ?4 AND state = 'translated'",
                params![cite.transcript_id, cite.transcript_revision, revision, cite.cue_ordinal], |row| row.get::<_, String>(0),
            ).optional()?
        } else {
            None
        };
        if !context.policy.terms.iter().any(|term| {
            term_matches(&term.text, &script)
                || (searches_english(&term.language)
                    && english
                        .as_ref()
                        .is_some_and(|text| term_matches(&term.text, text)))
        }) {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }

    pub(super) fn validate_task_schedules(
        &self,
        context: &CheckpointContext,
        checkpoint: &TaskCheckpoint,
    ) -> Result<()> {
        let spec = &context.spec;
        let mut seen = BTreeSet::new();
        for schedule in &checkpoint.coverage.schedules {
            if !seen.insert(&schedule.schedule) {
                return Err(Error::StorageIntegrity);
            }
            let exists: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM schedule_rules WHERE id = ?1)",
                [&schedule.schedule],
                |row| row.get(0),
            )?;
            let owned: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM monitor_capture_rules WHERE rule_id = ?1 AND monitor_id = ?2)",
                params![schedule.schedule, spec.monitor_id], |row| row.get(0),
            )?;
            if !exists || (!owned && !context.policy.schedules.contains(&schedule.schedule)) {
                return Err(Error::StorageIntegrity);
            }
        }
        Ok(())
    }
}
