use super::{Actor, Message, Worker};
use crate::{
    Error, Result,
    control::{RetainedOperation, RetainedPage, RetainedReadSpec, Snapshot},
    recordings::retained::{RetainedReadReceipt, stream_retained},
    storage::Store,
};

pub(super) struct LiveRetained {
    worker: Worker,
    spec: RetainedReadSpec,
    nonce: Option<String>,
}

impl Actor {
    pub(super) fn apply_retained(&mut self, command: RetainedOperation) -> Result<Snapshot> {
        let (id, created) = match command {
            command @ (RetainedOperation::Start { .. }
            | RetainedOperation::StartRange { .. }
            | RetainedOperation::StartFinding { .. }) => {
                let id = match &command {
                    RetainedOperation::Start { id, .. }
                    | RetainedOperation::StartRange { id, .. }
                    | RetainedOperation::StartFinding { id, .. } => id.clone(),
                    _ => return Err(Error::RequestState),
                };
                let created = self.start_retained(&id, &command)?;
                (Some(id), Some(created))
            }
            RetainedOperation::Stop { id, generation } => {
                self.library.store_mut().cancel_retained_reader(
                    &id,
                    generation,
                    Store::clock_ms()?,
                )?;
                if let Some(live) = self.retained_workers.get(&id)
                    && live.spec.generation == generation
                {
                    live.worker.stop.send_replace(true);
                }
                (Some(id), None)
            }
            RetainedOperation::Show { id } => (Some(id), None),
            RetainedOperation::List {} => (None, None),
        };
        let query = id
            .as_ref()
            .map_or(RetainedOperation::List {}, |id| RetainedOperation::Show {
                id: id.clone(),
            });
        let mut snapshot = crate::control::retained::apply(self.library.store(), query)?;
        if let Some(page) = snapshot.retained.as_mut() {
            page.newly_started = created;
            if created != Some(false) && id.is_some() {
                self.attach_retained(page);
            }
        }
        snapshot.captures.dispatch_available = self.library.store().dvr_status()?.decoder.is_some();
        Ok(snapshot)
    }

    fn start_retained(&mut self, id: &str, command: &RetainedOperation) -> Result<bool> {
        let sender = self.sender.upgrade().ok_or(Error::ServiceStopped)?;
        let now = Store::clock_ms()?;
        let (spec, created) = match command {
            RetainedOperation::Start {
                recording_id,
                seek_us,
                ..
            } => self
                .library
                .store_mut()
                .admit_retained_reader(id, recording_id, *seek_us, now)?,
            RetainedOperation::StartRange {
                recording_id,
                seek_us,
                end_us,
                ..
            } => self.library.store_mut().admit_retained_range(
                id,
                recording_id,
                *seek_us,
                *end_us,
                now,
            )?,
            RetainedOperation::StartFinding {
                monitor_id,
                finding_id,
                ..
            } => self
                .library
                .store_mut()
                .admit_retained_finding(id, monitor_id, finding_id, now)?,
            _ => return Err(Error::RequestState),
        };
        if !created {
            return Ok(false);
        }
        let directory = self.library.directory().to_owned();
        let ownership = self.library.hold_ownership();
        let ready_id = id.to_owned();
        let worker_id = id.to_owned();
        let generation = spec.generation;
        let worker_spec = spec.clone();
        let spawned = self.spawn_worker(
            move |signal| async move {
                stream_retained(
                    directory,
                    worker_spec,
                    ownership,
                    signal,
                    move |nonce| async move {
                        sender
                            .send(Message::RetainedReady {
                                id: ready_id,
                                generation,
                                nonce,
                            })
                            .await
                            .map_err(|_| Error::ServiceStopped)
                    },
                )
                .await
            },
            move |result| Message::RetainedFinished {
                id: worker_id,
                generation,
                result,
            },
        );
        match spawned {
            Ok(worker) => {
                self.retained_workers.insert(
                    id.to_owned(),
                    LiveRetained {
                        worker,
                        spec,
                        nonce: None,
                    },
                );
            }
            Err(error) => {
                self.library.store_mut().hold_retained_reader(
                    id,
                    generation,
                    "worker-start-unproven",
                    Store::clock_ms()?,
                )?;
                return Err(error);
            }
        }
        Ok(true)
    }

    pub(super) fn ready_retained(
        &mut self,
        id: &str,
        generation: u64,
        nonce: String,
    ) -> Result<()> {
        let view = self.library.store().retained_reader(id)?;
        let Some(live) = self.retained_workers.get_mut(id) else {
            return Ok(());
        };
        if live.spec.generation != generation {
            return Ok(());
        }
        if view.state != "running" || !crate::recordings::pipe_nonce_valid(&nonce) {
            live.worker.stop.send_replace(true);
            return Ok(());
        }
        live.nonce = Some(nonce);
        Ok(())
    }

    pub(super) fn finish_retained(
        &mut self,
        id: &str,
        generation: u64,
        result: Result<RetainedReadReceipt>,
    ) -> Result<()> {
        self.finish_retained_at(id, generation, result, Store::clock_ms())
    }

    pub(super) fn finish_retained_at(
        &mut self,
        id: &str,
        generation: u64,
        result: Result<RetainedReadReceipt>,
        observed_ms: Result<i64>,
    ) -> Result<()> {
        let Some(live) = self.retained_workers.get(id) else {
            return Ok(());
        };
        if live.spec.generation != generation {
            return Ok(());
        }
        // Finished-worker bookkeeping is independent of durable file protection.
        // Even a clock or catalog failure must leave shutdown able to observe idle.
        let live = self
            .retained_workers
            .remove(id)
            .ok_or(Error::StorageIntegrity)?;
        let observed_ms = observed_ms?;
        let persisted = match result {
            Ok(receipt) if receipt.matches(&live.spec) => self
                .library
                .store_mut()
                .finish_retained_reader(&receipt, observed_ms)
                .map(|_| ()),
            _ => self
                .library
                .store_mut()
                .hold_retained_reader(id, generation, "reader-completion-unproven", observed_ms)
                .map(|_| ()),
        };
        if persisted.is_err() {
            // A lost completion transaction retains durable protection. Failure
            // to persist even this hold stops the service; restart holds the row.
            let held = self.library.store_mut().hold_retained_reader(
                id,
                generation,
                "completion-persistence-failed",
                observed_ms,
            );
            held?;
            return persisted;
        }
        Ok(())
    }

    fn attach_retained(&self, page: &mut RetainedPage) {
        if page.entries.len() != 1 {
            return;
        }
        let view = &page.entries[0];
        if view.state == "running"
            && let Some(live) = self.retained_workers.get(&view.spec.request_id)
            && live.spec == view.spec
        {
            page.pipe_nonce.clone_from(&live.nonce);
        }
    }

    pub(super) fn stop_retained(&self) {
        for live in self.retained_workers.values() {
            live.worker.stop.send_replace(true);
        }
    }
}
