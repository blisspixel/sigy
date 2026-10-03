//! Historical task evidence is presented as observed, separately from live inspection.

use std::io::{self, Write};

use sigy_service::task::{evidence::TaskOutcome, snapshot::TaskEvidenceSnapshot};

use crate::explorer::text::sanitize;

pub(super) fn render(writer: &mut impl Write, snapshot: &TaskEvidenceSnapshot) -> io::Result<()> {
    writeln!(
        writer,
        "Task {} exact snapshot {}",
        sanitize(&snapshot.task_id, 257),
        snapshot.ordinal
    )?;
    writeln!(
        writer,
        "Observed {} ms | freeze now | scope {}",
        snapshot.observed_ms,
        sanitize(&snapshot.scope_sha256, 64)
    )?;
    let outcome = match snapshot.evidence.outcome {
        TaskOutcome::Pending => "partial observation; work was pending when frozen",
        TaskOutcome::Cited => "cited; observed complete capture and processing",
        TaskOutcome::NoLiteralMatch => "observed complete literal scan with no match",
        TaskOutcome::Partial => "partial observation; inspect the reasons below",
    };
    writeln!(writer, "Outcome: {outcome}")?;
    writeln!(
        writer,
        "Monitor {} version {} | {} owned entries | {} literal citations{}",
        sanitize(&snapshot.scope.monitor_id, 257),
        snapshot.scope.monitor_version,
        snapshot.evidence.entries.len(),
        snapshot.evidence.citations.len(),
        if snapshot.evidence.more {
            " | scan or output truncated"
        } else {
            ""
        }
    )?;
    for reason in &snapshot.evidence.reasons {
        writeln!(writer, "Reason: {}", sanitize(reason, 128))?;
    }
    for job in &snapshot.jobs {
        writeln!(
            writer,
            "Entry {} {} job {} | observed {} at generation {}{}",
            job.ordinal,
            sanitize(&job.stage, 32),
            sanitize(&job.job_id, 257),
            sanitize(&job.observed_state, 32),
            job.observed_generation,
            job.target
                .as_ref()
                .map_or_else(String::new, |target| format!(
                    " | target {}",
                    sanitize(target, 32)
                ))
        )?;
    }
    for citation in &snapshot.evidence.citations {
        writeln!(
            writer,
            "Citation {} revision {} cue {} | translation {} | recording {} | {}..{} us",
            sanitize(&citation.transcript_id, 257),
            citation.transcript_revision,
            citation.cue_ordinal,
            citation
                .translation_revision
                .map_or_else(|| "none".into(), |revision| revision.to_string()),
            sanitize(&citation.recording_id, 257),
            citation.start_us,
            citation.end_us
        )?;
    }
    writeln!(
        writer,
        "Later completion and correction do not expand this snapshot."
    )?;
    writeln!(
        writer,
        "Live inspection: task evidence {}",
        sanitize(&snapshot.task_id, 257)
    )?;
    writeln!(
        writer,
        "This observation grants no work authority; language and semantic quality remain unmeasured."
    )
}
