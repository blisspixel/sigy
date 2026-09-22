use super::{
    Failure, Operation, PROTOCOL_VERSION, RecordingOperation, Response, ServiceView, Snapshot,
    apply_library,
};
use crate::{
    Error, Result, library::Library, recordings, sources::http::HttpAcquirer,
    storage::dvr::Publication,
};
use std::{collections::HashMap, thread, time::Instant};
use tokio::sync::{mpsc, oneshot, watch};

mod click;
mod discovery;
mod listen;
mod playlist;

pub(super) enum Message {
    DirectoryFinished {
        id: String,
        result: Result<crate::discovery::RefreshBatch>,
    },
    PlaylistFinished {
        id: String,
        result: Result<crate::sources::playlist::ResolvedPlaylist>,
    },
    ClickFinished {
        id: String,
        result: Result<String>,
    },
    ListenReady {
        id: String,
        nonce: String,
        format: String,
    },
    ListenFinished {
        id: String,
        result: Result<String>,
    },
    Sweep,
    Request {
        operation: Operation,
        reply: oneshot::Sender<Response>,
    },
    Finished {
        id: String,
        generation: i64,
        result: Result<Publication>,
    },
    Shutdown,
}

struct Worker {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct AbortTask(tokio::task::AbortHandle);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct LiveListen {
    worker: Worker,
    pipe_nonce: Option<String>,
    format: Option<String>,
}

struct Actor {
    library: Library,
    runtime: tokio::runtime::Handle,
    sender: mpsc::WeakSender<Message>,
    workers: HashMap<String, Worker>,
    directory_worker: Option<Worker>,
    playlist_worker: Option<Worker>,
    click_worker: Option<Worker>,
    listen_workers: HashMap<String, LiveListen>,
    acquirer: HttpAcquirer,
}

impl Actor {
    fn apply(&mut self, operation: Operation) -> Result<Snapshot> {
        let mut created = None;
        let operation = match operation {
            Operation::Radio {
                command: super::DirectoryOperation::Refresh { id, request },
            } => {
                self.start_directory(&id, request)?;
                Operation::Radio {
                    command: super::DirectoryOperation::RefreshStatus { id },
                }
            }
            Operation::Playlist {
                command: super::PlaylistOperation::Resolve { id, revision_id },
            } => {
                self.start_playlist(&id, &revision_id)?;
                Operation::Playlist {
                    command: super::PlaylistOperation::Status { id },
                }
            }
            Operation::Radio {
                command: super::DirectoryOperation::Click { id, request },
            } => {
                self.start_click(&id, request)?;
                Operation::Radio {
                    command: super::DirectoryOperation::ClickStatus { id },
                }
            }
            Operation::Record { command }
                if matches!(
                    command,
                    RecordingOperation::Start { .. } | RecordingOperation::Hls { .. }
                ) =>
            {
                self.launch_recording(RecordingLaunch::from_command(command)?)?
            }
            Operation::Record {
                command: RecordingOperation::Stop { id },
            } => {
                self.library.store().recording(&id)?;
                if let Some(worker) = self.workers.get(&id) {
                    worker.stop.send_replace(true);
                }
                Operation::Record {
                    command: RecordingOperation::Show { id },
                }
            }
            Operation::Listen {
                command: super::ListenOperation::Start { id, revision_id },
            } => {
                created = Some(self.start_listen(&id, &revision_id)?);
                Operation::Listen {
                    command: super::ListenOperation::Status { id },
                }
            }
            Operation::Listen {
                command: super::ListenOperation::Stop { id },
            } => {
                self.stop_listen(&id)?;
                Operation::Listen {
                    command: super::ListenOperation::Status { id },
                }
            }
            other => other,
        };
        let mut snapshot = apply_library(&mut self.library, operation)?;
        if let Some(listen) = snapshot.listen.as_mut() {
            if let Some(created) = created {
                listen.newly_started = Some(created);
            }
            if created != Some(false) {
                self.attach_listen(listen);
            }
        }
        snapshot.captures.dispatch_available = self.library.store().dvr_status()?.decoder.is_some();
        Ok(snapshot)
    }

    fn launch_recording(&mut self, request: RecordingLaunch) -> Result<Operation> {
        let id = request.id.clone();
        self.start(request)?;
        Ok(Operation::Record {
            command: RecordingOperation::Show { id },
        })
    }

    fn start(&mut self, request: RecordingLaunch) -> Result<()> {
        let RecordingLaunch {
            id,
            source_revision,
            seconds,
            maximum_bytes: maximum,
            retention,
            hls,
            icy,
        } = request;
        let id_owned = id;
        let id = id_owned.as_str();
        let source_id = source_revision.as_str();
        let source = self
            .library
            .store()
            .source(source_id)?
            .ok_or(Error::NotFound)?
            .source;
        let decoder = self
            .library
            .store()
            .dvr_status()?
            .decoder
            .ok_or(Error::InvalidInput(
                "DVR decoder is not configured; use dvr configure",
            ))?;
        let limits = recordings::limits(maximum, seconds)?;
        let mut admitted = self
            .library
            .store_mut()
            .admit_recording(id, source_id, seconds, maximum, retention, icy);
        if matches!(admitted, Err(Error::StorageQuota)) {
            self.reclaim(maximum)?;
            admitted = self
                .library
                .store_mut()
                .admit_recording(id, source_id, seconds, maximum, retention, icy);
        }
        let Some(job) = admitted? else {
            return Ok(());
        };
        let setup = (|| {
            recordings::check_free_space(&self.library)?;
            let record = self.library.store().recording(id)?;
            let directory = self.library.directory().to_owned();
            recordings::checked_directory(&directory, true)?;
            Ok((record.object_key, directory))
        })();
        let (key, directory) = match setup {
            Ok(prepared) => prepared,
            Err(error) => {
                self.library
                    .store_mut()
                    .fail_recording(&job.version, &error)?;
                return Err(error);
            }
        };
        let generation = job.version.generation();
        let acquirer = self.acquirer.clone();
        let worker_id = id.to_owned();
        let worker = self.spawn_worker(
            move |signal| {
                recordings::capture(
                    recordings::CaptureRequest {
                        directory,
                        key,
                        source,
                        limits,
                        decoder,
                        acquirer,
                        hls,
                        icy,
                    },
                    signal,
                )
            },
            move |result| Message::Finished {
                id: worker_id,
                generation,
                result,
            },
        )?;
        self.workers.insert(id.into(), worker);
        Ok(())
    }

    fn spawn_worker<T, F>(
        &self,
        work: impl FnOnce(watch::Receiver<bool>) -> F + Send + 'static,
        finished: impl FnOnce(Result<T>) -> Message + Send + 'static,
    ) -> Result<Worker>
    where
        T: Send + 'static,
        F: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let sender = self.sender.upgrade().ok_or(Error::ServiceStopped)?;
        let (stop, signal) = watch::channel(false);
        let ownership = self.library.hold_ownership();
        let inner = self.runtime.spawn(async move {
            let _ownership = ownership;
            work(signal).await
        });
        let task = self.runtime.spawn(async move {
            let guard = AbortTask(inner.abort_handle());
            let result = inner
                .await
                .unwrap_or(Err(Error::Acquisition("service worker failed")));
            drop(guard);
            let _ = sender.send(finished(result)).await;
        });
        Ok(Worker { stop, task })
    }

    fn reclaim(&mut self, requested: u64) -> Result<()> {
        if requested > self.library.store().dvr_status()?.quota_bytes {
            return Err(Error::StorageQuota);
        }
        loop {
            if self.library.store().dvr_status()?.available_bytes >= requested {
                return Ok(());
            }
            let candidates = self.library.store().prune_candidates(true)?;
            if candidates.is_empty() {
                return Err(Error::StorageQuota);
            }
            for id in candidates {
                recordings::delete(&mut self.library, &id, true)?;
                if self.library.store().dvr_status()?.available_bytes >= requested {
                    return Ok(());
                }
            }
        }
    }

    fn finish(&mut self, id: &str, generation: i64, result: Result<Publication>) -> Result<()> {
        self.workers.remove(id);
        let job = self.library.store().capture(id)?.ok_or(Error::NotFound)?;
        if job.version.generation() != generation {
            return Err(Error::StaleCapture);
        }
        match result {
            Ok(publication) => self
                .library
                .store_mut()
                .publish_recording(&job.version, &publication),
            Err(error) => {
                // Failed bytes retain their reservation and are not offered for playback.
                self.library
                    .store_mut()
                    .fail_recording(&job.version, &error)?;
                Ok(())
            }
        }
    }

    fn stop(&self) {
        if let Some(worker) = &self.directory_worker {
            worker.stop.send_replace(true);
        }
        if let Some(worker) = &self.playlist_worker {
            worker.stop.send_replace(true);
        }
        if let Some(worker) = &self.click_worker {
            worker.stop.send_replace(true);
        }
        for session in self.listen_workers.values() {
            session.worker.stop.send_replace(true);
        }
        for worker in self.workers.values() {
            worker.stop.send_replace(true);
        }
    }

    fn idle(&self) -> bool {
        self.workers.is_empty()
            && self.listen_workers.is_empty()
            && self.directory_worker.is_none()
            && self.playlist_worker.is_none()
            && self.click_worker.is_none()
    }
}

fn mark_failed(
    actor: &mut Actor,
    stopping: &watch::Sender<bool>,
    stopped: &mut bool,
    failed: bool,
) {
    if failed {
        *stopped = true;
        actor.stop();
        stopping.send_replace(true);
    }
}

pub(super) fn spawn(
    library: Library,
    stopping: watch::Sender<bool>,
) -> Result<(mpsc::Sender<Message>, thread::JoinHandle<()>)> {
    let (sender, mut receiver) = mpsc::channel::<Message>(super::MAX_CLIENTS);
    let mut actor = Actor {
        library,
        runtime: tokio::runtime::Handle::current(),
        sender: sender.downgrade(),
        workers: HashMap::new(),
        directory_worker: None,
        playlist_worker: None,
        click_worker: None,
        listen_workers: HashMap::new(),
        acquirer: HttpAcquirer::default(),
    };
    let thread = thread::Builder::new()
        .name("sigy-catalog".into())
        .spawn(move || {
            let started = Instant::now();
            let mut stopped = false;
            let mut shutdown = false;
            while let Some(message) = receiver.blocking_recv() {
                dispatch(
                    &mut actor,
                    message,
                    &stopping,
                    &mut stopped,
                    &mut shutdown,
                    started,
                );
                if shutdown && actor.idle() {
                    break;
                }
            }
        })?;
    Ok((sender, thread))
}

fn dispatch(
    actor: &mut Actor,
    message: Message,
    stopping: &watch::Sender<bool>,
    stopped: &mut bool,
    shutdown: &mut bool,
    started: Instant,
) {
    match message {
        Message::DirectoryFinished { id, result } => {
            let failed = actor.finish_directory(&id, result).is_err();
            mark_failed(actor, stopping, stopped, failed);
        }
        Message::PlaylistFinished { id, result } => {
            let failed = actor.finish_playlist(&id, result).is_err();
            mark_failed(actor, stopping, stopped, failed);
        }
        Message::ClickFinished { id, result } => {
            let failed = actor.finish_click(&id, result).is_err();
            mark_failed(actor, stopping, stopped, failed);
        }
        Message::ListenReady { id, nonce, format } => actor.ready_listen(&id, nonce, format),
        Message::ListenFinished { id, result } => {
            let failed = actor.finish_listen(&id, result).is_err();
            mark_failed(actor, stopping, stopped, failed);
        }
        Message::Sweep => {
            let failed = !*stopped && recordings::prune(&mut actor.library).is_err();
            mark_failed(actor, stopping, stopped, failed);
        }
        Message::Shutdown => {
            *stopped = true;
            *shutdown = true;
            actor.stop();
        }
        Message::Finished {
            id,
            generation,
            result,
        } => {
            let failed = actor.finish(&id, generation, result).is_err();
            mark_failed(actor, stopping, stopped, failed);
        }
        Message::Request { operation, reply } => {
            answer(actor, operation, reply, stopping, stopped, started);
        }
    }
}

fn answer(
    actor: &mut Actor,
    operation: Operation,
    reply: oneshot::Sender<Response>,
    stopping: &watch::Sender<bool>,
    stopped: &mut bool,
    started: Instant,
) {
    let stop = matches!(operation, Operation::Stop {});
    let result = if *stopped {
        Err(Error::ServiceStopped)
    } else {
        actor.apply(operation)
    };
    if stop && result.is_ok() {
        *stopped = true;
        actor.stop();
        stopping.send_replace(true);
    }
    let result = result
        .map(|mut snapshot| {
            snapshot.service = Some(ServiceView {
                process_id: std::process::id(),
                uptime_seconds: started.elapsed().as_secs(),
                stopping: stop,
                maximum_clients: super::MAX_CLIENTS,
            });
            snapshot
        })
        .map_err(|error| failure(&error));
    let _ = reply.send(Response {
        version: PROTOCOL_VERSION,
        result,
    });
}

fn failure(error: &Error) -> Failure {
    Failure {
        code: match error {
            Error::InvalidInput(_) | Error::Money(_) => "invalid_request",
            Error::DestinationDenied => "destination_denied",
            Error::IdempotencyConflict => "idempotency_conflict",
            Error::NotFound => "not_found",
            Error::SourceCapacity | Error::CaptureCapacity | Error::PodcastCapacity => "capacity",
            Error::StorageQuota => "storage_quota",
            Error::Budget(_) => "budget_rejected",
            Error::ServiceStopped => "stopping",
            _ => "catalog_failure",
        }
        .into(),
        message: error.to_string(),
    }
}

struct RecordingLaunch {
    id: String,
    source_revision: String,
    seconds: u64,
    maximum_bytes: u64,
    retention: crate::storage::dvr::Retention,
    hls: bool,
    icy: bool,
}

impl RecordingLaunch {
    fn from_command(command: RecordingOperation) -> Result<Self> {
        Ok(match command {
            RecordingOperation::Start {
                id,
                source_revision,
                seconds,
                maximum_bytes,
                retention,
                icy,
            } => Self {
                id,
                source_revision,
                seconds,
                maximum_bytes,
                retention,
                hls: false,
                icy,
            },
            RecordingOperation::Hls {
                id,
                source_revision,
                seconds,
                maximum_bytes,
                retention,
            } => Self {
                id,
                source_revision,
                seconds,
                maximum_bytes,
                retention,
                hls: true,
                icy: false,
            },
            _ => return Err(Error::InvalidInput("recording command is not a start")),
        })
    }
}
