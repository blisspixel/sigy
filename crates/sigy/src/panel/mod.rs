//! Read-only status and one-station playback for desktop panels.

use std::{
    future::Future,
    io::{self, Write},
    path::Path,
    time::Duration,
};

use clap::Subcommand;
use sigy_service::{
    control::{self, DirectoryOperation, ListenOperation, Operation, RecordingOperation, Snapshot},
    discovery::StationFilter,
    library::Library,
    recordings::PlaybackDestination,
};

pub(crate) mod admit;
pub(crate) mod present;
pub(crate) mod session;

use admit::SourceChoice;
use present::{FavoriteRow, Freshness, ListenRow, Projection, RecordingRow, ServicePresence};
use session::{PanelSession, SessionHint};

#[derive(Debug, Subcommand)]
pub enum PanelCommand {
    /// Inspect the read-only panel state.
    Status,
    /// Emit a Waybar-compatible JSON object on stdout.
    Bar,
    /// Play one favorite station. Refuses unless exactly one audio revision exists.
    Play {
        /// Cached favorite station ID.
        station: String,
        /// Listen ID. Defaults to "panel-<station>".
        #[arg(long)]
        id: Option<String>,
        /// Cancel playback when stdin closes.
        #[arg(long)]
        cancel_on_stdin: bool,
        /// Cancel playback if this parent process exits before or during playback.
        #[arg(long)]
        parent_pid: Option<u32>,
        /// `null` discards samples. `system` uses a local output adapter when available.
        #[arg(long, default_value = "system", value_parser = ["system", "null"])]
        destination: String,
    },
    /// Stop an active panel listen.
    Stop {
        /// Listen ID to stop. Defaults to the active session hint.
        #[arg(long)]
        id: Option<String>,
    },
}

/// # Errors
/// Returns an error when any step of the panel command fails.
pub async fn execute(
    directory: &Path,
    command: &PanelCommand,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        PanelCommand::Status => execute_status(directory, json).await,
        PanelCommand::Bar => execute_bar(directory).await,
        PanelCommand::Play {
            station,
            id,
            cancel_on_stdin,
            parent_pid,
            destination,
        } => {
            Box::pin(execute_play(
                directory,
                station,
                id.as_deref(),
                *cancel_on_stdin,
                *parent_pid,
                destination,
                json,
            ))
            .await
        }
        PanelCommand::Stop { id } => execute_stop(directory, id.as_deref(), json).await,
    }
}

async fn execute_status(directory: &Path, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let projection = project(directory).await;
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, &present::status_json(&projection))?;
        writeln!(stdout)?;
    } else {
        writeln!(stdout, "{}", present::text(&projection))?;
    }
    Ok(())
}

async fn execute_bar(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let projection = project(directory).await;
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &present::bar_json(&projection))?;
    writeln!(stdout)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn project(directory: &Path) -> Projection {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);

    let (service, active_captures, listen) =
        match control::request(directory, Operation::Status {}).await {
            Ok(snapshot) => {
                let stopping = snapshot.service.as_ref().is_some_and(|s| s.stopping);
                let presence = if stopping {
                    ServicePresence::Stopping
                } else {
                    ServicePresence::Running
                };
                let captures = snapshot.captures.active;
                let listen = snapshot.listen.map(|l| ListenRow {
                    id: l.id,
                    state: l.state,
                    failure: l.failure,
                });
                (presence, captures, listen)
            }
            Err(_) => (ServicePresence::Absent, 0, None),
        };

    let dir_snapshot = fetch_op(
        directory,
        service,
        Operation::Radio {
            command: DirectoryOperation::Status {},
        },
    )
    .await;

    let (cached_stations, favorite_stations, stale_stations, newest_age, freshness) =
        match dir_snapshot.and_then(|s| s.directory) {
            Some(dir) => {
                let fresh = if dir.cached_stations == 0 {
                    Freshness::Empty
                } else if dir.stale_stations > 0 {
                    Freshness::Stale
                } else {
                    Freshness::Current
                };
                let newest = dir
                    .newest_observed_ms
                    .map(|ms| crate::explorer::text::age_label(ms, now_ms));
                (
                    dir.cached_stations,
                    dir.favorite_stations,
                    dir.stale_stations,
                    newest,
                    fresh,
                )
            }
            None => (0, 0, 0, None, Freshness::Empty),
        };

    let fav_snapshot = fetch_op(
        directory,
        service,
        Operation::Radio {
            command: DirectoryOperation::SearchOrdered {
                filter: StationFilter::default(),
                favorites_only: true,
                limit: 4,
                after: None,
            },
        },
    )
    .await;

    let (favorites, favorites_truncated) = match fav_snapshot.and_then(|s| s.ordered_station_page) {
        Some(page) => {
            let truncated = page.next_after.is_some()
                || u32::try_from(page.entries.len()).is_ok_and(|len| favorite_stations > len);
            let rows = page
                .entries
                .into_iter()
                .map(|entry| FavoriteRow {
                    id: entry.id,
                    name: entry.name,
                    country: entry.country,
                    hls: entry.hls,
                    codec: entry.codec,
                })
                .collect();
            (rows, truncated)
        }
        None => (Vec::new(), false),
    };

    let rec_snapshot = fetch_op(
        directory,
        service,
        Operation::Record {
            command: RecordingOperation::List {
                after: None,
                limit: 4,
            },
        },
    )
    .await;

    let (recordings, recordings_truncated) = match rec_snapshot.and_then(|s| s.recording_page) {
        Some(page) => {
            let truncated = page.next_after.is_some();
            let rows = page
                .entries
                .into_iter()
                .map(|entry| RecordingRow {
                    id: entry.id,
                    state: entry.state,
                    storage_state: entry.storage_state,
                })
                .collect();
            (rows, truncated)
        }
        None => (Vec::new(), false),
    };

    let session_hint = match session::read(directory) {
        SessionHint::Absent => "absent",
        SessionHint::Unreadable => "unreadable",
        SessionHint::Present(_) => "present",
    };

    Projection {
        service,
        freshness,
        cached_stations,
        favorite_stations,
        stale_stations,
        newest_age,
        favorites,
        favorites_truncated,
        active_captures,
        recordings,
        recordings_truncated,
        listen,
        session_hint,
    }
}

async fn fetch_op(
    directory: &Path,
    service: ServicePresence,
    operation: Operation,
) -> Option<Snapshot> {
    if service == ServicePresence::Absent {
        let mut library = Library::open(directory, false).ok()?;
        control::apply_library(&mut library, operation).ok()
    } else {
        control::request(directory, operation).await.ok()
    }
}

#[cfg(unix)]
pub(crate) fn is_process_alive(pid: u32) -> bool {
    let raw = match i32::try_from(pid) {
        Ok(raw) => raw,
        Err(_) => return false,
    };
    let pid = rustix::process::Pid::from_raw(raw);
    match rustix::process::test_kill_process(pid) {
        Ok(()) => true,
        Err(rustix::io::Errno::PERM) => true,
        Err(_) => false,
    }
}

#[cfg(windows)]
pub(crate) fn is_process_alive(pid: u32) -> bool {
    use winsafe::co;
    match winsafe::HPROCESS::OpenProcess(co::PROCESS::QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(process) => !matches!(process.WaitForSingleObject(Some(0)), Ok(co::WAIT::OBJECT_0)),
        Err(_) => false,
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn is_process_alive(_pid: u32) -> bool {
    true
}

async fn setup_cancel(
    cancel_on_stdin: bool,
    parent_pid: Option<u32>,
) -> Result<impl Future<Output = ()> + Send, Box<dyn std::error::Error>> {
    let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::channel::<()>(1);

    if cancel_on_stdin {
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<()>(1);
        std::thread::spawn(move || {
            use std::io::Read;
            let mut stdin = std::io::stdin().lock();
            let mut buf = [0u8; 128];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = stdin_tx.blocking_send(());
                        break;
                    }
                    Ok(_) => {}
                }
            }
        });

        tokio::select! {
            biased;
            _ = stdin_rx.recv() => {
                return Err(admit::STDIN_CLOSED.into());
            }
            () = tokio::time::sleep(Duration::from_millis(15)) => {}
        }

        let forward_tx = cancel_tx.clone();
        tokio::spawn(async move {
            if stdin_rx.recv().await.is_some() {
                let _ = forward_tx.send(()).await;
            }
        });
    }

    if let Some(pid) = parent_pid {
        let parent_tx = cancel_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if !is_process_alive(pid) {
                    let _ = parent_tx.send(()).await;
                    break;
                }
            }
        });
    }

    Ok(async move {
        let _ = cancel_rx.recv().await;
    })
}

#[allow(clippy::too_many_lines)]
async fn execute_play(
    directory: &Path,
    station: &str,
    id: Option<&str>,
    cancel_on_stdin: bool,
    parent_pid: Option<u32>,
    destination: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = PlaybackDestination::parse(destination)?;

    match session::read(directory) {
        SessionHint::Unreadable => return Err(admit::SESSION_UNREADABLE.into()),
        SessionHint::Present(active) => {
            if let Ok(snapshot) = control::request(
                directory,
                Operation::Listen {
                    command: ListenOperation::Status {
                        id: active.listen_id.clone(),
                    },
                },
            )
            .await
                && let Some(view) = snapshot.listen
                && view.state == "running"
            {
                return Err(admit::already_running(&active.listen_id).into());
            }
        }
        SessionHint::Absent => {}
    }

    let Ok(service_snapshot) = control::request(directory, Operation::Status {}).await else {
        return Err(admit::service_required().into());
    };
    if service_snapshot
        .service
        .as_ref()
        .is_some_and(|s| s.stopping)
    {
        return Err(admit::service_stopping().into());
    }

    if let Some(pid) = parent_pid
        && !is_process_alive(pid)
    {
        return Err(admit::PARENT_GONE.into());
    }

    let cancel = setup_cancel(cancel_on_stdin, parent_pid).await?;

    let linked = control::request(
        directory,
        Operation::Radio {
            command: DirectoryOperation::Linked {
                id: station.to_owned(),
                catalog: None,
            },
        },
    )
    .await?;
    let context = linked
        .linked_station_context
        .ok_or("linked station context missing")?;

    let is_favorite = {
        let fav_snapshot = control::request(
            directory,
            Operation::Radio {
                command: DirectoryOperation::SearchOrdered {
                    filter: StationFilter::default(),
                    favorites_only: true,
                    limit: 1024,
                    after: None,
                },
            },
        )
        .await?;
        fav_snapshot.ordered_station_page.as_ref().is_some_and(|p| {
            p.favorite_ids.iter().any(|fav| fav == station)
                || p.entries.iter().any(|entry| entry.id == station)
        })
    };

    let hls = context
        .sources
        .first()
        .is_some_and(|s| s.registered_station.hls);
    let sources: Vec<SourceChoice> = context
        .sources
        .iter()
        .map(|s| SourceChoice {
            kind: s.source.kind.clone(),
            revision_id: s.source.revision_id.clone(),
        })
        .collect();

    let revision_id = admit::admit(
        station,
        context.cached,
        is_favorite,
        hls,
        &sources,
        context.more_sources,
    )?;

    let listen_id = id.map_or_else(|| format!("panel-{station}"), str::to_owned);

    let session = PanelSession {
        listen_id: listen_id.clone(),
        station_id: station.to_owned(),
        revision_id: revision_id.clone(),
        state: "running".into(),
    };
    session::write(directory, &session)?;

    match crate::listen::start_listen(directory, &listen_id, &revision_id).await? {
        crate::listen::ListenStart::Existing(_) => {
            let _ = session::settle(directory, &listen_id, "completed");
            return Err(admit::reused_listen(&listen_id).into());
        }
        crate::listen::ListenStart::New => {}
    }

    let playback_result =
        crate::listen::finish_new_listen(directory, &listen_id, destination, cancel).await;

    match playback_result {
        Ok(played) => {
            let _ = session::settle(directory, &listen_id, &played.finished.state);
            let mut stdout = io::stdout().lock();
            if json {
                serde_json::to_writer(
                    &mut stdout,
                    &serde_json::json!({
                        "id": listen_id,
                        "station": station,
                        "revision": revision_id,
                        "destination": destination.as_str(),
                        "format": played.format,
                        "playhead_us": played.report.playhead_us,
                        "progress_advanced": played.report.progress_advanced,
                        "replayed": false,
                    }),
                )?;
                writeln!(stdout)?;
            } else {
                writeln!(
                    stdout,
                    "Listening to revision {} as {}.",
                    clean(&revision_id),
                    clean(&played.format)
                )?;
                writeln!(
                    stdout,
                    "Playhead {} microseconds.",
                    played.report.playhead_us
                )?;
            }
            Ok(())
        }
        Err(error) => {
            let state = if error.to_string().contains("cancelled") {
                "interrupted"
            } else {
                "failed"
            };
            let _ = session::settle(directory, &listen_id, state);
            Err(error)
        }
    }
}

async fn execute_stop(
    directory: &Path,
    id: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_id = match id {
        Some(specified) => specified.to_owned(),
        None => match session::read(directory) {
            SessionHint::Present(active) => active.listen_id,
            SessionHint::Absent => return Err("no active panel listen session".into()),
            SessionHint::Unreadable => return Err(admit::SESSION_UNREADABLE.into()),
        },
    };

    let stopped = crate::listen::stop_listen(directory, &target_id).await?;
    let _ = session::settle(directory, &target_id, &stopped.state);

    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "id": target_id,
                "state": stopped.state,
            }),
        )?;
        writeln!(stdout)?;
    } else {
        writeln!(stdout, "Stopped panel listen {}.", clean(&target_id))?;
    }
    Ok(())
}

fn clean(value: &str) -> String {
    crate::explorer::text::sanitize(value, 1024)
}
