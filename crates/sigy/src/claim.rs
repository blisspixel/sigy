//! `sigy claim`: a claim you write, linked to a stored finding or one retained cue.

use std::io::{self, Write};

use clap::{Subcommand, ValueEnum};
use sigy_service::{
    claim::{
        ClaimBasis, ClaimCandidatePage, ClaimCarrier, ClaimEvidenceKind, ClaimEvidenceLink,
        ClaimRelation, ClaimSighting, ClaimStatus, ClaimTimeline, ClaimView,
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
    /// Link one stored finding, or one retained cue. Pass one citation, not both.
    Sighting {
        claim: String,
        sighting: String,
        /// Monitor that owns the finding.
        #[arg(long)]
        monitor: Option<String>,
        /// Stored finding id.
        #[arg(long)]
        finding: Option<String>,
        /// Transcript that holds the cue.
        #[arg(long)]
        transcript: Option<String>,
        /// Transcript revision, from 1 to 64.
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..=64))]
        revision: Option<i64>,
        /// Translation revision, from 1 to 64.
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..=64))]
        translation: Option<i64>,
        /// Cue ordinal, from 0 to 255.
        #[arg(long, value_parser = clap::value_parser!(u32).range(0..=255))]
        cue: Option<u32>,
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
    /// List stored findings that match an analysis search. This writes nothing.
    Candidates {
        #[command(flatten)]
        search: crate::analysis::search::SearchArgs,
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
    ///
    /// # Errors
    /// A candidate search fails when its cursor cannot be read. The service checks the other bounds.
    pub fn operation(&self) -> Result<Operation, Box<dyn std::error::Error>> {
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
                transcript,
                revision,
                translation,
                cue,
            } => match (
                monitor.as_deref(),
                finding.as_deref(),
                transcript.as_deref(),
                *revision,
                *translation,
                *cue,
            ) {
                (Some(monitor), Some(finding), None, None, None, None) => {
                    ClaimOperation::Sighting {
                        claim: claim.clone(),
                        sighting: sighting.clone(),
                        monitor: monitor.to_owned(),
                        finding: finding.to_owned(),
                    }
                }
                (None, None, Some(transcript), Some(revision), Some(translation), Some(cue)) => {
                    ClaimOperation::Passage {
                        claim: claim.clone(),
                        sighting: sighting.clone(),
                        transcript_id: transcript.to_owned(),
                        transcript_revision: revision,
                        translation_revision: translation,
                        cue_ordinal: cue,
                    }
                }
                _ => {
                    return Err(
                        "A sighting cites one stored finding or one retained cue. Pass --monitor and --finding, or --transcript, --revision, --translation, and --cue."
                            .into(),
                    );
                }
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
            Self::Candidates { search } => ClaimOperation::Candidates {
                query: search.query()?,
            },
        };
        Ok(Operation::Claim { command })
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
        ClaimPage::Candidates { page } => render_candidates(writer, page),
    }
}

fn render_candidates(writer: &mut impl Write, page: &ClaimCandidatePage) -> io::Result<()> {
    writeln!(
        writer,
        "Candidates: {} of {} hits. This search writes nothing.",
        page.candidates.len(),
        page.hits_read
    )?;
    if page.candidates.is_empty() {
        writeln!(
            writer,
            "No stored finding matches this search. A passage that is not a finding is not a candidate."
        )?;
        return Ok(());
    }
    for candidate in &page.candidates {
        let transcript = if candidate.stale_transcript {
            "transcript stale"
        } else {
            "transcript current"
        };
        let translation = if candidate.stale_translation {
            "translation stale"
        } else {
            "translation current"
        };
        writeln!(
            writer,
            "{} revision {} translation {} cue {} | {} | recording {} | {transcript}; {translation}",
            sanitize(&candidate.transcript_id, 128),
            candidate.transcript_revision,
            candidate.translation_revision,
            candidate.cue_ordinal,
            sanitize(&candidate.source, 128),
            sanitize(&candidate.recording_id, 128)
        )?;
        writeln!(writer, "  {}", sanitize(&candidate.original, 2_000))?;
        if let Some(english) = &candidate.english {
            writeln!(writer, "  English: {}", sanitize(english, 2_000))?;
        }
        for finding in &candidate.findings {
            writeln!(
                writer,
                "  finding {} monitor {} {}",
                sanitize(&finding.finding_id, 128),
                sanitize(&finding.monitor_id, 128),
                sanitize(&finding.original_state, 32)
            )?;
            writeln!(
                writer,
                "Accept with: sigy claim sighting CLAIM SIGHTING --monitor {} --finding {}",
                sanitize(&finding.monitor_id, 128),
                sanitize(&finding.finding_id, 128)
            )?;
        }
        if candidate.more_findings {
            writeln!(writer, "  more findings exist")?;
        }
    }
    Ok(())
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
    render_basis(writer, &claim.basis)?;
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

fn render_basis(writer: &mut impl Write, basis: &ClaimBasis) -> io::Result<()> {
    match basis.state.as_str() {
        "none" => writeln!(writer, "Basis: none"),
        "current" => writeln!(writer, "Basis: current"),
        "review" => {
            let labels = basis
                .reasons
                .iter()
                .map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                writer,
                "Basis: review ({labels}) | evidence {} | sighting {} | finding {}",
                sanitize(basis.evidence_id.as_deref().unwrap_or("none"), 128),
                sanitize(basis.sighting_id.as_deref().unwrap_or("none"), 128),
                sanitize(basis.finding_id.as_deref().unwrap_or("none"), 128)
            )
        }
        other => writeln!(writer, "Basis: {}", sanitize(other, 32)),
    }
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
        "  {} {} | {} | {} | {} | {transcript}; {translation}",
        sanitize(&sighting.id, 128),
        citation(sighting),
        sanitize(&sighting.time_basis, 80),
        sanitize(&sighting.source_revision, 128),
        interval
    )
}

fn citation(sighting: &ClaimSighting) -> String {
    match (
        sighting.monitor_id.as_deref(),
        sighting.finding_id.as_deref(),
        sighting.transcript_id.as_deref(),
        sighting.transcript_revision,
        sighting.translation_revision,
        sighting.cue_ordinal,
    ) {
        (Some(monitor), Some(finding), None, None, None, None) => format!(
            "monitor {} finding {}",
            sanitize(monitor, 128),
            sanitize(finding, 128)
        ),
        (None, None, Some(transcript), Some(revision), Some(translation), Some(cue)) => format!(
            "transcript {transcript} revision {revision} translation {translation} cue {cue}",
            transcript = sanitize(transcript, 128)
        ),
        _ => "unreadable citation".to_owned(),
    }
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

#[cfg(test)]
mod tests {
    use sigy_service::claim::ClaimBasisReason;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn view(status: &str, evidence: Option<&str>, basis: ClaimBasis) -> ClaimView {
        ClaimView {
            id: "dam".into(),
            created_ms: 1,
            text_revision: 1,
            text: "Several stations carried one bulletin.".into(),
            revisions: Vec::new(),
            timeline: ClaimTimeline {
                searched: ClaimTimeline::SEARCHED.into(),
                library_search: false,
                languages: ClaimTimeline::LANGUAGES.into(),
                window: "none".into(),
                gaps: ClaimTimeline::GAPS.into(),
            },
            sightings: Vec::new(),
            evidence: Vec::new(),
            status: status.into(),
            status_label: status.into(),
            status_revision: i64::from(evidence.is_some()),
            status_setter: evidence.map(|_| "user".to_owned()),
            status_evidence: evidence.map(str::to_owned),
            basis,
            carriers: Vec::new(),
        }
    }

    fn rendered(claim: ClaimView) -> Result<String, Box<dyn std::error::Error>> {
        let mut output = Vec::new();
        render(
            &mut output,
            &ClaimPage::Claim {
                claim: Box::new(claim),
            },
        )?;
        Ok(String::from_utf8(output)?)
    }

    #[test]
    fn the_page_prints_the_stored_status_and_the_basis_warning() -> TestResult {
        let none = rendered(view("unverified", None, ClaimBasis::unverified()))?;
        assert!(none.contains("Status: unverified (unverified, revision 0, setter none)"));
        assert!(none.contains("Basis: none\n"));
        assert!(
            none.find("Basis: none").unwrap_or(usize::MAX) < none.find("Timeline").unwrap_or(0)
        );

        let current = ClaimBasis {
            state: "current".into(),
            evidence_id: Some("note".into()),
            sighting_id: Some("see".into()),
            finding_id: Some("world".into()),
            reasons: Vec::new(),
        };
        let kept = rendered(view("corroborated", Some("note"), current))?;
        assert!(kept.contains("Status: corroborated (corroborated, revision 1, setter user)"));
        assert!(kept.contains("Status evidence: note"));
        assert!(kept.contains("Basis: current\n"));

        let review = ClaimBasis {
            state: "review".into(),
            evidence_id: Some("note".into()),
            sighting_id: Some("see".into()),
            finding_id: Some("world".into()),
            reasons: vec![
                ClaimBasisReason::Correction,
                ClaimBasisReason::RetentionExpired,
            ],
        };
        let warned = rendered(view("corroborated", Some("note"), review))?;
        assert!(warned.contains("Status: corroborated (corroborated, revision 1, setter user)"));
        assert!(warned.contains(
            "Basis: review (correction, retention-expired) | evidence note | sighting see | finding world"
        ));
        Ok(())
    }

    #[test]
    fn candidates_name_the_sighting_command_and_insert_nothing() -> TestResult {
        use sigy_service::archive::ArchiveQuery;
        use sigy_service::claim::{ClaimCandidate, ClaimCandidateFinding};
        let page = ClaimCandidatePage {
            query: ArchiveQuery::new("feria").validate()?,
            hits_read: 2,
            stopped: None,
            candidates: vec![ClaimCandidate {
                source: "radio:v1".into(),
                recording_id: "one".into(),
                transcript_id: "pin".into(),
                transcript_revision: 1,
                translation_revision: 1,
                cue_ordinal: 0,
                start_us: 0,
                end_us: 1_000_000,
                original: "Una feria mundial".into(),
                english: Some("A world fair".into()),
                stale_transcript: false,
                stale_translation: true,
                findings: vec![ClaimCandidateFinding {
                    monitor_id: "fair".into(),
                    finding_id: "world".into(),
                    original_state: "retained".into(),
                }],
                more_findings: false,
            }],
        };
        let mut output = Vec::new();
        render(
            &mut output,
            &ClaimPage::Candidates {
                page: Box::new(page),
            },
        )?;
        let text = String::from_utf8(output)?;
        assert!(text.contains("Candidates: 1 of 2 hits. This search writes nothing."));
        assert!(text.contains("translation stale"));
        assert!(text.contains(
            "Accept with: sigy claim sighting CLAIM SIGHTING --monitor fair --finding world"
        ));
        assert!(!text.contains("score"));
        Ok(())
    }

    #[test]
    fn sighting_operation_accepts_one_complete_citation() -> TestResult {
        let finding = ClaimCommand::Sighting {
            claim: "dam".into(),
            sighting: "see".into(),
            monitor: Some("fair".into()),
            finding: Some("world".into()),
            transcript: None,
            revision: None,
            translation: None,
            cue: None,
        };
        assert!(matches!(
            finding.operation()?,
            Operation::Claim {
                command: ClaimOperation::Sighting { .. }
            }
        ));
        let cue = ClaimCommand::Sighting {
            claim: "dam".into(),
            sighting: "see".into(),
            monitor: None,
            finding: None,
            transcript: Some("pin".into()),
            revision: Some(1),
            translation: Some(1),
            cue: Some(0),
        };
        match cue.operation()? {
            Operation::Claim {
                command:
                    ClaimOperation::Passage {
                        transcript_id,
                        cue_ordinal,
                        ..
                    },
            } => {
                assert_eq!(transcript_id, "pin");
                assert_eq!(cue_ordinal, 0);
            }
            _ => return Err("passage operation was not built".into()),
        }
        let mixed = ClaimCommand::Sighting {
            claim: "dam".into(),
            sighting: "see".into(),
            monitor: Some("fair".into()),
            finding: Some("world".into()),
            transcript: Some("pin".into()),
            revision: Some(1),
            translation: Some(1),
            cue: Some(0),
        };
        let Err(error) = mixed.operation() else {
            return Err("a mixed citation was accepted".into());
        };
        assert!(error.to_string().contains("Pass --monitor and --finding"));
        Ok(())
    }

    #[test]
    fn a_passage_line_names_the_cue() -> TestResult {
        let mut claim = view("unverified", None, ClaimBasis::unverified());
        claim.sightings.push(ClaimSighting {
            id: "see".into(),
            monitor_id: None,
            finding_id: None,
            recording_id: "one".into(),
            source_revision: "radio:v1".into(),
            source_name: None,
            original: sigy_service::monitor::FindingOriginal::Retained,
            start_us: Some(0),
            end_us: Some(1_000_000),
            time_basis: "recording media clock".into(),
            stale_transcript: None,
            stale_translation: None,
            transcript_id: Some("pin".into()),
            transcript_revision: Some(1),
            translation_revision: Some(1),
            cue_ordinal: Some(0),
        });
        let text = rendered(claim.clone())?;
        assert!(text.contains(
            "see transcript pin revision 1 translation 1 cue 0 | recording media clock | radio:v1 | 0 us to 1000000 us | transcript current; translation current"
        ));
        assert!(!text.contains("monitor "));
        claim.sightings[0].monitor_id = Some("fair".into());
        let mixed = rendered(claim)?;
        assert!(mixed.contains("unreadable citation"));
        Ok(())
    }
}
