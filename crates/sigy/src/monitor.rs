//! `sigy monitor`: user versions, proposals and the action log.

use std::{
    io::{self, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Args, Subcommand, ValueEnum};
use sigy_service::{
    control::{MonitorOperation, MonitorPage, Operation},
    monitor::{
        ActionOrigin, BriefingExport, BriefingMember, BriefingPage, FindingCite, FindingOriginal,
        FindingPage, MonitorAction, MonitorCaptureBounds, MonitorCoverage, MonitorMatches,
        MonitorSpec, MonitorTerm, MonitorVersion, MonitorView, PassageMatch, Proposal,
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
    /// Daily planned capture minutes for new monitor-owned schedules. Requires both lifetime caps.
    #[arg(long, requires_all = ["capture_total_hours", "capture_total_mib"])]
    capture_daily_minutes: Option<u32>,
    /// Lifetime planned capture hours. This allowance never refills.
    #[arg(long, requires_all = ["capture_daily_minutes", "capture_total_mib"])]
    capture_total_hours: Option<u64>,
    /// Lifetime sum of worst-case capture ceilings, in MiB. Failures do not refund it.
    #[arg(long, requires_all = ["capture_daily_minutes", "capture_total_hours"])]
    capture_total_mib: Option<u64>,
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
            capture: match (
                self.capture_daily_minutes,
                self.capture_total_hours,
                self.capture_total_mib,
            ) {
                (Some(daily), Some(total), Some(bytes)) => Some(MonitorCaptureBounds {
                    daily_seconds: daily
                        .checked_mul(60)
                        .ok_or("daily capture minutes are too large")?,
                    total_seconds: total
                        .checked_mul(3600)
                        .ok_or("total capture hours are too large")?,
                    total_bytes: bytes
                        .checked_mul(1024 * 1024)
                        .ok_or("total capture bytes are too large")?,
                }),
                (None, None, None) => None,
                _ => return Err("capture requires daily minutes, total hours and total MiB".into()),
            },
        })
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Origin {
    User,
    Rule,
    Model,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OriginalChoice {
    Retained,
    Expired,
    Missing,
}

#[derive(Debug, Subcommand)]
pub enum FindingAction {
    /// Store one citation. The same citation changes nothing. A missing range is rejected.
    Add {
        #[arg(long)]
        transcript: String,
        #[arg(long)]
        transcript_revision: i64,
        #[arg(long)]
        translation_revision: i64,
        #[arg(long)]
        ordinal: u32,
        /// `retained` copies the cue interval. `expired` or `missing` stores no interval.
        #[arg(long, value_enum, default_value = "retained")]
        original: OriginalChoice,
    },
    /// Show one stored citation.
    Show,
}

#[derive(Debug, Subcommand)]
pub enum BriefingAction {
    /// Store one generation for this window. The same id and window changes nothing.
    Add {
        #[command(flatten)]
        window: WindowArgs,
    },
    /// Show one stored generation. Coverage is the copy stored with it.
    Show,
    /// Print the redacted JSON snapshot. This writes nothing and is not the catalog.
    Export,
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
    /// Cite one cue. The service copies the interval. Transcript text does not create a finding.
    Finding {
        monitor: String,
        finding: String,
        #[command(subcommand)]
        action: FindingAction,
    },
    /// Publish one briefing. Classification stays off. Transcript text does not create one.
    Briefing {
        monitor: String,
        briefing: String,
        #[command(subcommand)]
        action: BriefingAction,
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
            Self::Finding {
                monitor,
                finding,
                action,
            } => finding_operation(monitor, finding, action),
            Self::Briefing {
                monitor,
                briefing,
                action,
            } => briefing_operation(monitor, briefing, action)?,
        };
        Ok(Operation::Monitor { command })
    }
}

fn briefing_operation(
    monitor: &str,
    briefing: &str,
    action: &BriefingAction,
) -> Result<MonitorOperation, Box<dyn std::error::Error>> {
    Ok(match action {
        BriefingAction::Show => MonitorOperation::ShowBriefing {
            monitor: monitor.to_owned(),
            briefing: briefing.to_owned(),
        },
        BriefingAction::Export => MonitorOperation::ExportBriefing {
            monitor: monitor.to_owned(),
            briefing: briefing.to_owned(),
        },
        BriefingAction::Add { window } => {
            let (from_ms, to_ms) = window.window()?;
            MonitorOperation::PublishBriefing {
                monitor: monitor.to_owned(),
                briefing: briefing.to_owned(),
                from_ms,
                to_ms,
            }
        }
    })
}

fn finding_operation(monitor: &str, finding: &str, action: &FindingAction) -> MonitorOperation {
    match action {
        FindingAction::Show => MonitorOperation::ShowFinding {
            monitor: monitor.to_owned(),
            finding: finding.to_owned(),
        },
        FindingAction::Add {
            transcript,
            transcript_revision,
            translation_revision,
            ordinal,
            original,
        } => MonitorOperation::PublishFinding {
            monitor: monitor.to_owned(),
            finding: finding.to_owned(),
            cite: FindingCite {
                transcript_id: transcript.clone(),
                transcript_revision: *transcript_revision,
                translation_revision: *translation_revision,
                cue_ordinal: *ordinal,
                original: match original {
                    OriginalChoice::Retained => FindingOriginal::Retained,
                    OriginalChoice::Expired => FindingOriginal::Expired,
                    OriginalChoice::Missing => FindingOriginal::Missing,
                },
            },
        },
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
        "Daily audio cap {} for automatic processing (UTC days). Owned capture has separate explicit caps. Each stage is counted on its own.",
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
    )?;
    render_capture_bounds(writer, spec.capture.as_ref())
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
    render_capture(writer, view)?;
    render_processing(writer, view)
}

fn render_capture(writer: &mut impl Write, view: &MonitorView) -> io::Result<()> {
    let usage = &view.capture_usage;
    writeln!(
        writer,
        "Capture reservations: {} seconds today, {} lifetime seconds, {} lifetime worst-case bytes; {} admissions. Failed captures retain their full planned reservations.",
        usage.used_today_seconds,
        usage.used_total_seconds,
        usage.reserved_total_bytes,
        usage.admissions
    )?;
    for (reason, count) in &usage.refusals {
        writeln!(
            writer,
            "Capture refused: {} ({count} policy/occurrence records).",
            sanitize(reason, 64)
        )?;
    }
    Ok(())
}

fn render_capture_bounds(
    writer: &mut impl Write,
    bounds: Option<&MonitorCaptureBounds>,
) -> io::Result<()> {
    if let Some(bounds) = bounds {
        writeln!(
            writer,
            "Owned capture caps: {} seconds per UTC day, {} lifetime seconds, {} lifetime bytes. Processing pause does not pause capture.",
            bounds.daily_seconds, bounds.total_seconds, bounds.total_bytes
        )?;
    } else {
        writeln!(
            writer,
            "Owned capture is disabled. Standalone schedules remain independent."
        )?;
    }
    Ok(())
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
        "Admitted audio: {} today (UTC), {} in total | {} recognition admissions, {} translation admissions",
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
        MonitorPage::Finding { finding } => render_finding(writer, finding),
        MonitorPage::Briefing { briefing } => render_briefing(writer, briefing),
    }
}

/// Write the redacted briefing snapshot as JSON. The catalog is not included.
/// # Errors
/// Returns an error when the document cannot be written.
pub fn write_export(writer: &mut impl Write, export: &BriefingExport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *writer, export).map_err(io::Error::other)?;
    writeln!(writer)
}

fn render_briefing(writer: &mut impl Write, briefing: &BriefingPage) -> io::Result<()> {
    writeln!(
        writer,
        "# Briefing {}/{} generation {}",
        briefing.monitor_id, briefing.id, briefing.generation
    )?;
    writeln!(
        writer,
        "Classification is off. Finding membership is frozen at monitor version {}. Coverage below was frozen when this generation was stored.",
        briefing.monitor_version
    )?;
    render_coverage(writer, &briefing.coverage)?;
    render_reports(writer, briefing)?;
    writeln!(writer, "Support: none. Classification is off.")?;
    writeln!(writer, "Contradiction: unresolved. Classification is off.")?;
    writeln!(writer, "Independence: unresolved.")?;
    writeln!(
        writer,
        "Wording remains uncertain. This is not human review."
    )
}

fn render_reports(writer: &mut impl Write, briefing: &BriefingPage) -> io::Result<()> {
    writeln!(
        writer,
        "Reports: {}. Repetition groups: {}. Corroboration: {}. Repetition is not independent corroboration.",
        briefing.members.len(),
        briefing.corroboration,
        briefing.corroboration
    )?;
    for ordinal in 0..briefing.corroboration {
        let members: Vec<&BriefingMember> = briefing
            .members
            .iter()
            .filter(|member| member.group_ordinal == ordinal)
            .collect();
        if members.len() > 1 {
            let ids = members
                .iter()
                .map(|member| member.finding_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                writer,
                "Repetition group {ordinal}: {ids}. These copies count once."
            )?;
        }
        for member in members {
            writeln!(
                writer,
                "Finding {}: {}",
                member.finding_id,
                sanitize(&member.original_script, 400)
            )?;
        }
    }
    Ok(())
}

fn render_finding(writer: &mut impl Write, finding: &FindingPage) -> io::Result<()> {
    writeln!(
        writer,
        "Finding {}/{} | transcript {} revision {} | translation revision {} | cue {}",
        finding.monitor_id,
        finding.id,
        finding.transcript_id,
        finding.transcript_revision,
        finding.translation_revision,
        finding.cue_ordinal
    )?;
    writeln!(
        writer,
        "Original script: {}",
        sanitize(&finding.original_script, 400)
    )?;
    match &finding.english {
        Some(english) => writeln!(
            writer,
            "English (translation {}, machine output): {}",
            finding.translation_revision,
            sanitize(english, 400)
        )?,
        None => match &finding.untranslated_reason {
            Some(reason) => writeln!(
                writer,
                "English: untranslated in translation {} ({})",
                finding.translation_revision,
                sanitize(reason, 64)
            )?,
            None => writeln!(
                writer,
                "English: untranslated in translation {}.",
                finding.translation_revision
            )?,
        },
    }
    match finding.original {
        FindingOriginal::Retained => match (finding.start_us, finding.end_us) {
            (Some(start), Some(end)) => writeln!(
                writer,
                "Retained interval: {} to {} on recording {}.",
                cue_clock(start),
                cue_clock(end),
                finding.recording_id
            )?,
            _ => writeln!(
                writer,
                "Original audio was cited as retained. The stored interval is incomplete."
            )?,
        },
        FindingOriginal::Expired => {
            writeln!(
                writer,
                "Original audio is expired. No retained interval is cited."
            )?;
        }
        FindingOriginal::Missing => {
            writeln!(
                writer,
                "Original audio is missing for this cue. No retained interval is cited."
            )?;
        }
    }
    if finding.stale_transcript == Some(true) {
        writeln!(
            writer,
            "A newer transcript revision exists, so this finding is stale. The cited revision stays readable."
        )?;
    }
    if finding.stale_translation == Some(true) {
        writeln!(
            writer,
            "A newer translation of this transcript revision exists, so this finding is stale."
        )?;
    }
    writeln!(
        writer,
        "The cited revision stays readable. Wording remains uncertain. This is not human review."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Only {
        #[command(subcommand)]
        command: MonitorCommand,
    }

    fn page(original: FindingOriginal) -> FindingPage {
        FindingPage {
            monitor_id: "fair".into(),
            id: "world".into(),
            transcript_id: "pin".into(),
            transcript_revision: 1,
            translation_revision: 1,
            cue_ordinal: 0,
            original,
            recording_id: "one".into(),
            start_us: matches!(original, FindingOriginal::Retained).then_some(0),
            end_us: matches!(original, FindingOriginal::Retained).then_some(1_000_000),
            original_script: "Una feria mundial".into(),
            english: Some("A world fair".into()),
            untranslated_reason: None,
            stale_transcript: None,
            stale_translation: None,
        }
    }

    fn rendered(finding: &FindingPage) -> Result<String, Box<dyn std::error::Error>> {
        let mut buffer = Vec::new();
        render_finding(&mut buffer, finding)?;
        Ok(String::from_utf8(buffer)?)
    }

    #[test]
    fn capture_limits_are_explicit_checked_and_separate_from_processing()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = [
            "sigy",
            "create",
            "news",
            "--name",
            "News",
            "--goal",
            "Follow news",
            "--term",
            "ar:سد",
            "--source",
            "a:v1",
            "--daily-minutes",
            "1",
            "--total-hours",
            "1",
        ];
        let independent = Only::try_parse_from(base)?;
        let value = serde_json::to_value(independent.command.operation()?)?;
        assert!(value["command"]["spec"].get("capture").is_none());
        let mut words = base.to_vec();
        words.extend(["--capture-daily-minutes", "2"]);
        assert!(Only::try_parse_from(&words).is_err());
        words.extend(["--capture-total-hours", "3", "--capture-total-mib", "4"]);
        let bounded = Only::try_parse_from(&words)?;
        let value = serde_json::to_value(bounded.command.operation()?)?;
        assert_eq!(value["command"]["spec"]["daily_audio_seconds"], 60);
        assert_eq!(value["command"]["spec"]["capture"]["daily_seconds"], 120);
        assert_eq!(value["command"]["spec"]["capture"]["total_seconds"], 10800);
        assert_eq!(
            value["command"]["spec"]["capture"]["total_bytes"],
            4_194_304
        );
        let mut overflow = base.to_vec();
        overflow.extend([
            "--capture-daily-minutes",
            "1",
            "--capture-total-hours",
            "1",
            "--capture-total-mib",
            "18446744073709551615",
        ]);
        assert!(
            Only::try_parse_from(&overflow)?
                .command
                .operation()
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn a_finding_names_the_citation_and_keeps_wording_uncertain()
    -> Result<(), Box<dyn std::error::Error>> {
        let retained = rendered(&page(FindingOriginal::Retained))?;
        assert!(retained.contains(
            "Finding fair/world | transcript pin revision 1 | translation revision 1 | cue 0"
        ));
        assert!(retained.contains("Original script: Una feria mundial"));
        assert!(retained.contains("English (translation 1, machine output): A world fair"));
        assert!(retained.contains("Retained interval: 00:00.000 to 00:01.000 on recording one."));
        assert!(retained.contains(
            "The cited revision stays readable. Wording remains uncertain. This is not human review."
        ));
        assert!(!retained.contains("No retained interval"));

        let expired = rendered(&page(FindingOriginal::Expired))?;
        assert!(expired.contains("Original audio is expired. No retained interval is cited."));
        assert!(!expired.contains("Retained interval:"));

        let missing = rendered(&page(FindingOriginal::Missing))?;
        assert!(
            missing
                .contains("Original audio is missing for this cue. No retained interval is cited.")
        );

        let mut untranslated = page(FindingOriginal::Retained);
        untranslated.english = None;
        untranslated.untranslated_reason = Some("unsupported-language".into());
        let untranslated = rendered(&untranslated)?;
        assert!(
            untranslated.contains("English: untranslated in translation 1 (unsupported-language)")
        );

        let mut stale = page(FindingOriginal::Retained);
        stale.stale_transcript = Some(true);
        stale.stale_translation = Some(true);
        let stale = rendered(&stale)?;
        assert!(stale.contains(
            "A newer transcript revision exists, so this finding is stale. The cited revision stays readable."
        ));
        assert!(stale.contains(
            "A newer translation of this transcript revision exists, so this finding is stale."
        ));
        Ok(())
    }

    #[test]
    fn finding_add_defaults_to_retained_and_show_only_reads()
    -> Result<(), Box<dyn std::error::Error>> {
        let add = Only::try_parse_from([
            "sigy",
            "finding",
            "fair",
            "world",
            "add",
            "--transcript",
            "pin",
            "--transcript-revision",
            "1",
            "--translation-revision",
            "1",
            "--ordinal",
            "0",
        ])?;
        let operation = add.command.operation()?;
        match operation {
            Operation::Monitor {
                command:
                    MonitorOperation::PublishFinding {
                        monitor,
                        finding,
                        cite,
                    },
            } => {
                assert_eq!((monitor.as_str(), finding.as_str()), ("fair", "world"));
                assert_eq!(cite.transcript_id, "pin");
                assert_eq!(
                    (
                        cite.transcript_revision,
                        cite.translation_revision,
                        cite.cue_ordinal
                    ),
                    (1, 1, 0)
                );
                assert_eq!(cite.original, FindingOriginal::Retained);
            }
            other => return Err(format!("{other:?}").into()),
        }
        let show = Only::try_parse_from(["sigy", "finding", "fair", "world", "show"])?;
        match show.command.operation()? {
            Operation::Monitor {
                command: MonitorOperation::ShowFinding { monitor, finding },
            } => assert_eq!((monitor.as_str(), finding.as_str()), ("fair", "world")),
            other => return Err(format!("{other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn a_briefing_states_coverage_then_one_repetition_and_unresolved_conflict()
    -> Result<(), Box<dyn std::error::Error>> {
        let page = BriefingPage {
            monitor_id: "fair".into(),
            id: "week".into(),
            generation: 1,
            from_ms: 0,
            to_ms: 60_000,
            monitor_version: 1,
            classification: "off".into(),
            corroboration: 2,
            coverage: MonitorCoverage {
                id: "fair".into(),
                version: 1,
                from_ms: 0,
                to_ms: 60_000,
                daily_audio_seconds: 60,
                sources: Vec::new(),
                schedules: Vec::new(),
            },
            members: vec![
                member("copy", 0, "Una feria mundial"),
                member("world", 0, "Una feria mundial"),
                member("local", 1, "Una feria local"),
            ],
        };
        let mut buffer = Vec::new();
        render_briefing(&mut buffer, &page)?;
        let text = String::from_utf8(buffer)?;
        let coverage = text.find("Daily audio cap").ok_or("coverage")?;
        let repetition = text
            .find("Repetition group 0: copy, world.")
            .ok_or("group")?;
        assert!(coverage < repetition);
        assert!(text.contains("# Briefing fair/week generation 1"));
        assert!(text.contains("Reports: 3. Repetition groups: 2. Corroboration: 2."));
        assert!(text.contains("Repetition is not independent corroboration."));
        assert!(text.contains("Contradiction: unresolved. Classification is off."));
        assert!(text.contains("Support: none. Classification is off."));
        assert!(text.contains("Independence: unresolved."));
        assert!(text.contains("Classification is off."));
        assert!(text.contains("Coverage below was frozen when this generation was stored."));
        assert!(!text.contains("current read"));
        assert!(!text.contains("classifier"));
        let document = sigy_service::monitor::briefing_export(&page);
        assert!(!document.catalog);
        assert_eq!(document.document, "sigy.briefing");
        assert_eq!(document.authority, "none");
        let mut exported = Vec::new();
        write_export(&mut exported, &document)?;
        let json = String::from_utf8(exported)?;
        assert!(json.contains("This snapshot is not the catalog."));
        assert!(!json.contains("http"));
        assert!(!json.contains("ledger"));
        assert!(!json.contains("secret"));
        let exported = Only::try_parse_from(["sigy", "briefing", "fair", "week", "export"])?;
        match exported.command.operation()? {
            Operation::Monitor {
                command: MonitorOperation::ExportBriefing { monitor, briefing },
            } => assert_eq!((monitor.as_str(), briefing.as_str()), ("fair", "week")),
            other => return Err(format!("{other:?}").into()),
        }
        let add = Only::try_parse_from([
            "sigy",
            "briefing",
            "fair",
            "week",
            "add",
            "--from-ms",
            "0",
            "--to-ms",
            "60000",
        ])?;
        match add.command.operation()? {
            Operation::Monitor {
                command:
                    MonitorOperation::PublishBriefing {
                        monitor,
                        briefing,
                        from_ms,
                        to_ms,
                    },
            } => {
                assert_eq!((monitor.as_str(), briefing.as_str()), ("fair", "week"));
                assert_eq!((from_ms, to_ms), (0, 60_000));
            }
            other => return Err(format!("{other:?}").into()),
        }
        Ok(())
    }

    fn member(id: &str, group: u32, script: &str) -> BriefingMember {
        BriefingMember {
            finding_id: id.into(),
            group_ordinal: group,
            original_script: script.into(),
            english: None,
            untranslated_reason: None,
            stale_transcript: None,
            stale_translation: None,
        }
    }
}
