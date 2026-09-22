use clap::Subcommand;
use sigy_service::control::{Operation, ScheduleOperation, SchedulePage};
use std::io::{self, Write};

#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// Bind one source to a civil clock. Only the next occurrence is materialized.
    Create {
        id: String,
        #[arg(long)]
        source: String,
        /// IANA time zone, for example `America/New_York`.
        #[arg(long)]
        zone: String,
        /// One civil instant, `YYYY-MM-DDTHH:MM:SS`, with no offset.
        #[arg(long)]
        once: Option<String>,
        /// Daily civil time, `HH:MM:SS`.
        #[arg(long)]
        daily: Option<String>,
        /// Weekday name for a weekly rule. Pair it with `--at`.
        #[arg(long)]
        weekly: Option<String>,
        /// Civil time for `--weekly`.
        #[arg(long)]
        at: Option<String>,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        #[arg(long, default_value_t = 64)]
        max_mib: u64,
    },
    /// Change future occurrences. An admitted plan stays as it was.
    Revise {
        id: String,
        #[arg(long)]
        zone: String,
        #[arg(long)]
        once: Option<String>,
        #[arg(long)]
        daily: Option<String>,
        #[arg(long)]
        weekly: Option<String>,
        #[arg(long)]
        at: Option<String>,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        #[arg(long, default_value_t = 64)]
        max_mib: u64,
    },
    /// List schedule rules. This does not start a recording.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 16)]
        limit: u32,
    },
    /// Show one rule and the occurrences already materialized.
    Show { id: String },
}

impl ScheduleCommand {
    pub fn operation(&self) -> Result<Operation, Box<dyn std::error::Error>> {
        let maximum_bytes = |max_mib: u64| {
            max_mib
                .checked_mul(1024 * 1024)
                .ok_or("recording byte ceiling is too large")
        };
        Ok(Operation::Schedule {
            command: match self {
                Self::Create {
                    id,
                    source,
                    zone,
                    once,
                    daily,
                    weekly,
                    at,
                    seconds,
                    max_mib,
                } => ScheduleOperation::Create {
                    id: id.clone(),
                    source_revision: source.clone(),
                    zone: zone.clone(),
                    once: once.clone(),
                    daily: daily.clone(),
                    weekly: weekly.clone(),
                    at: at.clone(),
                    seconds: *seconds,
                    maximum_bytes: maximum_bytes(*max_mib)?,
                    analysis_profile: None,
                },
                Self::Revise {
                    id,
                    zone,
                    once,
                    daily,
                    weekly,
                    at,
                    seconds,
                    max_mib,
                } => ScheduleOperation::Revise {
                    id: id.clone(),
                    zone: zone.clone(),
                    once: once.clone(),
                    daily: daily.clone(),
                    weekly: weekly.clone(),
                    at: at.clone(),
                    seconds: *seconds,
                    maximum_bytes: maximum_bytes(*max_mib)?,
                    analysis_profile: None,
                },
                Self::List { after, limit } => ScheduleOperation::List {
                    after: after.clone(),
                    limit: *limit,
                },
                Self::Show { id } => ScheduleOperation::Show { id: id.clone() },
            },
        })
    }
}

pub fn render(writer: &mut impl Write, page: &SchedulePage) -> io::Result<()> {
    for rule in &page.rules {
        let when = match rule.recurrence.as_str() {
            "once" => rule.civil_date.clone().unwrap_or_default(),
            "weekly" => format!(
                "{} {:02}:{:02}:{:02}",
                rule.weekday.as_deref().unwrap_or("weekday"),
                rule.hour,
                rule.minute,
                rule.second
            ),
            _ => format!("{:02}:{:02}:{:02}", rule.hour, rule.minute, rule.second),
        };
        writeln!(
            writer,
            "{} {} {} {} {} seconds {} bytes revision {}",
            rule.id,
            rule.zone,
            rule.recurrence,
            when,
            rule.duration_seconds,
            rule.maximum_bytes,
            rule.revision
        )?;
    }
    for occurrence in &page.occurrences {
        let outcome = occurrence
            .miss_reason
            .as_deref()
            .unwrap_or(occurrence.state.as_str());
        let start = occurrence
            .start_ms
            .map_or_else(|| "none".to_owned(), |value| value.to_string());
        let end = occurrence
            .end_ms
            .map_or_else(|| "none".to_owned(), |value| value.to_string());
        let offset = occurrence
            .offset_seconds
            .map_or_else(|| "none".to_owned(), |value| value.to_string());
        let recording = occurrence
            .recording_id
            .clone()
            .unwrap_or_else(|| "none".to_owned());
        writeln!(
            writer,
            "  {} {outcome} start {start} end {end} offset {offset} recording {recording}",
            occurrence.civil_date
        )?;
    }
    if let Some(created) = page.newly_created {
        writeln!(
            writer,
            "{}",
            if created {
                "Schedule created. No analysis profile is attached."
            } else {
                "Schedule unchanged. No analysis profile is attached."
            }
        )?;
    }
    if let Some(after) = &page.next_after {
        writeln!(writer, "Next page after {after}")?;
    }
    Ok(())
}
