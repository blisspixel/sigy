use crate::style::{Ink, Tone, tone_for_state};
use clap::Subcommand;
use sigy_service::{
    control::{DvrOperation, Operation, RecordingOperation, RecordingPage},
    storage::dvr::{DvrStatus, Retention},
};
use std::{
    io::{self, Write},
    path::PathBuf,
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
                    decoder: decoder
                        .canonicalize()?
                        .to_str()
                        .ok_or("decoder path is not valid Unicode")?
                        .into(),
                },
            },
        })
    }
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
        "DVR: {} bytes used/reserved of {} bytes. {} bytes available.",
        policy.charged_bytes, policy.quota_bytes, policy.available_bytes
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
        "Free-space floor: {} bytes. Decoder: {}.",
        policy.minimum_free_bytes,
        if policy.decoder.is_some() {
            ink.tint(Tone::Ok, "configured")
        } else {
            ink.tint(Tone::Warn, "not configured; use dvr configure")
        }
    )
}

pub fn render_records(writer: &mut impl Write, page: &RecordingPage, ink: Ink) -> io::Result<()> {
    for record in &page.entries {
        writeln!(
            writer,
            "{} | {} | {} | {} | {} | {} bytes charged | source {}",
            record.id,
            record.profile.as_str(),
            ink.tint(tone_for_state(&record.state), &record.state),
            ink.tint(tone_for_state(&record.storage_state), &record.storage_state),
            record.retention.as_str(),
            record.charged_bytes,
            record.source_revision
        )?;
        if let Some(reason) = &record.end_reason {
            writeln!(writer, "  Capture ended: {reason}. Decoded media verified.")?;
        }
        if let Some(detail) = &record.failure_detail {
            writeln!(
                writer,
                "  {}",
                ink.tint(Tone::Fail, &format!("Failure: {detail}"))
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
        writeln!(writer, "Continue with record list --after {after}")?;
    }
    Ok(())
}
