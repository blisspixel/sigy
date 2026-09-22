use crate::style::{Ink, Tone};
use clap::Subcommand;
use sigy_service::control::{AnalysisDisposition, AnalysisOperation, AnalysisPage, Operation};
use std::io::{self, Write};

#[derive(Debug, Subcommand)]
pub enum AnalysisCommand {
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
}

impl AnalysisCommand {
    #[must_use]
    pub fn operation(&self) -> Operation {
        match self {
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
        }
        .into()
    }
}

pub fn render(writer: &mut impl Write, page: &AnalysisPage, ink: Ink) -> io::Result<()> {
    let input = &page.input;
    let disposition = match page.disposition {
        Some(AnalysisDisposition::Created) => "Analysis pin created.",
        Some(AnalysisDisposition::Unchanged) => "Analysis pin unchanged.",
        Some(AnalysisDisposition::Replaced) => "Analysis worker replaced.",
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
    Ok(())
}
