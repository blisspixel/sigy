//! `sigy monitor`: user versions, proposals and the action log.

use std::{
    io::{self, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Args, Subcommand, ValueEnum};
use sigy_service::{
    control::{MonitorOperation, MonitorPage, Operation},
    monitor::{
        ActionOrigin, MonitorAction, MonitorCoverage, MonitorMatches, MonitorSpec, MonitorTerm,
        MonitorVersion, MonitorView, PassageMatch, Proposal,
    },
};

use crate::explorer::{text::sanitize, utc_label};

/// A window on capture start times: the last `--hours` ending now, or an exact range.
#[derive(Debug, Args)]
pub struct WindowArgs {
    /// Hours before now, 1 to 744.
    #[arg(long, default_value_t = 24, value_parser = clap::value_parser!(u32).range(1..=744), conflicts_with_all = ["from_ms", "to_ms"])]
    hours: u32,
    /// Exact window start, Unix milliseconds.
    #[arg(long, requires = "to_ms")]
    from_ms: Option<i64>,
    /// Exact window end (exclusive), Unix milliseconds.
    #[arg(long, requires = "from_ms")]
    to_ms: Option<i64>,
}

impl WindowArgs {
    fn window(&self) -> Result<(i64, i64), Box<dyn std::error::Error>> {
        if let (Some(from), Some(to)) = (self.from_ms, self.to_ms) {
            return Ok((from, to));
        }
        let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        Ok((now - i64::from(self.hours) * 3_600_000, now))
    }
}

#[derive(Debug, Args)]
pub struct SpecArgs {
    /// Short name.
    #[arg(long)]
    name: String,
    /// What to follow, in your own words.
    #[arg(long)]
    goal: String,
    /// A literal term as LANGUAGE:TEXT, such as ar:سد or fr:barrage. Use und for any language.
    #[arg(long = "term", required = true)]
    terms: Vec<String>,
    /// Source revision to follow now.
    #[arg(long = "source", required = true)]
    sources: Vec<String>,
    /// Source revision the monitor may add later without a new version.
    #[arg(long = "candidate")]
    candidates: Vec<String>,
    /// Saved schedule that captures for this monitor.
    #[arg(long = "schedule")]
    schedules: Vec<String>,
    /// Most audio to process per day, in minutes.
    #[arg(long)]
    daily_minutes: u32,
    /// Most audio to process in total, in hours.
    #[arg(long)]
    total_hours: u64,
    #[arg(long)]
    recognition_profile: Option<String>,
    #[arg(long)]
    translation_profile: Option<String>,
}

impl SpecArgs {
    fn spec(&self) -> Result<MonitorSpec, Box<dyn std::error::Error>> {
        let terms = self
            .terms
            .iter()
            .map(|term| {
                let (language, text) = term
                    .split_once(':')
                    .ok_or("a term is LANGUAGE:TEXT, such as fr:barrage")?;
                Ok(MonitorTerm {
                    language: language.trim().to_ascii_lowercase(),
                    text: text.trim().to_owned(),
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        Ok(MonitorSpec {
            name: self.name.trim().to_owned(),
            goal: self.goal.trim().to_owned(),
            terms,
            sources: self.sources.clone(),
            candidate_sources: self.candidates.clone(),
            schedules: self.schedules.clone(),
            daily_audio_seconds: self
                .daily_minutes
                .checked_mul(60)
                .ok_or("daily minutes are too large")?,
            total_audio_seconds: self
                .total_hours
                .checked_mul(3600)
                .ok_or("total hours are too large")?,
            recognition_profile: self.recognition_profile.clone(),
            translation_profile: self.translation_profile.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Origin {
    User,
    Rule,
    Model,
}

#[derive(Debug, Subcommand)]
pub enum MonitorCommand {
    /// Create a monitor. Nothing is captured, sent or spent by this command.
    Create {
        id: String,
        #[command(flatten)]
        spec: SpecArgs,
    },
    /// Save a new version. Pass the version you last saw so a concurrent change is not lost.
    Revise {
        id: String,
        #[arg(long)]
        expected_version: u32,
        #[command(flatten)]
        spec: SpecArgs,
    },
    /// Show the current version and state, or one historical version.
    Show {
        id: String,
        #[arg(long)]
        version: Option<u32>,
    },
    /// List every action with its origin, the version it was checked against, and the decision.
    Actions {
        id: String,
        #[arg(long)]
        after: Option<u32>,
    },
    List,
    /// Pause processing for a monitor. Capture schedules are unchanged.
    Pause {
        id: String,
        /// Action ID; repeating it never records a second action.
        #[arg(long)]
        action_id: String,
    },
    Resume {
        id: String,
        #[arg(long)]
        action_id: String,
    },
    /// Count each stage separately for the sources the monitor follows now: captures,
    /// published audio, gaps, pins, transcripts and translations. Nothing is started.
    Coverage {
        id: String,
        #[command(flatten)]
        window: WindowArgs,
    },
    /// Show where the monitor's terms appear in published transcripts and English translations.
    Matches {
        id: String,
        #[command(flatten)]
        window: WindowArgs,
    },
    /// Record a proposal from a rule or model. Only what the current version allows is applied.
    Propose {
        id: String,
        #[arg(long)]
        action_id: String,
        #[arg(long, value_enum, default_value = "model")]
        origin: Origin,
        #[arg(long, conflicts_with_all = ["remove_source", "request"])]
        add_source: Option<String>,
        #[arg(long, conflicts_with = "request")]
        remove_source: Option<String>,
        /// Any other request, kept verbatim and refused.
        #[arg(long)]
        request: Option<String>,
    },
}

impl MonitorCommand {
    /// # Errors
    /// Fails when a term or limit cannot be read.
    pub fn operation(&self) -> Result<Operation, Box<dyn std::error::Error>> {
        let command = match self {
            Self::Create { id, spec } => MonitorOperation::Create {
                id: id.clone(),
                spec: Box::new(spec.spec()?),
            },
            Self::Revise {
                id,
                expected_version,
                spec,
            } => MonitorOperation::Revise {
                id: id.clone(),
                expected_version: *expected_version,
                spec: Box::new(spec.spec()?),
            },
            Self::Show { id, version } => MonitorOperation::Show {
                id: id.clone(),
                version: *version,
            },
            Self::Actions { id, after } => MonitorOperation::Actions {
                id: id.clone(),
                after: *after,
            },
            Self::List => MonitorOperation::List {},
            Self::Coverage { id, window } => {
                let (from_ms, to_ms) = window.window()?;
                MonitorOperation::Coverage {
                    id: id.clone(),
                    from_ms,
                    to_ms,
                }
            }
            Self::Matches { id, window } => {
                let (from_ms, to_ms) = window.window()?;
                MonitorOperation::Matches {
                    id: id.clone(),
                    from_ms,
                    to_ms,
                }
            }
            Self::Pause { id, action_id } | Self::Resume { id, action_id } => {
                MonitorOperation::Propose {
                    id: id.clone(),
                    action_id: action_id.clone(),
                    origin: ActionOrigin::User,
                    proposal: if matches!(self, Self::Pause { .. }) {
                        Proposal::Pause
                    } else {
                        Proposal::Resume
                    },
                }
            }
            Self::Propose {
                id,
                action_id,
                origin,
                add_source,
                remove_source,
                request,
            } => MonitorOperation::Propose {
                id: id.clone(),
                action_id: action_id.clone(),
                origin: match origin {
                    Origin::User => ActionOrigin::User,
                    Origin::Rule => ActionOrigin::Rule,
                    Origin::Model => ActionOrigin::Model,
                },
                proposal: match (add_source, remove_source, request) {
                    (Some(source), _, _) => Proposal::AddSource {
                        source: source.clone(),
                    },
                    (_, Some(source), _) => Proposal::RemoveSource {
                        source: source.clone(),
                    },
                    (_, _, Some(request)) => Proposal::Other {
                        request: request.clone(),
                    },
                    _ => {
                        return Err(
                            "propose needs --add-source, --remove-source or --request".into()
                        );
                    }
                },
            },
        };
        Ok(Operation::Monitor { command })
    }
}

fn duration(seconds: u64) -> String {
    format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60)
}

fn audio(us: u64) -> String {
    let seconds = us / 1_000_000;
    format!(
        "{}h{:02}m{:02}s",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn cue_clock(us: u64) -> String {
    let ms = us / 1000;
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

fn render_coverage(writer: &mut impl Write, coverage: &MonitorCoverage) -> io::Result<()> {
    writeln!(
        writer,
        "Monitor {} version {} | captures started {} to {}",
        coverage.id,
        coverage.version,
        utc_label(coverage.from_ms),
        utc_label(coverage.to_ms)
    )?;
    writeln!(
        writer,
        "Daily audio cap {} for automatic processing (UTC days). Monitors do not schedule captures yet. Each stage is counted on its own.",
        duration(u64::from(coverage.daily_audio_seconds))
    )?;
    for source in &coverage.sources {
        writeln!(writer, "{}:", source.source)?;
        writeln!(
            writer,
            "  captured: {} started, {} published, {} recorded, {} gaps ({})",
            source.captures,
            source.published,
            audio(source.recorded_us),
            source.gaps,
            audio(source.gap_us)
        )?;
        writeln!(
            writer,
            "  analyzed: {} pinned, {} with text ({}), {} no text ({})",
            source.pinned,
            source.transcribed,
            audio(source.transcribed_us),
            source.no_text,
            audio(source.no_text_us)
        )?;
        let reasons: Vec<String> = source
            .untranslated_reasons
            .iter()
            .map(|(reason, count)| format!("{} {count}", sanitize(reason, 64)))
            .collect();
        writeln!(
            writer,
            "  translated: {} cues, {} untranslated{}, {} cues not yet sent to translation",
            source.translated_cues,
            source.untranslated_cues,
            if reasons.is_empty() {
                String::new()
            } else {
                format!(" ({})", reasons.join(", "))
            },
            source.cues_without_translation
        )?;
        if source.truncated {
            writeln!(
                writer,
                "  More captures exist than one request reads; narrow the window."
            )?;
        }
    }
    for schedule in &coverage.schedules {
        writeln!(
            writer,
            "Schedule {}: {} admitted, {} missed (window elapsed), {} missed (spring forward), {} waiting",
            schedule.schedule,
            schedule.admitted,
            schedule.missed_elapsed,
            schedule.missed_spring_forward,
            schedule.waiting
        )?;
    }
    Ok(())
}

fn render_match(writer: &mut impl Write, found: &PassageMatch) -> io::Result<()> {
    writeln!(
        writer,
        "{} | recording {} started {} | transcript {} revision {} cue {} at {} to {}",
        found.source,
        found.recording_id,
        utc_label(found.capture_start_ms),
        found.transcript_id,
        found.transcript_revision,
        found.cue_ordinal,
        cue_clock(found.start_us),
        cue_clock(found.end_us)
    )?;
    writeln!(
        writer,
        "  {}:{} found in the {}",
        found.term_language,
        sanitize(&found.term, 200),
        found.field
    )?;
    writeln!(writer, "  Original: {}", sanitize(&found.original, 400))?;
    match (&found.english, found.translation_revision) {
        (Some(english), Some(revision)) => writeln!(
            writer,
            "  English (translation {revision}, machine output): {}",
            sanitize(english, 400)
        ),
        (None, Some(revision)) => {
            writeln!(writer, "  English: untranslated in translation {revision}")
        }
        _ => writeln!(writer, "  English: not translated"),
    }
}

fn render_matches(writer: &mut impl Write, matches: &MonitorMatches) -> io::Result<()> {
    writeln!(
        writer,
        "Monitor {} version {} | captures started {} to {} | {} transcripts read | {} matches",
        matches.id,
        matches.version,
        utc_label(matches.from_ms),
        utc_label(matches.to_ms),
        matches.transcripts_scanned,
        matches.matches.len()
    )?;
    writeln!(
        writer,
        "Terms match as written, ignoring letter case, with no stemming or accent folding. Recognized text is uncertain."
    )?;
    for found in &matches.matches {
        render_match(writer, found)?;
    }
    if matches.more {
        writeln!(
            writer,
            "More matches exist; narrow the window with --from-ms and --to-ms."
        )?;
    }
    Ok(())
}

fn render_version(writer: &mut impl Write, version: &MonitorVersion) -> io::Result<()> {
    let spec = &version.spec;
    writeln!(
        writer,
        "Version {} | {} | spec sha256 {}",
        version.version,
        sanitize(&spec.name, 120),
        version.spec_sha256
    )?;
    writeln!(writer, "Goal: {}", sanitize(&spec.goal, 2000))?;
    let terms: Vec<String> = spec
        .terms
        .iter()
        .map(|term| format!("{}:{}", term.language, sanitize(&term.text, 200)))
        .collect();
    writeln!(writer, "Terms: {}", terms.join(", "))?;
    writeln!(writer, "Sources: {}", spec.sources.join(", "))?;
    if !spec.candidate_sources.is_empty() {
        writeln!(
            writer,
            "Approved candidates: {}",
            spec.candidate_sources.join(", ")
        )?;
    }
    if !spec.schedules.is_empty() {
        writeln!(writer, "Schedules: {}", spec.schedules.join(", "))?;
    }
    writeln!(
        writer,
        "Caps: {} of audio per day, {} in total | recognition {} | translation {} | paid processing off.",
        duration(u64::from(spec.daily_audio_seconds)),
        duration(spec.total_audio_seconds),
        spec.recognition_profile.as_deref().unwrap_or("not set"),
        spec.translation_profile.as_deref().unwrap_or("not set")
    )
}

fn render_monitor(
    writer: &mut impl Write,
    view: &MonitorView,
    created: Option<bool>,
) -> io::Result<()> {
    match created {
        Some(true) => writeln!(writer, "Monitor version saved.")?,
        Some(false) => writeln!(writer, "Monitor unchanged.")?,
        None => {}
    }
    writeln!(
        writer,
        "Monitor {} | {} | {} actions recorded",
        view.id,
        if view.paused { "paused" } else { "active" },
        view.actions
    )?;
    render_version(writer, &view.version)?;
    writeln!(writer, "Following now: {}", view.active_sources.join(", "))?;
    render_processing(writer, view)
}

fn render_processing(writer: &mut impl Write, view: &MonitorView) -> io::Result<()> {
    let processing = &view.processing;
    if view.version.spec.recognition_profile.is_none() {
        writeln!(
            writer,
            "Automatic processing is off: save a version with --recognition-profile to transcribe new recordings."
        )?;
    }
    writeln!(
        writer,
        "Processed audio: {} today (UTC), {} in total | {} recognitions and {} translations queued",
        audio(processing.used_today_us),
        audio(processing.used_total_us),
        processing.recognition_queued,
        processing.translation_queued
    )?;
    if !processing.skipped.is_empty() {
        let skipped: Vec<String> = processing
            .skipped
            .iter()
            .map(|(stage, reason, count)| format!("{stage} {} {count}", sanitize(reason, 64)))
            .collect();
        writeln!(writer, "Skipped: {}", skipped.join(", "))?;
    }
    Ok(())
}

fn render_action(writer: &mut impl Write, action: &MonitorAction) -> io::Result<()> {
    let detail = match &action.proposal {
        Proposal::AddSource { source } | Proposal::RemoveSource { source } => format!(" {source}"),
        Proposal::Other { request } => format!(" \"{}\"", sanitize(request, 200)),
        Proposal::Pause | Proposal::Resume => String::new(),
    };
    writeln!(
        writer,
        "#{} {} {}{} | checked against version {} | {}: {} | {} USD",
        action.ordinal,
        action.origin.as_str(),
        action.proposal.kind(),
        detail,
        action.policy_version,
        action.decision,
        action.reason,
        action.amount_usd
    )
}

pub fn render(writer: &mut impl Write, page: &MonitorPage) -> io::Result<()> {
    match page {
        MonitorPage::Monitor { monitor, created } => render_monitor(writer, monitor, *created),
        MonitorPage::Version { version } => render_version(writer, version),
        MonitorPage::Action { action } => render_action(writer, action),
        MonitorPage::Actions { id, actions } => {
            if actions.is_empty() {
                writeln!(writer, "No actions recorded for {id}.")?;
            }
            for action in actions {
                render_action(writer, action)?;
            }
            if let Some(last) = actions.last()
                && actions.len() == 16
            {
                writeln!(
                    writer,
                    "More: sigy monitor actions {id} --after {}",
                    last.ordinal
                )?;
            }
            Ok(())
        }
        MonitorPage::Coverage { coverage } => render_coverage(writer, coverage),
        MonitorPage::Matches { matches } => render_matches(writer, matches),
        MonitorPage::List { ids } => {
            if ids.is_empty() {
                writeln!(writer, "No monitors. Create one with sigy monitor create.")?;
            }
            for id in ids {
                writeln!(writer, "{id}")?;
            }
            Ok(())
        }
    }
}
