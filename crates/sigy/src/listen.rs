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
    /// Attach a playhead to one capture. This does not start or stop capture.
    Attach {
        session: String,
        /// Recording id. Not a source URL.
        #[arg(long)]
        recording: String,
    },
    /// Pause this playhead. The capture worker keeps running.
    Pause { session: String },
    /// Park at the end of the newest published segment. The open tail is not read.
    Live { session: String },
    /// Move this playhead inside one published segment. This does not signal capture.
    Seek {
        session: String,
        /// Timeline offset in microseconds.
        #[arg(long)]
        seek_us: u64,
    },
    /// Play this playhead's published segment, then drop the playhead.
    Play {
        session: String,
        /// `null` discards samples. `system` uses a local output device when the decoder has one.
        #[arg(long, default_value = "system")]
        destination: String,
    },
    /// Drop this playhead. The capture continues.
    Detach { session: String },
    /// Show one playhead. No source URL or media path is included.
    Session { session: String },
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
        ListenCommand::Attach { session, recording } => {
            playback(
                directory,
                control::PlaybackOperation::Attach {
                    id: session.clone(),
                    recording_id: recording.clone(),
                },
                json,
            )
            .await
        }
        ListenCommand::Pause { session } => {
            playback(
                directory,
                control::PlaybackOperation::Pause {
                    id: session.clone(),
                },
                json,
            )
            .await
        }
        ListenCommand::Live { session } => {
            playback(
                directory,
                control::PlaybackOperation::Live {
                    id: session.clone(),
                },
                json,
            )
            .await
        }
        ListenCommand::Seek { session, seek_us } => {
            playback(
                directory,
                control::PlaybackOperation::Seek {
                    id: session.clone(),
                    seek_us: *seek_us,
                },
                json,
            )
            .await
        }
        ListenCommand::Play {
            session,
            destination,
        } => play_session(directory, session, destination, json).await,
        ListenCommand::Detach { session } => {
            playback(
                directory,
                control::PlaybackOperation::Detach {
                    id: session.clone(),
                },
                json,
            )
            .await
        }
        ListenCommand::Session { session } => {
            playback(
                directory,
                control::PlaybackOperation::Show {
                    id: session.clone(),
                },
                json,
            )
            .await
        }
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
    let one_file = record.state == "completed"
        && record.storage_state == "retained"
        && record.intervals.len() <= 1;
    if !one_file {
        return play_segment(directory, id, &decoder, destination, seek_us, &record, json).await;
    }
    if let Some(gap) = sigy_service::storage::dvr::blocking_gap(&record.gaps, seek_us) {
        return Err(gap.cause.seek_denial().into());
    }
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

async fn play_segment(
    directory: &Path,
    id: &str,
    decoder: &str,
    destination: PlaybackDestination,
    seek_us: u64,
    record: &sigy_service::storage::dvr::Recording,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let tail_open =
        record.state == "running" && record.open_ceiling > 0 && record.open_object_key.is_some();
    let located = recordings::locate(&record.intervals, &record.gaps, tail_open, seek_us);
    let recordings::Located::Segment {
        ordinal,
        object_key,
        format,
        file_seek_us,
        file_decoded_us,
    } = located
    else {
        return Err(segment_refusal(&located).into());
    };
    let path = recordings::media_path(directory, &object_key)?;
    if !path.is_file() {
        return Err("retained media is missing".into());
    }
    let report = recordings::play_retained_file(
        decoder,
        &path,
        &format,
        destination,
        file_seek_us,
        file_decoded_us,
    )
    .await?;
    let playhead_us = record
        .intervals
        .iter()
        .find(|interval| interval.ordinal == ordinal)
        .map_or(report.playhead_us, |interval| {
            interval.decoded_start_us.saturating_add(report.playhead_us)
        });
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "id": id,
                "destination": destination.as_str(),
                "seek_us": seek_us,
                "playhead_us": playhead_us,
                "segment_ordinal": ordinal,
                "progress_advanced": report.progress_advanced,
            }),
        )?;
        writeln!(stdout)?;
    } else {
        writeln!(
            stdout,
            "Playing segment {ordinal} of recording {id} to {}.",
            destination.as_str()
        )?;
        writeln!(stdout, "Playhead {playhead_us} microseconds.")?;
    }
    Ok(())
}

fn segment_refusal(located: &recordings::Located) -> &'static str {
    match located {
        recordings::Located::Gap { cause } => cause.seek_denial(),
        recordings::Located::OpenTail { .. } => "open tail is not readable",
        recordings::Located::Outside { .. } => "seek is outside the retained audio",
        recordings::Located::Unpublished | recordings::Located::Segment { .. } => {
            "recording has no verified retained media"
        }
    }
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

async fn play_session(
    directory: &Path,
    session_id: &str,
    destination: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = PlaybackDestination::parse(destination)?;
    let shown = view(
        directory,
        Operation::Playback {
            command: control::PlaybackOperation::Show {
                id: session_id.to_owned(),
            },
        },
    )
    .await?;
    let session = shown.playback.ok_or("playback session is missing")?;
    if session.state == "expired" {
        return Err("paused position expired; seek inside retained audio or return to live".into());
    }
    let recording_id = session.recording_id.clone();
    let playhead = session.playhead_us;
    let shown = view(
        directory,
        Operation::Record {
            command: RecordingOperation::Show {
                id: recording_id.clone(),
            },
        },
    )
    .await?;
    let record = shown
        .recording_page
        .and_then(|page| page.entries.into_iter().next())
        .ok_or("recording not found")?;
    let segment = session_segment(&record, playhead)?;
    let decoder = decoder_path(directory).await?;
    let path = recordings::media_path(directory, &segment.object_key)?;
    if !path.is_file() {
        return Err("retained media is missing".into());
    }
    let played = recordings::play_retained_file(
        &decoder,
        &path,
        &segment.format,
        destination,
        segment.file_seek_us,
        segment.file_decoded_us,
    )
    .await;
    let _ = view(
        directory,
        Operation::Playback {
            command: control::PlaybackOperation::Detach {
                id: session_id.to_owned(),
            },
        },
    )
    .await;
    let report = played?;
    let playhead_us = record
        .intervals
        .iter()
        .find(|interval| interval.ordinal == segment.ordinal)
        .map_or(report.playhead_us, |interval| {
            interval.decoded_start_us.saturating_add(report.playhead_us)
        });
    write_session_play(
        json,
        &SessionPlayReport {
            session_id,
            recording_id: &recording_id,
            destination,
            seek_us: playhead,
            playhead_us,
            ordinal: segment.ordinal,
            progress_advanced: report.progress_advanced,
        },
    )
}

struct SessionPlayReport<'a> {
    session_id: &'a str,
    recording_id: &'a str,
    destination: PlaybackDestination,
    seek_us: u64,
    playhead_us: u64,
    ordinal: u32,
    progress_advanced: bool,
}

struct SessionSegment {
    ordinal: u32,
    object_key: String,
    format: String,
    file_seek_us: u64,
    file_decoded_us: u64,
}

fn session_segment(
    record: &sigy_service::storage::dvr::Recording,
    playhead: u64,
) -> Result<SessionSegment, Box<dyn std::error::Error>> {
    let tail_open =
        record.state == "running" && record.open_ceiling > 0 && record.open_object_key.is_some();
    let located = recordings::locate(&record.intervals, &record.gaps, tail_open, playhead);
    let recordings::Located::Segment {
        ordinal,
        object_key,
        format,
        file_seek_us,
        file_decoded_us,
    } = located
    else {
        return Err(segment_refusal(&located).into());
    };
    if record.open_object_key.as_deref() == Some(object_key.as_str()) {
        return Err("open tail is not readable".into());
    }
    Ok(SessionSegment {
        ordinal,
        object_key,
        format,
        file_seek_us,
        file_decoded_us,
    })
}

fn write_session_play(
    json: bool,
    report: &SessionPlayReport<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "id": report.session_id,
                "recording_id": report.recording_id,
                "destination": report.destination.as_str(),
                "seek_us": report.seek_us,
                "playhead_us": report.playhead_us,
                "segment_ordinal": report.ordinal,
                "progress_advanced": report.progress_advanced,
                "detached": true,
            }),
        )?;
        writeln!(stdout)?;
    } else {
        writeln!(
            stdout,
            "Playing segment {} of playhead {} to {}.",
            report.ordinal,
            report.session_id,
            report.destination.as_str()
        )?;
        writeln!(
            stdout,
            "Playhead {} microseconds. Session closed.",
            report.playhead_us
        )?;
    }
    Ok(())
}

async fn playback(
    directory: &Path,
    command: control::PlaybackOperation,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = view(directory, Operation::Playback { command }).await?;
    let session = snapshot.playback.ok_or("playback session is missing")?;
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(&mut stdout, &session)?;
        writeln!(stdout)?;
    } else {
        writeln!(
            stdout,
            "Playhead {} on {} is {}. Position {} us.",
            session.id, session.recording_id, session.state, session.playhead_us
        )?;
        if session.open_tail {
            writeln!(stdout, "Open tail: visible, not readable.")?;
        }
        if session.state == "expired" {
            writeln!(stdout, "Paused position expired.")?;
        }
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
