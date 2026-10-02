use crate::explorer::text::bytes_label;
use crate::style::{Ink, Tone, tone_for_state};
use clap::Subcommand;
use sigy_service::{
    control::{DvrOperation, Operation, RecordingOperation, RecordingPage},
    storage::dvr::{DvrStatus, Retention},
};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Subcommand)]
pub enum DvrCommand {
    /// Show the rolling storage policy and reserved/retained bytes.
    Status,
    /// Set the storage policy and an installed `FFmpeg` decoder. No downloads.
    Configure {
        #[arg(long, default_value_t = 50)]
        quota_gb: u64,
        #[arg(long, default_value_t = 14)]
        retention_days: u32,
        #[arg(long, default_value_t = 256)]
        minimum_free_mib: u64,
        /// Absolute path to a trusted local ffmpeg executable.
        #[arg(long)]
        decoder: PathBuf,
    },
    /// Reclaim expired or explicitly processed temporary media. Protect kept/archive.
    Prune,
}

impl DvrCommand {
    pub fn operation(&self) -> Result<Operation, Box<dyn std::error::Error>> {
        Ok(Operation::Dvr {
            command: match self {
                Self::Status => DvrOperation::Status {},
                Self::Prune => DvrOperation::Prune {},
                Self::Configure {
                    quota_gb,
                    retention_days,
                    minimum_free_mib,
                    decoder,
                } => DvrOperation::Configure {
                    quota_bytes: quota_gb
                        .checked_mul(1_000_000_000)
                        .ok_or("quota is too large")?,
                    minimum_free_bytes: minimum_free_mib
                        .checked_mul(1024 * 1024)
                        .ok_or("free-space floor is too large")?,
                    retention_days: *retention_days,
                    decoder: decoder_path(decoder)?,
                },
            },
        })
    }
}

/// Resolves the user's decoder argument. Sigy never searches `PATH` for it.
fn decoder_path(decoder: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let shown = clean(&decoder.display().to_string());
    let resolved = decoder.canonicalize().map_err(|error| {
        let problem = if error.kind() == io::ErrorKind::NotFound {
            "does not exist".to_owned()
        } else {
            format!("cannot be read ({error})")
        };
        format!(
            "decoder {shown} {problem}. Pass the absolute path of an installed ffmpeg executable; `where ffmpeg` on Windows or `command -v ffmpeg` elsewhere prints it"
        )
    })?;
    Ok(resolved
        .to_str()
        .ok_or("decoder path is not valid Unicode")?
        .into())
}

#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    /// Record one HLS media playlist through the service. Master playlists fail.
    Hls {
        id: String,
        #[arg(long)]
        source: String,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        #[arg(long, default_value_t = 64)]
        max_mib: u64,
        #[arg(long, default_value = "temporary")]
        retention: Retention,
        /// Reload a live playlist until the time or byte ceiling. A skipped sequence,
        /// discontinuity, or failed reload ends the capture and records a gap.
        #[arg(long, default_value_t = false)]
        live: bool,
    },
    /// Start one finite recording owned by the service. Reuse the ID to reconcile.
    Start {
        id: String,
        #[arg(long)]
        source: String,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        #[arg(long, default_value_t = 64)]
        max_mib: u64,
        #[arg(long, default_value = "temporary")]
        retention: Retention,
        /// Request interleaved ICY metadata. Off by default. Titles are observations, not audio.
        #[arg(long, default_value_t = false)]
        icy: bool,
    },
    /// Finish the received portion of a running recording and validate it.
    Stop { id: String },
    /// Stop receiving. The uncovered plan is a gap, not a silence file.
    Pause { id: String },
    /// List recordings, including failed, interrupted, and deleted history.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 16)]
        limit: u32,
    },
    /// Inspect one recording, including history after the file is deleted.
    Show { id: String },
    /// Print the verified recording's local path for a media player.
    Path { id: String },
    /// Export a versioned JSON sidecar snapshot to stdout, without URL paths or keys.
    Metadata { id: String },
    /// Protect a recording from automatic expiration and eviction.
    Keep { id: String },
    /// Mark protected long-term retention in this library. This is not a backup.
    Archive { id: String },
    /// Return media to the rolling age and capacity policy.
    Temporary { id: String },
    /// Acknowledge completed external processing; temporary media becomes reclaimable.
    Processed {
        id: String,
        #[arg(long)]
        receipt: String,
    },
    /// Explicitly remove inactive media, including kept/archived media. Keep its history.
    Delete { id: String },
    /// Protect every published segment the range intersects. The open tail stays temporary.
    Hold {
        id: String,
        #[arg(long)]
        start_us: u64,
        #[arg(long)]
        end_us: u64,
    },
}

impl RecordCommand {
    pub fn operation(&self) -> Result<Operation, Box<dyn std::error::Error>> {
        Ok(Operation::Record {
            command: match self {
                Self::Hls {
                    id,
                    source,
                    seconds,
                    max_mib,
                    retention,
                    live,
                } => RecordingOperation::Hls {
                    id: id.clone(),
                    source_revision: source.clone(),
                    seconds: *seconds,
                    maximum_bytes: max_mib
                        .checked_mul(1024 * 1024)
                        .ok_or("recording byte ceiling is too large")?,
                    retention: *retention,
                    live: *live,
                },
                Self::Start {
                    id,
                    source,
                    seconds,
                    max_mib,
                    retention,
                    icy,
                } => RecordingOperation::Start {
                    id: id.clone(),
                    source_revision: source.clone(),
                    seconds: *seconds,
                    maximum_bytes: max_mib
                        .checked_mul(1024 * 1024)
                        .ok_or("recording byte ceiling is too large")?,
                    retention: *retention,
                    icy: *icy,
                },
                Self::Stop { id } => RecordingOperation::Stop { id: id.clone() },
                Self::Pause { id } => RecordingOperation::Pause { id: id.clone() },
                Self::Metadata { id } => RecordingOperation::Metadata { id: id.clone() },
                Self::Show { id } | Self::Path { id } => {
                    RecordingOperation::Show { id: id.clone() }
                }
                Self::List { after, limit } => RecordingOperation::List {
                    after: after.clone(),
                    limit: *limit,
                },
                Self::Keep { id } => RecordingOperation::Retain {
                    id: id.clone(),
                    retention: Retention::Kept,
                },
                Self::Archive { id } => RecordingOperation::Retain {
                    id: id.clone(),
                    retention: Retention::Archived,
                },
                Self::Temporary { id } => RecordingOperation::Retain {
                    id: id.clone(),
                    retention: Retention::Temporary,
                },
                Self::Processed { id, receipt } => RecordingOperation::Processed {
                    id: id.clone(),
                    receipt: receipt.clone(),
                },
                Self::Delete { id } => RecordingOperation::Delete { id: id.clone() },
                Self::Hold {
                    id,
                    start_us,
                    end_us,
                } => RecordingOperation::Hold {
                    id: id.clone(),
                    start_us: *start_us,
                    end_us: *end_us,
                },
            },
        })
    }
}

pub fn render_policy(writer: &mut impl Write, policy: &DvrStatus, ink: Ink) -> io::Result<()> {
    writeln!(
        writer,
        "DVR: {} used or reserved of {} ({} of {} bytes). {} available.",
        bytes_label(policy.charged_bytes),
        bytes_label(policy.quota_bytes),
        policy.charged_bytes,
        policy.quota_bytes,
        bytes_label(policy.available_bytes)
    )?;
    writeln!(
        writer,
        "Temporary retention: {} days, with oldest-first eviction under quota pressure.",
        policy.retention_days
    )?;
    writeln!(
        writer,
        "Kept and archived recordings are protected and count toward the quota."
    )?;
    writeln!(
        writer,
        "Free-space floor: {}. Decoder: {}.",
        bytes_label(policy.minimum_free_bytes),
        if policy.decoder.is_some() {
            ink.tint(Tone::Ok, "configured")
        } else {
            format!(
                "{}. Set it with dvr configure --decoder ABSOLUTE_PATH_TO_FFMPEG",
                ink.tint(Tone::Warn, "not configured")
            )
        }
    )
}

pub fn render_records(writer: &mut impl Write, page: &RecordingPage, ink: Ink) -> io::Result<()> {
    for record in &page.entries {
        writeln!(
            writer,
            "{} | {} | {} | {} | {} | {} bytes charged | source {}",
            clean(&record.id),
            record.profile.as_str(),
            ink.tint(tone_for_state(&record.state), &clean(&record.state)),
            ink.tint(
                tone_for_state(&record.storage_state),
                &clean(&record.storage_state)
            ),
            record.retention.as_str(),
            record.charged_bytes,
            clean(&record.source_revision)
        )?;
        if let Some(reason) = &record.end_reason {
            writeln!(
                writer,
                "  Capture ended: {}. Decoded media verified.",
                clean(reason)
            )?;
        }
        if let Some(detail) = &record.failure_detail {
            writeln!(
                writer,
                "  {}",
                ink.tint(Tone::Fail, &format!("Failure: {}", clean(detail)))
            )?;
        }
        for interval in &record.intervals {
            writeln!(
                writer,
                "  Interval {}: bytes {}-{}, decoded {}-{} us.",
                interval.ordinal,
                interval.byte_start,
                interval.byte_end,
                interval.decoded_start_us,
                interval.decoded_end_us
            )?;
        }
        for gap in &record.gaps {
            writeln!(
                writer,
                "  Gap {}: {} from {} to {} us. No audio file.",
                gap.ordinal,
                gap.cause.as_str(),
                gap.start_us,
                gap.end_us
            )?;
        }
        for interval in &record.intervals {
            if interval.released {
                writeln!(
                    writer,
                    "  Released segment {}: no audio file.",
                    interval.ordinal
                )?;
            }
        }
        for hold in &record.holds {
            writeln!(
                writer,
                "  Hold {}: {} to {} us. Segments {}.",
                hold.ordinal,
                hold.start_us,
                hold.end_us,
                hold.segments
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        if record.open_ceiling > 0 && record.open_object_key.is_some() {
            writeln!(
                writer,
                "  {}",
                ink.tint(Tone::Warn, "Open tail: visible, not readable.")
            )?;
        }
    }
    if page.entries.is_empty() {
        writeln!(writer, "No recordings on this page.")?;
    }
    if let Some(after) = &page.next_after {
        writeln!(writer, "Continue with record list --after {}", clean(after))?;
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

    fn operation(words: &[&str]) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let cli = crate::Cli::try_parse_from(words)?;
        let operation = match cli.command {
            crate::Command::Dvr { command } => command.operation()?,
            crate::Command::Record { command } => command.operation()?,
            _ => return Err("not a DVR command".into()),
        };
        Ok(serde_json::to_value(operation)?["command"].clone())
    }

    #[test]
    fn quota_conversion_is_exact_and_refuses_overflow_or_missing_decoder() -> TestResult {
        let temp = tempfile::tempdir()?;
        let decoder = temp.path().join("local decoder");
        std::fs::write(&decoder, b"fixture")?;
        let decoder_text = decoder.to_str().ok_or("decoder is not Unicode")?;
        let configure = operation(&[
            "sigy",
            "dvr",
            "configure",
            "--decoder",
            decoder_text,
            "--quota-gb",
            "9007199255",
            "--minimum-free-mib",
            "2",
            "--retention-days",
            "30",
        ])?;
        assert_eq!(configure["quota_bytes"], 9_007_199_255_000_000_000_u64);
        assert_eq!(configure["minimum_free_bytes"], 2_097_152);
        assert_eq!(configure["retention_days"], 30);
        assert_eq!(
            configure["decoder"],
            decoder.canonicalize()?.to_str().ok_or("path")?
        );
        for flag in ["--quota-gb", "--minimum-free-mib"] {
            assert!(
                operation(&[
                    "sigy",
                    "dvr",
                    "configure",
                    "--decoder",
                    decoder_text,
                    flag,
                    "18446744073709551615"
                ])
                .is_err()
            );
        }
        let absent = operation(&["sigy", "dvr", "configure", "--decoder", "absent-decoder"])
            .err()
            .ok_or("an absent decoder must be refused")?
            .to_string();
        assert!(
            absent.contains("decoder absent-decoder does not exist"),
            "{absent}"
        );
        assert!(absent.contains("absolute path"), "{absent}");
        let hostile = operation(&["sigy", "dvr", "configure", "--decoder", "ffmpeg\u{1b}[2J"])
            .err()
            .ok_or("a hostile decoder name must be refused")?
            .to_string();
        assert!(hostile.contains("decoder ffmpeg[2J"), "{hostile}");
        assert!(!hostile.contains('\u{1b}'));
        assert_eq!(operation(&["sigy", "dvr", "status"])?["action"], "status");
        assert_eq!(operation(&["sigy", "dvr", "prune"])?["action"], "prune");
        Ok(())
    }

    #[test]
    fn capture_admission_preserves_opt_in_flags_and_retention() -> TestResult {
        let ordinary = operation(&["sigy", "record", "start", "clip", "--source", "station"])?;
        assert_eq!(ordinary["icy"], false);
        assert_eq!(ordinary["maximum_bytes"], 67_108_864);
        let icy = operation(&[
            "sigy",
            "record",
            "start",
            "clip",
            "--source",
            "station",
            "--icy",
            "--retention",
            "kept",
            "--seconds",
            "7",
        ])?;
        assert_eq!(icy["icy"], true);
        assert_eq!(icy["retention"], "kept");
        assert_eq!(icy["seconds"], 7);
        let hls = operation(&[
            "sigy", "record", "hls", "clip", "--source", "station", "--live",
        ])?;
        assert_eq!(hls["live"], true);
        assert_eq!(
            operation(&["sigy", "record", "hls", "clip", "--source", "station"])?["live"],
            false
        );
        for mode in ["start", "hls"] {
            assert!(
                operation(&[
                    "sigy",
                    "record",
                    mode,
                    "clip",
                    "--source",
                    "station",
                    "--max-mib",
                    "18446744073709551615"
                ])
                .is_err()
            );
        }
        assert!(
            operation(&[
                "sigy", "record", "start", "clip", "--source", "station", "--live"
            ])
            .is_err()
        );
        assert!(
            operation(&[
                "sigy", "record", "hls", "clip", "--source", "station", "--icy"
            ])
            .is_err()
        );
        assert_eq!(
            operation(&[
                "sigy",
                "record",
                "hold",
                "clip",
                "--start-us",
                "123",
                "--end-us",
                "456"
            ])?["start_us"],
            123
        );
        assert_eq!(
            operation(&[
                "sigy",
                "record",
                "processed",
                "clip",
                "--receipt",
                "external-check"
            ])?["receipt"],
            "external-check"
        );
        for (word, retention) in [
            ("keep", "kept"),
            ("archive", "archived"),
            ("temporary", "temporary"),
        ] {
            assert_eq!(
                operation(&["sigy", "record", word, "clip"])?["retention"],
                retention
            );
        }
        for word in ["stop", "pause", "show", "path", "metadata", "delete"] {
            assert_eq!(operation(&["sigy", "record", word, "clip"])?["id"], "clip");
        }
        assert_eq!(
            operation(&["sigy", "record", "list", "--after", "clip", "--limit", "7"])?["after"],
            "clip"
        );
        Ok(())
    }

    fn page() -> Result<RecordingPage, serde_json::Error> {
        serde_json::from_value(serde_json::json!({"next_after":"clip","entries":[{
            "id":"clip", "source_revision":"station", "state":"interrupted", "object_key":"private",
            "duration_seconds":60, "maximum_bytes":1024, "retention":"temporary", "storage_state":"retained",
            "charged_bytes":9_007_199_254_740_993_u64, "media_bytes":100, "sha256":null, "format":null,
            "decoded_microseconds":1000, "end_reason":null, "processing_receipt":null, "failure_detail":null,
            "profile":"radio", "escrow_bytes":1024, "open_ceiling":900, "open_object_key":"tail", "lease_renewals":1,
            "intervals":[{"ordinal":0,"decoded_start_us":0,"decoded_end_us":1000,"byte_start":0,"byte_end":100,
                "object_key":"private","sha256":"hash","format":"wav","ceiling_bytes":1024,"released":true}],
            "gaps":[{"ordinal":1,"cause":"capture_pause","start_us":1000,"end_us":2000}],
            "holds":[{"ordinal":0,"start_us":0,"end_us":2000,"segments":[0],"gaps":[1]}]
        }]}))
    }

    #[test]
    fn recording_history_distinguishes_gaps_released_files_and_unreadable_tail() -> TestResult {
        let mut page = page()?;
        let mut bytes = Vec::new();
        render_records(&mut bytes, &page, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("9007199254740993 bytes charged"));
        assert!(text.contains("Gap 1: capture_pause from 1000 to 2000 us. No audio file."));
        assert!(text.contains("Released segment 0: no audio file."));
        assert!(text.contains("Hold 0: 0 to 2000 us. Segments 0."));
        assert!(text.contains("Open tail: visible, not readable."));
        assert!(!text.contains("private"));
        let record = &mut page.entries[0];
        record.id = "أخبار\u{1b}[2J".into();
        record.state = "future-state\u{7}".into();
        record.storage_state = "unknown\u{1b}]52;c;payload\u{7}".into();
        record.source_revision = "station\r\nforged".into();
        record.end_reason = Some("limit\u{1b}[31m".into());
        record.failure_detail = Some("fault\u{1b}[2J".into());
        record.open_object_key = None;
        page.next_after = Some("clip\u{1b}[2J".into());
        bytes = Vec::new();
        render_records(&mut bytes, &page, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("أخبار"));
        assert!(text.contains("future-state"));
        assert!(text.contains("Capture ended: limit"));
        assert!(text.contains("Failure: fault"));
        assert!(!text.contains("Open tail"));
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        page.entries.clear();
        page.next_after = None;
        bytes = Vec::new();
        render_records(&mut bytes, &page, Ink::stdout(true))?;
        assert_eq!(String::from_utf8(bytes)?, "No recordings on this page.\n");
        Ok(())
    }

    #[test]
    fn policy_renders_exact_bytes_and_does_not_disclose_decoder_path() -> TestResult {
        let mut policy = DvrStatus {
            quota_bytes: 9_007_199_254_740_993,
            charged_bytes: 17,
            reserved_bytes: 7,
            available_bytes: 9_007_199_254_740_976,
            minimum_free_bytes: 256,
            retention_days: 14,
            decoder: None,
        };
        let mut bytes = Vec::new();
        render_policy(&mut bytes, &policy, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("DVR: 17 B used or reserved of 9.0 PB (17 of 9007199254740993 bytes). 9.0 PB available."), "{text}");
        assert!(text.contains("Free-space floor: 256 B."), "{text}");
        assert!(text.contains("not configured. Set it with dvr configure --decoder"));
        assert!(text.contains("Kept and archived recordings are protected"));
        policy.decoder = Some("private-decoder-path".into());
        bytes = Vec::new();
        render_policy(&mut bytes, &policy, Ink::stdout(true))?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("Decoder: configured"));
        assert!(!text.contains("private-decoder-path"));
        Ok(())
    }
}
