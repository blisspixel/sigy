use std::{io::Write, path::Path, time::Duration};

use clap::Subcommand;
use sigy_service::{
    control::{self, DvrOperation, ListenOperation, ListenView, Operation},
    library::Library,
    recordings::{self, PlaybackDestination},
};

mod retained;
use retained::ReaderCommand;

/// Playback needs the user's own decoder; Sigy never downloads one.
const NO_DECODER: &str =
    "no decoder is configured. Set one with `sigy dvr configure --decoder ABSOLUTE_PATH_TO_FFMPEG`";

#[derive(Debug, Subcommand)]
pub enum ListenCommand {
    /// Play one retained recording locally. This does not contact the source or stop capture.
    File {
        /// Recording ID from record list.
        id: String,
        /// `null` discards samples. `system` uses a local output adapter when available.
        #[arg(long, default_value = "system", value_parser = ["system", "null"])]
        destination: String,
        /// Timeline offset in microseconds, inside one sealed retained interval.
        #[arg(long, default_value_t = 0)]
        seek_us: u64,
        /// Exact protected-reader request ID. Reuse inspects its receipt without replaying audio.
        #[arg(long, value_parser = retained::reader_id)]
        request: Option<String>,
    },
    /// Listen to one direct audio revision. An episode enclosure has no live edge.
    Source {
        /// New listen ID. Reusing it does not open the source again.
        id: String,
        /// Immutable `http_audio` revision. This command does not accept a URL.
        #[arg(long)]
        revision: String,
        /// `null` discards samples. `system` uses a local output adapter when available.
        #[arg(long, default_value = "system", value_parser = ["system", "null"])]
        destination: String,
    },
    /// Attach a playhead to one capture. This does not start or stop capture.
    Attach {
        /// Playhead name you choose, used by the other playhead commands.
        session: String,
        /// Recording id. Not a source URL.
        #[arg(long)]
        recording: String,
    },
    /// Pause this playhead. The capture worker keeps running.
    Pause {
        /// Playhead name from listen attach.
        session: String,
    },
    /// Park at the end of the newest published segment. The open tail is not read.
    Live {
        /// Playhead name from listen attach.
        session: String,
    },
    /// Move this playhead inside one published segment. This does not signal capture.
    Seek {
        /// Playhead name from listen attach.
        session: String,
        /// Timeline offset in microseconds.
        #[arg(long)]
        seek_us: u64,
    },
    /// Play this playhead's published segment, then drop the playhead.
    Play {
        /// Playhead name from listen attach.
        session: String,
        /// `null` discards samples. `system` uses a local output adapter when available.
        #[arg(long, default_value = "system", value_parser = ["system", "null"])]
        destination: String,
    },
    /// Drop this playhead. The capture continues.
    Detach {
        /// Playhead name from listen attach.
        session: String,
    },
    /// Show one playhead. No source URL or media path is included.
    Session {
        /// Playhead name from listen attach.
        session: String,
    },
    /// End one listen. This does not stop a recording.
    Stop {
        /// Listen ID from listen source.
        id: String,
    },
    /// Show one listen receipt. No source URL is included.
    Status {
        /// Listen ID from listen source.
        id: String,
    },
    /// Inspect or stop a protected retained reader. This does not start playback.
    Reader {
        #[command(subcommand)]
        command: ReaderCommand,
    },
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
            request,
        } => {
            play_file(
                directory,
                id,
                destination,
                *seek_us,
                request.as_deref(),
                json,
            )
            .await
        }
        ListenCommand::Source {
            id,
            revision,
            destination,
        } => play_source(directory, id, revision, destination, json).await,
        ListenCommand::Play {
            session,
            destination,
        } => play_session(directory, session, destination, json).await,
        ListenCommand::Attach { .. }
        | ListenCommand::Pause { .. }
        | ListenCommand::Live { .. }
        | ListenCommand::Seek { .. }
        | ListenCommand::Detach { .. }
        | ListenCommand::Session { .. } => playback_control(directory, command, json).await,
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
        ListenCommand::Reader { command } => retained::inspect(directory, command, json).await,
    }
}

async fn playback_control(
    directory: &Path,
    command: &ListenCommand,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let operation = match command {
        ListenCommand::Attach { session, recording } => control::PlaybackOperation::Attach {
            id: session.clone(),
            recording_id: recording.clone(),
        },
        ListenCommand::Pause { session } => control::PlaybackOperation::Pause {
            id: session.clone(),
        },
        ListenCommand::Live { session } => control::PlaybackOperation::Live {
            id: session.clone(),
        },
        ListenCommand::Seek { session, seek_us } => control::PlaybackOperation::Seek {
            id: session.clone(),
            seek_us: *seek_us,
        },
        ListenCommand::Detach { session } => control::PlaybackOperation::Detach {
            id: session.clone(),
        },
        ListenCommand::Session { session } => control::PlaybackOperation::Show {
            id: session.clone(),
        },
        _ => return Err("command is not a playback control".into()),
    };
    playback(directory, operation, json).await
}
async fn play_file(
    directory: &Path,
    id: &str,
    destination: &str,
    seek_us: u64,
    request: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = PlaybackDestination::parse(destination)?;
    let result = retained::play(directory, id, destination, seek_us, request).await?;
    retained::write_play(json, &result, None, true)
}

#[cfg(test)]
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
        writeln!(
            stdout,
            "Listening to revision {} as {}.",
            clean(revision),
            clean(format)
        )?;
        writeln!(stdout, "Playhead {} microseconds.", report.playhead_us)?;
    }
    Ok(())
}

fn replay(id: &str, listen: &ListenView, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();
    render_replay(&mut stdout, id, listen, json)
}

fn render_replay(
    stdout: &mut impl Write,
    id: &str,
    listen: &ListenView,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if listen.state != "completed" {
        return Err(failure_text(listen).into());
    }
    if json {
        serde_json::to_writer(
            &mut *stdout,
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
            "Listen {} already completed for revision {}.",
            clean(id),
            clean(&listen.source_revision)
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
        .ok_or_else(|| NO_DECODER.into())
}

fn failure_text(listen: &ListenView) -> String {
    clean(
        &listen
            .failure
            .clone()
            .unwrap_or_else(|| format!("listen {}", listen.state)),
    )
}

fn write_snapshot(
    json: bool,
    snapshot: &sigy_service::control::Snapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();
    render_snapshot(&mut stdout, json, snapshot)
}

fn render_snapshot(
    stdout: &mut impl Write,
    json: bool,
    snapshot: &sigy_service::control::Snapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        serde_json::to_writer(&mut *stdout, snapshot)?;
        writeln!(stdout)?;
        return Ok(());
    }
    let Some(listen) = &snapshot.listen else {
        return Err("listen not found".into());
    };
    writeln!(
        stdout,
        "Listen {}: {} | revision {}",
        clean(&listen.id),
        clean(&listen.state),
        clean(&listen.source_revision)
    )?;
    if let Some(failure) = &listen.failure {
        writeln!(stdout, "Reason: {}", clean(failure))?;
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
    let played = if session.state == "expired" {
        Err("paused position expired; seek inside retained audio or return to live".into())
    } else {
        retained::play(
            directory,
            &session.recording_id,
            destination,
            session.playhead_us,
            None,
        )
        .await
    };
    let detached = view(
        directory,
        Operation::Playback {
            command: control::PlaybackOperation::Detach {
                id: session_id.to_owned(),
            },
        },
    )
    .await;
    let result = played?;
    let detach_confirmed = detached.is_ok();
    retained::write_play(json, &result, Some(session_id), detach_confirmed)?;
    detached?;
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
    render_playback(&mut stdout, json, &session)
}

fn render_playback(
    stdout: &mut impl Write,
    json: bool,
    session: &control::PlaybackView,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        serde_json::to_writer(&mut *stdout, session)?;
        writeln!(stdout)?;
    } else {
        writeln!(
            stdout,
            "Playhead {} on {} is {}. Position {} us.",
            clean(&session.id),
            clean(&session.recording_id),
            clean(&session.state),
            session.playhead_us
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

fn clean(value: &str) -> String {
    crate::explorer::text::sanitize(value, 1024)
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

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn snapshot() -> Result<control::Snapshot, serde_json::Error> {
        serde_json::from_value(
            serde_json::json!({"schema_version":39,"sqlite_version":"fixture",
            "provider_dispatch_available":false,"budgets":[],"captures":{"dispatch_available":false,
                "scheduled":0,"active":0,"interrupted":0,"terminal":0}}),
        )
    }

    fn receipt() -> ListenView {
        ListenView {
            id: "listen\u{1b}[2J".into(),
            source_revision: "أخبار\u{1b}[31m".into(),
            state: "failed".into(),
            started_ms: 0,
            completed_ms: Some(1),
            format: None,
            failure: Some("unavailable\u{1b}]52;c;payload\u{7}\nforged".into()),
            pipe_nonce: None,
            newly_started: Some(false),
        }
    }

    #[test]
    fn listen_receipts_sanitize_plain_output_and_preserve_exact_json_evidence() -> TestResult {
        let mut snapshot = snapshot()?;
        assert!(render_snapshot(&mut Vec::new(), false, &snapshot).is_err());
        snapshot.listen = Some(receipt());
        let mut bytes = Vec::new();
        render_snapshot(&mut bytes, false, &snapshot)?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("Listen listen"));
        assert!(text.contains(": failed | revision أخبار"));
        assert!(text.contains("Reason: unavailable"));
        assert_eq!(text.lines().count(), 2);
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        bytes = Vec::new();
        render_snapshot(&mut bytes, true, &snapshot)?;
        let read: control::Snapshot = serde_json::from_slice(&bytes)?;
        assert_eq!(read.listen.ok_or("receipt")?.failure, receipt().failure);
        snapshot.listen.as_mut().ok_or("receipt")?.failure = None;
        bytes = Vec::new();
        render_snapshot(&mut bytes, false, &snapshot)?;
        assert_eq!(String::from_utf8(bytes)?.lines().count(), 1);
        Ok(())
    }

    #[test]
    fn replay_never_reports_success_for_failed_or_unknown_listen() -> TestResult {
        let mut receipt = receipt();
        for state in ["failed", "running", "interrupted", "future-state\u{1b}[2J"] {
            receipt.state = state.into();
            let mut bytes = Vec::new();
            let error = render_replay(&mut bytes, &receipt.id, &receipt, false)
                .err()
                .ok_or("must fail")?;
            assert!(bytes.is_empty());
            assert!(!error.to_string().chars().any(char::is_control));
        }
        receipt.failure = None;
        assert!(failure_text(&receipt).starts_with("listen future-state"));
        assert!(!failure_text(&receipt).chars().any(char::is_control));
        receipt.state = "completed".into();
        let mut bytes = Vec::new();
        render_replay(&mut bytes, &receipt.id, &receipt, false)?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("already completed for revision أخبار"));
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        bytes = Vec::new();
        render_replay(&mut bytes, &receipt.id, &receipt, true)?;
        let document: serde_json::Value = serde_json::from_slice(&bytes)?;
        assert_eq!(document["replayed"], true);
        assert_eq!(document["format"], serde_json::Value::Null);
        assert_eq!(document["revision"], receipt.source_revision);
        Ok(())
    }

    #[test]
    fn playback_output_preserves_expiry_unreadable_tail_and_exact_media_clock() -> TestResult {
        let mut session = control::PlaybackView {
            id: "session\u{1b}[2J".into(),
            recording_id: "أخبار\u{7}".into(),
            state: "expired".into(),
            playhead_us: 9_007_199_254_740_993,
            live_us: None,
            earliest_us: None,
            open_tail: true,
        };
        let mut bytes = Vec::new();
        render_playback(&mut bytes, false, &session)?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("Position 9007199254740993 us"));
        assert!(text.contains("Open tail: visible, not readable"));
        assert!(text.contains("Paused position expired"));
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        bytes = Vec::new();
        render_playback(&mut bytes, true, &session)?;
        let read: control::PlaybackView = serde_json::from_slice(&bytes)?;
        assert_eq!(read.playhead_us, session.playhead_us);
        assert_eq!(read.recording_id, session.recording_id);
        session.state = "future-state\u{1b}[31m".into();
        session.open_tail = false;
        bytes = Vec::new();
        render_playback(&mut bytes, false, &session)?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("future-state"));
        assert_eq!(text.lines().count(), 1);
        Ok(())
    }

    #[test]
    fn segment_refusals_never_describe_gaps_or_unpublished_media_as_silence() {
        use sigy_service::storage::dvr::GapCause;
        let gap = recordings::Located::Gap {
            cause: GapCause::CapturePause,
        };
        assert!(segment_refusal(&gap).contains("gap"));
        assert_eq!(
            segment_refusal(&recordings::Located::OpenTail { live_us: 100 }),
            "open tail is not readable"
        );
        assert_eq!(
            segment_refusal(&recordings::Located::Outside { live_us: 100 }),
            "seek is outside the retained audio"
        );
        assert_eq!(
            segment_refusal(&recordings::Located::Unpublished),
            "recording has no verified retained media"
        );
    }
}
