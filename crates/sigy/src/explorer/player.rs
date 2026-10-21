//! One supervised quick-listen client. Service receipts remain playback authority.

use super::state::Explorer;
use crate::panel::session::SessionHint;
use std::{
    io::Read,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
};

mod observation;

use observation::{Lane, Update};

pub(super) fn drain_pipe(
    mut pipe: impl Read + Send + 'static,
    diagnostic: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 1024];
        while let Ok(count) = pipe.read(&mut buffer) {
            if count == 0 {
                break;
            }
            if let Ok(mut output) = diagnostic.lock() {
                let count = count.min(8192_usize.saturating_sub(output.len()));
                output.extend_from_slice(&buffer[..count]);
            }
        }
    })
}

#[derive(Default)]
pub(super) struct Player {
    child: Option<Child>,
    input: Option<ChildStdin>,
    diagnostic: Arc<Mutex<Vec<u8>>>,
    diagnostic_task: Option<std::thread::JoinHandle<()>>,
    status: Lane,
    stop: Lane,
    generation: u64,
    current: Option<String>,
    stopped: Option<String>,
    retry_inspection: bool,
    /// An intentional stop or replacement. A non-zero exit is not a surprise failure.
    released: bool,
}

impl Player {
    pub(super) fn start(&mut self, directory: &Path, station: &str) -> std::io::Result<()> {
        if let Some(child) = &mut self.child
            && child.try_wait()?.is_none()
        {
            return Err(std::io::Error::other(
                "A play is already active. S stops it before another play.",
            ));
        }
        self.reap_diagnostic();
        if self.diagnostic_task.is_some() {
            return Err(std::io::Error::other(
                "The previous listen client is still draining diagnostics.",
            ));
        }
        self.advance()?;
        self.current = None;
        self.retry_inspection = false;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--data-dir")
            .arg(directory)
            .args([
                "play",
                station,
                "--cancel-on-stdin",
                "--parent-pid",
                &std::process::id().to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        self.input = child.stdin.take();
        self.diagnostic = Arc::default();
        if let Some(stderr) = child.stderr.take() {
            let diagnostic = Arc::clone(&self.diagnostic);
            self.diagnostic_task = Some(drain_pipe(stderr, diagnostic));
        }
        self.child = Some(child);
        self.released = false;
        Ok(())
    }

    /// The child has not exited, or its diagnostic drain is still running.
    #[must_use]
    pub(super) fn occupied(&mut self) -> bool {
        self.reap_diagnostic();
        let running = self
            .child
            .as_mut()
            .is_some_and(|child| !matches!(child.try_wait(), Ok(Some(_))));
        running || self.diagnostic_task.is_some()
    }

    pub(super) fn request_status(&mut self, runtime: &tokio::runtime::Runtime, directory: &Path) {
        let directory = directory.to_owned();
        self.status
            .start(runtime, self.generation, move |send| async move {
                observation::observe(&directory, send).await;
            });
    }

    pub(super) fn poll(&mut self, model: &mut Explorer) {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    self.input = None;
                    if !status.success() && !self.released {
                        let detail = self.diagnostic.lock().map_or_else(
                            |_| "Listen client failed.".to_owned(),
                            |bytes| String::from_utf8_lossy(&bytes).into_owned(),
                        );
                        model.note_message(&detail);
                    }
                    self.released = false;
                }
                Ok(None) => (),
                Err(error) => model.note_message(&error.to_string()),
            }
        }
        self.reap_diagnostic();
        for (generation, update) in self.status.poll() {
            self.adopt(model, generation, update, false);
        }
        for (generation, update) in self.stop.poll() {
            self.adopt(model, generation, update, true);
        }
    }

    fn adopt(&mut self, model: &mut Explorer, generation: u64, update: Update, stop: bool) {
        if generation != self.generation {
            return;
        }
        match update {
            Update::Hint(SessionHint::Present(hint)) => self.current = Some(hint.listen_id),
            Update::Hint(SessionHint::Absent) => self.current = None,
            Update::Hint(SessionHint::Unreadable) => {
                self.current = None;
                model.note_message(
                    "Playback session hint is unreadable. Completion remains unresolved.",
                );
            }
            Update::View(id, result) if id.is_empty() || self.current.as_ref() == Some(&id) => {
                match result {
                    Ok(view) => {
                        if !stop
                            && self.retry_inspection
                            && view.state == "running"
                            && self.stopped.as_ref() == Some(&id)
                        {
                            self.stopped = None;
                            self.retry_inspection = false;
                            model.note_message("Listen still running after an unresolved stop. S explicitly retries the same ID. Recordings keep running.");
                        }
                        model.observe_playback(view);
                    }
                    Err(error) => {
                        if stop {
                            self.retry_inspection = true;
                        }
                        model.note_message(&error);
                    }
                }
                // Fence status reads started before this stop observation.
                if stop && let Err(error) = self.advance() {
                    model.note_message(&error.to_string());
                }
            }
            Update::View(..) => (),
        }
    }

    pub(super) fn stop(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        directory: &Path,
        model: &mut Explorer,
    ) {
        self.released = true;
        self.input = None;
        let Some(id) = self.current.clone() else {
            model.note_message("Listen client cancellation requested. Service identity is not yet observed; completion is unresolved. Recordings keep running.");
            return;
        };
        if self.stop.busy() {
            model.note_message("A previous stop is still being observed. Client cancellation requested; completion is unresolved. Recordings keep running.");
            return;
        }
        if self.stopped.as_ref() == Some(&id) {
            model.note_message("Listen stop already requested. The same receipt is being observed; completion remains separate. Recordings keep running.");
            return;
        }
        if let Err(error) = self.advance() {
            model.note_message(&error.to_string());
            return;
        }
        self.stopped = Some(id.clone());
        self.retry_inspection = false;
        let directory = directory.to_owned();
        self.stop
            .start(runtime, self.generation, move |send| async move {
                let result = observation::exchange(&directory, &id, true).await;
                let _ = send.send(Update::View(id, result)).await;
            });
        model.note_message("Listen stop requested. Service state and completion are observed separately. Recordings keep running.");
    }

    fn advance(&mut self) -> std::io::Result<()> {
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            std::io::Error::other("Playback observation generation exhausted. Reopen the explorer.")
        })?;
        Ok(())
    }

    fn reap_diagnostic(&mut self) {
        if self
            .diagnostic_task
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
            && let Some(task) = self.diagnostic_task.take()
        {
            let _ = task.join();
        }
    }
}

#[cfg(test)]
impl Player {
    pub(super) fn prepare_listen_identity(&mut self, id: &str) {
        self.current = Some(id.to_owned());
    }

    #[must_use]
    pub(super) const fn observation_generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub(super) fn stop_pending(&self) -> bool {
        self.stop.busy()
    }
}

#[cfg(test)]
mod tests;

impl Drop for Player {
    fn drop(&mut self) {
        self.input = None;
        let child = self.child.take();
        let diagnostic = self.diagnostic_task.take();
        if child.is_some() || diagnostic.is_some() {
            // Closing stdin cancels through the normal decoder/group drain path.
            // Reap off the terminal thread; parent-pid also fences abrupt TUI death.
            std::thread::spawn(move || {
                if let Some(mut child) = child {
                    let _ = child.wait();
                }
                if let Some(task) = diagnostic {
                    let _ = task.join();
                }
            });
        }
    }
}
