//! Exact task briefings keep frozen collection coverage separate from live monitor totals.

use std::io::{self, Write};

use sigy_service::task::run::TaskEvidenceBriefing;

use crate::explorer::text::sanitize;

pub(super) fn render(
    writer: &mut impl Write,
    briefing: Option<&TaskEvidenceBriefing>,
) -> io::Result<()> {
    let Some(briefing) = briefing else {
        return writeln!(
            writer,
            "No exact task briefing published. Inspect task execution for progress and skipped findings."
        );
    };
    writeln!(
        writer,
        "Task {} exact briefing {} | monitor {} generation {} | created {} Unix ms",
        sanitize(&briefing.task_id, 257),
        sanitize(&briefing.id, 257),
        sanitize(&briefing.monitor_id, 257),
        briefing.generation,
        briefing.created_ms
    )?;
    super::snapshot::render(writer, &briefing.snapshot)?;
    writeln!(
        writer,
        "Published members: {}. Groups describe repeated original scripts; independent corroboration remains unmeasured.",
        briefing.members.len()
    )?;
    for member in &briefing.members {
        writeln!(
            writer,
            "Group {} | finding {} | inspect: monitor finding {} {} show",
            member.group_ordinal,
            sanitize(&member.finding_id, 257),
            sanitize(&briefing.monitor_id, 257),
            sanitize(&member.finding_id, 257)
        )?;
    }
    writeln!(
        writer,
        "Membership and observed coverage are immutable. Inspect each finding for its cited original, translation and current media availability."
    )
}
