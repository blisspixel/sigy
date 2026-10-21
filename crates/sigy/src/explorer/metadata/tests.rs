use super::Reads;
use crate::explorer::{
    screen::render,
    state::{DirectoryView, Effect, Explorer, Health, Key, Link, Modes, PlaybackView, StationRow},
    terminal::dispatch_metadata,
};
use sigy_service::{
    control::{self, DirectoryCatalog, Operation},
    library::Library,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::oneshot;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

fn explorer() -> Explorer {
    Explorer::new(
        Modes {
            reduced_motion: true,
            linear: true,
            monochrome: true,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        0,
    )
}

fn row(id: &str) -> StationRow {
    StationRow {
        id: id.into(),
        name: "Alpha".into(),
        favorite: false,
        directory_health: Health::Succeeded,
        directory_languages: "navajo".into(),
        observed_ms: 1_000,
        hls: false,
        coordinates: None,
        metadata: None,
    }
}

fn directory_view() -> DirectoryView {
    DirectoryView {
        cached_stations: 1,
        maximum_stations: 10_000,
        favorite_stations: 0,
        refresh: None,
    }
}

fn catalog() -> DirectoryCatalog {
    DirectoryCatalog {
        namespace: "a".repeat(32),
        comparison: "fixture".into(),
        revision: 1,
    }
}

fn present(
    reads: &mut Reads,
    runtime: &tokio::runtime::Runtime,
    directory: &Path,
    model: &mut Explorer,
    effect: &Effect,
) -> TestResult {
    dispatch_metadata(model, effect, reads, runtime, directory, |_| Ok(()))?;
    Ok(())
}

fn drain(
    runtime: &tokio::runtime::Runtime,
    reads: &mut Reads,
    directory: &Path,
    model: &mut Explorer,
) -> TestResult {
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                reads.poll(runtime, directory, model);
                if !reads.read_busy() && !reads.favorite_busy() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
    })?;
    Ok(())
}

fn wait_until(runtime: &tokio::runtime::Runtime, mut ready: impl FnMut() -> bool) -> TestResult {
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !ready() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
    })?;
    Ok(())
}

struct Cleanup {
    gates: Vec<Option<oneshot::Sender<()>>>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Cleanup {
    fn gate(&mut self) -> oneshot::Receiver<()> {
        let (send, receive) = oneshot::channel();
        self.gates.push(Some(send));
        receive
    }

    fn release(&mut self, index: usize) {
        if let Some(sender) = self.gates.get_mut(index).and_then(Option::take) {
            let _ = sender.send(());
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for sender in self.gates.drain(..).flatten() {
            let _ = sender.send(());
        }
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
    }
}

fn library() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().to_path_buf();
    drop(Library::open(&path, true)?);
    Ok((directory, path))
}

#[test]
fn key_burst_keeps_one_task_and_one_latest_read() -> TestResult {
    let runtime = runtime()?;
    let (_library, path) = library()?;
    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    let mut reads = Reads::default();
    reads.withhold_next(cleanup.gate());
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    for _ in 0..30 {
        let effect = model.handle(Key::Char('g'));
        present(&mut reads, &runtime, &path, &mut model, &effect)?;
    }
    assert_eq!(reads.read_started(), 1);
    assert_eq!(reads.queued_epoch(), Some(model.read_epoch()));
    reads.withhold_next(cleanup.gate());
    cleanup.release(0);
    let reads_wait = &mut reads;
    let model_wait = &mut model;
    wait_until(&runtime, || {
        reads_wait.poll(&runtime, &path, model_wait);
        reads_wait.read_started() == 2
    })?;
    assert_eq!(reads.inflight_epoch(), Some(model.read_epoch()));
    assert!(reads.queued_epoch().is_none());
    assert!(
        !model.status().contains("Cached page read"),
        "the superseded read must not replace the latest request"
    );
    cleanup.release(1);
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert_eq!(reads.read_started(), 2);
    assert!(model.status().contains("Cached page read"));
    assert_eq!(model.link(), &Link::LocalCatalog);
    assert!(model.playback().is_none());
    Ok(())
}

#[test]
fn delivered_read_keeps_its_slot_until_the_task_finishes() -> TestResult {
    let runtime = runtime()?;
    let (_library, path) = library()?;
    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    let mut reads = Reads::default();
    reads.hold_after_send(cleanup.gate());
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    let effect = model.handle(Key::Char('g'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    let reads_wait = &mut reads;
    let model_wait = &mut model;
    wait_until(&runtime, || {
        reads_wait.poll(&runtime, &path, model_wait);
        model_wait.status().contains("Cached page read") && reads_wait.read_busy()
    })?;
    assert_eq!(reads.read_started(), 1);
    let effect = model.handle(Key::Char('g'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    assert_eq!(reads.read_started(), 1);
    assert_eq!(reads.queued_epoch(), Some(model.read_epoch()));
    cleanup.release(0);
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert_eq!(reads.read_started(), 2);
    assert!(!reads.read_busy());
    Ok(())
}

#[test]
fn closed_receiver_reports_once_and_a_newer_epoch_discards_it() -> TestResult {
    let runtime = runtime()?;
    let mut reads = Reads::default();
    reads.close_next();
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    let path = PathBuf::from("unused");
    let effect = model.handle(Key::Char('g'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert!(model.status().contains("without a result"));
    assert_eq!(model.link(), &Link::LocalCatalog);

    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    reads.withhold_next(cleanup.gate());
    reads.close_next();
    let effect = model.handle(Key::Char('g'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    model.note_disconnect(1, "fixture");
    let status = model.status().to_owned();
    cleanup.release(0);
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert_eq!(model.status(), status);
    assert!(!model.status().contains("without a result"));
    Ok(())
}

#[test]
fn stale_success_and_failure_leave_the_newer_view() -> TestResult {
    let runtime = runtime()?;
    let (_library, path) = library()?;
    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    let mut reads = Reads::default();
    reads.withhold_next(cleanup.gate());
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    model.observe_playback(PlaybackView {
        id: "kept-listen".into(),
        state: "running".into(),
        format: None,
        source_revision: "revision".into(),
        failure: None,
    });
    let effect = model.handle(Key::Char('r'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    model.note_disconnect(1, "fixture");
    cleanup.release(0);
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert!(model.status().contains("Disconnected"));
    assert_eq!(model.schema_version(), 0);
    assert_eq!(
        model.playback().map(|view| view.id.as_str()),
        Some("kept-listen")
    );

    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    reads.withhold_next(cleanup.gate());
    let effect = model.handle(Key::Char('g'));
    present(
        &mut reads,
        &runtime,
        Path::new("missing-library"),
        &mut model,
        &effect,
    )?;
    assert_eq!(model.handle(Key::Left), Effect::None);
    let status = model.status().to_owned();
    cleanup.release(1);
    drain(
        &runtime,
        &mut reads,
        Path::new("missing-library"),
        &mut model,
    )?;
    assert_eq!(model.status(), status);
    assert_eq!(model.link(), &Link::LocalCatalog);
    assert_eq!(model.rows(), []);
    Ok(())
}

#[test]
fn finding_monitor_and_random_reads_honor_the_epoch() -> TestResult {
    let runtime = runtime()?;
    let (_library, path) = library()?;
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    let finding = crate::explorer::finding::fixture();
    assert!(model.findings.load("world-news", "one", &finding));
    model.monitors.load_ids(vec!["kept".into()]);
    let mut reads = Reads::default();
    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    finding_reads_keep_the_loaded_page(&runtime, &path, &mut reads, &mut cleanup, &mut model)?;
    monitor_reads_keep_or_replace_by_epoch(&runtime, &path, &mut reads, &mut cleanup, &mut model)?;
    random_reads_keep_or_replace_by_epoch(&runtime, &path, &mut reads, &mut cleanup, &mut model)?;
    Ok(())
}

fn finding_page(model: &Explorer) -> String {
    model.findings.lines(20, 80, 0).join("\n")
}

fn finding_reads_keep_the_loaded_page(
    runtime: &tokio::runtime::Runtime,
    path: &Path,
    reads: &mut Reads,
    cleanup: &mut Cleanup,
    model: &mut Explorer,
) -> TestResult {
    reads.withhold_next(cleanup.gate());
    present(
        reads,
        runtime,
        path,
        model,
        &Effect::Finding {
            monitor: "missing".into(),
            finding: "missing".into(),
        },
    )?;
    model.invalidate_reads();
    let status = model.status().to_owned();
    cleanup.release(0);
    drain(runtime, reads, path, model)?;
    assert_eq!(model.status(), status);
    assert!(finding_page(model).contains("world-news / one"));

    present(
        reads,
        runtime,
        path,
        model,
        &Effect::Finding {
            monitor: "missing".into(),
            finding: "missing".into(),
        },
    )?;
    drain(runtime, reads, path, model)?;
    assert_ne!(model.status(), status);
    assert!(finding_page(model).contains("world-news / one"));

    present(
        reads,
        runtime,
        path,
        model,
        &Effect::FindingOriginal {
            recording: "recording-one".into(),
        },
    )?;
    drain(runtime, reads, path, model)?;
    assert!(finding_page(model).contains("world-news / one"));
    assert!(model.playback().is_none());
    Ok(())
}

fn monitor_reads_keep_or_replace_by_epoch(
    runtime: &tokio::runtime::Runtime,
    path: &Path,
    reads: &mut Reads,
    cleanup: &mut Cleanup,
    model: &mut Explorer,
) -> TestResult {
    reads.withhold_next(cleanup.gate());
    present(reads, runtime, path, model, &Effect::MonitorList)?;
    model.invalidate_reads();
    cleanup.release(1);
    drain(runtime, reads, path, model)?;
    assert_eq!(model.monitors.selected(), Some("kept"));

    present(reads, runtime, path, model, &Effect::MonitorList)?;
    drain(runtime, reads, path, model)?;
    assert!(model.monitors.lines(8).join("\n").contains("No monitors"));

    present(
        reads,
        runtime,
        path,
        model,
        &Effect::MonitorDetail {
            id: "missing".into(),
        },
    )?;
    drain(runtime, reads, path, model)?;
    assert!(!model.status().contains("No work was admitted"));
    assert_eq!(model.link(), &Link::LocalCatalog);
    Ok(())
}

fn random_reads_keep_or_replace_by_epoch(
    runtime: &tokio::runtime::Runtime,
    path: &Path,
    reads: &mut Reads,
    cleanup: &mut Cleanup,
    model: &mut Explorer,
) -> TestResult {
    reads.withhold_next(cleanup.gate());
    let stale = model.handle(Key::Char('e'));
    assert!(matches!(stale, Effect::ExploreRandom(_)));
    present(reads, runtime, path, model, &stale)?;
    model.invalidate_reads();
    let held = model.status().to_owned();
    cleanup.release(2);
    drain(runtime, reads, path, model)?;
    assert_eq!(model.status(), held);
    assert!(
        !model
            .status()
            .contains("No cached stations with coordinates")
    );
    assert!(model.playback().is_none());

    let effect = model.handle(Key::Char('e'));
    assert!(matches!(effect, Effect::ExploreRandom(_)));
    present(reads, runtime, path, model, &effect)?;
    drain(runtime, reads, path, model)?;
    assert!(
        model
            .status()
            .contains("No cached stations with coordinates match these filters.")
    );
    assert!(model.playback().is_none());
    assert_eq!(model.link(), &Link::LocalCatalog);
    Ok(())
}

#[test]
fn linked_failure_is_kept_or_discarded_with_the_epoch() -> TestResult {
    let runtime = runtime()?;
    let (_library, path) = library()?;
    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    let mut reads = Reads::default();
    let mut model = seeded();
    let effect = model.handle(Key::Char('i'));
    reads.withhold_next(cleanup.gate());
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    model.invalidate_reads();
    cleanup.release(0);
    drain(&runtime, &mut reads, &path, &mut model)?;
    let stale = linked_message(&model);
    assert!(stale.contains("request pending"));
    assert!(!stale.contains("unavailable"));

    let mut model = seeded();
    let effect = model.handle(Key::Char('i'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert!(linked_message(&model).contains("Linked history unavailable"));
    assert!(model.playback().is_none());
    Ok(())
}

fn linked_message(model: &Explorer) -> String {
    model
        .context
        .as_ref()
        .map(|context| context.linked_message.clone())
        .unwrap_or_default()
}

#[test]
fn busy_favorite_is_not_cancelled_or_repeated() -> TestResult {
    let runtime = runtime()?;
    let (_library, path) = library()?;
    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    let mut reads = Reads::default();
    let mut model = with_station();
    reads.withhold_next(cleanup.gate());
    let effect = model.handle(Key::Char('f'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    assert_eq!(reads.favorite_started(), 1);
    let effect = model.handle(Key::Char('f'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    assert_eq!(reads.favorite_started(), 1);
    assert!(reads.favorite_busy());
    assert!(model.status().contains("not cancelled or repeated"));
    cleanup.release(0);
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert_eq!(reads.favorite_started(), 1);
    assert!(!reads.favorite_busy());
    assert!(!model.status().contains("not cancelled or repeated"));
    assert!(model.playback().is_none());
    Ok(())
}

#[test]
fn reload_keeps_player_status_and_service_reads_stay_local_to_ipc() -> TestResult {
    let runtime = runtime()?;
    let (library, path) = library()?;
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    model.observe_playback(PlaybackView {
        id: "kept-listen".into(),
        state: "running".into(),
        format: None,
        source_revision: "revision".into(),
        failure: None,
    });
    let mut reads = Reads::default();
    let effect = model.handle(Key::Char('r'));
    present(&mut reads, &runtime, &path, &mut model, &effect)?;
    drain(&runtime, &mut reads, &path, &mut model)?;
    assert_ne!(model.schema_version(), 0);
    assert_eq!(
        model.playback().map(|view| view.id.as_str()),
        Some("kept-listen")
    );
    assert_eq!(model.link(), &Link::LocalCatalog);

    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    let (shutdown, signal) = oneshot::channel();
    cleanup.shutdown = Some(shutdown);
    let opened = Library::open(library.path(), true)?;
    let service = runtime.spawn(control::run(opened, async {
        let _ = signal.await;
    }));
    let before = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(snapshot) = control::request(library.path(), Operation::Status {}).await {
                    break snapshot;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
    })?;
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    let effect = model.handle(Key::Char('g'));
    present(&mut reads, &runtime, library.path(), &mut model, &effect)?;
    drain(&runtime, &mut reads, library.path(), &mut model)?;
    assert!(matches!(model.link(), Link::Service { .. }));
    assert!(model.status().contains("Cached page read"));
    let after = runtime.block_on(control::request(library.path(), Operation::Status {}))?;
    assert_eq!(
        serde_json::to_value(&before.budgets)?,
        serde_json::to_value(&after.budgets)?
    );
    assert_eq!(
        serde_json::to_value(&before.captures)?,
        serde_json::to_value(&after.captures)?
    );
    assert_eq!(before.source_page.is_none(), after.source_page.is_none());
    let _ = cleanup.shutdown.take().map(|sender| sender.send(()));
    runtime.block_on(service)??;
    Ok(())
}

#[test]
fn withheld_metadata_leaves_render_back_stop_and_quit_running() -> TestResult {
    let runtime = runtime()?;
    let directory = tempfile::tempdir()?;
    let mut cleanup = Cleanup {
        gates: Vec::new(),
        shutdown: None,
    };
    let (shutdown, signal) = oneshot::channel();
    cleanup.shutdown = Some(shutdown);
    let opened = Library::open(directory.path(), true)?;
    let service = runtime.spawn(control::run(opened, async {
        let _ = signal.await;
    }));
    let before = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(snapshot) = control::request(directory.path(), Operation::Status {}).await
                {
                    break snapshot;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
    })?;
    let mut model = seeded();
    let effect = model.handle(Key::Char('i'));
    let mut reads = Reads::default();
    reads.withhold_next(cleanup.gate());
    present(&mut reads, &runtime, directory.path(), &mut model, &effect)?;
    assert!(reads.read_busy());
    let mut player = crate::explorer::player::Player::default();
    player.prepare_listen_identity("fixed-listen");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24))?;
    for _ in 0..100 {
        reads.poll(&runtime, directory.path(), &mut model);
        player.poll(&mut model);
        model.handle(Key::Redraw);
        model.handle(Key::Down);
        model.handle(Key::Up);
        terminal.draw(|frame| render(frame, &model))?;
    }
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(model.context.is_none());
    for _ in 0..100 {
        player.stop(&runtime, directory.path(), &mut model);
    }
    let player_wait = &mut player;
    let model_wait = &mut model;
    wait_until(&runtime, || {
        player_wait.poll(model_wait);
        reads.poll(&runtime, directory.path(), model_wait);
        !player_wait.stop_pending()
    })?;
    assert!(reads.read_busy());
    assert_eq!(player.observation_generation(), 2);
    assert_eq!(model.handle(Key::Quit), Effect::Detach);
    let during = runtime.block_on(control::request(directory.path(), Operation::Status {}))?;
    assert_eq!(
        serde_json::to_value(&before.budgets)?,
        serde_json::to_value(&during.budgets)?
    );
    assert_eq!(
        serde_json::to_value(&before.captures)?,
        serde_json::to_value(&during.captures)?
    );
    cleanup.release(0);
    drain(&runtime, &mut reads, directory.path(), &mut model)?;
    let after = runtime.block_on(control::request(directory.path(), Operation::Status {}))?;
    assert_eq!(
        serde_json::to_value(&before.budgets)?,
        serde_json::to_value(&after.budgets)?
    );
    assert_eq!(
        serde_json::to_value(&before.captures)?,
        serde_json::to_value(&after.captures)?
    );
    assert_eq!(before.source_page.is_none(), after.source_page.is_none());
    let _ = cleanup.shutdown.take().map(|sender| sender.send(()));
    runtime.block_on(service)??;
    Ok(())
}

fn with_station() -> Explorer {
    let mut model = explorer();
    model.note_link(Link::LocalCatalog);
    let Effect::Search(query) = model.handle(Key::Char('g')) else {
        panic!("search");
    };
    assert!(model.apply_ordered_search(
        query.generation,
        vec![row("station-a")],
        directory_view(),
        None,
        &catalog(),
    ));
    model
}

fn seeded() -> Explorer {
    let mut model = with_station();
    assert_eq!(model.handle(Key::Enter), Effect::None);
    model
}
