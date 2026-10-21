//! Bounded metadata reads. The terminal thread queues work and adopts fenced results.

use std::{path::Path, path::PathBuf, sync::Arc};

use sigy_service::{
    Error,
    control::{DirectoryCatalog, MonitorOperation, Snapshot},
};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

use super::{
    client,
    state::{Desk, Effect, Explorer, SearchQuery},
};

const UNRESOLVED: &str = "Metadata read ended without a result. The previous view was kept.";
const FAVORITE_BUSY: &str =
    "A favorite change is still settling. It was not cancelled or repeated.";
const FAVORITE_UNRESOLVED: &str = "Favorite change ended without a result. It was not repeated.";

#[derive(Clone)]
enum Kind {
    Search(SearchQuery),
    Random(SearchQuery),
    Linked {
        generation: u64,
        id: String,
        catalog: DirectoryCatalog,
    },
    Reload {
        query: SearchQuery,
        observed_ms: i64,
    },
    MonitorList,
    MonitorDetail {
        id: String,
    },
    Finding {
        monitor: String,
        finding: String,
        query: String,
    },
    FindingOriginal {
        recording: String,
        query: String,
    },
}

struct Job {
    epoch: u64,
    kind: Kind,
}

enum Attempt<T> {
    Finished(Box<Result<T, Error>>),
    Closed,
}

enum ReadOutcome {
    Search {
        epoch: u64,
        query: SearchQuery,
        result: Attempt<(bool, Snapshot)>,
    },
    Random {
        epoch: u64,
        query: SearchQuery,
        result: Attempt<(bool, Snapshot)>,
    },
    Linked {
        epoch: u64,
        generation: u64,
        id: String,
        catalog: DirectoryCatalog,
        result: Attempt<(bool, Snapshot)>,
    },
    Reload {
        epoch: u64,
        result: Attempt<Desk>,
    },
    MonitorList {
        epoch: u64,
        result: Attempt<(bool, Snapshot)>,
    },
    MonitorDetail {
        epoch: u64,
        result: Attempt<Vec<Result<(bool, Snapshot), Error>>>,
    },
    Finding {
        epoch: u64,
        monitor: String,
        finding: String,
        query: String,
        result: Attempt<(bool, Snapshot)>,
    },
    FindingOriginal {
        epoch: u64,
        recording: String,
        query: String,
        result: Attempt<(bool, Snapshot)>,
    },
}

enum FavoriteOutcome {
    Finished {
        generation: u64,
        id: String,
        favorite: bool,
        result: Box<Result<(bool, Snapshot), Error>>,
    },
    Closed {
        generation: u64,
    },
}

struct Slot<T> {
    token: u64,
    receive: mpsc::Receiver<T>,
    task: JoinHandle<()>,
    delivered: bool,
    fallback: Option<T>,
}

struct Gate {
    withhold: Option<tokio::sync::oneshot::Receiver<()>>,
    hold_after: Option<tokio::sync::oneshot::Receiver<()>>,
    close: bool,
}

pub(super) struct Reads {
    inflight: Option<Slot<ReadOutcome>>,
    favorites: Option<Slot<FavoriteOutcome>>,
    queued: Option<Job>,
    catalog: Arc<tokio::sync::Mutex<()>>,
    withhold: Option<tokio::sync::oneshot::Receiver<()>>,
    hold_after: Option<tokio::sync::oneshot::Receiver<()>>,
    close_next: bool,
    #[cfg(test)]
    read_started: u32,
    #[cfg(test)]
    favorite_started: u32,
}

impl Default for Reads {
    fn default() -> Self {
        Self {
            inflight: None,
            favorites: None,
            queued: None,
            catalog: Arc::new(tokio::sync::Mutex::new(())),
            withhold: None,
            hold_after: None,
            close_next: false,
            #[cfg(test)]
            read_started: 0,
            #[cfg(test)]
            favorite_started: 0,
        }
    }
}

impl Reads {
    pub(super) fn submit(
        &mut self,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
        effect: &Effect,
    ) {
        match effect {
            Effect::Search(query) => {
                self.enqueue(runtime, directory, model, Kind::Search(query.clone()));
            }
            Effect::ExploreRandom(query) => {
                self.enqueue(runtime, directory, model, Kind::Random(query.clone()));
            }
            Effect::LinkedContext {
                generation,
                id,
                catalog,
            } => self.enqueue(
                runtime,
                directory,
                model,
                Kind::Linked {
                    generation: *generation,
                    id: id.clone(),
                    catalog: catalog.clone(),
                },
            ),
            Effect::Reload => {
                if let Ok(now) = super::now_ms() {
                    model.tick_clock(now);
                }
                let kind = Kind::Reload {
                    query: model.current_search(),
                    observed_ms: model.now_ms(),
                };
                self.enqueue(runtime, directory, model, kind);
            }
            Effect::MonitorList => self.enqueue(runtime, directory, model, Kind::MonitorList),
            Effect::MonitorDetail { id } => self.enqueue(
                runtime,
                directory,
                model,
                Kind::MonitorDetail { id: id.clone() },
            ),
            Effect::Finding { monitor, finding } => self.enqueue(
                runtime,
                directory,
                model,
                Kind::Finding {
                    monitor: monitor.clone(),
                    finding: finding.clone(),
                    query: model.findings.query().to_owned(),
                },
            ),
            Effect::FindingOriginal { recording } => self.enqueue(
                runtime,
                directory,
                model,
                Kind::FindingOriginal {
                    recording: recording.clone(),
                    query: model.findings.query().to_owned(),
                },
            ),
            Effect::SetFavorite {
                generation,
                id,
                favorite,
            } => self.submit_favorite(runtime, directory, model, *generation, id, *favorite),
            Effect::None
            | Effect::Detach
            | Effect::Play { .. }
            | Effect::PlayRecording { .. }
            | Effect::PlayFinding { .. }
            | Effect::StopListen
            | Effect::StopRetained
            | Effect::Fetch(_) => {}
        }
    }

    pub(super) fn poll(&mut self, runtime: &Runtime, directory: &Path, model: &mut Explorer) {
        for outcome in poll_slot(&mut self.inflight) {
            adopt_read(model, outcome);
        }
        self.pump(runtime, directory, model);
        for outcome in poll_slot(&mut self.favorites) {
            adopt_favorite(model, outcome);
        }
    }

    fn enqueue(&mut self, runtime: &Runtime, directory: &Path, model: &mut Explorer, kind: Kind) {
        let Some(epoch) = model.begin_read() else {
            model
                .note_message("Metadata read generation is exhausted. The previous view was kept.");
            return;
        };
        let job = Job { epoch, kind };
        if self.inflight.is_some() {
            self.queued = Some(job);
            return;
        }
        self.spawn_read(runtime, directory, job);
    }

    fn pump(&mut self, runtime: &Runtime, directory: &Path, model: &Explorer) {
        if self.inflight.is_some() {
            return;
        }
        let Some(job) = self.queued.take() else {
            return;
        };
        if job.epoch == model.read_epoch() {
            self.spawn_read(runtime, directory, job);
        }
    }

    fn spawn_read(&mut self, runtime: &Runtime, directory: &Path, job: Job) {
        let (send, receive) = mpsc::channel(2);
        let fallback = closed_read(job.epoch, &job.kind);
        let epoch = job.epoch;
        let kind = job.kind;
        let directory = directory.to_path_buf();
        let catalog = Arc::clone(&self.catalog);
        let gate = self.take_gate();
        #[cfg(test)]
        {
            self.read_started = self.read_started.saturating_add(1);
        }
        // Dropping this handle does not cancel the task. Shutdown still waits for it.
        let task = runtime.spawn(async move {
            Box::pin(run_read(directory, catalog, epoch, kind, send, gate)).await;
        });
        self.inflight = Some(Slot {
            token: epoch,
            receive,
            task,
            delivered: false,
            fallback: Some(fallback),
        });
    }

    fn submit_favorite(
        &mut self,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
        generation: u64,
        id: &str,
        favorite: bool,
    ) {
        if let Some(inflight) = self.favorites.as_ref().map(|slot| slot.token) {
            model.restore_pending_favorite(inflight);
            model.note_message(FAVORITE_BUSY);
            return;
        }
        let (send, receive) = mpsc::channel(2);
        let directory = directory.to_path_buf();
        let catalog = Arc::clone(&self.catalog);
        let gate = self.take_gate();
        let id_owned = id.to_owned();
        #[cfg(test)]
        {
            self.favorite_started = self.favorite_started.saturating_add(1);
        }
        let task = runtime.spawn(async move {
            run_favorite(
                directory, catalog, generation, id_owned, favorite, send, gate,
            )
            .await;
        });
        self.favorites = Some(Slot {
            token: generation,
            receive,
            task,
            delivered: false,
            fallback: Some(FavoriteOutcome::Closed { generation }),
        });
    }

    fn take_gate(&mut self) -> Gate {
        Gate {
            withhold: self.withhold.take(),
            hold_after: self.hold_after.take(),
            close: std::mem::take(&mut self.close_next),
        }
    }
}

#[cfg(test)]
impl Reads {
    pub(super) fn withhold_next(&mut self, gate: tokio::sync::oneshot::Receiver<()>) {
        self.withhold = Some(gate);
    }

    pub(super) fn hold_after_send(&mut self, gate: tokio::sync::oneshot::Receiver<()>) {
        self.hold_after = Some(gate);
    }

    pub(super) fn close_next(&mut self) {
        self.close_next = true;
    }

    #[must_use]
    pub(super) const fn read_started(&self) -> u32 {
        self.read_started
    }

    #[must_use]
    pub(super) const fn favorite_started(&self) -> u32 {
        self.favorite_started
    }

    #[must_use]
    pub(super) const fn read_busy(&self) -> bool {
        self.inflight.is_some()
    }

    #[must_use]
    pub(super) const fn favorite_busy(&self) -> bool {
        self.favorites.is_some()
    }

    #[must_use]
    pub(super) fn queued_epoch(&self) -> Option<u64> {
        self.queued.as_ref().map(|job| job.epoch)
    }

    #[must_use]
    pub(super) fn inflight_epoch(&self) -> Option<u64> {
        self.inflight.as_ref().map(|slot| slot.token)
    }
}

fn poll_slot<T>(slot: &mut Option<Slot<T>>) -> Vec<T> {
    let Some(pending) = slot.as_mut() else {
        return Vec::new();
    };
    let mut updates = Vec::with_capacity(2);
    for _ in 0..2 {
        match pending.receive.try_recv() {
            Ok(update) => {
                pending.delivered = true;
                updates.push(update);
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                if !pending.delivered
                    && let Some(fallback) = pending.fallback.take()
                {
                    pending.delivered = true;
                    updates.push(fallback);
                }
                break;
            }
        }
    }
    let done = pending.task.is_finished() && pending.receive.is_empty();
    if done {
        *slot = None;
    }
    updates
}

async fn run_read(
    directory: PathBuf,
    catalog: Arc<tokio::sync::Mutex<()>>,
    epoch: u64,
    kind: Kind,
    send: mpsc::Sender<ReadOutcome>,
    gate: Gate,
) {
    if let Some(wait) = gate.withhold {
        let _ = wait.await;
    }
    if gate.close {
        return;
    }
    {
        let _guard = catalog.lock().await;
        let outcome = Box::pin(execute(&directory, epoch, kind)).await;
        let _ = send.send(outcome).await;
    }
    if let Some(wait) = gate.hold_after {
        let _ = wait.await;
    }
}

async fn run_favorite(
    directory: PathBuf,
    catalog: Arc<tokio::sync::Mutex<()>>,
    generation: u64,
    id: String,
    favorite: bool,
    send: mpsc::Sender<FavoriteOutcome>,
    gate: Gate,
) {
    if let Some(wait) = gate.withhold {
        let _ = wait.await;
    }
    if gate.close {
        return;
    }
    {
        let _guard = catalog.lock().await;
        let result = client::fetch(&directory, client::favorite_operation(&id, favorite)).await;
        let _ = send
            .send(FavoriteOutcome::Finished {
                generation,
                id,
                favorite,
                result: Box::new(result),
            })
            .await;
    }
    if let Some(wait) = gate.hold_after {
        let _ = wait.await;
    }
}

async fn execute(directory: &Path, epoch: u64, kind: Kind) -> ReadOutcome {
    match kind {
        Kind::Search(query) => ReadOutcome::Search {
            epoch,
            result: finish(client::fetch(directory, client::search_operation(&query)).await),
            query,
        },
        Kind::Random(query) => ReadOutcome::Random {
            epoch,
            result: finish(client::fetch(directory, client::random_operation(&query)).await),
            query,
        },
        Kind::Linked {
            generation,
            id,
            catalog,
        } => {
            let operation = sigy_service::control::Operation::Radio {
                command: sigy_service::control::DirectoryOperation::Linked {
                    id: id.clone(),
                    catalog: Some(catalog.clone()),
                },
            };
            ReadOutcome::Linked {
                epoch,
                generation,
                result: finish(client::fetch(directory, operation).await),
                id,
                catalog,
            }
        }
        Kind::Reload { query, observed_ms } => ReadOutcome::Reload {
            epoch,
            result: finish(Box::pin(client::load_desk(directory, &query, observed_ms)).await),
        },
        Kind::MonitorList => ReadOutcome::MonitorList {
            epoch,
            result: finish(
                client::fetch(
                    directory,
                    client::monitor_operation(MonitorOperation::List {}),
                )
                .await,
            ),
        },
        Kind::MonitorDetail { id } => ReadOutcome::MonitorDetail {
            epoch,
            result: finish(Ok(client::monitor_detail_steps(directory, &id).await)),
        },
        Kind::Finding {
            monitor,
            finding,
            query,
        } => ReadOutcome::Finding {
            epoch,
            result: finish(
                client::fetch(directory, client::finding_operation(&monitor, &finding)).await,
            ),
            monitor,
            finding,
            query,
        },
        Kind::FindingOriginal { recording, query } => ReadOutcome::FindingOriginal {
            epoch,
            result: finish(client::fetch(directory, client::original_operation(&recording)).await),
            recording,
            query,
        },
    }
}

fn finish<T>(result: Result<T, Error>) -> Attempt<T> {
    Attempt::Finished(Box::new(result))
}

fn closed_read(epoch: u64, kind: &Kind) -> ReadOutcome {
    match kind {
        Kind::Search(query) => ReadOutcome::Search {
            epoch,
            query: query.clone(),
            result: Attempt::Closed,
        },
        Kind::Random(query) => ReadOutcome::Random {
            epoch,
            query: query.clone(),
            result: Attempt::Closed,
        },
        Kind::Linked {
            generation,
            id,
            catalog,
        } => ReadOutcome::Linked {
            epoch,
            generation: *generation,
            id: id.clone(),
            catalog: catalog.clone(),
            result: Attempt::Closed,
        },
        Kind::Reload { .. } => ReadOutcome::Reload {
            epoch,
            result: Attempt::Closed,
        },
        Kind::MonitorList => ReadOutcome::MonitorList {
            epoch,
            result: Attempt::Closed,
        },
        Kind::MonitorDetail { .. } => ReadOutcome::MonitorDetail {
            epoch,
            result: Attempt::Closed,
        },
        Kind::Finding {
            monitor,
            finding,
            query,
        } => ReadOutcome::Finding {
            epoch,
            monitor: monitor.clone(),
            finding: finding.clone(),
            query: query.clone(),
            result: Attempt::Closed,
        },
        Kind::FindingOriginal { recording, query } => ReadOutcome::FindingOriginal {
            epoch,
            recording: recording.clone(),
            query: query.clone(),
            result: Attempt::Closed,
        },
    }
}

fn adopt_read(model: &mut Explorer, outcome: ReadOutcome) {
    if !current(model, epoch_of(&outcome)) {
        return;
    }
    match outcome {
        ReadOutcome::Search { query, result, .. } => adopt_query(model, &query, result, false),
        ReadOutcome::Random { query, result, .. } => adopt_query(model, &query, result, true),
        ReadOutcome::Linked {
            generation,
            id,
            catalog,
            result,
            ..
        } => match result {
            Attempt::Closed => model.fail_linked_context(generation, UNRESOLVED),
            Attempt::Finished(result) => {
                client::adopt_linked(model, generation, &id, &catalog, *result);
            }
        },
        ReadOutcome::Reload { result, .. } => adopt_reload(model, result),
        ReadOutcome::MonitorList { result, .. } => match result {
            Attempt::Closed => model.note_message(UNRESOLVED),
            Attempt::Finished(result) => client::adopt_monitor_list(model, *result),
        },
        ReadOutcome::MonitorDetail { result, .. } => match result {
            Attempt::Closed => model.note_message(UNRESOLVED),
            Attempt::Finished(result) => match *result {
                Ok(steps) => client::adopt_monitor_detail(model, steps),
                Err(error) => client::adopt_monitor_detail(model, vec![Err(error)]),
            },
        },
        ReadOutcome::Finding {
            monitor,
            finding,
            query,
            result,
            ..
        } => {
            if model.findings.query() != query {
                return;
            }
            match result {
                Attempt::Closed => model.note_message(UNRESOLVED),
                Attempt::Finished(result) => {
                    client::adopt_finding(model, &monitor, &finding, *result);
                }
            }
        }
        ReadOutcome::FindingOriginal {
            recording,
            query,
            result,
            ..
        } => {
            if model.findings.query() != query {
                return;
            }
            match result {
                Attempt::Closed => model.note_message(UNRESOLVED),
                Attempt::Finished(result) => client::adopt_original(model, &recording, *result),
            }
        }
    }
}

fn current(model: &Explorer, epoch: u64) -> bool {
    model.read_epoch() == epoch
}

fn epoch_of(outcome: &ReadOutcome) -> u64 {
    match outcome {
        ReadOutcome::Search { epoch, .. }
        | ReadOutcome::Random { epoch, .. }
        | ReadOutcome::Linked { epoch, .. }
        | ReadOutcome::Reload { epoch, .. }
        | ReadOutcome::MonitorList { epoch, .. }
        | ReadOutcome::MonitorDetail { epoch, .. }
        | ReadOutcome::Finding { epoch, .. }
        | ReadOutcome::FindingOriginal { epoch, .. } => *epoch,
    }
}

fn adopt_query(
    model: &mut Explorer,
    query: &SearchQuery,
    result: Attempt<(bool, Snapshot)>,
    random: bool,
) {
    match result {
        Attempt::Closed => {
            let _ = model.fail_search(query.generation, UNRESOLVED);
        }
        Attempt::Finished(result) => {
            if random {
                client::adopt_random(model, query, *result);
            } else {
                client::adopt_search(model, query, *result);
            }
        }
    }
}

fn adopt_reload(model: &mut Explorer, result: Attempt<Desk>) {
    match result {
        Attempt::Closed => model.note_message(UNRESOLVED),
        Attempt::Finished(result) => match *result {
            Ok(desk) => {
                let playback = model.playback_snapshot();
                model.apply_desk(desk);
                model.restore_playback(playback);
            }
            Err(error) => client::report(model, &error),
        },
    }
}

fn adopt_favorite(model: &mut Explorer, outcome: FavoriteOutcome) {
    match outcome {
        FavoriteOutcome::Closed { generation } => {
            let _ = model.fail_favorite(generation, FAVORITE_UNRESOLVED);
        }
        FavoriteOutcome::Finished {
            generation,
            id,
            favorite,
            result,
        } => client::adopt_favorite(model, generation, &id, favorite, *result),
    }
}

#[cfg(test)]
mod tests;
