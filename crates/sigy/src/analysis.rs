use crate::explorer::text::sanitize;
use crate::style::{Ink, Tone};
use clap::Subcommand;
use sigy_service::control::{
    AnalysisDisposition, AnalysisOperation, AnalysisPage, Operation, ProfileOperation,
    RecognitionView, TranslationProfileOperation,
};
use sigy_service::recognition::{LocalAsrJob, RecognitionProfile, TranscriptCuePage};
use sigy_service::translation::{TranslationJob, TranslationPage, TranslationProfile};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum AnalysisCommand {
    /// Verify retained bytes in a supervised local job. Does not recognize speech.
    Verify {
        id: String,
        #[arg(long)]
        input: String,
        #[arg(long)]
        revision: i64,
    },
    /// Inspect a verification job, including historical failures and cancellation.
    Job { id: String },
    /// Cancel this exact worker generation and retain its input until reading stops.
    Cancel {
        id: String,
        #[arg(long)]
        generation: u32,
    },
    /// Inspect stored language evidence. Does not run detection or infer from source hints.
    Languages {
        #[command(subcommand)]
        command: crate::languages::LanguageCommand,
    },
    /// Pin one completed recording. The worker receives no source URL.
    Admit {
        id: String,
        #[arg(long)]
        recording: String,
        /// Retire an unpublished worker and admit the next revision.
        #[arg(long)]
        replace_worker: bool,
    },
    /// Show the current analysis pin.
    Show { id: String },
    /// Publish one revision. An older revision cannot replace a newer one.
    Publish {
        id: String,
        #[arg(long)]
        revision: i64,
    },
    /// Recognize speech in a published pin with a local profile. Runs in the service.
    Transcribe {
        /// Job ID. Repeating the same request returns the same job and never reruns it.
        id: String,
        /// Published analysis pin.
        #[arg(long)]
        input: String,
        /// Published analysis revision. An older revision is refused.
        #[arg(long)]
        revision: i64,
        /// Recognition profile ID.
        #[arg(long)]
        profile: String,
        /// Transcript revision this result follows. Defaults to the current one.
        #[arg(long)]
        parent: Option<i64>,
    },
    /// Show recognized text for a pin. Machine output in the original script, unreviewed.
    Transcript {
        id: String,
        /// Transcript revision. Defaults to the newest.
        #[arg(long)]
        revision: Option<i64>,
        /// Continue after this cue ordinal.
        #[arg(long)]
        after: Option<u32>,
    },
    /// Manage local recognizer profiles. Files are hashed now and before every run.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Translate a recognized transcript into English with a local profile. Runs in the service.
    Translate {
        /// Job ID. Repeating the same request returns the same job and never reruns it.
        id: String,
        /// Analysis pin whose transcript is translated.
        #[arg(long)]
        input: String,
        /// Transcript revision. Defaults to the newest.
        #[arg(long)]
        transcript_revision: Option<i64>,
        /// Translation profile ID.
        #[arg(long)]
        profile: String,
    },
    /// Show original and English text side by side. Machine translation, unreviewed.
    Translation {
        id: String,
        #[arg(long)]
        transcript_revision: Option<i64>,
        #[arg(long)]
        revision: Option<i64>,
        #[arg(long)]
        after: Option<u32>,
    },
    /// Manage local translation profiles. Files are hashed now and before every run.
    TranslationProfile {
        #[command(subcommand)]
        command: TranslationProfileCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum TranslationProfileCommand {
    /// Hash a local llama.cpp runtime and translation model into a profile.
    Add {
        id: String,
        /// Directory holding llama-completion and its libraries.
        #[arg(long)]
        runtime_dir: PathBuf,
        /// Executable file name inside the runtime directory.
        #[arg(long, default_value = DEFAULT_TRANSLATOR)]
        executable: String,
        /// Translation model file (GGUF).
        #[arg(long)]
        model: PathBuf,
        /// Source languages the model declares, as comma-separated codes such as ar,es,fr.
        /// Other detected languages are left untranslated with a reason.
        #[arg(long)]
        languages: String,
        #[arg(long)]
        threads: Option<u32>,
        /// Committed-memory ceiling for one translator process, in MiB.
        #[arg(long, default_value_t = 3072)]
        memory_mib: u64,
        /// Wall deadline for one cue, in seconds.
        #[arg(long, default_value_t = 120)]
        cue_deadline_seconds: u64,
    },
    List,
    Show {
        id: String,
    },
}

const DEFAULT_TRANSLATOR: &str = if cfg!(windows) {
    "llama-completion.exe"
} else {
    "llama-completion"
};

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Hash a local whisper.cpp runtime, model and speech-activity model into a profile.
    Add {
        id: String,
        /// Directory holding the recognizer executable and its libraries.
        #[arg(long)]
        runtime_dir: PathBuf,
        /// Executable file name inside the runtime directory.
        #[arg(long, default_value = DEFAULT_EXECUTABLE)]
        executable: String,
        /// Recognition model file (ggml).
        #[arg(long)]
        model: PathBuf,
        /// Speech-activity model file. Required so silence does not become text.
        #[arg(long)]
        vad_model: PathBuf,
        /// Recognizer threads. Defaults to half the available processors, at most four.
        #[arg(long)]
        threads: Option<u32>,
        /// Committed-memory ceiling for the recognizer process, in MiB.
        #[arg(long, default_value_t = 3072)]
        memory_mib: u64,
        /// Wall deadline for one job, in seconds.
        #[arg(long, default_value_t = 600)]
        deadline_seconds: u64,
    },
    List,
    Show {
        id: String,
    },
}

const DEFAULT_EXECUTABLE: &str = if cfg!(windows) {
    "whisper-cli.exe"
} else {
    "whisper-cli"
};

fn default_threads() -> u32 {
    let available = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    u32::try_from((available / 2).clamp(1, 4)).unwrap_or(1)
}

impl AnalysisCommand {
    /// # Errors
    /// Fails when a profile's local files cannot be hashed or are invalid.
    pub fn operation(&self) -> Result<Operation, Box<dyn std::error::Error>> {
        Ok(match self {
            Self::Verify {
                id,
                input,
                revision,
            } => AnalysisOperation::Verify {
                id: id.clone(),
                input: input.clone(),
                revision: *revision,
            },
            Self::Job { id } => AnalysisOperation::Job { id: id.clone() },
            Self::Cancel { id, generation } => AnalysisOperation::Cancel {
                id: id.clone(),
                generation: *generation,
            },
            Self::Languages { command } => AnalysisOperation::Languages {
                command: command.operation(),
            },
            Self::Admit {
                id,
                recording,
                replace_worker,
            } => AnalysisOperation::Admit {
                id: id.clone(),
                recording_id: recording.clone(),
                replace_worker: *replace_worker,
            },
            Self::Show { id } => AnalysisOperation::Show { id: id.clone() },
            Self::Publish { id, revision } => AnalysisOperation::Publish {
                id: id.clone(),
                revision: *revision,
            },
            Self::Transcribe {
                id,
                input,
                revision,
                profile,
                parent,
            } => AnalysisOperation::Transcribe {
                id: id.clone(),
                input: input.clone(),
                revision: *revision,
                profile: profile.clone(),
                parent_revision: *parent,
            },
            Self::Transcript {
                id,
                revision,
                after,
            } => AnalysisOperation::Transcript {
                id: id.clone(),
                revision: *revision,
                after: *after,
            },
            Self::Profile { command } => AnalysisOperation::Profile {
                command: command.operation()?,
            },
            Self::Translate {
                id,
                input,
                transcript_revision,
                profile,
            } => AnalysisOperation::Translate {
                id: id.clone(),
                input: input.clone(),
                transcript_revision: *transcript_revision,
                profile: profile.clone(),
            },
            Self::Translation {
                id,
                transcript_revision,
                revision,
                after,
            } => AnalysisOperation::Translation {
                id: id.clone(),
                transcript_revision: *transcript_revision,
                revision: *revision,
                after: *after,
            },
            Self::TranslationProfile { command } => AnalysisOperation::TranslationProfile {
                command: command.operation()?,
            },
        }
        .into())
    }
}

impl TranslationProfileCommand {
    fn operation(&self) -> Result<TranslationProfileOperation, Box<dyn std::error::Error>> {
        Ok(match self {
            Self::Add {
                id,
                runtime_dir,
                executable,
                model,
                languages,
                threads,
                memory_mib,
                cue_deadline_seconds,
            } => {
                let profile = sigy_service::recognizer::translate::describe_translation_profile(
                    id,
                    &std::path::absolute(runtime_dir)?,
                    executable,
                    &std::path::absolute(model)?,
                    languages,
                    threads.unwrap_or_else(default_threads),
                    memory_mib
                        .checked_mul(1024 * 1024)
                        .ok_or("memory limit is too large")?,
                    cue_deadline_seconds
                        .checked_mul(1000)
                        .ok_or("deadline is too large")?,
                )?;
                TranslationProfileOperation::Add {
                    profile: Box::new(profile),
                }
            }
            Self::List => TranslationProfileOperation::List {},
            Self::Show { id } => TranslationProfileOperation::Show { id: id.clone() },
        })
    }
}

impl ProfileCommand {
    fn operation(&self) -> Result<ProfileOperation, Box<dyn std::error::Error>> {
        Ok(match self {
            Self::Add {
                id,
                runtime_dir,
                executable,
                model,
                vad_model,
                threads,
                memory_mib,
                deadline_seconds,
            } => {
                let absolute = |path: &PathBuf| std::path::absolute(path);
                let profile = sigy_service::recognizer::describe_profile(
                    id,
                    &absolute(runtime_dir)?,
                    executable,
                    &absolute(model)?,
                    &absolute(vad_model)?,
                    threads.unwrap_or_else(default_threads),
                    memory_mib
                        .checked_mul(1024 * 1024)
                        .ok_or("memory limit is too large")?,
                    deadline_seconds
                        .checked_mul(1000)
                        .ok_or("deadline is too large")?,
                )?;
                ProfileOperation::Add {
                    profile: Box::new(profile),
                }
            }
            Self::List => ProfileOperation::List {},
            Self::Show { id } => ProfileOperation::Show { id: id.clone() },
        })
    }
}

pub fn render_recognition(writer: &mut impl Write, view: &RecognitionView) -> io::Result<()> {
    match view {
        RecognitionView::Profiles { profiles } => {
            if profiles.is_empty() {
                writeln!(
                    writer,
                    "No recognition profiles. Add one with analysis profile add."
                )?;
            }
            for profile in profiles {
                render_profile(writer, profile)?;
            }
            Ok(())
        }
        RecognitionView::Profile { profile, created } => {
            writeln!(
                writer,
                "{}",
                if *created {
                    "Recognition profile stored."
                } else {
                    "Recognition profile."
                }
            )?;
            render_profile(writer, profile)
        }
        RecognitionView::Job { job } => render_recognition_job(writer, job),
        RecognitionView::Transcript { page } => render_transcript(writer, page),
        RecognitionView::Empty { id } => writeln!(writer, "Nothing stored yet for {id}."),
        RecognitionView::TranslationProfiles { profiles } => {
            if profiles.is_empty() {
                writeln!(
                    writer,
                    "No translation profiles. Add one with analysis translation-profile add."
                )?;
            }
            for profile in profiles {
                render_translation_profile(writer, profile)?;
            }
            Ok(())
        }
        RecognitionView::TranslationProfile { profile, created } => {
            writeln!(
                writer,
                "{}",
                if *created {
                    "Translation profile stored."
                } else {
                    "Translation profile."
                }
            )?;
            render_translation_profile(writer, profile)
        }
        RecognitionView::TranslationJob { job } => render_translation_job(writer, job),
        RecognitionView::Translation { page } => render_translation(writer, page),
    }
}

fn render_translation_profile(
    writer: &mut impl Write,
    profile: &TranslationProfile,
) -> io::Result<()> {
    writeln!(
        writer,
        "{} | {} | {} | declared languages {} | {} threads | {} MiB | {} s per cue | profile sha256 {}.",
        profile.id,
        profile.engine,
        profile.template,
        profile.languages,
        profile.threads,
        profile.memory_bytes / (1024 * 1024),
        profile.cue_deadline_ms / 1000,
        profile.profile_sha256
    )?;
    writeln!(
        writer,
        "Runtime {} ({} files, sha256 {}). Model sha256 {} ({} bytes). Declared languages are not measured quality.",
        sanitize(&profile.runtime_dir, 1024),
        profile.runtime_files,
        profile.runtime_sha256,
        profile.model_sha256,
        profile.model_bytes
    )
}

fn render_translation_job(writer: &mut impl Write, job: &TranslationJob) -> io::Result<()> {
    writeln!(
        writer,
        "Translation {} generation {} attempt {}: {}.",
        job.request.id, job.generation, job.attempt, job.state
    )?;
    render_queue(writer, &job.state, "translation")?;
    writeln!(
        writer,
        "Transcript {} revision {} | profile {} | {} USD.",
        job.request.transcript_id,
        job.request.transcript_revision,
        job.request.profile,
        job.amount_usd
    )?;
    if let Some(reason) = &job.reason {
        writeln!(writer, "Reason {reason}.")?;
    }
    if job.state == "succeeded" {
        writeln!(
            writer,
            "Read it with: sigy analysis translation {}",
            job.request.transcript_id
        )?;
    }
    Ok(())
}

fn render_translation(writer: &mut impl Write, page: &TranslationPage) -> io::Result<()> {
    writeln!(
        writer,
        "Translation {} of transcript {} revision {} | profile {} | {} of {} cues translated | {} USD.",
        page.revision,
        page.transcript_id,
        page.transcript_revision,
        page.profile,
        page.translated_count,
        page.cue_count,
        page.amount_usd
    )?;
    writeln!(
        writer,
        "Machine translation into English, unreviewed. It can be wrong; the original is the evidence."
    )?;
    for pair in &page.pairs {
        writeln!(
            writer,
            "[{} - {}] {}",
            clock(pair.start_us),
            clock(pair.end_us),
            sanitize(&pair.original, 4096)
        )?;
        match (&pair.english, &pair.reason) {
            (Some(english), _) => writeln!(writer, "    en: {}", sanitize(english, 4096))?,
            (None, Some(reason)) => writeln!(writer, "    en: (untranslated: {reason})")?,
            (None, None) => writeln!(writer, "    en: (untranslated)")?,
        }
    }
    if let Some(next) = page.next_after_ordinal {
        writeln!(
            writer,
            "More cues: sigy analysis translation {} --transcript-revision {} --revision {} --after {next}",
            page.transcript_id, page.transcript_revision, page.revision
        )?;
    }
    Ok(())
}

fn render_profile(writer: &mut impl Write, profile: &RecognitionProfile) -> io::Result<()> {
    writeln!(
        writer,
        "{} | {} | {} threads | {} MiB | {} s deadline | profile sha256 {}.",
        profile.id,
        profile.engine,
        profile.threads,
        profile.memory_bytes / (1024 * 1024),
        profile.deadline_ms / 1000,
        profile.profile_sha256
    )?;
    writeln!(
        writer,
        "Runtime {} ({} files, sha256 {}). Model sha256 {} ({} bytes). Speech-activity model sha256 {}.",
        sanitize(&profile.runtime_dir, 1024),
        profile.runtime_files,
        profile.runtime_sha256,
        profile.model_sha256,
        profile.model_bytes,
        profile.vad_sha256
    )
}

fn render_recognition_job(writer: &mut impl Write, job: &LocalAsrJob) -> io::Result<()> {
    writeln!(
        writer,
        "Recognition {} generation {} attempt {}: {}.",
        job.request.id, job.generation, job.attempt, job.state
    )?;
    render_queue(writer, &job.state, "recognition")?;
    writeln!(
        writer,
        "Input {} revision {} | profile {} | follows transcript revision {} | {} USD.",
        job.request.analysis_id,
        job.request.analysis_revision,
        job.request.profile,
        job.request.parent_revision,
        job.amount_usd
    )?;
    if let Some(reason) = &job.reason {
        writeln!(writer, "Reason {reason}.")?;
    }
    if job.state == "succeeded" {
        writeln!(
            writer,
            "Read the text with: sigy analysis transcript {}",
            job.request.analysis_id
        )?;
    }
    Ok(())
}

/// A queued job waits for a free slot of its kind and for its transcript lineage.
fn render_queue(writer: &mut impl Write, state: &str, kind: &str) -> io::Result<()> {
    if state == "queued" {
        writeln!(
            writer,
            "Queued. It starts when a {kind} slot is free and no other job on the same transcript runs."
        )?;
    }
    Ok(())
}

fn clock(us: u64) -> String {
    let ms = us / 1000;
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

fn render_transcript(writer: &mut impl Write, page: &TranscriptCuePage) -> io::Result<()> {
    let summary = &page.transcript;
    writeln!(
        writer,
        "Transcript {} revision {} | {} {} | profile {} | {} cues | {} USD.",
        summary.id,
        summary.revision,
        summary.kind,
        summary.outcome,
        summary.profile,
        summary.cue_count,
        summary.amount_usd
    )?;
    if summary.kind == "legacy_placeholder" {
        writeln!(writer, "Legacy placeholder. No speech was recognized.")?;
    } else {
        writeln!(
            writer,
            "Machine-recognized text in the original script. Unreviewed; wording {}.",
            summary.wording
        )?;
    }
    if let Some(coverage) = &page.coverage {
        writeln!(
            writer,
            "Covers interval {}: {} to {} | {} samples at {} Hz.",
            coverage.interval_ordinal,
            clock(coverage.start_us),
            clock(coverage.end_us),
            coverage.sample_count,
            coverage.sample_rate
        )?;
    }
    if summary.outcome == "no_text" {
        writeln!(
            writer,
            "No speech text was recognized in the covered audio."
        )?;
    }
    for cue in &page.cues {
        writeln!(
            writer,
            "[{} - {}] {}",
            clock(cue.start_us),
            clock(cue.end_us),
            sanitize(&cue.script, 4096)
        )?;
    }
    if let Some(next) = page.next_after_ordinal {
        writeln!(
            writer,
            "More cues: sigy analysis transcript {} --revision {} --after {next}",
            summary.id, summary.revision
        )?;
    }
    Ok(())
}

pub fn render_job(
    writer: &mut impl Write,
    job: &sigy_service::control::AnalysisJob,
) -> io::Result<()> {
    writeln!(
        writer,
        "Verification {} generation {} attempt {}: {}.",
        job.id, job.generation, job.attempt, job.state
    )?;
    render_queue(writer, &job.state, "verification")?;
    writeln!(
        writer,
        "Input {} revision {} | {} bytes across {} files | {} USD.",
        job.analysis_id,
        job.analysis_revision,
        job.expected_bytes,
        job.expected_files,
        job.amount_usd
    )?;
    if let Some(reason) = &job.reason {
        writeln!(writer, "Reason {reason}.")?;
    }
    writeln!(writer, "No speech recognition or translation performed.")
}

pub fn render(writer: &mut impl Write, page: &AnalysisPage, ink: Ink) -> io::Result<()> {
    if let Some(languages) = &page.languages {
        return crate::languages::render(writer, languages);
    }
    let input = &page.input;
    let disposition = match page.disposition {
        Some(AnalysisDisposition::Created) => "Analysis pin created.",
        Some(AnalysisDisposition::Unchanged) => "Analysis pin unchanged.",
        Some(AnalysisDisposition::Replaced) => "Analysis worker replaced.",
        Some(AnalysisDisposition::Transcribed) => "Local transcript stored.",
        Some(AnalysisDisposition::TranscriptUnchanged) => "Local transcript unchanged.",
        None => "Analysis pin.",
    };
    writeln!(writer, "{disposition}")?;
    writeln!(
        writer,
        "{} revision {} {}. No source URL is attached.",
        input.id,
        input.revision,
        ink.tint(Tone::Warn, &input.state)
    )?;
    writeln!(
        writer,
        "Recording {} | sha256 {} | planned {} us.",
        input.recording_id, input.media_sha256, input.planned_us
    )?;
    for interval in &input.intervals {
        writeln!(
            writer,
            "Interval {}: {} to {} us | sha256 {}.",
            interval.ordinal, interval.start_us, interval.end_us, interval.sha256
        )?;
    }
    for gap in &input.gaps {
        writeln!(
            writer,
            "Gap {}: {} from {} to {} us. No audio file.",
            gap.ordinal, gap.cause, gap.start_us, gap.end_us
        )?;
    }
    if let (Some(transcript), Some(decision)) = (&page.transcript, &page.decision) {
        if transcript.profile == "local-unmeasured" {
            writeln!(writer, "Legacy placeholder. No speech was recognized.")?;
        }
        let kind = if transcript.role == "original" {
            "Original script."
        } else {
            "Transcript role is not original."
        };
        let request = if decision.paid_request {
            "Paid request recorded."
        } else {
            "No paid request."
        };
        writeln!(
            writer,
            "{kind} Profile {}. Decision {} USD. {request}",
            transcript.profile, decision.amount_usd
        )?;
        for cue in &transcript.cues {
            write!(
                writer,
                "Cue {}: {} to {} us. Wording {}.",
                cue.ordinal,
                cue.start_us,
                cue.end_us,
                ink.tint(Tone::Warn, &cue.wording)
            )?;
            if cue.script.is_empty() {
                writeln!(writer)?;
            } else {
                writeln!(writer, " {}", sanitize(&cue.script, 4096))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigy_service::control::{
        AnalysisDecisionView, AnalysisView, TranscriptCueView, TranscriptView,
    };

    #[test]
    fn uncertain_wording_is_labeled_and_no_paid_request_is_claimed()
    -> Result<(), Box<dyn std::error::Error>> {
        let page = AnalysisPage {
            languages: None,
            input: AnalysisView {
                id: "pin".into(),
                revision: 1,
                recording_id: "one".into(),
                media_sha256: "ab".repeat(32),
                state: "published".into(),
                planned_us: 1_000_000,
                intervals: Vec::new(),
                gaps: Vec::new(),
            },
            disposition: Some(AnalysisDisposition::Transcribed),
            transcript: Some(TranscriptView {
                revision: 1,
                role: "original".into(),
                profile: "local-unmeasured".into(),
                cues: vec![TranscriptCueView {
                    ordinal: 0,
                    start_us: 0,
                    end_us: 1_000_000,
                    script: String::new(),
                    wording: "uncertain".into(),
                }],
            }),
            decision: Some(AnalysisDecisionView {
                amount_usd: "0.000000".into(),
                paid_request: false,
            }),
        };
        let mut buffer = Vec::new();
        render(&mut buffer, &page, Ink::stdout(true))?;
        let text = String::from_utf8(buffer)?;
        assert!(text.contains("Local transcript stored."));
        assert!(text.contains("Original script."));
        assert!(text.contains("Decision 0.000000 USD."));
        assert!(text.contains("No paid request."));
        assert!(text.contains("Wording uncertain."));
        assert!(!text.contains("example.com"));
        Ok(())
    }

    #[test]
    fn transcript_display_strips_terminal_controls_without_changing_stored_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = "alpha\u{1b}[31m\nbeta\u{202e}gamma";
        let page = AnalysisPage {
            languages: None,
            input: AnalysisView {
                id: "pin".into(),
                revision: 1,
                recording_id: "one".into(),
                media_sha256: "ab".repeat(32),
                state: "published".into(),
                planned_us: 1_000_000,
                intervals: Vec::new(),
                gaps: Vec::new(),
            },
            disposition: None,
            transcript: Some(TranscriptView {
                revision: 1,
                role: "original".into(),
                profile: "local-unmeasured".into(),
                cues: vec![TranscriptCueView {
                    ordinal: 0,
                    start_us: 0,
                    end_us: 1_000_000,
                    script: script.into(),
                    wording: "uncertain".into(),
                }],
            }),
            decision: Some(AnalysisDecisionView {
                amount_usd: "0.000000".into(),
                paid_request: false,
            }),
        };
        let mut buffer = Vec::new();
        render(&mut buffer, &page, Ink::stdout(false))?;
        let output = String::from_utf8(buffer)?;
        assert!(output.contains("alpha[31mbetagamma"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\u{202e}'));
        assert!(!output.contains("alpha[31m\nbeta"));
        assert_eq!(
            page.transcript
                .as_ref()
                .map(|row| row.cues[0].script.as_str()),
            Some(script)
        );
        Ok(())
    }
}
