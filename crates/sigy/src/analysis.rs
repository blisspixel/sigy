use crate::explorer::text::sanitize;
use crate::style::{Ink, Tone};
use clap::Subcommand;
use sigy_service::control::{AnalysisDisposition, AnalysisOperation, AnalysisPage, Operation};
use std::io::{self, Write};

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
    /// Transcribe a published pin. Unavailable until a measured recognizer is configured.
    Transcribe {
        id: String,
        /// Published analysis revision. An older revision is refused.
        #[arg(long)]
        revision: i64,
    },
}

impl AnalysisCommand {
    #[must_use]
    pub fn operation(&self) -> Operation {
        match self {
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
            Self::Transcribe { id, revision } => AnalysisOperation::Transcribe {
                id: id.clone(),
                revision: *revision,
            },
        }
        .into()
    }
}

pub fn render_job(
    writer: &mut impl Write,
    job: &sigy_service::control::AnalysisJob,
) -> io::Result<()> {
    writeln!(
        writer,
        "Verification {} generation {}: {}.",
        job.id, job.generation, job.state
    )?;
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
