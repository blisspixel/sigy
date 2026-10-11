//! Two finite observation lanes. Neither lane waits on the terminal thread.

use std::{future::Future, path::Path};

use sigy_service::control::{self, ListenOperation, Operation};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

use crate::panel::session::{self, SessionHint};

use super::super::{client, state::PlaybackView};

pub(super) enum Update {
    Hint(SessionHint),
    View(String, Result<PlaybackView, String>),
}

struct Pending {
    generation: u64,
    receive: mpsc::Receiver<Update>,
    task: JoinHandle<()>,
    delivered: bool,
}

#[derive(Default)]
pub(super) struct Lane {
    pending: Option<Pending>,
}

impl Lane {
    pub(super) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn start<F, Fut>(&mut self, runtime: &Runtime, generation: u64, work: F)
    where
        F: FnOnce(mpsc::Sender<Update>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.busy() {
            return;
        }
        let (send, receive) = mpsc::channel(2);
        self.pending = Some(Pending {
            generation,
            receive,
            task: runtime.spawn(work(send)),
            delivered: false,
        });
    }

    pub(super) fn poll(&mut self) -> Vec<(u64, Update)> {
        let Some(pending) = &mut self.pending else {
            return Vec::new();
        };
        let mut updates = Vec::with_capacity(2);
        for _ in 0..2 {
            match pending.receive.try_recv() {
                Ok(update) => {
                    pending.delivered |= !matches!(update, Update::Hint(SessionHint::Present(_)));
                    updates.push((pending.generation, update));
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if !pending.delivered {
                        pending.delivered = true;
                        updates.push((pending.generation, Update::View(String::new(), Err(
                            "Playback observation ended without a receipt. Completion remains unresolved.".into(),
                        ))));
                    }
                    break;
                }
            }
        }
        // Retain the slot until the task's destructors have actually run. A
        // delivered response alone must not allow tasks to accumulate.
        if pending.task.is_finished() && pending.receive.is_empty() {
            self.pending = None;
        }
        updates
    }
}

pub(super) async fn observe(directory: &Path, send: mpsc::Sender<Update>) {
    let hint = session::read(directory);
    let id = match &hint {
        SessionHint::Present(hint) => Some(hint.listen_id.clone()),
        SessionHint::Absent | SessionHint::Unreadable => None,
    };
    if send.send(Update::Hint(hint)).await.is_err() {
        return;
    }
    let Some(id) = id else {
        return;
    };
    let result = exchange(directory, &id, false).await;
    let _ = send.send(Update::View(id, result)).await;
}

pub(super) async fn exchange(
    directory: &Path,
    id: &str,
    stop: bool,
) -> Result<PlaybackView, String> {
    let command = if stop {
        ListenOperation::Stop { id: id.to_owned() }
    } else {
        ListenOperation::Status { id: id.to_owned() }
    };
    let snapshot = control::request(directory, Operation::Listen { command })
        .await
        .map_err(|error| {
            format!(
                "Listen delivery or completion unresolved. Inspect the same ID {}: {error}",
                super::super::text::sanitize(id, 64)
            )
        })?;
    receipt(id, snapshot.listen)
}

pub(super) fn receipt(
    id: &str,
    listen: Option<control::ListenView>,
) -> Result<PlaybackView, String> {
    let listen = listen.ok_or("Playback receipt is missing.")?;
    if listen.id != id {
        return Err("Playback receipt identity mismatch.".into());
    }
    Ok(client::playback_from(&listen))
}
