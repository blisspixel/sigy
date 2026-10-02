//! Explicit task processing authority, its receipts and shared job states.

use std::io::{self, Write};

use clap::Args;
use sigy_service::task::processing::{
    JobSharing, MAX_PROCESSING_AUDIO_SECONDS, TaskProcessingSpec, TaskProcessingStep,
    TaskProcessingView,
};

use crate::explorer::text::sanitize;

#[derive(Debug, Args)]
pub struct ProcessingArgs {
    /// Existing local recognition profile. Its stored hash is bound to the grant.
    #[arg(long)]
    recognition_profile: String,
    /// Existing local translation profile. Omit to recognize without translating.
    #[arg(long)]
    translation_profile: Option<String>,
    /// Lifetime recognition audio across this task's collected recordings. Never refills.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_PROCESSING_AUDIO_SECONDS)))]
    max_audio_seconds: u32,
}

impl ProcessingArgs {
    pub(super) fn spec(&self) -> sigy_service::Result<TaskProcessingSpec> {
        let spec = TaskProcessingSpec {
            recognition_profile: self.recognition_profile.clone(),
            translation_profile: self.translation_profile.clone(),
            maximum_audio_seconds: self.max_audio_seconds,
        };
        spec.validate()?;
        Ok(spec)
    }
}

fn optional(value: Option<&str>, limit: usize) -> String {
    value.map_or_else(|| "none".into(), |text| sanitize(text, limit))
}

fn sharing(value: Option<&JobSharing>) -> String {
    value.map_or_else(
        || "none".into(),
        |shared| {
            format!(
                "direct {}, monitors {}, tasks {}",
                if shared.direct { "yes" } else { "no" },
                shared.monitors,
                shared.tasks
            )
        },
    )
}

fn render_step(writer: &mut impl Write, step: &TaskProcessingStep) -> io::Result<()> {
    writeln!(
        writer,
        "Entry {} {} | recording {} | {}{} | charged {} us | {} Unix ms",
        step.ordinal,
        sanitize(&step.stage, 32),
        sanitize(&step.recording_id, 257),
        sanitize(&step.decision, 32),
        step.reason
            .as_deref()
            .map_or_else(String::new, |reason| format!(" ({})", sanitize(reason, 64))),
        step.audio_us,
        step.created_ms
    )?;
    if step.job_id.is_some() {
        writeln!(
            writer,
            "  Job {} | state now {} | input {} revision {} | interests: {}",
            optional(step.job_id.as_deref(), 257),
            optional(step.job_state.as_deref(), 64),
            optional(step.input_id.as_deref(), 257),
            step.input_revision
                .map_or_else(|| "none".into(), |revision| revision.to_string()),
            sharing(step.sharing.as_ref())
        )?;
    }
    Ok(())
}

pub(super) fn render(
    writer: &mut impl Write,
    processing: Option<&TaskProcessingView>,
) -> io::Result<()> {
    let Some(processing) = processing else {
        return writeln!(
            writer,
            "No processing granted. Collection alone grants no task-owned recognition or translation."
        );
    };
    writeln!(
        writer,
        "Task {} processing | generation {} | request {}",
        sanitize(&processing.id, 257),
        processing.generation,
        sanitize(&processing.request_id, 257)
    )?;
    writeln!(
        writer,
        "State: {}. Scope: {}. Created {} Unix ms; updated {} Unix ms.",
        if processing.cancelled {
            "future admissions cancelled"
        } else {
            "granted"
        },
        if processing.scope_current {
            "current"
        } else {
            "stale; future admissions held"
        },
        processing.created_ms,
        processing.updated_ms
    )?;
    if let Some(reason) = &processing.hold_reason {
        writeln!(writer, "Admission hold: {}", sanitize(reason, 128))?;
    }
    writeln!(
        writer,
        "Recognition profile {} (SHA-256 {})",
        sanitize(&processing.spec.recognition_profile, 257),
        sanitize(&processing.recognition_profile_sha256, 64)
    )?;
    match (
        &processing.spec.translation_profile,
        &processing.translation_profile_sha256,
    ) {
        (Some(profile), Some(sha256)) => writeln!(
            writer,
            "Translation profile {} (SHA-256 {})",
            sanitize(profile, 257),
            sanitize(sha256, 64)
        )?,
        _ => writeln!(
            writer,
            "Translation profile: none. Recognized text stays untranslated."
        )?,
    }
    writeln!(
        writer,
        "Audio charged to this task: {} of {} us lifetime allowance. It never refills.",
        processing.charged_audio_us,
        processing.spec.maximum_audio_us()
    )?;
    writeln!(
        writer,
        "Grant SHA-256: {}",
        sanitize(&processing.grant_sha256, 64)
    )?;
    if processing.steps.is_empty() {
        writeln!(writer, "No processing receipts yet.")?;
    }
    for step in &processing.steps {
        render_step(writer, step)?;
    }
    writeln!(
        writer,
        "The running service admits work after the client exits. Shared jobs are not duplicated; each authority charges its own allowance once."
    )?;
    writeln!(
        writer,
        "Cancellation stops future task admissions only; admitted jobs and direct or monitor interests continue."
    )?;
    writeln!(
        writer,
        "Paid allowance: USD 0. A finished job is not language quality or semantic goal success."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigy_service::task::processing::PROCESSING_TEMPLATE;

    fn view() -> TaskProcessingView {
        TaskProcessingView {
            id: "water\u{1b}[2J".into(),
            request_id: "grant".into(),
            template: PROCESSING_TEMPLATE.into(),
            paid_allowance_usd: "0.000000".into(),
            spec: TaskProcessingSpec {
                recognition_profile: "asr".into(),
                translation_profile: Some("mt".into()),
                maximum_audio_seconds: 60,
            },
            recognition_profile_sha256: "a".repeat(64),
            translation_profile_sha256: Some("b".repeat(64)),
            grant_sha256: "c".repeat(64),
            collection_sha256: "d".repeat(64),
            scope_sha256: "e".repeat(64),
            created_ms: 1,
            updated_ms: 2,
            generation: 1,
            cancelled: false,
            scope_current: true,
            hold_reason: None,
            charged_audio_us: 1_000_000,
            steps: vec![
                TaskProcessingStep {
                    ordinal: 0,
                    stage: "recognition".into(),
                    recording_id: "one".into(),
                    decision: "queued".into(),
                    reason: None,
                    input_id: Some("pin".into()),
                    input_revision: Some(1),
                    job_id: Some("job\u{1b}[2J".into()),
                    audio_us: 1_000_000,
                    created_ms: 2,
                    job_state: Some("succeeded".into()),
                    sharing: Some(JobSharing {
                        direct: false,
                        monitors: 1,
                        tasks: 1,
                    }),
                },
                TaskProcessingStep {
                    ordinal: 1,
                    stage: "recognition".into(),
                    recording_id: "two".into(),
                    decision: "skipped".into(),
                    reason: Some("recording-failed".into()),
                    input_id: None,
                    input_revision: None,
                    job_id: None,
                    audio_us: 0,
                    created_ms: 2,
                    job_state: None,
                    sharing: None,
                },
            ],
        }
    }

    #[test]
    fn status_separates_receipts_shared_jobs_and_quality() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut output = Vec::new();
        render(&mut output, Some(&view()))?;
        let text = String::from_utf8(output)?;
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("Audio charged to this task: 1000000 of 60000000 us"));
        assert!(text.contains("interests: direct no, monitors 1, tasks 1"));
        assert!(text.contains("Entry 1 recognition | recording two | skipped (recording-failed)"));
        assert!(text.contains("not language quality"));
        assert!(!text.contains("Admission hold"));
        let mut held = view();
        held.cancelled = true;
        held.scope_current = false;
        held.generation = 2;
        held.hold_reason = Some("cancelled".into());
        held.spec.translation_profile = None;
        held.translation_profile_sha256 = None;
        held.steps.clear();
        let mut output = Vec::new();
        render(&mut output, Some(&held))?;
        let text = String::from_utf8(output)?;
        assert!(text.contains("future admissions cancelled"));
        assert!(text.contains("stale; future admissions held"));
        assert!(text.contains("Admission hold: cancelled"));
        assert!(text.contains("Translation profile: none."));
        assert!(text.contains("No processing receipts yet."));
        let mut output = Vec::new();
        render(&mut output, None)?;
        assert!(String::from_utf8(output)?.contains("No processing granted"));
        Ok(())
    }
}
