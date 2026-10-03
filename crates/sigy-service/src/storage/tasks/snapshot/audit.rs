//! Historical snapshot audits compare immutable lineage, never today's media availability.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params, types::ValueRef};

use super::{Store, observations, snapshot_hash};
use crate::{
    Error, Result,
    monitor::MATCH_PAGE,
    task::{
        MAX_CHECKPOINTS,
        processing::TaskProcessingStep,
        snapshot::{MAX_SNAPSHOT_BYTES, SNAPSHOT_MODE, SNAPSHOT_TEMPLATE, TaskEvidenceSnapshot},
    },
};

mod entries;
mod history;
mod literal;

struct Raw {
    request: String,
    expected: u32,
    observed: i64,
    scope: String,
    json: String,
    digest: String,
}

impl Store {
    pub(in crate::storage) fn checked_evidence_snapshot(
        &self,
        id: &str,
        ordinal: u32,
    ) -> Result<TaskEvidenceSnapshot> {
        let raw = self.connection.query_row(
            "SELECT request_id, expected_snapshot, observed_ms, scope_sha256, payload_json, payload_sha256 FROM task_evidence_snapshots WHERE task_id = ?1 AND ordinal = ?2",
            params![id, ordinal], |row| {
                let ValueRef::Text(bytes) = row.get_ref(4)? else {
                    return Err(rusqlite::Error::InvalidQuery);
                };
                if bytes.len() > MAX_SNAPSHOT_BYTES {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let json = std::str::from_utf8(bytes).map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(Raw {
                    request: row.get(0)?, expected: row.get(1)?, observed: row.get(2)?,
                    scope: row.get(3)?, json: json.into(), digest: row.get(5)?,
                })
            },
        ).optional()?.ok_or(Error::NotFound)?;
        let snapshot: TaskEvidenceSnapshot =
            serde_json::from_str(&raw.json).map_err(|_| Error::StorageIntegrity)?;
        let scope = self.checked_task_scope(id)?;
        if raw.digest != snapshot_hash(&raw.json)
            || snapshot.task_id != id
            || snapshot.ordinal != ordinal
            || snapshot.request_id != raw.request
            || snapshot.expected_snapshot != raw.expected
            || ordinal.checked_sub(1) != Some(raw.expected)
            || snapshot.observed_ms != raw.observed
            || snapshot.observed_ms < scope.3
            || snapshot.scope != scope.0
            || snapshot.scope_sha256 != scope.1
            || raw.scope != scope.1
            || snapshot.monitor_spec_sha256 != scope.2
            || snapshot.template != SNAPSHOT_TEMPLATE
            || snapshot.mode != SNAPSHOT_MODE
            || snapshot.collection.id != id
            || snapshot.evidence.id != id
            || snapshot.evidence.monitor_id != scope.0.monitor_id
            || snapshot.evidence.monitor_version != scope.0.monitor_version
            || snapshot.evidence.entries.len() > 2
            || snapshot.evidence.citations.len() > MATCH_PAGE
            || snapshot.jobs.len() > 4
            || snapshot.media.len() > 2
        {
            return Err(Error::StorageIntegrity);
        }
        self.validate_snapshot_lineage(&snapshot)?;
        validate_counters(&snapshot)?;
        Ok(snapshot)
    }

    fn validate_snapshot_lineage(&self, snapshot: &TaskEvidenceSnapshot) -> Result<()> {
        let current = self
            .task_collection(&snapshot.task_id)?
            .ok_or(Error::StorageIntegrity)?;
        if snapshot.collection.grant_sha256 != current.grant_sha256
            || snapshot.collection.scope_sha256 != snapshot.scope_sha256
            || snapshot.collection.spec != current.spec
            || snapshot.collection.captures.len() != current.captures.len()
            || snapshot.evidence.entries.len() != current.captures.len()
        {
            return Err(Error::StorageIntegrity);
        }
        for (index, capture) in snapshot.collection.captures.iter().enumerate() {
            if let Some(occurrence) = &capture.occurrence_id {
                let ordinal = u32::try_from(index).map_err(|_| Error::StorageIntegrity)?;
                let admitted: Option<i64> = self.connection.query_row(
                    "SELECT admitted_ms FROM task_collection_admissions WHERE task_id=?1 AND ordinal=?2 AND occurrence_id=?3",
                    params![snapshot.task_id, ordinal, occurrence], |row| row.get(0),
                ).optional()?;
                let valid = match (capture.recording_id.is_some(), admitted) {
                    (true, Some(time)) => time <= snapshot.observed_ms,
                    (false, Some(time)) => snapshot.observed_ms <= time,
                    (false, None) => true,
                    (true, None) => false,
                };
                if !valid {
                    return Err(Error::StorageIntegrity);
                }
            }
            let present = current.captures.get(index).ok_or(Error::StorageIntegrity)?;
            let entry = snapshot
                .evidence
                .entries
                .get(index)
                .ok_or(Error::StorageIntegrity)?;
            let planned = current
                .spec
                .captures
                .get(index)
                .ok_or(Error::StorageIntegrity)?;
            if capture.rule_id != present.rule_id
                || (capture.occurrence_id.is_some()
                    && capture.occurrence_id != present.occurrence_id)
                || (capture.recording_id.is_some() && capture.recording_id != present.recording_id)
                || entry.ordinal as usize != index
                || entry.source_revision != planned.source_revision
                || entry.planned_start_ms != planned.start_ms
                || entry.recording_id != capture.recording_id
                || entry.planned_us != u64::from(planned.duration_seconds) * 1_000_000
            {
                return Err(Error::StorageIntegrity);
            }
        }
        self.validate_snapshot_processing(snapshot)?;
        let mut jobs = observations::jobs(&self.connection, snapshot.processing.as_ref())?;
        for (present, frozen) in jobs.iter_mut().zip(&snapshot.jobs) {
            let step = snapshot
                .processing
                .as_ref()
                .and_then(|p| {
                    p.steps
                        .iter()
                        .find(|s| s.ordinal == frozen.ordinal && s.stage == frozen.stage)
                })
                .ok_or(Error::StorageIntegrity)?;
            if frozen.observed_generation == 0
                || frozen.observed_generation > present.observed_generation
                || ![
                    "queued",
                    "running",
                    "cancelling",
                    "succeeded",
                    "failed",
                    "cancelled",
                    "interrupted",
                ]
                .contains(&frozen.observed_state.as_str())
                || step.job_state.as_deref() != Some(frozen.observed_state.as_str())
            {
                return Err(Error::StorageIntegrity);
            }
            history::validate(&self.connection, frozen, snapshot.observed_ms)?;
            present.observed_generation = frozen.observed_generation;
            present.observed_state.clone_from(&frozen.observed_state);
        }
        if jobs != snapshot.jobs {
            return Err(Error::StorageIntegrity);
        }
        self.validate_snapshot_media(snapshot)?;
        entries::validate(&self.connection, snapshot)?;
        self.validate_snapshot_citations(snapshot)
    }

    fn validate_snapshot_media(&self, snapshot: &TaskEvidenceSnapshot) -> Result<()> {
        let media = observations::media(&self.connection, &snapshot.collection)?;
        if media.len() != snapshot.media.len() {
            return Err(Error::StorageIntegrity);
        }
        for (present, frozen) in media.iter().zip(&snapshot.media) {
            let published_bytes: Option<i64> = self.connection.query_row(
                "SELECT max(byte_end) FROM recording_intervals WHERE recording_id=?1",
                [&frozen.recording_id],
                |row| row.get(0),
            )?;
            let published_bytes = published_bytes
                .map(u64::try_from)
                .transpose()
                .map_err(|_| Error::StorageIntegrity)?;
            if present.ordinal != frozen.ordinal
                || present.recording_id != frozen.recording_id
                || (frozen.media_sha256.is_some() && frozen.media_sha256 != present.media_sha256)
                || (frozen.decoded_us.is_some() && frozen.decoded_us != present.decoded_us)
                || frozen.media_bytes.is_some_and(|bytes| {
                    bytes == 0 || published_bytes.is_none_or(|maximum| bytes > maximum)
                })
            {
                return Err(Error::StorageIntegrity);
            }
        }
        Ok(())
    }

    fn validate_snapshot_processing(&self, snapshot: &TaskEvidenceSnapshot) -> Result<()> {
        let Some(frozen) = &snapshot.processing else {
            return if snapshot.jobs.is_empty() {
                Ok(())
            } else {
                Err(Error::StorageIntegrity)
            };
        };
        let current = self
            .task_processing(&snapshot.task_id)?
            .ok_or(Error::StorageIntegrity)?;
        if frozen.id != snapshot.task_id
            || frozen.request_id != current.request_id
            || frozen.created_ms != current.created_ms
            || frozen.created_ms > snapshot.observed_ms
            || frozen.generation == 0
            || frozen.generation > current.generation
            || frozen.grant_sha256 != current.grant_sha256
            || frozen.collection_sha256 != snapshot.collection.grant_sha256
            || frozen.scope_sha256 != snapshot.scope_sha256
            || frozen.spec != current.spec
            || frozen.recognition_profile_sha256 != current.recognition_profile_sha256
            || frozen.translation_profile_sha256 != current.translation_profile_sha256
            || frozen.steps.len() > 4
        {
            return Err(Error::StorageIntegrity);
        }
        let mut seen = BTreeSet::new();
        let mut charged = 0_u64;
        for step in &frozen.steps {
            if (step.job_id.is_none() && step.job_state.is_some())
                || step.created_ms > snapshot.observed_ms
                || !seen.insert((step.ordinal, step.stage.as_str()))
            {
                return Err(Error::StorageIntegrity);
            }
            charged = charged
                .checked_add(step.audio_us)
                .ok_or(Error::StorageIntegrity)?;
            let present = current
                .steps
                .iter()
                .find(|p| p.ordinal == step.ordinal && p.stage == step.stage)
                .ok_or(Error::StorageIntegrity)?;
            if !same_receipt(step, present) {
                return Err(Error::StorageIntegrity);
            }
        }
        if charged != frozen.charged_audio_us || charged > frozen.spec.maximum_audio_us() {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }

    fn validate_snapshot_citations(&self, snapshot: &TaskEvidenceSnapshot) -> Result<()> {
        let policy = self
            .monitor_version(&snapshot.scope.monitor_id, snapshot.scope.monitor_version)?
            .spec;
        let mut seen = BTreeSet::new();
        for citation in &snapshot.evidence.citations {
            if !seen.insert(serde_json::to_string(citation)?) {
                return Err(Error::StorageIntegrity);
            }
            literal::validate(&self.connection, snapshot, &policy.terms, citation)?;
            let entry = snapshot
                .evidence
                .entries
                .iter()
                .find(|e| e.recording_id.as_ref() == Some(&citation.recording_id))
                .ok_or(Error::StorageIntegrity)?;
            let job = snapshot
                .jobs
                .iter()
                .find(|j| j.ordinal == entry.ordinal && j.stage == "recognition")
                .ok_or(Error::StorageIntegrity)?;
            let start = i64::try_from(citation.start_us).map_err(|_| Error::StorageIntegrity)?;
            let end = i64::try_from(citation.end_us).map_err(|_| Error::StorageIntegrity)?;
            let valid: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM transcripts t JOIN transcript_cues c ON c.transcript_id = t.id AND c.revision = t.revision WHERE t.id = ?1 AND t.revision = ?2 AND t.job_id = ?3 AND t.recording_id = ?4 AND c.ordinal = ?5 AND c.start_us = ?6 AND c.end_us = ?7)",
                params![citation.transcript_id, citation.transcript_revision, job.job_id, citation.recording_id, citation.cue_ordinal, start, end], |row| row.get(0),
            )?;
            if !valid
                || citation.source != entry.source_revision
                || Some(citation.transcript_revision) != entry.transcript_revision
                || citation.translation_revision != entry.translation_revision
            {
                return Err(Error::StorageIntegrity);
            }
            if let Some(revision) = citation.translation_revision {
                let translated = snapshot
                    .jobs
                    .iter()
                    .find(|j| j.ordinal == entry.ordinal && j.stage == "translation")
                    .ok_or(Error::StorageIntegrity)?;
                let valid: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM translations WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3 AND job_id = ?4 AND target = 'en')",
                    params![citation.transcript_id, citation.transcript_revision, revision, translated.job_id], |row| row.get(0),
                )?;
                if !valid {
                    return Err(Error::StorageIntegrity);
                }
            }
        }
        literal::validate_membership(&self.connection, snapshot, &policy.terms)?;
        Ok(())
    }

    pub(crate) fn audit_task_evidence_snapshots(&self) -> Result<()> {
        let excessive: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks t WHERE (SELECT count(*) FROM task_checkpoints c WHERE c.task_id = t.id) + (SELECT count(*) FROM task_evidence_snapshots s WHERE s.task_id = t.id) > ?1)",
            [MAX_CHECKPOINTS], |row| row.get(0),
        )?;
        if excessive {
            return Err(Error::StorageIntegrity);
        }
        let mut statement = self.connection.prepare(
            "SELECT task_id, ordinal FROM task_evidence_snapshots ORDER BY task_id, ordinal",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            self.task_evidence_snapshot(&row.get::<_, String>(0)?, row.get(1)?)?;
        }
        Ok(())
    }
}

fn same_receipt(left: &TaskProcessingStep, right: &TaskProcessingStep) -> bool {
    left.ordinal == right.ordinal
        && left.stage == right.stage
        && left.recording_id == right.recording_id
        && left.decision == right.decision
        && left.reason == right.reason
        && left.input_id == right.input_id
        && left.input_revision == right.input_revision
        && left.job_id == right.job_id
        && left.audio_us == right.audio_us
        && left.created_ms == right.created_ms
}

fn validate_counters(snapshot: &TaskEvidenceSnapshot) -> Result<()> {
    use crate::task::evidence::{TaskEvidenceEntry, TaskOutcome};
    type Counter = fn(&TaskEvidenceEntry) -> u64;
    let evidence = &snapshot.evidence;
    let counters: [(Counter, u64); 5] = [
        (|e: &TaskEvidenceEntry| e.planned_us, evidence.planned_us),
        (|e: &TaskEvidenceEntry| e.recorded_us, evidence.recorded_us),
        (
            |e: &TaskEvidenceEntry| e.uncovered_us,
            evidence.uncovered_us,
        ),
        (
            |e: &TaskEvidenceEntry| e.recognized_us,
            evidence.recognized_us,
        ),
        (
            |e: &TaskEvidenceEntry| e.unprocessed_us,
            evidence.unprocessed_us,
        ),
    ];
    for (select, total) in counters {
        let sum = evidence
            .entries
            .iter()
            .try_fold(0_u64, |sum, e| sum.checked_add(select(e)));
        if sum != Some(total) {
            return Err(Error::StorageIntegrity);
        }
    }
    let expected = if evidence.more {
        TaskOutcome::Partial
    } else if evidence.entries.iter().any(|e| e.pending) {
        TaskOutcome::Pending
    } else if !evidence.reasons.is_empty() {
        TaskOutcome::Partial
    } else if evidence.citations.is_empty() {
        TaskOutcome::NoLiteralMatch
    } else {
        TaskOutcome::Cited
    };
    if evidence.outcome != expected
        || evidence.template != crate::task::evidence::EVIDENCE_TEMPLATE
        || evidence.paid_allowance_usd != "0.000000"
        || (evidence.more && evidence.reasons.is_empty())
        || (matches!(
            evidence.outcome,
            TaskOutcome::Cited | TaskOutcome::NoLiteralMatch
        ) && (!evidence.reasons.is_empty()
            || evidence.more
            || evidence.entries.iter().any(|e| e.pending)))
        || (evidence.outcome == TaskOutcome::Cited && evidence.citations.is_empty())
        || (evidence.outcome == TaskOutcome::NoLiteralMatch && !evidence.citations.is_empty())
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
