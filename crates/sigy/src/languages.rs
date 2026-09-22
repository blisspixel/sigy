use std::io::{self, Write};

use clap::Subcommand;
use sigy_service::{
    control::LanguageOperation,
    storage::languages::{LanguagePage, LanguageSummary},
};

use crate::explorer::text::sanitize;

#[derive(Debug, Subcommand)]
pub enum LanguageCommand {
    /// List the latest evidence revision for each track on this exact analysis revision.
    List {
        id: String,
        #[arg(long)]
        revision: i64,
        #[arg(long)]
        after: Option<String>,
    },
    /// Show up to 16 spans from one immutable evidence revision.
    Show {
        id: String,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        after: Option<u32>,
    },
}

impl LanguageCommand {
    pub fn operation(&self) -> LanguageOperation {
        match self {
            Self::List {
                id,
                revision,
                after,
            } => LanguageOperation::List {
                id: id.clone(),
                revision: *revision,
                after: after.clone(),
            },
            Self::Show {
                id,
                revision,
                after,
            } => LanguageOperation::Show {
                id: id.clone(),
                revision: *revision,
                after: *after,
            },
        }
    }
}

pub fn render(writer: &mut impl Write, page: &LanguagePage) -> io::Result<()> {
    match page {
        LanguagePage::List {
            records,
            next_after,
        } => {
            if records.is_empty() {
                writeln!(writer, "No stored language evidence on this page.")?;
            }
            for record in records {
                summary(writer, record)?;
            }
            if let Some(after) = next_after {
                writeln!(writer, "More evidence: --after {}", sanitize(after, 128))?;
            }
        }
        LanguagePage::Evidence {
            summary: record,
            spans,
            next_after,
        } => {
            summary(writer, record)?;
            for span in spans {
                write!(
                    writer,
                    "Span {}: {} to {} us | {}",
                    span.ordinal,
                    span.start_us,
                    span.end_us,
                    sanitize(&span.observation, 32)
                )?;
                for label in &span.languages {
                    write!(
                        writer,
                        " | {} (provider: {})",
                        sanitize(&label.tag, 128),
                        sanitize(&label.provider_label, 128)
                    )?;
                }
                writeln!(
                    writer,
                    " | {} route {} ({}; basis {}).",
                    sanitize(&span.route.task, 32),
                    sanitize(&span.route.capability, 32),
                    sanitize(&span.route.profile, 128),
                    sanitize(&span.route.basis, 32)
                )?;
            }
            if let Some(after) = next_after {
                writeln!(writer, "More spans: --after {after}")?;
            }
        }
    }
    Ok(())
}

fn summary(writer: &mut impl Write, value: &LanguageSummary) -> io::Result<()> {
    writeln!(
        writer,
        "{} revision {} | input {} revision {} | {} | {} spans.",
        sanitize(&value.id, 128),
        value.revision,
        sanitize(&value.analysis_id, 128),
        value.analysis_revision,
        sanitize(&value.outcome, 32),
        value.span_count
    )?;
    writeln!(
        writer,
        "Origin {} | resolution {} | profile {} | sha256 {} | alias map {}.",
        sanitize(&value.method.origin, 32),
        sanitize(&value.method.resolution, 32),
        sanitize(&value.method.profile, 128),
        sanitize(&value.method.profile_sha256, 64),
        sanitize(&value.method.alias_map, 128)
    )?;
    if let Some(reason) = &value.reason {
        writeln!(writer, "Reason {}.", sanitize(reason, 128))?;
    }
    if let Some(transcript) = &value.transcript {
        writeln!(
            writer,
            "Transcript {} revision {}.",
            sanitize(&transcript.id, 128),
            transcript.revision
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigy_service::languages::{LanguageLabel, LanguageMethod, LanguageRoute, LanguageSpan};

    #[test]
    fn commands_require_exact_revisions_and_expose_only_reads()
    -> Result<(), Box<dyn std::error::Error>> {
        use clap::Parser;
        let cli = crate::Cli::try_parse_from([
            "sigy",
            "analysis",
            "languages",
            "show",
            "lid",
            "--revision",
            "2",
            "--after",
            "15",
        ])?;
        let crate::Command::Analysis { command } = cli.command else {
            return Err("wrong command".into());
        };
        assert!(matches!(
            command.operation(),
            sigy_service::control::Operation::Analysis {
                command: sigy_service::control::AnalysisOperation::Languages {
                    command: LanguageOperation::Show {
                        revision: 2,
                        after: Some(15),
                        ..
                    }
                }
            }
        ));
        assert!(
            crate::Cli::try_parse_from(["sigy", "analysis", "languages", "show", "lid"]).is_err()
        );
        assert!(
            crate::Cli::try_parse_from(["sigy", "analysis", "languages", "publish", "lid"])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn mixed_original_labels_survive_without_terminal_controls()
    -> Result<(), Box<dyn std::error::Error>> {
        let page = LanguagePage::Evidence {
            summary: Box::new(LanguageSummary {
                id: "evidence".into(),
                revision: 1,
                analysis_id: "pin".into(),
                analysis_revision: 1,
                transcript: None,
                method: LanguageMethod {
                    origin: "acoustic".into(),
                    profile: "fixture".into(),
                    profile_sha256: "a".repeat(64),
                    resolution: "block".into(),
                    alias_map: "fixture-v1".into(),
                },
                outcome: "succeeded".into(),
                reason: None,
                span_count: 1,
            }),
            spans: vec![LanguageSpan {
                ordinal: 0,
                interval_ordinal: 0,
                start_us: 0,
                end_us: 100,
                cue_ordinal: None,
                observation: "mixed".into(),
                languages: vec![
                    LanguageLabel {
                        tag: "ar".into(),
                        provider_label: "العربية\u{1b}[2J".into(),
                    },
                    LanguageLabel {
                        tag: "zh".into(),
                        provider_label: "中文\u{202e}".into(),
                    },
                ],
                route: LanguageRoute {
                    task: "translation_en".into(),
                    capability: "unsupported".into(),
                    profile: "fixture-route".into(),
                    profile_sha256: "b".repeat(64),
                    basis: "declared".into(),
                    basis_sha256: "c".repeat(64),
                },
            }],
            next_after: None,
        };
        let mut buffer = Vec::new();
        render(&mut buffer, &page)?;
        let text = String::from_utf8(buffer)?;
        assert!(text.contains("mixed") && text.contains("العربية") && text.contains("中文"));
        assert!(text.contains("translation_en route unsupported"));
        assert!(!text.contains('\u{1b}') && !text.contains('\u{202e}'));
        assert!(!text.contains("confidence") && !text.contains("human"));
        Ok(())
    }
}
