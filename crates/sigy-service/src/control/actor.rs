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

pub(super) enum Message {
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

struct Actor {
    library: Library,
    runtime: tokio::runtime::Handle,
    sender: mpsc::WeakSender<Message>,
    workers: HashMap<String, Worker>,
    acquirer: HttpAcquirer,
}

impl Actor {
    fn apply(&mut self, operation: Operation) -> Result<Snapshot> {
        let operation = match operation {
            Operation::Record {
                command:
                    RecordingOperation::Start {
                        id,
                        source_revision,
                        seconds,
                        maximum_bytes,
                        retention,
                    },
            } => {
                self.start(&id, &source_revision, seconds, maximum_bytes, retention)?;
                Operation::Record {
                    command: RecordingOperation::Show { id },
                }
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
            other => other,
        };
        let mut snapshot = apply_library(&mut self.library, operation)?;
        snapshot.captures.dispatch_available = self.library.store().dvr_status()?.decoder.is_some();
        Ok(snapshot)
    }

    fn start(
        &mut self,
        id: &str,
        source_id: &str,
        seconds: u64,
        maximum: u64,
        retention: crate::storage::dvr::Retention,
    ) -> Result<()> {
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
            .admit_recording(id, source_id, seconds, maximum, retention);
        if matches!(admitted, Err(Error::StorageQuota)) {
            self.reclaim(maximum)?;
            admitted = self
                .library
                .store_mut()
                .admit_recording(id, source_id, seconds, maximum, retention);
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
        let sender = self.sender.upgrade().ok_or(Error::ServiceStopped)?;
        let acquirer = self.acquirer.clone();
        let (stop, signal) = watch::channel(false);
        let ownership = self.library.hold_ownership();
        let worker_id = id.to_owned();
        let runtime = self.runtime.clone();
        let task = self.runtime.spawn(async move {
            let inner = runtime.spawn(async move {
                let _ownership = ownership;
                recordings::capture(directory, key, source, limits, decoder, acquirer, signal).await
            });
            let guard = AbortTask(inner.abort_handle());
            let result = inner
                .await
                .unwrap_or(Err(Error::Acquisition("recording worker failed")));
            drop(guard);
            let _ = sender
                .send(Message::Finished {
                    id: worker_id,
                    generation,
                    result,
                })
                .await;
        });
        self.workers.insert(id.into(), Worker { stop, task });
        Ok(())
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
        for worker in self.workers.values() {
            worker.stop.send_replace(true);
        }
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
        acquirer: HttpAcquirer::default(),
    };
    let thread = thread::Builder::new()
        .name("sigy-catalog".into())
        .spawn(move || {
            let started = Instant::now();
            let mut stopped = false;
            let mut shutdown = false;
            while let Some(message) = receiver.blocking_recv() {
                match message {
                    Message::Sweep => {
                        if !stopped && recordings::prune(&mut actor.library).is_err() {
                            stopped = true;
                            actor.stop();
                            stopping.send_replace(true);
                        }
                    }
                    Message::Shutdown => {
                        stopped = true;
                        shutdown = true;
                        actor.stop();
                    }
                    Message::Finished {
                        id,
                        generation,
                        result,
                    } => {
                        if actor.finish(&id, generation, result).is_err() {
                            stopped = true;
                            actor.stop();
                            stopping.send_replace(true);
                        }
                    }
                    Message::Request { operation, reply } => {
                        let stop = matches!(operation, Operation::Stop {});
                        let result = if stopped {
                            Err(Error::ServiceStopped)
                        } else {
                            actor.apply(operation)
                        };
                        if stop && result.is_ok() {
                            stopped = true;
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
                }
                if shutdown && actor.workers.is_empty() {
                    break;
                }
            }
        })?;
    Ok((sender, thread))
}

fn failure(error: &Error) -> Failure {
    Failure {
        code: match error {
            Error::InvalidInput(_) | Error::Money(_) => "invalid_request",
            Error::DestinationDenied => "destination_denied",
            Error::IdempotencyConflict => "idempotency_conflict",
            Error::NotFound => "not_found",
            Error::SourceCapacity | Error::CaptureCapacity => "capacity",
            Error::StorageQuota => "storage_quota",
            Error::Budget(_) => "budget_rejected",
            Error::ServiceStopped => "stopping",
            _ => "catalog_failure",
        }
        .into(),
        message: error.to_string(),
    }
}
