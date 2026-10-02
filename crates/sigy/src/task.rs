//! Finite monitor-bound tasks, frozen observations, explicit publication, collection and
//! processing grants, and read-only collection-to-evidence reconciliation.

use std::io::{self, Write};

use clap::{Args, Subcommand};
use sigy_service::task::run::TaskRunSpec;
use sigy_service::{
    control::{Operation, TaskOperation, TaskPage},
    task::{MAX_CHECKPOINTS, TaskCheckpoint, TaskSpec, TaskView},
};

mod collection;
mod evidence;
mod execution;
mod processing;

use crate::explorer::text::sanitize;

#[derive(Debug, Args)]
pub struct ScopeArgs {
    /// A bounded goal, preserved as text. This command does not interpret it.
    #[arg(long)]
    goal: String,
    /// Existing monitor identifier.
    #[arg(long)]
    monitor: String,
    /// Monitor version inspected before admission.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    monitor_version: u32,
    /// Exact stored monitor action count inspected before admission, including refusals.
    #[arg(long)]
    monitor_actions: u32,
    /// Inclusive capture-start window, Unix milliseconds.
    #[arg(long)]
    from_ms: i64,
    /// Exclusive capture-start window, Unix milliseconds.
    #[arg(long)]
    to_ms: i64,
}

impl ScopeArgs {
    fn spec(&self) -> Result<TaskSpec, Box<dyn std::error::Error>> {
        let spec = TaskSpec {
            goal: self.goal.clone(),
            monitor_id: self.monitor.clone(),
            monitor_version: self.monitor_version,
            monitor_actions: self.monitor_actions,
            from_ms: self.from_ms,
            to_ms: self.to_ms,
        };
        spec.validate()?;
        Ok(spec)
    }
}

#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Store an immutable monitor-bound observation task. Creates no worker jobs.
    Create {
        id: String,
        #[command(flatten)]
        scope: ScopeArgs,
    },
    /// Read accepted scope and its latest frozen checkpoint.
    Show { id: String },
    /// List task identifiers in stable order.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u32).range(1..=16))]
        limit: u32,
    },
    /// Freeze bounded coverage and literal citations. Creates no worker jobs.
    Checkpoint {
        id: String,
        request_id: String,
        /// Exact current checkpoint ordinal, zero before the first checkpoint.
        #[arg(long, value_parser = clap::value_parser!(u32).range(0..=128))]
        expected_checkpoint: u32,
    },
    /// Read one historical frozen checkpoint.
    CheckpointShow {
        id: String,
        #[arg(value_parser = clap::value_parser!(u32).range(1..=128))]
        ordinal: u32,
    },
    /// Delegate bounded literal findings and one briefing from a frozen checkpoint.
    Execute {
        id: String,
        request_id: String,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=128))]
        checkpoint: u32,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=64))]
        max_findings: u32,
        /// Exact current run generation, zero before execution admission.
        #[arg(long)]
        expected_generation: u32,
    },
    /// Inspect durable execution state and publication receipts.
    Execution { id: String },
    /// Cancel future task-owned publications. Preserves existing findings and shared work.
    Cancel {
        id: String,
        request_id: String,
        /// Exact current run generation.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        expected_generation: u32,
    },
    /// Grant one or two new finite UTC recording schedules for this task.
    Collect {
        id: String,
        request_id: String,
        #[command(flatten)]
        bounds: collection::CollectionArgs,
        /// Zero before collection admission. Exact replay uses the original value.
        #[arg(long, value_parser = clap::value_parser!(u32).range(0..=0))]
        expected_generation: u32,
    },
    /// Read only this task's collection schedule and recording bindings.
    Collection { id: String },
    /// Stop future task collection admissions. Admitted recordings continue.
    CancelCollection {
        id: String,
        request_id: String,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        expected_generation: u32,
    },
    /// Grant finite local recognition and translation of this task's collected recordings.
    Process {
        id: String,
        request_id: String,
        #[command(flatten)]
        bounds: processing::ProcessingArgs,
        /// Zero before processing admission. Exact replay uses the original value.
        #[arg(long, value_parser = clap::value_parser!(u32).range(0..=0))]
        expected_generation: u32,
    },
    /// Read this task's processing receipts, charges and shared job states.
    Processing { id: String },
    /// Stop future task processing admissions. Admitted jobs and other interests continue.
    CancelProcessing {
        id: String,
        request_id: String,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        expected_generation: u32,
    },
    /// Reconcile collected recordings through task processing to literal citations.
    Evidence { id: String },
}

impl TaskCommand {
    pub fn operation(&self) -> Result<Operation, Box<dyn std::error::Error>> {
        let command = match self {
            Self::Create { id, scope } => TaskOperation::Create {
                id: id.clone(),
                spec: Box::new(scope.spec()?),
            },
            Self::Show { id } => TaskOperation::Show { id: id.clone() },
            Self::List { after, limit } => TaskOperation::List {
                after: after.clone(),
                limit: *limit,
            },
            Self::Checkpoint {
                id,
                request_id,
                expected_checkpoint,
            } => TaskOperation::Checkpoint {
                id: id.clone(),
                request_id: request_id.clone(),
                expected_checkpoint: *expected_checkpoint,
            },
            Self::CheckpointShow { id, ordinal } => TaskOperation::ShowCheckpoint {
                id: id.clone(),
                ordinal: *ordinal,
            },
            Self::Execute {
                id,
                request_id,
                checkpoint,
                max_findings,
                expected_generation,
            } => TaskOperation::Execute {
                id: id.clone(),
                request_id: request_id.clone(),
                spec: Box::new(TaskRunSpec {
                    checkpoint_ordinal: *checkpoint,
                    maximum_findings: *max_findings,
                }),
                expected_generation: *expected_generation,
            },
            Self::Execution { id } => TaskOperation::Execution { id: id.clone() },
            Self::Cancel {
                id,
                request_id,
                expected_generation,
            } => TaskOperation::Cancel {
                id: id.clone(),
                request_id: request_id.clone(),
                expected_generation: *expected_generation,
            },
            Self::Collect {
                id,
                request_id,
                bounds,
                expected_generation,
            } => TaskOperation::Collect {
                id: id.clone(),
                request_id: request_id.clone(),
                spec: Box::new(bounds.spec()?),
                expected_generation: *expected_generation,
            },
            Self::Collection { id } => TaskOperation::Collection { id: id.clone() },
            Self::CancelCollection {
                id,
                request_id,
                expected_generation,
            } => TaskOperation::CancelCollection {
                id: id.clone(),
                request_id: request_id.clone(),
                expected_generation: *expected_generation,
            },
            Self::Process {
                id,
                request_id,
                bounds,
                expected_generation,
            } => TaskOperation::Process {
                id: id.clone(),
                request_id: request_id.clone(),
                spec: Box::new(bounds.spec()?),
                expected_generation: *expected_generation,
            },
            Self::Processing { id } => TaskOperation::Processing { id: id.clone() },
            Self::CancelProcessing {
                id,
                request_id,
                expected_generation,
            } => TaskOperation::CancelProcessing {
                id: id.clone(),
                request_id: request_id.clone(),
                expected_generation: *expected_generation,
            },
            Self::Evidence { id } => TaskOperation::Evidence { id: id.clone() },
        };
        Ok(Operation::Task { command })
    }
}

pub fn render(writer: &mut impl Write, page: &TaskPage) -> io::Result<()> {
    match page {
        TaskPage::Task { task, created } => render_task(writer, task, *created),
        TaskPage::List { ids, next_after } => {
            if ids.is_empty() {
                writeln!(writer, "No tasks on this page.")?;
            }
            for id in ids {
                writeln!(writer, "{}", sanitize(id, 257))?;
            }
            if let Some(after) = next_after {
                writeln!(
                    writer,
                    "Next page: task list --after {}",
                    sanitize(after, 257)
                )?;
            }
            Ok(())
        }
        TaskPage::Checkpoint { checkpoint } => render_checkpoint(writer, checkpoint),
        TaskPage::Execution { run } => execution::render(writer, run.as_deref()),
        TaskPage::Collection { collection } => collection::render(writer, collection.as_deref()),
        TaskPage::Processing { processing } => processing::render(writer, processing.as_deref()),
        TaskPage::Evidence { evidence } => evidence::render(writer, evidence.as_deref()),
    }
}

fn render_task(writer: &mut impl Write, task: &TaskView, created: Option<bool>) -> io::Result<()> {
    writeln!(writer, "Task {}", sanitize(&task.id, 257))?;
    if let Some(created) = created {
        writeln!(
            writer,
            "Admission: {}",
            if created { "stored" } else { "unchanged" }
        )?;
    }
    writeln!(writer, "Goal: {}", sanitize(&task.spec.goal, 2_048))?;
    writeln!(
        writer,
        "Monitor {} version {}, stored actions {} (including refusals)",
        sanitize(&task.spec.monitor_id, 257),
        task.spec.monitor_version,
        task.spec.monitor_actions
    )?;
    writeln!(
        writer,
        "Capture-start window: {} to {} Unix ms (end exclusive)",
        task.spec.from_ms, task.spec.to_ms
    )?;
    writeln!(writer, "Created: {} Unix ms", task.created_ms)?;
    writeln!(
        writer,
        "Scope SHA-256: {}",
        sanitize(&task.scope_sha256, 64)
    )?;
    writeln!(
        writer,
        "Monitor specification SHA-256: {}",
        sanitize(&task.monitor_spec_sha256, 64)
    )?;
    writeln!(
        writer,
        "Scope: {}. Checkpoints: {}/{MAX_CHECKPOINTS}; remaining {}.",
        if task.scope_current {
            "current"
        } else {
            "stale; new checkpoints refused"
        },
        task.checkpoint,
        MAX_CHECKPOINTS.saturating_sub(task.checkpoint)
    )?;
    writeln!(
        writer,
        "Paid allowance: USD 0. Scope and checkpoints alone create no worker jobs."
    )?;
    if let Some(checkpoint) = &task.latest_checkpoint {
        render_checkpoint(writer, checkpoint)?;
    } else {
        writeln!(
            writer,
            "No checkpoint recorded. Semantic task completion is unmeasured."
        )?;
    }
    Ok(())
}

fn render_checkpoint(writer: &mut impl Write, checkpoint: &TaskCheckpoint) -> io::Result<()> {
    writeln!(
        writer,
        "Task {} checkpoint {} | request {} | observed {} Unix ms",
        sanitize(&checkpoint.task_id, 257),
        checkpoint.ordinal,
        sanitize(&checkpoint.request_id, 257),
        checkpoint.observed_ms
    )?;
    writeln!(
        writer,
        "Window elapsed: {}. Monitor processing paused: {}. Transcripts scanned: {}. Literal citations: {}.",
        checkpoint.window_elapsed,
        checkpoint.monitor_paused,
        checkpoint.transcripts_scanned,
        checkpoint.citations.len()
    )?;
    writeln!(
        writer,
        "Frozen observations; semantic task completion is unmeasured."
    )?;
    render_coverage(writer, &checkpoint.coverage)?;
    for citation in &checkpoint.citations {
        writeln!(
            writer,
            "Source {} | recording {} | transcript {} revision {} cue {} | translation {} | media {} to {} us",
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
    if checkpoint.more {
        writeln!(
            writer,
            "Observation truncated; this checkpoint is not complete coverage."
        )?;
    }
    writeln!(
        writer,
        "Citations are historical references, not retention holds or audio playback."
    )
}

fn render_coverage(
    writer: &mut impl Write,
    coverage: &sigy_service::monitor::MonitorCoverage,
) -> io::Result<()> {
    for source in &coverage.sources {
        writeln!(
            writer,
            "Source {}: {} captures, {} published, {} recorded us, {} gap us; {} with text, {} no text; {} translated cues, {} untranslated, {} not yet sent.",
            sanitize(&source.source, 257),
            source.captures,
            source.published,
            source.recorded_us,
            source.gap_us,
            source.transcribed,
            source.no_text,
            source.translated_cues,
            source.untranslated_cues,
            source.cues_without_translation
        )?;
        writeln!(
            writer,
            "  Analysis: {} pinned, {} with-text us, {} no-text us. Capture gaps: {}.",
            source.pinned, source.transcribed_us, source.no_text_us, source.gaps
        )?;
        for (reason, count) in &source.untranslated_reasons {
            writeln!(
                writer,
                "  Untranslated reason {}: {} cues",
                sanitize(reason, 64),
                count
            )?;
        }
        if source.truncated {
            writeln!(writer, "Source coverage truncated; narrow the window.")?;
        }
    }
    for schedule in &coverage.schedules {
        writeln!(
            writer,
            "Schedule {}: {} admitted, {} missed (window elapsed), {} missed (spring forward), {} waiting",
            sanitize(&schedule.schedule, 257),
            schedule.admitted,
            schedule.missed_elapsed,
            schedule.missed_spring_forward,
            schedule.waiting
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use sigy_service::{
        monitor::{MonitorCoverage, ScheduleCoverage, SourceCoverage},
        task::TaskCitation,
    };

    use super::*;

    #[derive(Parser)]
    struct Only {
        #[command(subcommand)]
        command: TaskCommand,
    }

    fn parse(arguments: &[&str]) -> Result<Operation, Box<dyn std::error::Error>> {
        Only::try_parse_from(arguments)?.command.operation()
    }

    #[test]
    fn create_preserves_script_and_refuses_unbounded_or_hostile_goals()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut args = vec![
            "sigy",
            "create",
            "water",
            "--goal",
            "تابع المياه",
            "--monitor",
            "monitor",
            "--monitor-version",
            "2",
            "--monitor-actions",
            "3",
            "--from-ms",
            "0",
            "--to-ms",
            "60000",
        ];
        match parse(&args)? {
            Operation::Task {
                command: TaskOperation::Create { id, spec },
            } => {
                assert_eq!(id, "water");
                assert_eq!(spec.goal, "تابع المياه");
                assert_eq!(spec.monitor_actions, 3);
            }
            other => return Err(format!("{other:?}").into()),
        }
        for goal in ["", "  ", "x\u{1b}[2Jy"] {
            args[4] = goal;
            assert!(parse(&args).is_err());
        }
        let oversized = "x".repeat(2_049);
        args[4] = &oversized;
        assert!(parse(&args).is_err());
        args[4] = "follow water";
        args[14] = "0";
        assert!(parse(&args).is_err());
        Ok(())
    }

    #[test]
    fn reads_and_checkpoints_map_to_explicit_operations() -> Result<(), Box<dyn std::error::Error>>
    {
        assert!(
            matches!(parse(&["sigy", "show", "water"] )?, Operation::Task { command: TaskOperation::Show { id } } if id == "water")
        );
        assert!(matches!(
            parse(&["sigy", "list"])?,
            Operation::Task {
                command: TaskOperation::List {
                    after: None,
                    limit: 16
                }
            }
        ));
        assert!(
            matches!(parse(&["sigy", "list", "--after", "water", "--limit", "1"] )?, Operation::Task { command: TaskOperation::List { after: Some(after), limit: 1 } } if after == "water")
        );
        assert!(
            matches!(parse(&["sigy", "checkpoint", "water", "check-1", "--expected-checkpoint", "0"] )?, Operation::Task { command: TaskOperation::Checkpoint { id, request_id, expected_checkpoint: 0 } } if id == "water" && request_id == "check-1")
        );
        assert!(
            matches!(parse(&["sigy", "checkpoint-show", "water", "1"] )?, Operation::Task { command: TaskOperation::ShowCheckpoint { id, ordinal: 1 } } if id == "water")
        );
        for limit in ["0", "17", "4294967295"] {
            assert!(parse(&["sigy", "list", "--limit", limit]).is_err());
        }
        assert!(parse(&["sigy", "checkpoint", "water", "check-1"]).is_err());
        assert!(parse(&["sigy", "checkpoint-show", "water", "0"]).is_err());
        Ok(())
    }

    #[test]
    fn collection_requires_finite_explicit_bounds_and_distinct_sources()
    -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            "sigy",
            "collect",
            "water",
            "grant",
            "--source",
            "one:v1",
            "--start-ms",
            "1000",
            "--seconds",
            "60",
            "--max-bytes",
            "1024",
            "--expected-generation",
            "0",
        ];
        match parse(&args)? {
            Operation::Task {
                command:
                    TaskOperation::Collect {
                        id,
                        request_id,
                        spec,
                        expected_generation: 0,
                    },
            } => {
                assert_eq!(id, "water");
                assert_eq!(request_id, "grant");
                assert_eq!(spec.captures.len(), 1);
                assert_eq!(spec.captures[0].maximum_bytes, 1024);
            }
            other => return Err(format!("{other:?}").into()),
        }
        for (index, values) in [
            (7, &["1001", "9223372036854775000"][..]),
            (9, &["0", "901"]),
            (11, &["0", "268435457"]),
            (13, &["1"]),
        ] {
            for value in values {
                let mut invalid = args;
                invalid[index] = value;
                assert!(parse(&invalid).is_err(), "{invalid:?}");
            }
        }
        let mut multiple = args.to_vec();
        multiple.extend(["--source", "two:v1"]);
        assert!(
            matches!(parse(&multiple)?, Operation::Task { command: TaskOperation::Collect { spec, .. } } if spec.captures.len() == 2)
        );
        multiple.extend(["--source", "three:v1"]);
        assert!(parse(&multiple).is_err());
        let mut duplicate = args.to_vec();
        duplicate.extend(["--source", "one:v1"]);
        assert!(parse(&duplicate).is_err());
        assert!(
            matches!(parse(&["sigy", "collection", "water"] )?, Operation::Task { command: TaskOperation::Collection { id } } if id == "water")
        );
        assert!(matches!(
            parse(&[
                "sigy",
                "cancel-collection",
                "water",
                "stop",
                "--expected-generation",
                "1"
            ])?,
            Operation::Task {
                command: TaskOperation::CancelCollection {
                    expected_generation: 1,
                    ..
                }
            }
        ));
        assert!(
            parse(&[
                "sigy",
                "cancel-collection",
                "water",
                "stop",
                "--expected-generation",
                "0"
            ])
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn processing_requires_explicit_profiles_and_a_finite_allowance()
    -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            "sigy",
            "process",
            "water",
            "grant",
            "--recognition-profile",
            "asr",
            "--max-audio-seconds",
            "60",
            "--expected-generation",
            "0",
        ];
        assert!(matches!(
            parse(&args)?,
            Operation::Task {
                command: TaskOperation::Process { ref id, ref spec, expected_generation: 0, .. }
            } if id == "water" && spec.recognition_profile == "asr" && spec.translation_profile.is_none() && spec.maximum_audio_seconds == 60
        ));
        let mut translated = args.to_vec();
        translated.extend(["--translation-profile", "mt"]);
        assert!(matches!(
            parse(&translated)?,
            Operation::Task { command: TaskOperation::Process { ref spec, .. } } if spec.translation_profile.as_deref() == Some("mt")
        ));
        for (index, value) in [(5, "../asr"), (7, "0"), (7, "1801"), (9, "1")] {
            let mut invalid = args;
            invalid[index] = value;
            assert!(parse(&invalid).is_err(), "{invalid:?}");
        }
        assert!(
            matches!(parse(&["sigy", "processing", "water"])?, Operation::Task { command: TaskOperation::Processing { id } } if id == "water")
        );
        assert!(
            matches!(parse(&["sigy", "evidence", "water"])?, Operation::Task { command: TaskOperation::Evidence { id } } if id == "water")
        );
        assert!(matches!(
            parse(&[
                "sigy",
                "cancel-processing",
                "water",
                "stop",
                "--expected-generation",
                "1"
            ])?,
            Operation::Task {
                command: TaskOperation::CancelProcessing {
                    expected_generation: 1,
                    ..
                }
            }
        ));
        assert!(
            parse(&[
                "sigy",
                "cancel-processing",
                "water",
                "stop",
                "--expected-generation",
                "0"
            ])
            .is_err()
        );
        Ok(())
    }

    fn checkpoint() -> TaskCheckpoint {
        TaskCheckpoint {
            task_id: "water".into(),
            ordinal: 1,
            request_id: "check-1".into(),
            observed_ms: 60_000,
            coverage: MonitorCoverage {
                id: "monitor".into(),
                version: 2,
                from_ms: 0,
                to_ms: 60_000,
                daily_audio_seconds: 60,
                sources: vec![SourceCoverage {
                    source: "source:v1".into(),
                    captures: 2,
                    published: 1,
                    recorded_us: 1_000_000,
                    pinned: 1,
                    transcribed_us: 800_000,
                    no_text_us: 200_000,
                    gaps: 1,
                    gap_us: 500_000,
                    transcribed: 1,
                    translated_cues: 2,
                    untranslated_cues: 1,
                    untranslated_reasons: vec![("unsupported\u{1b}[2J".into(), 1)],
                    truncated: true,
                    ..SourceCoverage::default()
                }],
                schedules: vec![ScheduleCoverage {
                    schedule: "evening\u{1b}[2J".into(),
                    admitted: 1,
                    missed_elapsed: 2,
                    missed_spring_forward: 3,
                    waiting: 4,
                }],
            },
            citations: vec![TaskCitation {
                source: "source:v1".into(),
                recording_id: "recording".into(),
                transcript_id: "pin".into(),
                transcript_revision: 2,
                translation_revision: Some(3),
                cue_ordinal: 0,
                start_us: 10,
                end_us: 20,
            }],
            transcripts_scanned: 1,
            more: true,
            window_elapsed: true,
            monitor_paused: true,
        }
    }

    fn rendered(page: &TaskPage) -> Result<String, Box<dyn std::error::Error>> {
        let mut output = Vec::new();
        render(&mut output, page)?;
        Ok(String::from_utf8(output)?)
    }

    #[test]
    fn checkpoint_reports_uncertainty_and_historical_ids() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut observed = checkpoint();
        let text = rendered(&TaskPage::Checkpoint {
            checkpoint: Box::new(observed.clone()),
        })?;
        assert!(text.contains("Window elapsed: true. Monitor processing paused: true."));
        assert!(text.contains("semantic task completion is unmeasured"));
        assert!(text.contains("recording recording | transcript pin revision 2 cue 0 | translation 3 | media 10 to 20 us"));
        assert!(text.contains("Observation truncated"));
        assert!(text.contains(
            "Source source:v1: 2 captures, 1 published, 1000000 recorded us, 500000 gap us"
        ));
        assert!(text.contains("Source coverage truncated"));
        assert!(text.contains("1 pinned, 800000 with-text us, 200000 no-text us. Capture gaps: 1"));
        assert!(text.contains("Untranslated reason unsupported[2J: 1 cues"));
        assert!(text.contains(
            "1 admitted, 2 missed (window elapsed), 3 missed (spring forward), 4 waiting"
        ));
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("not retention holds or audio playback"));
        observed.more = false;
        observed.coverage.sources[0].truncated = false;
        observed.citations[0].translation_revision = None;
        let text = rendered(&TaskPage::Checkpoint {
            checkpoint: Box::new(observed),
        })?;
        assert!(text.contains("translation none"));
        assert!(!text.contains("Observation truncated"));
        assert!(!text.contains("Source coverage truncated"));
        Ok(())
    }

    #[test]
    fn task_and_list_output_sanitize_untrusted_text() -> Result<(), Box<dyn std::error::Error>> {
        let mut task = TaskView {
            template: sigy_service::task::TASK_TEMPLATE.into(),
            paid_allowance_usd: "0.000000".into(),
            id: "water".into(),
            spec: TaskSpec {
                goal: "تابع\u{1b}[2J المياه".into(),
                monitor_id: "monitor".into(),
                monitor_version: 2,
                monitor_actions: 3,
                from_ms: 0,
                to_ms: 60_000,
            },
            scope_sha256: "a".repeat(64),
            monitor_spec_sha256: "b".repeat(64),
            created_ms: 1,
            checkpoint: 0,
            scope_current: true,
            latest_checkpoint: None,
        };
        for created in [None, Some(true), Some(false)] {
            let text = rendered(&TaskPage::Task {
                task: Box::new(task.clone()),
                created,
            })?;
            assert!(!text.contains('\u{1b}'));
            assert!(text.contains("Scope: current. Checkpoints: 0/128; remaining 128."));
            assert!(text.contains("No checkpoint recorded"));
        }
        task.scope_current = false;
        task.checkpoint = MAX_CHECKPOINTS;
        task.latest_checkpoint = Some(Box::new(checkpoint()));
        let text = rendered(&TaskPage::Task {
            task: Box::new(task),
            created: None,
        })?;
        assert!(text.contains("stale; new checkpoints refused"));
        assert!(text.contains("remaining 0"));
        assert!(text.contains("checkpoint 1"));
        let text = rendered(&TaskPage::List {
            ids: vec!["water\u{1b}[2J".into()],
            next_after: Some("water\nunsafe".into()),
        })?;
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("Next page: task list --after"));
        assert!(
            rendered(&TaskPage::List {
                ids: Vec::new(),
                next_after: None
            })?
            .contains("No tasks")
        );
        Ok(())
    }
}
