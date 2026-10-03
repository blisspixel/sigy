//! Execution receipts explain operational progress separately from meaning or quality.

use std::io::{self, Write};

use sigy_service::task::run::{TaskRunState, TaskRunView};

use crate::explorer::text::sanitize;

pub(super) fn render(writer: &mut impl Write, run: Option<&TaskRunView>) -> io::Result<()> {
    let Some(run) = run else {
        return writeln!(
            writer,
            "No execution admitted. Scope and checkpoints alone grant no publication authority."
        );
    };
    writeln!(
        writer,
        "Task {} execution | generation {} | request {}",
        sanitize(&run.task_id, 257),
        run.generation,
        sanitize(&run.request_id, 257)
    )?;
    writeln!(
        writer,
        "Template: {}. Frozen {}: {}. Finding ceiling: {}.",
        run.spec.template(),
        run.spec.origin(),
        run.spec.ordinal(),
        run.spec.maximum_findings()
    )?;
    writeln!(
        writer,
        "State: {}. Created {} Unix ms; updated {} Unix ms.",
        state_label(run.state),
        run.created_ms,
        run.updated_ms
    )?;
    writeln!(
        writer,
        "Findings: {} planned, {} published, {} skipped. Briefing: {}.",
        run.planned_findings,
        run.published_findings,
        run.skipped_findings,
        run.briefing_id
            .as_deref()
            .map_or_else(|| "none".into(), |id| sanitize(id, 257))
    )?;
    match run.state {
        TaskRunState::Running => writeln!(
            writer,
            "Admitted progress is durable. The running service advances one publication per tick; offline admission waits for service start."
        )?,
        TaskRunState::Completed => writeln!(
            writer,
            "The finite publication workflow ended. Semantic goal success and language quality remain unmeasured."
        )?,
        TaskRunState::Partial => writeln!(
            writer,
            "The finite workflow ended with incomplete evidence. Inspect skipped receipts and frozen coverage."
        )?,
        TaskRunState::Cancelled => writeln!(
            writer,
            "Future task-owned publications are cancelled. Existing findings, independent schedules and shared analysis remain."
        )?,
        TaskRunState::Revoked => writeln!(
            writer,
            "Monitor policy drift revoked further task publications. Historical receipts remain readable."
        )?,
    }
    for step in &run.steps {
        writeln!(
            writer,
            "Step {} | {} | effect {} | citation {} | finding {} | {} Unix ms{}",
            step.ordinal,
            sanitize(&step.kind, 64),
            sanitize(&step.effect_id, 257),
            step.citation_ordinal
                .map_or_else(|| "none".into(), |ordinal| ordinal.to_string()),
            step.finding_id
                .as_deref()
                .map_or_else(|| "none".into(), |id| sanitize(id, 257)),
            step.recorded_ms,
            step.reason
                .as_deref()
                .map_or_else(String::new, |reason| format!(
                    " | {}",
                    sanitize(reason, 128)
                ))
        )?;
    }
    writeln!(
        writer,
        "Paid allowance: USD 0. This template starts no collection, playback, native processing or model request."
    )?;
    writeln!(
        writer,
        "Literal findings are places to inspect. Published artifacts do not establish semantic task completion."
    )
}

fn state_label(state: TaskRunState) -> &'static str {
    match state {
        TaskRunState::Running => "running",
        TaskRunState::Completed => "completed",
        TaskRunState::Partial => "partial",
        TaskRunState::Cancelled => "cancelled",
        TaskRunState::Revoked => "revoked",
    }
}

#[cfg(test)]
mod tests {
    use sigy_service::task::run::{TaskRunSpec, TaskRunStep};

    use super::*;

    fn run(state: TaskRunState) -> TaskRunView {
        TaskRunView {
            task_id: "water".into(),
            request_id: "delegate".into(),
            spec: sigy_service::task::run::TaskRunSelection::Checkpoint(TaskRunSpec {
                checkpoint_ordinal: 1,
                maximum_findings: 4,
            }),
            generation: 1,
            state,
            created_ms: 1,
            updated_ms: 2,
            planned_findings: 2,
            published_findings: 1,
            skipped_findings: 1,
            briefing_id: Some("report".into()),
            steps: vec![TaskRunStep {
                ordinal: 1,
                kind: "skipped".into(),
                effect_id: "effect".into(),
                citation_ordinal: Some(0),
                finding_id: None,
                reason: Some("unsupported\u{1b}[2J".into()),
                recorded_ms: 2,
            }],
        }
    }

    #[test]
    fn states_distinguish_admission_operational_end_and_semantic_uncertainty()
    -> Result<(), Box<dyn std::error::Error>> {
        for (state, detail) in [
            (
                TaskRunState::Running,
                "offline admission waits for service start",
            ),
            (
                TaskRunState::Completed,
                "Semantic goal success and language quality remain unmeasured",
            ),
            (TaskRunState::Partial, "incomplete evidence"),
            (TaskRunState::Cancelled, "shared analysis remain"),
            (TaskRunState::Revoked, "Historical receipts remain readable"),
        ] {
            let mut output = Vec::new();
            render(&mut output, Some(&run(state)))?;
            let text = String::from_utf8(output)?;
            assert!(text.contains(detail));
            assert!(text.contains("2 planned, 1 published, 1 skipped"));
            assert!(text.contains("citation 0 | finding none"));
            assert!(!text.contains('\u{1b}'));
            assert!(text.contains("Paid allowance: USD 0"));
        }
        let mut output = Vec::new();
        render(&mut output, None)?;
        assert!(String::from_utf8(output)?.contains("No execution admitted"));
        Ok(())
    }
}
