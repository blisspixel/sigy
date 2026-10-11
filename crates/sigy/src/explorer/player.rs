//! One supervised quick-listen client. Service receipts remain playback authority.

use super::state::Explorer;
use crate::panel::session::{self, SessionHint};
use sigy_service::control::{self, ListenOperation, Operation};
use std::{
    io::Read,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(super) struct Player {
    child: Option<Child>,
    input: Option<ChildStdin>,
    diagnostic: Arc<Mutex<Vec<u8>>>,
}

impl Player {
    pub(super) fn start(&mut self, directory: &Path, station: &str) -> std::io::Result<()> {
        if let Some(child) = &mut self.child
            && child.try_wait()?.is_none()
        {
            return Err(std::io::Error::other(
                "A quick listen is already active. S stops it before another play.",
            ));
        }
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
        if let Some(mut stderr) = child.stderr.take() {
            let diagnostic = Arc::clone(&self.diagnostic);
            std::thread::spawn(move || {
                let mut buffer = [0_u8; 1024];
                while let Ok(count) = stderr.read(&mut buffer) {
                    if count == 0 {
                        break;
                    }
                    if let Ok(mut output) = diagnostic.lock() {
                        let count = count.min(8192_usize.saturating_sub(output.len()));
                        output.extend_from_slice(&buffer[..count]);
                    }
                }
            });
        }
        self.child = Some(child);
        Ok(())
    }

    pub(super) fn poll(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        directory: &Path,
        model: &mut Explorer,
    ) {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    self.input = None;
                    if !status.success() {
                        let detail = self.diagnostic.lock().map_or_else(
                            |_| "Listen client failed.".to_owned(),
                            |bytes| String::from_utf8_lossy(&bytes).into_owned(),
                        );
                        model.note_message(&detail);
                    }
                }
                Ok(None) => (),
                Err(error) => model.note_message(&error.to_string()),
            }
        }
        if let SessionHint::Present(hint) = session::read(directory) {
            match runtime.block_on(control::request(
                directory,
                Operation::Listen {
                    command: ListenOperation::Status { id: hint.listen_id },
                },
            )) {
                Ok(snapshot) => {
                    if let Some(listen) = snapshot.listen {
                        model.observe_playback(super::client::playback_from(&listen));
                    }
                }
                Err(error) => {
                    model.note_message(&format!("Playback observation unavailable: {error}"));
                }
            }
        }
    }

    pub(super) fn stop(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        directory: &Path,
        model: &mut Explorer,
    ) {
        self.input = None;
        let SessionHint::Present(hint) = session::read(directory) else {
            model.note_message("No quick-listen session is available. Recordings keep running.");
            return;
        };
        match runtime.block_on(crate::listen::stop_listen(directory, &hint.listen_id)) {
            Ok(listen) => {
                model.observe_playback(super::client::playback_from(&listen));
                model.note_message("Listen stop requested. Recordings keep running; completion is observed separately.");
            }
            Err(error) => model.note_message(&error.to_string()),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.input = None;
        if let Some(mut child) = self.child.take() {
            // Closing stdin cancels through the normal decoder/group drain path.
            // Reap off the terminal thread; parent-pid also fences abrupt TUI death.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
