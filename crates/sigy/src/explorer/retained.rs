//! One retained-listen child. Service reader receipts stay separate from process exit.

use std::{
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
};

use sigy_service::control::{self, Operation, RetainedOperation, Snapshot};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

use super::{player::drain_pipe, state::Explorer, text::sanitize};

/// Map a reader receipt and the client process onto one visible phase.
///
/// `receipt` is the service state word. Absence means no receipt has been read.
#[must_use]
pub(super) fn phase(
    receipt: Option<&str>,
    stop_requested: bool,
    client_alive: bool,
) -> &'static str {
    match receipt {
        Some("cancelling") => "stopping",
        Some("completed" | "failed") => "closed",
        Some("running") if stop_requested => "stopping",
        Some("running") => "decoding",
        None if stop_requested && client_alive => "stopping",
        None if client_alive => "pending",
        _ => "unresolved",
    }
}

struct Receipt {
    request: String,
    state: String,
    generation: u64,
}

struct Pending {
    fence: u64,
    receive: mpsc::Receiver<Result<Receipt, String>>,
    task: JoinHandle<()>,
    delivered: bool,
}

#[derive(Default)]
struct Lane {
    pending: Option<Pending>,
}

impl Lane {
    fn busy(&self) -> bool {
        self.pending.is_some()
    }

    fn start<F>(&mut self, runtime: &Runtime, fence: u64, work: F)
    where
        F: std::future::Future<Output = Result<Receipt, String>> + Send + 'static,
    {
        if self.busy() {
            return;
        }
        let (send, receive) = mpsc::channel(1);
        self.pending = Some(Pending {
            fence,
            receive,
            task: runtime.spawn(async move {
                let _ = send.send(work.await).await;
            }),
            delivered: false,
        });
    }

    fn poll(&mut self) -> Option<(u64, Result<Receipt, String>)> {
        let (fence, update, finished) = {
            let pending = self.pending.as_mut()?;
            let update = match pending.receive.try_recv() {
                Ok(result) => {
                    pending.delivered = true;
                    Some(result)
                }
                Err(mpsc::error::TryRecvError::Empty) => None,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if pending.delivered {
                        None
                    } else {
                        pending.delivered = true;
                        Some(Err(
                            "Retained reader observation ended without a receipt.".into()
                        ))
                    }
                }
            };
            let finished = pending.task.is_finished() && pending.receive.is_empty();
            (pending.fence, update, finished)
        };
        if finished {
            self.pending = None;
        }
        update.map(|result| (fence, result))
    }
}

#[derive(Default)]
struct Notices {
    failure: bool,
    show: bool,
}

#[derive(Default)]
pub(super) struct Client {
    child: Option<Child>,
    input: Option<ChildStdin>,
    diagnostic: Arc<Mutex<Vec<u8>>>,
    stdout_task: Option<std::thread::JoinHandle<()>>,
    stderr_task: Option<std::thread::JoinHandle<()>>,
    request: Option<String>,
    reader_generation: Option<u64>,
    receipt_state: Option<String>,
    phase_label: String,
    stop_requested: bool,
    released: bool,
    notices: Notices,
    fence: u64,
    show: Lane,
    stop_lane: Lane,
}

impl Client {
    pub(super) fn occupied(&mut self) -> bool {
        self.reap_drains();
        let running = self
            .child
            .as_mut()
            .is_some_and(|child| !matches!(child.try_wait(), Ok(Some(_))));
        running || self.stdout_task.is_some() || self.stderr_task.is_some()
    }

    pub(super) fn holds_protection(&self) -> bool {
        self.request.is_some() && self.phase_label != "closed"
    }

    pub(super) fn has_request(&self) -> bool {
        self.request.is_some()
    }

    pub(super) fn start_file(
        &mut self,
        directory: &Path,
        recording: &str,
        request: &str,
    ) -> std::io::Result<()> {
        self.launch(
            directory,
            request,
            &[
                "listen",
                "file",
                recording,
                "--request",
                request,
                "--destination",
                "system",
                "--cancel-on-stdin",
                "--parent-pid",
            ],
        )
    }

    pub(super) fn start_finding(
        &mut self,
        directory: &Path,
        monitor: &str,
        finding: &str,
        request: &str,
    ) -> std::io::Result<()> {
        self.launch(
            directory,
            request,
            &[
                "listen",
                "finding",
                monitor,
                finding,
                "--request",
                request,
                "--destination",
                "system",
                "--cancel-on-stdin",
                "--parent-pid",
            ],
        )
    }

    pub(super) fn request_status(&mut self, runtime: &Runtime, directory: &Path) {
        if self.phase_label == "closed" && !self.occupied() {
            return;
        }
        let Some(id) = self.request.clone() else {
            return;
        };
        if self.show.busy() || !self.advance_fence() {
            return;
        }
        let fence = self.fence;
        let directory = directory.to_owned();
        self.show.start(runtime, fence, async move {
            read_receipt(&directory, &id, false, None).await
        });
    }

    pub(super) fn poll(&mut self, model: &mut Explorer) {
        self.observe_exit(model);
        self.reap_drains();
        if let Some((fence, result)) = self.show.poll() {
            self.adopt(model, fence, result, false);
        }
        if let Some((fence, result)) = self.stop_lane.poll() {
            self.adopt(model, fence, result, true);
        }
    }

    pub(super) fn stop(&mut self, runtime: &Runtime, directory: &Path, model: &mut Explorer) {
        self.released = true;
        self.stop_requested = true;
        self.input = None;
        self.publish(model);
        let Some(id) = self.request.clone() else {
            model.note_message("Retained listen cancellation requested. Request identity is unknown; closure is unresolved. Capture keeps running.");
            return;
        };
        let Some(generation) = self.reader_generation else {
            model.note_message("Retained listen stop requested. Reader generation is not yet observed; closure is unresolved. Capture keeps running.");
            return;
        };
        if self.stop_lane.busy() {
            model.note_message("A previous retained stop is still being observed. Client cancellation requested; closure is unresolved. Capture keeps running.");
            return;
        }
        if !self.advance_fence() {
            model.note_message("Retained reader observation generation is exhausted. Closure is unresolved. Capture keeps running.");
            return;
        }
        let fence = self.fence;
        let directory = directory.to_owned();
        self.stop_lane.start(runtime, fence, async move {
            read_receipt(&directory, &id, true, Some(generation)).await
        });
        model.note_message("Retained reader stop requested. Original-file protection follows service completion. Capture keeps running.");
    }

    fn launch(&mut self, directory: &Path, request: &str, args: &[&str]) -> std::io::Result<()> {
        if self.occupied() {
            return Err(std::io::Error::other(
                "A retained listen is still stopping. The next listen waits for that client to exit.",
            ));
        }
        if !self.advance_fence() {
            return Err(std::io::Error::other(
                "Retained reader observation generation is exhausted. Reopen the explorer.",
            ));
        }
        let parent = std::process::id().to_string();
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--data-dir")
            .arg(directory)
            .args(args)
            .arg(&parent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take();
        let diagnostic = Arc::<Mutex<Vec<u8>>>::default();
        let stdout_task = child
            .stdout
            .take()
            .map(|pipe| drain_pipe(pipe, Arc::clone(&diagnostic)));
        let stderr_task = child
            .stderr
            .take()
            .map(|pipe| drain_pipe(pipe, Arc::clone(&diagnostic)));
        self.child = Some(child);
        self.input = input;
        self.diagnostic = diagnostic;
        self.stdout_task = stdout_task;
        self.stderr_task = stderr_task;
        self.request = Some(request.to_owned());
        self.reader_generation = None;
        self.receipt_state = None;
        self.phase_label = "pending".into();
        self.stop_requested = false;
        self.released = false;
        self.notices = Notices::default();
        Ok(())
    }

    fn observe_exit(&mut self, model: &mut Explorer) {
        let Some(child) = &mut self.child else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.child = None;
                self.input = None;
                if !status.success() && !self.released {
                    let detail = self.diagnostic.lock().map_or_else(
                        |_| "Retained listen client failed.".to_owned(),
                        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
                    );
                    model.note_message(&detail);
                }
                self.released = false;
                self.publish(model);
            }
            Ok(None) => {}
            Err(error) => model.note_message(&error.to_string()),
        }
    }

    fn adopt(
        &mut self,
        model: &mut Explorer,
        fence: u64,
        result: Result<Receipt, String>,
        stop: bool,
    ) {
        if fence != self.fence {
            return;
        }
        match result {
            Ok(receipt) => {
                if self.request.as_deref() != Some(receipt.request.as_str()) {
                    return;
                }
                self.reader_generation = Some(receipt.generation);
                self.receipt_state = Some(receipt.state.clone());
                if receipt.state == "failed" && !self.notices.failure {
                    self.notices.failure = true;
                    model.note_message(&format!(
                        "Retained reader failed. Inspect sigy listen reader show {}.",
                        sanitize(&receipt.request, 64)
                    ));
                }
                self.publish(model);
            }
            Err(error) => {
                if stop || !self.notices.show {
                    self.notices.show = true;
                    model.note_message(&error);
                }
                self.publish(model);
            }
        }
    }

    fn publish(&mut self, model: &mut Explorer) {
        let Some(request) = self.request.clone() else {
            return;
        };
        let label = phase(
            self.receipt_state.as_deref(),
            self.stop_requested,
            self.child.is_some(),
        );
        self.phase_label = label.to_owned();
        model.set_retained_phase(&request, label, self.receipt_state.as_deref());
    }

    fn advance_fence(&mut self) -> bool {
        match self.fence.checked_add(1) {
            Some(next) => {
                self.fence = next;
                true
            }
            None => false,
        }
    }

    fn reap_drains(&mut self) {
        reap(&mut self.stdout_task);
        reap(&mut self.stderr_task);
    }
}

fn reap(task: &mut Option<std::thread::JoinHandle<()>>) {
    if task
        .as_ref()
        .is_some_and(std::thread::JoinHandle::is_finished)
        && let Some(task) = task.take()
    {
        let _ = task.join();
    }
}

async fn read_receipt(
    directory: &Path,
    id: &str,
    stop: bool,
    generation: Option<u64>,
) -> Result<Receipt, String> {
    let command = if stop {
        let Some(generation) = generation else {
            return Err(
                "Retained reader stop was not sent because its generation is unknown.".into(),
            );
        };
        RetainedOperation::Stop {
            id: id.to_owned(),
            generation,
        }
    } else {
        RetainedOperation::Show { id: id.to_owned() }
    };
    let snapshot = control::request(directory, Operation::Retained { command })
        .await
        .map_err(|error| {
            format!(
                "Retained reader delivery is unresolved. Inspect sigy listen reader show {}: {}",
                sanitize(id, 64),
                sanitize(&error.to_string(), 80)
            )
        })?;
    receipt_from(id, snapshot)
}

fn receipt_from(id: &str, snapshot: Snapshot) -> Result<Receipt, String> {
    let page = snapshot
        .retained
        .ok_or("Retained reader receipt is missing.")?;
    let view = page
        .entries
        .into_iter()
        .find(|entry| entry.spec.request_id == id)
        .ok_or("Retained reader receipt identity mismatch.")?;
    Ok(Receipt {
        request: view.spec.request_id,
        state: view.state,
        generation: view.spec.generation,
    })
}

impl Drop for Client {
    fn drop(&mut self) {
        self.input = None;
        let child = self.child.take();
        let stdout = self.stdout_task.take();
        let stderr = self.stderr_task.take();
        if child.is_some() || stdout.is_some() || stderr.is_some() {
            std::thread::spawn(move || {
                if let Some(mut child) = child {
                    let _ = child.wait();
                }
                if let Some(task) = stdout {
                    let _ = task.join();
                }
                if let Some(task) = stderr {
                    let _ = task.join();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::phase;

    #[test]
    fn phase_follows_the_receipt_and_the_client() {
        assert_eq!(phase(None, false, true), "pending");
        assert_eq!(phase(None, true, true), "stopping");
        assert_eq!(phase(None, true, false), "unresolved");
        assert_eq!(phase(None, false, false), "unresolved");
        assert_eq!(phase(Some("running"), false, true), "decoding");
        assert_eq!(phase(Some("running"), true, true), "stopping");
        assert_eq!(phase(Some("running"), true, false), "stopping");
        assert_eq!(phase(Some("cancelling"), false, true), "stopping");
        assert_eq!(phase(Some("completed"), false, false), "closed");
        assert_eq!(phase(Some("failed"), true, false), "closed");
        assert_eq!(phase(Some("recovery_held"), false, false), "unresolved");
        assert_eq!(phase(Some("starting"), false, true), "unresolved");
    }
}
