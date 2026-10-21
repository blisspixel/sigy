//! `sigy claim`: a claim you write, linked to a finding that is already stored.

use std::io::{self, Write};

use clap::{Subcommand, ValueEnum};
use sigy_service::{
    claim::{
        ClaimCarrier, ClaimEvidenceKind, ClaimEvidenceLink, ClaimRelation, ClaimSighting,
        ClaimStatus, ClaimTimeline, ClaimView,
    },
    control::{ClaimOperation, ClaimPage, Operation},
};

use crate::explorer::text::sanitize;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EvidenceChoice {
    Supports,
    Disputes,
    Context,
    Unresolved,
}

impl From<EvidenceChoice> for ClaimEvidenceKind {
    fn from(choice: EvidenceChoice) -> Self {
        match choice {
            EvidenceChoice::Supports => Self::Supports,
            EvidenceChoice::Disputes => Self::Disputes,
            EvidenceChoice::Context => Self::Context,
            EvidenceChoice::Unresolved => Self::Unresolved,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RelationChoice {
    Quotation,
    Negation,
    Sarcasm,
    #[value(name = "changed-date")]
    ChangedDate,
    Translation,
    #[value(name = "shared-press-release")]
    SharedPressRelease,
    Correction,
}

impl From<RelationChoice> for ClaimRelation {
    fn from(choice: RelationChoice) -> Self {
        match choice {
            RelationChoice::Quotation => Self::Quotation,
            RelationChoice::Negation => Self::Negation,
            RelationChoice::Sarcasm => Self::Sarcasm,
            RelationChoice::ChangedDate => Self::ChangedDate,
            RelationChoice::Translation => Self::Translation,
            RelationChoice::SharedPressRelease => Self::SharedPressRelease,
            RelationChoice::Correction => Self::Correction,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum StatusChoice {
    Disputed,
    Corroborated,
    Retracted,
}

impl From<StatusChoice> for ClaimStatus {
    fn from(choice: StatusChoice) -> Self {
        match choice {
            StatusChoice::Disputed => Self::Disputed,
            StatusChoice::Corroborated => Self::Corroborated,
            StatusChoice::Retracted => Self::Retracted,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ClaimCommand {
    /// Store one claim in your own words. Repeating the same first text changes nothing.
    Create {
        id: String,
        /// The claim, from 1 to 2000 characters.
        #[arg(long)]
        text: String,
    },
    /// Append a wording. Pass the text revision you last saw.
    Revise {
        id: String,
        /// The text revision you last saw, from 1 to 64.
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..=64))]
        expected_revision: i64,
        /// The new wording, from 1 to 2000 characters.
        #[arg(long)]
        text: String,
    },
    /// List claim ids.
    List,
    /// Read one claim. The timeline is printed before any sighting.
    Show { id: String },
    /// Link one stored finding. A passage that is not a finding is refused.
    Sighting {
        claim: String,
        sighting: String,
        /// Monitor that owns the finding.
        #[arg(long)]
        monitor: String,
        /// Stored finding id.
        #[arg(long)]
        finding: String,
    },
    /// Record what you say a sighting does. This does not set a status.
    Evidence {
        claim: String,
        evidence: String,
        /// Sighting on this claim.
        #[arg(long)]
        sighting: String,
        /// supports, disputes, context, or unresolved.
        #[arg(long = "type", value_enum)]
        kind: EvidenceChoice,
        /// Your note, from 1 to 500 characters.
        #[arg(long)]
        note: String,
        /// A relation you choose. Omitting it leaves the relation unspecified.
        #[arg(long, value_enum)]
        relation: Option<RelationChoice>,
    },
    /// Set disputed, corroborated, or retracted. Cite one evidence link you already stored.
    Status {
        claim: String,
        /// How many status rows you last saw. Zero means the claim is unverified.
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..=32))]
        expected_revision: i64,
        /// disputed, corroborated, or retracted.
        #[arg(long, value_enum)]
        status: StatusChoice,
        /// Evidence id on this claim.
        #[arg(long)]
        evidence: String,
    },
}

impl ClaimCommand {
    /// The service command for this claim request.
    #[must_use]
    pub fn operation(&self) -> Operation {
        let command = match self {
            Self::Create { id, text } => ClaimOperation::Create {
                id: id.clone(),
                text: text.clone(),
            },
            Self::Revise {
                id,
                expected_revision,
                text,
            } => ClaimOperation::Revise {
                id: id.clone(),
                expected_revision: *expected_revision,
                text: text.clone(),
            },
            Self::List => ClaimOperation::List {},
            Self::Show { id } => ClaimOperation::Show { id: id.clone() },
            Self::Sighting {
                claim,
                sighting,
                monitor,
                finding,
            } => ClaimOperation::Sighting {
                claim: claim.clone(),
                sighting: sighting.clone(),
                monitor: monitor.clone(),
                finding: finding.clone(),
            },
            Self::Evidence {
                claim,
                evidence,
                sighting,
                kind,
                note,
                relation,
            } => ClaimOperation::Evidence {
                claim: claim.clone(),
                evidence: evidence.clone(),
                sighting: sighting.clone(),
                kind: ClaimEvidenceKind::from(*kind),
                note: note.clone(),
                relation: relation.map(ClaimRelation::from),
            },
            Self::Status {
                claim,
                expected_revision,
                status,
                evidence,
            } => ClaimOperation::Status {
                claim: claim.clone(),
                expected_revision: *expected_revision,
                status: ClaimStatus::from(*status),
                evidence: evidence.clone(),
            },
        };
        Operation::Claim { command }
    }
}

/// Print one claim page. The timeline block is written before the first sighting.
///
/// # Errors
/// Returns an error when the output cannot be written.
pub fn render(writer: &mut impl Write, page: &ClaimPage) -> io::Result<()> {
    match page {
        ClaimPage::List { ids } => {
            if ids.is_empty() {
                writeln!(
                    writer,
                    "No claims. Create one with sigy claim create ID --text TEXT."
                )?;
            }
            for id in ids {
                writeln!(writer, "{id}")?;
            }
            Ok(())
        }
        ClaimPage::Claim { claim } => render_claim(writer, claim),
    }
}

fn render_claim(writer: &mut impl Write, claim: &ClaimView) -> io::Result<()> {
    writeln!(writer, "Claim {}", sanitize(&claim.id, 128))?;
    writeln!(
        writer,
        "Status: {} ({}, revision {}, setter {})",
        sanitize(&claim.status_label, 80),
        sanitize(&claim.status, 32),
        claim.status_revision,
        claim.status_setter.as_deref().unwrap_or("none")
    )?;
    if let Some(evidence) = &claim.status_evidence {
        writeln!(writer, "Status evidence: {}", sanitize(evidence, 128))?;
    }
    render_timeline(writer, &claim.timeline)?;
    writeln!(
        writer,
        "Text revision {}: {}",
        claim.text_revision,
        sanitize(&claim.text, 2_000)
    )?;
    writeln!(writer, "History")?;
    for revision in &claim.revisions {
        writeln!(
            writer,
            "  {}: {}",
            revision.revision,
            sanitize(&revision.text, 2_000)
        )?;
    }
    writeln!(writer, "Sightings")?;
    if claim.sightings.is_empty() {
        writeln!(writer, "  none")?;
    }
    for sighting in &claim.sightings {
        render_sighting(writer, sighting)?;
    }
    writeln!(writer, "Evidence")?;
    if claim.evidence.is_empty() {
        writeln!(writer, "  none")?;
    }
    for evidence in &claim.evidence {
        render_evidence(writer, evidence)?;
    }
    writeln!(writer, "Carriers")?;
    if claim.carriers.is_empty() {
        writeln!(writer, "  none")?;
    }
    for carrier in &claim.carriers {
        render_carrier(writer, carrier)?;
    }
    Ok(())
}

fn render_timeline(writer: &mut impl Write, timeline: &ClaimTimeline) -> io::Result<()> {
    writeln!(writer, "Timeline")?;
    writeln!(writer, "Searched: {}", sanitize(&timeline.searched, 80))?;
    writeln!(
        writer,
        "Library search: {}",
        if timeline.library_search { "yes" } else { "no" }
    )?;
    writeln!(writer, "Languages: {}", sanitize(&timeline.languages, 120))?;
    writeln!(writer, "Window: {}", sanitize(&timeline.window, 160))?;
    writeln!(writer, "Gaps: {}", sanitize(&timeline.gaps, 200))?;
    Ok(())
}

fn render_sighting(writer: &mut impl Write, sighting: &ClaimSighting) -> io::Result<()> {
    let interval = match (sighting.start_us, sighting.end_us) {
        (Some(start), Some(end)) => format!("{start} us to {end} us"),
        _ => "no interval".to_owned(),
    };
    let transcript = if sighting.stale_transcript == Some(true) {
        "transcript stale"
    } else {
        "transcript current"
    };
    let translation = if sighting.stale_translation == Some(true) {
        "translation stale"
    } else {
        "translation current"
    };
    writeln!(
        writer,
        "  {} monitor {} finding {} | {} | {} | {} | {transcript}; {translation}",
        sanitize(&sighting.id, 128),
        sanitize(&sighting.monitor_id, 128),
        sanitize(&sighting.finding_id, 128),
        sanitize(&sighting.time_basis, 80),
        sanitize(&sighting.source_revision, 128),
        interval
    )
}

fn render_evidence(writer: &mut impl Write, evidence: &ClaimEvidenceLink) -> io::Result<()> {
    writeln!(
        writer,
        "  {} sighting {} {} {} {} | {}",
        sanitize(&evidence.id, 128),
        sanitize(&evidence.sighting_id, 128),
        evidence.kind.as_str(),
        sanitize(&evidence.relation, 40),
        sanitize(&evidence.origin, 16),
        sanitize(&evidence.note, 500)
    )
}

fn render_carrier(writer: &mut impl Write, carrier: &ClaimCarrier) -> io::Result<()> {
    writeln!(
        writer,
        "  {} | sightings {} | repetition {} | independence {}",
        sanitize(&carrier.source_revision, 128),
        carrier
            .sighting_ids
            .iter()
            .map(|id| sanitize(id, 128))
            .collect::<Vec<_>>()
            .join(", "),
        sanitize(&carrier.repetition_group, 32),
        sanitize(&carrier.independence, 32)
    )
}
