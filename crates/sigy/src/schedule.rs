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
        /// Create a new rule owned by this monitor's explicit capture policy.
        #[arg(long, requires = "monitor_version")]
        monitor: Option<String>,
        /// Monitor version last inspected. Cannot adopt an existing standalone rule.
        #[arg(long, requires = "monitor")]
        monitor_version: Option<u32>,
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
                    monitor,
                    monitor_version,
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
                    monitor_owner: match (monitor, monitor_version) {
                        (Some(id), Some(version)) => {
                            Some(sigy_service::monitor::MonitorScheduleOwner {
                                monitor_id: id.clone(),
                                version: *version,
                            })
                        }
                        (None, None) => None,
                        _ => return Err("monitor ownership requires its inspected version".into()),
                    },
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
            "once" => clean(rule.civil_date.as_deref().unwrap_or("unknown")),
            "weekly" => format!(
                "{} {:02}:{:02}:{:02}",
                clean(rule.weekday.as_deref().unwrap_or("unknown")),
                rule.hour,
                rule.minute,
                rule.second
            ),
            _ => format!("{:02}:{:02}:{:02}", rule.hour, rule.minute, rule.second),
        };
        writeln!(
            writer,
            "{} {} {} {} {} seconds {} bytes revision {}",
            clean(&rule.id),
            clean(&rule.zone),
            clean(&rule.recurrence),
            when,
            rule.duration_seconds,
            rule.maximum_bytes,
            rule.revision
        )?;
        if let Some(owner) = &rule.monitor_owner {
            writeln!(
                writer,
                "  Capture owner {} attached at monitor version {}. Current capture bounds govern future admissions.",
                clean(&owner.monitor_id),
                owner.version
            )?;
        }
        if rule.task_owned {
            writeln!(
                writer,
                "  Task-owned finite rule. Revision is refused; inspect task collection for its grant and recording state."
            )?;
        }
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
            "  {} {} start {start} end {end} offset {offset} recording {}",
            clean(&occurrence.civil_date),
            clean(outcome),
            clean(&recording)
        )?;
        if let Some(admission) = &occurrence.capture_admission {
            writeln!(
                writer,
                "  Reserved {} seconds and {} bytes under monitor {} version {} (sha256 {}), rule revision {}. Zero USD.",
                admission.planned_seconds,
                admission.maximum_bytes,
                clean(&admission.monitor_id),
                admission.policy_version,
                clean(&admission.policy_sha256),
                admission.rule_revision
            )?;
        }
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
        writeln!(writer, "Next page after {}", clean(after))?;
    }
    Ok(())
}

fn clean(text: &str) -> String {
    crate::explorer::text::sanitize(text, 1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn operation(words: &[&str]) -> Result<Operation, Box<dyn std::error::Error>> {
        let cli = crate::Cli::try_parse_from(words)?;
        let crate::Command::Schedule { command } = cli.command else {
            return Err("not a schedule".into());
        };
        command.operation()
    }

    #[test]
    fn owned_schedule_requires_explicit_monitor_and_inspected_version() -> TestResult {
        let mut words = vec![
            "sigy",
            "schedule",
            "create",
            "owned",
            "--source",
            "a:v1",
            "--zone",
            "Etc/UTC",
            "--daily",
            "12:00:00",
            "--monitor",
            "news",
        ];
        assert!(operation(&words).is_err());
        words.extend(["--monitor-version", "7"]);
        let value = serde_json::to_value(operation(&words)?)?;
        assert_eq!(value["command"]["monitor_owner"]["monitor_id"], "news");
        assert_eq!(value["command"]["monitor_owner"]["version"], 7);
        assert_eq!(value["command"]["source_revision"], "a:v1");
        assert_eq!(value["command"]["maximum_bytes"], 67_108_864);
        Ok(())
    }

    #[test]
    fn civil_clock_commands_preserve_timezone_flags_and_never_attach_analysis() -> TestResult {
        for clock in [["--once", "2030-01-01T12:34:56"], ["--daily", "12:34:56"]] {
            let value = serde_json::to_value(operation(&[
                "sigy",
                "schedule",
                "create",
                "morning",
                "--source",
                "station-r2",
                "--zone",
                "America/Toronto",
                clock[0],
                clock[1],
            ])?)?;
            assert_eq!(value["command"]["zone"], "America/Toronto");
            assert_eq!(value["command"]["maximum_bytes"], 67_108_864_u64);
            assert_eq!(
                value["command"]["analysis_profile"],
                serde_json::Value::Null
            );
        }
        let revise = serde_json::to_value(operation(&[
            "sigy",
            "schedule",
            "revise",
            "morning",
            "--zone",
            "Asia/Kolkata",
            "--weekly",
            "monday",
            "--at",
            "03:04:05",
            "--max-mib",
            "1",
        ])?)?;
        assert_eq!(revise["command"]["weekly"], "monday");
        assert_eq!(revise["command"]["at"], "03:04:05");
        assert_eq!(revise["command"]["maximum_bytes"], 1_048_576);
        assert_eq!(
            serde_json::to_value(operation(&[
                "sigy", "schedule", "list", "--after", "morning", "--limit", "7"
            ])?)?["command"]["limit"],
            7
        );
        assert_eq!(
            serde_json::to_value(operation(&["sigy", "schedule", "show", "morning"])?)?["command"]
                ["id"],
            "morning"
        );
        assert!(
            operation(&[
                "sigy",
                "schedule",
                "create",
                "morning",
                "--source",
                "station",
                "--zone",
                "UTC",
                "--daily",
                "12:00:00",
                "--max-mib",
                "18446744073709551615"
            ])
            .is_err()
        );
        assert!(
            operation(&[
                "sigy",
                "schedule",
                "create",
                "morning",
                "--source",
                "station",
                "--zone",
                "UTC",
                "--analysis-profile",
                "profile"
            ])
            .is_err()
        );
        Ok(())
    }

    fn page() -> Result<SchedulePage, serde_json::Error> {
        serde_json::from_value(
            serde_json::json!({"rules":[{"id":"morning","source_revision":"station","zone":"America/Toronto","recurrence":"weekly","civil_date":null,"weekday":"monday","hour":3,"minute":4,"second":5,"duration_seconds":60,"maximum_bytes":9_007_199_254_740_993_i64,"revision":2}],"occurrences":[{"id":"slot","civil_date":"2030-03-10","state":"missed","miss_reason":"spring-forward","start_ms":null,"end_ms":null,"offset_seconds":null,"transition_ms":null,"duration_seconds":60,"maximum_bytes":1,"recording_id":null}],"next_after":"morning","newly_created":true}),
        )
    }

    #[test]
    fn rendering_keeps_missed_clock_evidence_exact_and_strips_terminal_controls() -> TestResult {
        let mut page = page()?;
        let mut bytes = Vec::new();
        render(&mut bytes, &page)?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("weekly monday 03:04:05"));
        assert!(text.contains("9007199254740993 bytes revision 2"));
        assert!(text.contains("spring-forward start none end none offset none recording none"));
        assert!(text.contains("No analysis profile is attached"));
        assert!(!text.contains("Task-owned finite rule"));
        page.rules[0].task_owned = true;
        page.rules[0].recurrence = "once".into();
        page.rules[0].id = "أخبار\u{1b}[2J\u{7}".into();
        page.occurrences[0].miss_reason = None;
        page.occurrences[0].state = "unknown\u{1b}]52;c;payload\u{7}".into();
        page.occurrences[0].start_ms = Some(123);
        page.occurrences[0].end_ms = Some(456);
        page.occurrences[0].offset_seconds = Some(-18000);
        page.occurrences[0].recording_id = Some("retained\u{1b}[31m".into());
        page.newly_created = Some(false);
        bytes = Vec::new();
        render(&mut bytes, &page)?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("أخبار"));
        assert!(text.contains("once unknown"));
        assert!(text.contains("start 123 end 456 offset -18000 recording retained"));
        assert!(text.contains("Schedule unchanged"));
        assert!(text.contains("Task-owned finite rule. Revision is refused"));
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        for recurrence in ["daily", "future-cadence"] {
            page.rules[0].recurrence = recurrence.into();
            render(&mut Vec::new(), &page)?;
        }
        Ok(())
    }
}
