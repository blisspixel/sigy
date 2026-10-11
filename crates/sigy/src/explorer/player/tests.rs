use super::*;
use crate::{
    explorer::state::{Effect, Key, Modes, PlaybackView},
    panel::session::{self, PanelSession},
};
use sigy_service::{
    control::{self, ListenView, Operation},
    library::Library,
};
use std::time::Duration;
use tokio::sync::oneshot;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

fn model() -> Explorer {
    Explorer::new(Modes::resolve(true, true, true), 0)
}

fn hint(id: &str) -> SessionHint {
    SessionHint::Present(PanelSession {
        listen_id: id.into(),
        station_id: "station".into(),
        revision_id: "revision".into(),
        state: "running".into(),
    })
}

fn view(id: &str, state: &str) -> Update {
    Update::View(
        id.into(),
        Ok(PlaybackView {
            id: id.into(),
            state: state.into(),
            format: None,
            source_revision: "revision".into(),
            failure: None,
        }),
    )
}

#[test]
fn stale_sessions_and_reads_cannot_overwrite_stop_or_replacement() -> TestResult {
    let mut player = Player::default();
    let mut model = model();
    player.adopt(&mut model, 0, Update::Hint(hint("first")), false);
    player.adopt(&mut model, 0, view("first", "running"), false);
    player.advance()?;
    player.adopt(&mut model, 0, Update::Hint(hint("old")), false);
    player.adopt(&mut model, 0, view("first", "running"), false);
    player.adopt(&mut model, 1, view("first", "cancelled"), true);
    assert_eq!(player.generation, 2);
    player.adopt(&mut model, 1, view("first", "running"), false);
    assert_eq!(
        model.playback().map(|view| view.state.as_str()),
        Some("cancelled")
    );
    player.adopt(&mut model, 2, Update::Hint(hint("second")), false);
    player.adopt(&mut model, 2, view("first", "failed"), true);
    player.adopt(&mut model, 2, view("second", "running"), false);
    assert_eq!(
        model.playback().map(|view| view.id.as_str()),
        Some("second")
    );
    player.adopt(&mut model, 2, Update::Hint(SessionHint::Unreadable), false);
    assert!(player.current.is_none());
    assert!(model.status().contains("unreadable"));
    player.adopt(&mut model, 2, Update::Hint(SessionHint::Absent), false);
    player.generation = u64::MAX;
    assert!(player.advance().is_err());
    Ok(())
}

#[test]
fn withheld_status_does_not_block_keys_stop_or_real_ipc() -> TestResult {
    let runtime = runtime()?;
    let temporary = tempfile::tempdir()?;
    let directory = temporary.path();
    let library = Library::open(directory, true)?;
    let (shutdown, signal) = oneshot::channel();
    let service = runtime.spawn(control::run(library, async {
        let _ = signal.await;
    }));
    let before = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(snapshot) = control::request(directory, Operation::Status {}).await {
                    break snapshot;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
    })?;
    let (release, withheld) = oneshot::channel();
    let (begun, started) = oneshot::channel();
    let mut player = Player::default();
    let mut model = model();
    player.adopt(&mut model, 0, Update::Hint(hint("fixed-listen")), false);
    player.status.start(&runtime, 0, |send| async move {
        let _ = begun.send(());
        let _ = withheld.await;
        let _ = send.send(view("fixed-listen", "running")).await;
    });
    runtime.block_on(started)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24))?;
    for _ in 0..100 {
        player.poll(&mut model);
        model.handle(Key::Redraw);
        model.handle(Key::Down);
        model.handle(Key::Up);
        terminal.draw(|frame| super::super::screen::render(frame, &model))?;
    }
    for _ in 0..100 {
        player.stop(&runtime, directory, &mut model);
    }
    assert!(player.status.busy());
    assert!(player.stop.busy());
    assert_eq!(
        player.generation, 1,
        "repeated keys cannot dispatch more stops"
    );
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while player.stop.busy() {
                player.poll(&mut model);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
    })?;
    assert_eq!(
        player.generation, 2,
        "stop completes while status remains withheld"
    );
    assert!(model.status().contains("fixed-listen"));
    assert!(model.status().contains("unresolved"));
    assert!(player.status.busy());
    let _ = release.send(());
    drain(&runtime, &mut player, &mut model)?;
    assert!(model.playback().is_none(), "pre-stop status is stale");
    let generation = player.generation;
    player.stop(&runtime, directory, &mut model);
    assert_eq!(player.generation, generation);
    assert!(!player.stop.busy());
    assert_eq!(model.handle(Key::Quit), Effect::Detach);
    let after = runtime.block_on(control::request(directory, Operation::Status {}))?;
    assert_eq!(
        serde_json::to_value(before.budgets)?,
        serde_json::to_value(after.budgets)?
    );
    assert_eq!(
        serde_json::to_value(before.captures)?,
        serde_json::to_value(after.captures)?
    );
    let _ = shutdown.send(());
    runtime.block_on(service)??;
    Ok(())
}

fn drain(
    runtime: &tokio::runtime::Runtime,
    player: &mut Player,
    model: &mut Explorer,
) -> TestResult {
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                player.poll(model);
                if !player.status.busy() && !player.stop.busy() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
    })?;
    Ok(())
}

#[test]
fn absent_oversized_and_disconnected_hints_leave_completion_unresolved() -> TestResult {
    let runtime = runtime()?;
    let temporary = tempfile::tempdir()?;
    let mut player = Player::default();
    let mut model = model();
    player.stop(&runtime, temporary.path(), &mut model);
    assert!(model.status().contains("unresolved"));
    player.request_status(&runtime, temporary.path());
    drain(&runtime, &mut player, &mut model)?;
    assert!(player.current.is_none());
    std::fs::write(
        temporary.path().join("panel-session.json"),
        vec![b' '; 4097],
    )?;
    player.request_status(&runtime, temporary.path());
    drain(&runtime, &mut player, &mut model)?;
    assert!(model.status().contains("unreadable"));
    session::write(
        temporary.path(),
        &PanelSession {
            listen_id: "missing".into(),
            station_id: "station".into(),
            revision_id: "revision".into(),
            state: "running".into(),
        },
    )?;
    player.request_status(&runtime, temporary.path());
    drain(&runtime, &mut player, &mut model)?;
    assert_eq!(player.current.as_deref(), Some("missing"));
    assert!(model.status().contains("unresolved"));
    player
        .status
        .start(&runtime, player.generation, |_send| async {});
    drain(&runtime, &mut player, &mut model)?;
    assert!(model.status().contains("without a receipt"));
    Ok(())
}

#[test]
fn lane_pressure_is_bounded_and_dropped_observation_does_not_cancel_work() -> TestResult {
    let runtime = runtime()?;
    let mut lane = Lane::default();
    let (full, filled) = oneshot::channel();
    let (done, finished) = oneshot::channel();
    lane.start(&runtime, 7, |send| async move {
        let _ = send.try_send(view("one", "running"));
        let _ = send.try_send(view("two", "running"));
        assert!(send.try_send(view("three", "running")).is_err());
        let _ = full.send(());
        send.closed().await;
        let _ = done.send(());
    });
    runtime.block_on(filled)?;
    lane.start(&runtime, 8, |_send| async {
        panic!("busy lane started another task");
    });
    let updates = lane.poll();
    assert_eq!(updates.len(), 2);
    assert!(updates.iter().all(|(generation, _)| *generation == 7));
    assert!(
        lane.busy(),
        "a queued response does not free active task capacity"
    );
    drop(lane);
    runtime.block_on(finished)?;
    Ok(())
}

#[test]
fn receipts_require_exact_identity_and_preserve_failure() -> TestResult {
    assert!(observation::receipt("exact", None).is_err());
    let listen = ListenView {
        id: "other".into(),
        source_revision: "revision".into(),
        state: "failed".into(),
        started_ms: 0,
        completed_ms: None,
        format: None,
        failure: Some("decoder failed".into()),
        pipe_nonce: None,
        newly_started: None,
    };
    assert!(observation::receipt("exact", Some(listen.clone())).is_err());
    let view = observation::receipt("other", Some(listen))?;
    assert_eq!(view.state, "failed");
    assert_eq!(view.failure.as_deref(), Some("decoder failed"));
    Ok(())
}

#[test]
fn uncertain_stop_requires_fresh_same_id_inspection_before_explicit_retry() {
    let mut player = Player::default();
    let mut model = model();
    player.adopt(&mut model, 0, Update::Hint(hint("first")), false);
    player.stopped = Some("first".into());
    player.adopt(
        &mut model,
        0,
        Update::View("first".into(), Err("unresolved".into())),
        true,
    );
    assert_eq!(player.generation, 1);
    player.adopt(&mut model, 0, view("first", "running"), false);
    player.adopt(&mut model, 1, view("other", "running"), false);
    assert_eq!(player.stopped.as_deref(), Some("first"));
    player.adopt(&mut model, 1, view("first", "running"), false);
    assert!(player.stopped.is_none());
    assert!(model.status().contains("explicitly retries"));
    assert!(!player.stop.busy(), "inspection dispatches no stop");
    player.stopped = Some("first".into());
    player.adopt(&mut model, 1, view("first", "running"), true);
    player.adopt(&mut model, 2, view("first", "running"), false);
    assert_eq!(
        player.stopped.as_deref(),
        Some("first"),
        "an accepted stop is not resent while draining"
    );
}

#[test]
fn exited_client_is_reaped_and_diagnostic_drain_holds_replacement() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let mut player = Player::default();
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--invalid-sigy-fixture-argument")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    player.input = child.stdin.take();
    player.child = Some(child);
    let (release, drain) = std::sync::mpsc::channel();
    player.diagnostic_task = Some(std::thread::spawn(move || {
        let _ = drain.recv();
    }));
    player
        .diagnostic
        .lock()
        .map_err(|_| "diagnostic lock")?
        .extend_from_slice(b"fixture child failure");
    let mut model = model();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while player.child.is_some() {
        player.poll(&mut model);
        if std::time::Instant::now() >= deadline {
            return Err("client did not exit".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(player.input.is_none());
    assert!(model.status().contains("fixture child failure"));
    assert!(player.start(temporary.path(), "station").is_err());
    assert!(
        player.child.is_none(),
        "diagnostic hold starts no replacement"
    );
    release.send(())?;
    while player.diagnostic_task.is_some() {
        player.poll(&mut model);
        if std::time::Instant::now() >= deadline {
            return Err("diagnostic did not drain".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
