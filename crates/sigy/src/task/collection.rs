//! Explicit recording authority and task-owned provenance remain visible at the interface.

use std::io::{self, Write};

use clap::Args;
use sigy_service::task::collection::{TaskCaptureSpec, TaskCollectionSpec, TaskCollectionView};

use crate::explorer::text::sanitize;

#[derive(Debug, Args)]
pub struct CollectionArgs {
    /// Already authorized source revision. Repeat for a second distinct source.
    #[arg(long = "source", required = true, action = clap::ArgAction::Append)]
    sources: Vec<String>,
    /// UTC Unix milliseconds, on a whole second, inside the task window.
    #[arg(long)]
    start_ms: i64,
    /// Planned recording seconds per source, including a late-start gap.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=900))]
    seconds: u32,
    /// Worst-case retained bytes reserved per source before connection.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..=268_435_456))]
    max_bytes: u64,
}

impl CollectionArgs {
    pub(super) fn spec(&self) -> sigy_service::Result<TaskCollectionSpec> {
        let spec = TaskCollectionSpec {
            captures: self
                .sources
                .iter()
                .map(|source| TaskCaptureSpec {
                    source_revision: source.clone(),
                    start_ms: self.start_ms,
                    duration_seconds: self.seconds,
                    maximum_bytes: self.max_bytes,
                })
                .collect(),
        };
        spec.validate()?;
        Ok(spec)
    }
}

pub(super) fn render(
    writer: &mut impl Write,
    collection: Option<&TaskCollectionView>,
) -> io::Result<()> {
    let Some(collection) = collection else {
        return writeln!(
            writer,
            "No collection granted. Task scope alone grants no recording authority."
        );
    };
    writeln!(
        writer,
        "Task {} collection | generation {} | request {}",
        sanitize(&collection.id, 257),
        collection.generation,
        sanitize(&collection.request_id, 257)
    )?;
    writeln!(
        writer,
        "State: {}. Scope: {}. Created {} Unix ms; updated {} Unix ms.",
        if collection.cancelled {
            "future admissions cancelled"
        } else {
            "granted"
        },
        if collection.scope_current {
            "current"
        } else {
            "stale; future admissions held"
        },
        collection.created_ms,
        collection.updated_ms
    )?;
    writeln!(
        writer,
        "Grant SHA-256: {}",
        sanitize(&collection.grant_sha256, 64)
    )?;
    writeln!(
        writer,
        "Scope SHA-256: {}",
        sanitize(&collection.scope_sha256, 64)
    )?;
    if let Some(reason) = &collection.hold_reason {
        writeln!(writer, "Admission hold: {}", sanitize(reason, 128))?;
    }
    for (spec, capture) in collection.spec.captures.iter().zip(&collection.captures) {
        writeln!(
            writer,
            "Source {} | start {} Unix ms | {} planned seconds | {} maximum bytes",
            sanitize(&spec.source_revision, 257),
            spec.start_ms,
            spec.duration_seconds,
            spec.maximum_bytes
        )?;
        writeln!(
            writer,
            "  Schedule {} | occurrence {} | schedule state {} | recording {} | recording state {}",
            sanitize(&capture.rule_id, 257),
            capture
                .occurrence_id
                .as_deref()
                .map_or_else(|| "none".into(), |id| sanitize(id, 257)),
            sanitize(&capture.state, 64),
            capture
                .recording_id
                .as_deref()
                .map_or_else(|| "none".into(), |id| sanitize(id, 257)),
            capture
                .recording_state
                .as_deref()
                .map_or_else(|| "none".into(), |state| sanitize(state, 64))
        )?;
    }
    writeln!(
        writer,
        "The service runs these finite schedules after the client exits. Inspect each recording for media publication, gaps and retention."
    )?;
    writeln!(
        writer,
        "Cancellation stops future admissions only; already admitted recordings and independent work continue. Capture reservations never refill."
    )?;
    writeln!(
        writer,
        "Paid allowance: USD 0. This grant starts no task-owned analysis or model planning. Schedule admission does not establish recorded evidence or semantic goal success."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigy_service::task::collection::{COLLECTION_TEMPLATE, TaskCollectionCapture};

    #[test]
    fn status_distinguishes_schedule_admission_from_media_and_sanitizes_reasons()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut collection = TaskCollectionView {
            id: "water\u{1b}[2J".into(),
            request_id: "grant".into(),
            template: COLLECTION_TEMPLATE.into(),
            paid_allowance_usd: "0.000000".into(),
            spec: TaskCollectionSpec {
                captures: vec![TaskCaptureSpec {
                    source_revision: "one:v1".into(),
                    start_ms: 1000,
                    duration_seconds: 60,
                    maximum_bytes: 1024,
                }],
            },
            grant_sha256: "a".repeat(64),
            scope_sha256: "b".repeat(64),
            created_ms: 1,
            updated_ms: 2,
            generation: 1,
            cancelled: false,
            scope_current: true,
            hold_reason: None,
            captures: vec![TaskCollectionCapture {
                rule_id: "owned".into(),
                occurrence_id: None,
                state: "waiting".into(),
                recording_id: None,
                recording_state: None,
            }],
        };
        for state in ["running", "completed", "failed", "interrupted"] {
            collection.captures[0].state = "admitted".into();
            collection.captures[0].recording_id = Some("recording".into());
            collection.captures[0].recording_state = Some(state.into());
            let mut output = Vec::new();
            render(&mut output, Some(&collection))?;
            let text = String::from_utf8(output)?;
            assert!(text.contains(&format!(
                "schedule state admitted | recording recording | recording state {state}"
            )));
            assert!(!text.contains('\u{1b}'));
            assert!(text.contains("no task-owned analysis or model planning"));
            assert!(!text.contains("Admission hold"));
        }
        collection.cancelled = true;
        collection.scope_current = false;
        collection.hold_reason = Some("policy-drift\u{1b}[2J".into());
        let mut output = Vec::new();
        render(&mut output, Some(&collection))?;
        let text = String::from_utf8(output)?;
        assert!(text.contains("future admissions cancelled"));
        assert!(text.contains("stale; future admissions held"));
        assert!(text.contains("Admission hold: policy-drift[2J"));
        assert!(!text.contains('\u{1b}'));
        let mut output = Vec::new();
        render(&mut output, None)?;
        assert!(String::from_utf8(output)?.contains("No collection granted"));
        Ok(())
    }
}
