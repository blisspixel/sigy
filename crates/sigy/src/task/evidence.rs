//! Collection-to-evidence status keeps missing coverage, unprocessed audio and literal
//! citations separate, and never presents a citation as semantic support.

use std::io::{self, Write};

use sigy_service::task::evidence::{
    TaskEvidenceEntry, TaskEvidenceStage, TaskEvidenceView, TaskOutcome,
};

use crate::explorer::text::sanitize;

fn optional(value: Option<&str>, limit: usize) -> String {
    value.map_or_else(|| "none".into(), |text| sanitize(text, limit))
}

fn outcome(value: TaskOutcome) -> &'static str {
    match value {
        TaskOutcome::Pending => "pending; collection or processing can still advance",
        TaskOutcome::Cited => "cited; every planned entry was recorded, recognized and translated",
        TaskOutcome::NoLiteralMatch => "complete coverage with no literal match",
        TaskOutcome::Partial => "partial; see the reasons below",
    }
}

fn stage(label: &str, value: Option<&TaskEvidenceStage>) -> String {
    value.map_or_else(
        || format!("{label}: no receipt"),
        |stage| {
            format!(
                "{label}: {}{} | job {} | state now {}",
                sanitize(&stage.decision, 32),
                stage
                    .reason
                    .as_deref()
                    .map_or_else(String::new, |reason| format!(" ({})", sanitize(reason, 64))),
                optional(stage.job_id.as_deref(), 257),
                optional(stage.job_state.as_deref(), 64)
            )
        },
    )
}

fn reasons(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values
            .iter()
            .map(|reason| sanitize(reason, 64))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn render_entry(writer: &mut impl Write, entry: &TaskEvidenceEntry) -> io::Result<()> {
    writeln!(
        writer,
        "Entry {} | source {} | planned start {} Unix ms, {} us | schedule {} | recording {} ({}, {})",
        entry.ordinal,
        sanitize(&entry.source_revision, 257),
        entry.planned_start_ms,
        entry.planned_us,
        sanitize(&entry.schedule_state, 32),
        optional(entry.recording_id.as_deref(), 257),
        optional(entry.capture_state.as_deref(), 32),
        optional(entry.storage_state.as_deref(), 32)
    )?;
    writeln!(
        writer,
        "  Recorded {} us, uncovered {} us, recognized {} us, unprocessed {} us.",
        entry.recorded_us, entry.uncovered_us, entry.recognized_us, entry.unprocessed_us
    )?;
    writeln!(
        writer,
        "  {} | transcript revision {} ({})",
        stage("Recognition", entry.recognition.as_ref()),
        entry
            .transcript_revision
            .map_or_else(|| "none".into(), |revision| revision.to_string()),
        optional(entry.transcript_outcome.as_deref(), 32)
    )?;
    writeln!(
        writer,
        "  {} | revision {}, {} of {} cues translated",
        stage("Translation", entry.translation.as_ref()),
        entry
            .translation_revision
            .map_or_else(|| "none".into(), |revision| revision.to_string()),
        entry.translated_cues,
        entry.cues
    )?;
    writeln!(
        writer,
        "  {}Reasons: {}.",
        if entry.pending { "Pending. " } else { "" },
        reasons(&entry.reasons)
    )
}

pub(super) fn render(
    writer: &mut impl Write,
    evidence: Option<&TaskEvidenceView>,
) -> io::Result<()> {
    let Some(evidence) = evidence else {
        return writeln!(
            writer,
            "No collection granted. There is no task-owned evidence to reconcile."
        );
    };
    writeln!(
        writer,
        "Task {} evidence | monitor {} version {} | outcome: {}",
        sanitize(&evidence.id, 257),
        sanitize(&evidence.monitor_id, 257),
        evidence.monitor_version,
        outcome(evidence.outcome)
    )?;
    writeln!(
        writer,
        "Planned {} us; recorded {} us; uncovered {} us; recognized {} us; unprocessed {} us.",
        evidence.planned_us,
        evidence.recorded_us,
        evidence.uncovered_us,
        evidence.recognized_us,
        evidence.unprocessed_us
    )?;
    writeln!(writer, "Reasons: {}.", reasons(&evidence.reasons))?;
    for entry in &evidence.entries {
        render_entry(writer, entry)?;
    }
    for citation in &evidence.citations {
        writeln!(
            writer,
            "Citation: source {} | recording {} | transcript {} revision {} cue {} | translation {} | media {} to {} us",
            sanitize(&citation.source, 257),
            sanitize(&citation.recording_id, 257),
            sanitize(&citation.transcript_id, 257),
            citation.transcript_revision,
            citation.cue_ordinal,
            citation
                .translation_revision
                .map_or_else(|| "none".into(), |revision| revision.to_string()),
            citation.start_us,
            citation.end_us
        )?;
    }
    if evidence.more {
        writeln!(writer, "Citations truncated at the page bound.")?;
    }
    writeln!(
        writer,
        "Citations are literal matches of version {} terms in the exact revisions this task's jobs published. They are not semantic support; wording, language and translation quality are unmeasured.",
        evidence.monitor_version
    )?;
    writeln!(
        writer,
        "Publish findings with task checkpoint and task execute; a checkpoint observes the monitor's broader window and newest revisions. Paid allowance: USD 0."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigy_service::task::{TaskCitation, evidence::EVIDENCE_TEMPLATE};

    fn view() -> TaskEvidenceView {
        let stage = TaskEvidenceStage {
            decision: "queued".into(),
            reason: None,
            job_id: Some("job\u{1b}[2J".into()),
            job_state: Some("succeeded".into()),
        };
        TaskEvidenceView {
            id: "water".into(),
            template: EVIDENCE_TEMPLATE.into(),
            monitor_id: "monitor".into(),
            monitor_version: 2,
            outcome: TaskOutcome::Cited,
            reasons: Vec::new(),
            planned_us: 30_000_000,
            recorded_us: 30_000_000,
            uncovered_us: 0,
            recognized_us: 30_000_000,
            unprocessed_us: 0,
            entries: vec![TaskEvidenceEntry {
                ordinal: 0,
                source_revision: "a:v1".into(),
                planned_start_ms: 1000,
                planned_us: 30_000_000,
                schedule_state: "admitted".into(),
                recording_id: Some("recording".into()),
                capture_state: Some("completed".into()),
                storage_state: Some("retained".into()),
                recorded_us: 30_000_000,
                uncovered_us: 0,
                recognized_us: 30_000_000,
                unprocessed_us: 0,
                recognition: Some(stage.clone()),
                transcript_revision: Some(1),
                transcript_outcome: Some("text".into()),
                translation: Some(stage),
                translation_revision: Some(1),
                cues: 1,
                translated_cues: 1,
                reasons: Vec::new(),
                pending: false,
            }],
            citations: vec![TaskCitation {
                source: "a:v1".into(),
                recording_id: "recording".into(),
                transcript_id: "pin\u{1b}[2J".into(),
                transcript_revision: 1,
                translation_revision: Some(1),
                cue_ordinal: 0,
                start_us: 0,
                end_us: 30_000_000,
            }],
            more: false,
            paid_allowance_usd: "0.000000".into(),
        }
    }

    #[test]
    fn evidence_separates_coverage_processing_and_literal_citations()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut output = Vec::new();
        render(&mut output, Some(&view()))?;
        let text = String::from_utf8(output)?;
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("outcome: cited"));
        assert!(text.contains("Recorded 30000000 us, uncovered 0 us, recognized 30000000 us"));
        assert!(text.contains("Recognition: queued | job job[2J | state now succeeded"));
        assert!(text.contains("revision 1, 1 of 1 cues translated"));
        assert!(text.contains("not semantic support"));
        let mut partial = view();
        partial.outcome = TaskOutcome::Partial;
        partial.reasons = vec!["capture-missed".into(), "uncovered-time".into()];
        partial.entries[0].recognition = None;
        partial.entries[0].translation = Some(TaskEvidenceStage {
            decision: "skipped".into(),
            reason: Some("no-translation-profile".into()),
            job_id: None,
            job_state: None,
        });
        partial.entries[0].pending = true;
        partial.more = true;
        let mut output = Vec::new();
        render(&mut output, Some(&partial))?;
        let text = String::from_utf8(output)?;
        assert!(text.contains("outcome: partial"));
        assert!(text.contains("Reasons: capture-missed, uncovered-time."));
        assert!(text.contains("Recognition: no receipt"));
        assert!(text.contains("Translation: skipped (no-translation-profile) | job none"));
        assert!(text.contains("Pending. Reasons: none."));
        assert!(text.contains("Citations truncated"));
        for (value, label) in [
            (TaskOutcome::Pending, "outcome: pending"),
            (TaskOutcome::NoLiteralMatch, "no literal match"),
        ] {
            partial.outcome = value;
            let mut output = Vec::new();
            render(&mut output, Some(&partial))?;
            assert!(String::from_utf8(output)?.contains(label));
        }
        let mut output = Vec::new();
        render(&mut output, None)?;
        assert!(String::from_utf8(output)?.contains("No collection granted"));
        Ok(())
    }
}
