use std::{io::Write, path::Path, time::Duration};

use clap::Subcommand;
use sigy_service::{
    control::{self, DvrOperation, ListenOperation, ListenView, Operation, RecordingOperation},
    library::Library,
    recordings::{self, PlaybackDestination},
};

#[derive(Debug, Subcommand)]
pub enum ListenCommand {
    /// Play one retained recording locally. This does not contact the source or stop capture.
    File {
        id: String,
        /// `null` discards samples. `system` uses a local output device when the decoder has one.
        #[arg(long, default_value = "system")]
        destination: String,
        /// Start offset in microseconds, inside the published duration.
        #[arg(long, default_value_t = 0)]
        seek_us: u64,
    },
    /// Listen to one direct audio revision. An episode enclosure has no live edge.
    Source {
        id: String,
        /// Immutable `http_audio` revision. This command does not accept a URL.
        #[arg(long)]
        revision: String,
        /// `null` discards samples. `system` uses a local output device when the decoder has one.
        #[arg(long, default_value = "system")]
        destination: String,
    },
    /// End one listen. This does not stop a recording.
    Stop { id: String },
    /// Show one listen receipt. No source URL is included.
    Status { id: String },
}

/// # Errors
/// Returns an error when the recording is not retained, the seek is outside it, or playback fails.
pub async fn execute(
    directory: &Path,
    command: &ListenCommand,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        ListenCommand::File {
            id,
            destination,
            seek_us,
        } => play_file(directory, id, destination, *seek_us, json).await,
        ListenCommand::Source {
            id,
            revision,
            destination,
        } => play_source(directory, id, revision, destination, json).await,
        ListenCommand::Stop { id } => {
            let snapshot = view(
                directory,
                Operation::Listen {
                    command: ListenOperation::Stop { id: id.clone() },
                },
            )
            .await?;
            write_snapshot(json, &snapshot)
        }
        ListenCommand::Status { id } => {
            let snapshot = view(
                directory,
                Operation::Listen {
                    command: ListenOperation::Status { id: id.clone() },
                },
            )
            .await?;
            write_snapshot(json, &snapshot)
        }
    }
}

async fn play_file(
    directory: &Path,
    id: &str,
    destination: &str,
    seek_us: u64,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = PlaybackDestination::parse(destination)?;
    let policy = view(
        directory,
        Operation::Dvr {
            command: DvrOperation::Status {},
        },
    )
    .await?;
    let decoder = policy
        .dvr
        .and_then(|status| status.decoder)
        .ok_or("decoder is not configured; use dvr configure")?;
    let shown = view(
        directory,
        Operation::Record {
            command: RecordingOperation::Show { id: id.to_owned() },
        },
    )
    .await?;
    let record = shown
        .recording_page
        .and_then(|page| page.entries.into_iter().next())
        .ok_or("recording not found")?;
    if record.state != "completed" || record.storage_state != "retained" {
        return Err("recording has no verified retained media".into());
    }
    if record.intervals.len() > 1 {
        return Err("segmented playback is not available".into());
    }
    let format = record
        .format
        .as_deref()
        .ok_or("recording has no verified retained media")?;
    let decoded_us = record
        .decoded_microseconds
        .filter(|duration| *duration > 0)
        .ok_or("recording has no verified retained media")?;
    let path = recordings::media_path(directory, &record.object_key)?;
    if !path.is_file() {
        return Err("retained media is missing".into());
    }
    let report =
        recordings::play_retained_file(&decoder, &path, format, destination, seek_us, decoded_us)
            .await?;
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "id": id,
                "destination": destination.as_str(),
                "seek_us": seek_us,
                "playhead_us": report.playhead_us,
                "decoded_us": decoded_us,
                "progress_advanced": report.progress_advanced,
            }),
        )?;
        writeln!(stdout)?;
    } else {
        writeln!(
            stdout,
            "Playing retained recording {id} to {}.",
            destination.as_str()
        )?;
        writeln!(
            stdout,
            "Playhead {} of {decoded_us} microseconds.",
            report.playhead_us
        )?;
    }
    Ok(())
}

async fn play_source(
    directory: &Path,
    id: &str,
    revision: &str,
    destination: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = PlaybackDestination::parse(destination)?;
    let started = listen_view(
        directory,
        ListenOperation::Start {
            id: id.to_owned(),
            revision_id: revision.to_owned(),
        },
    )
    .await?;
    if started.newly_started != Some(true) {
        return replay(id, &started, json);
    }
    let ready = wait_until(directory, id, true).await?;
    let nonce = ready
        .pipe_nonce
        .as_deref()
        .ok_or("listen pipe is not available")?;
    let format = ready
        .format
        .as_deref()
        .ok_or("listen format is not available")?;
    let decoder = decoder_path(directory).await?;
    let played =
        recordings::play_direct_listen(&decoder, directory, nonce, format, destination).await;
    if played.is_err() {
        let _ = view(
            directory,
            Operation::Listen {
                command: ListenOperation::Stop { id: id.to_owned() },
            },
        )
        .await;
    }
    let report = played?;
    let finished = wait_until(directory, id, false).await?;
    if finished.state != "completed" {
        return Err(failure_text(&finished).into());
    }
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "id": id,
                "revision": revision,
                "destination": destination.as_str(),
                "format": format,
                "playhead_us": report.playhead_us,
                "progress_advanced": report.progress_advanced,
                "replayed": false,
            }),
        )?;
        writeln!(stdout)?;
    } else {
        writeln!(stdout, "Listening to revision {revision} as {format}.")?;
        writeln!(stdout, "Playhead {} microseconds.", report.playhead_us)?;
    }
    Ok(())
}

fn replay(id: &str, listen: &ListenView, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    if listen.state != "completed" {
        return Err(failure_text(listen).into());
    }
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "id": id,
                "revision": listen.source_revision,
                "state": listen.state,
                "format": listen.format,
                "replayed": true,
            }),
        )?;
        writeln!(stdout)?;
    } else {
        writeln!(
            stdout,
            "Listen {id} already completed for revision {}.",
            listen.source_revision
        )?;
    }
    Ok(())
}

async fn wait_until(
    directory: &Path,
    id: &str,
    need_pipe: bool,
) -> Result<ListenView, Box<dyn std::error::Error>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let snapshot = view(
            directory,
            Operation::Listen {
                command: ListenOperation::Status { id: id.to_owned() },
            },
        )
        .await?;
        let listen = snapshot.listen.ok_or("listen not found")?;
        if snapshot.service.is_none() && listen.state == "running" {
            return Err("service stopped before the listen completed".into());
        }
        let ready = listen.pipe_nonce.is_some() && listen.format.is_some();
        if (need_pipe && ready) || (!need_pipe && listen.state != "running") {
            return Ok(listen);
        }
        if listen.state != "running" {
            return Err(failure_text(&listen).into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("listen timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn listen_view(
    directory: &Path,
    command: ListenOperation,
) -> Result<ListenView, Box<dyn std::error::Error>> {
    let snapshot = view(directory, Operation::Listen { command }).await?;
    snapshot.listen.ok_or_else(|| "listen not found".into())
}

async fn decoder_path(directory: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let policy = view(
        directory,
        Operation::Dvr {
            command: DvrOperation::Status {},
        },
    )
    .await?;
    policy
        .dvr
        .and_then(|status| status.decoder)
        .ok_or_else(|| "decoder is not configured; use dvr configure".into())
}

fn failure_text(listen: &ListenView) -> String {
    listen
        .failure
        .clone()
        .unwrap_or_else(|| format!("listen {}", listen.state))
}

fn write_snapshot(
    json: bool,
    snapshot: &sigy_service::control::Snapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(&mut stdout, snapshot)?;
        writeln!(stdout)?;
        return Ok(());
    }
    let Some(listen) = &snapshot.listen else {
        return Err("listen not found".into());
    };
    writeln!(
        stdout,
        "Listen {}: {} | revision {}",
        listen.id, listen.state, listen.source_revision
    )?;
    if let Some(failure) = &listen.failure {
        writeln!(stdout, "Reason: {failure}")?;
    }
    Ok(())
}

async fn view(
    directory: &Path,
    operation: Operation,
) -> Result<sigy_service::control::Snapshot, Box<dyn std::error::Error>> {
    match Library::open(directory, false) {
        Ok(mut library) => Ok(control::apply_library(&mut library, operation)?),
        Err(sigy_service::Error::LibraryBusy) => Ok(control::request(directory, operation).await?),
        Err(error) => Err(error.into()),
    }
}
