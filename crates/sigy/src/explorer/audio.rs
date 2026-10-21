//! One foreground audio client. Live play and retained listen do not overlap.

use std::path::Path;

use tokio::runtime::Runtime;

use super::{player::Player, retained::Client, state::Effect, state::Explorer};

#[derive(Clone, PartialEq, Eq)]
enum Start {
    Station(String),
    Recording {
        id: String,
        request: String,
    },
    Finding {
        monitor: String,
        finding: String,
        request: String,
    },
}

impl Start {
    fn request_id(&self) -> Option<&str> {
        match self {
            Self::Station(_) => None,
            Self::Recording { request, .. } | Self::Finding { request, .. } => Some(request),
        }
    }

    fn is_retained(&self) -> bool {
        self.request_id().is_some()
    }
}

#[derive(Default)]
pub(super) struct Audio {
    live: Player,
    retained: Client,
    queued: Option<Start>,
}

impl Audio {
    pub(super) fn poll(&mut self, directory: &Path, model: &mut Explorer) {
        self.live.poll(model);
        self.retained.poll(model);
        self.pump(directory, model);
    }

    pub(super) fn request_status(&mut self, runtime: &Runtime, directory: &Path) {
        self.live.request_status(runtime, directory);
        self.retained.request_status(runtime, directory);
    }

    pub(super) fn apply(
        &mut self,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
        effect: &Effect,
    ) {
        match effect {
            Effect::Play { station } => {
                self.enqueue(Start::Station(station.clone()), runtime, directory, model);
            }
            Effect::PlayRecording { id, request } => {
                model.begin_retained(request);
                self.enqueue(
                    Start::Recording {
                        id: id.clone(),
                        request: request.clone(),
                    },
                    runtime,
                    directory,
                    model,
                );
            }
            Effect::PlayFinding {
                monitor,
                finding,
                request,
            } => {
                model.begin_retained(request);
                self.enqueue(
                    Start::Finding {
                        monitor: monitor.clone(),
                        finding: finding.clone(),
                        request: request.clone(),
                    },
                    runtime,
                    directory,
                    model,
                );
            }
            Effect::StopListen => self.stop(true, runtime, directory, model),
            Effect::StopRetained => self.stop(false, runtime, directory, model),
            Effect::None
            | Effect::Detach
            | Effect::LinkedContext { .. }
            | Effect::Search(_)
            | Effect::ExploreRandom(_)
            | Effect::Fetch(_)
            | Effect::SetFavorite { .. }
            | Effect::Reload
            | Effect::MonitorList
            | Effect::MonitorDetail { .. }
            | Effect::Finding { .. }
            | Effect::FindingOriginal { .. } => {}
        }
    }

    /// Close stdin and request a known stop. The caller does not wait for process exit.
    pub(super) fn quit(
        &mut self,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
    ) -> bool {
        let discarded_start = self.queued.take();
        if let Some(request) = discarded_start.as_ref().and_then(Start::request_id) {
            model.set_retained_phase(request, "unresolved", None);
        }
        let discarded = discarded_start.is_some();
        let live = self.live.occupied();
        let retained = self.retained.occupied();
        if live {
            self.live.stop(runtime, directory, model);
        }
        if retained {
            self.retained.stop(runtime, directory, model);
        }
        discarded || live || retained
    }

    fn enqueue(&mut self, start: Start, runtime: &Runtime, directory: &Path, model: &mut Explorer) {
        if self.foreground_occupied() {
            self.stop_foreground(runtime, directory, model);
            let replaced = self.queued.replace(start).is_some();
            model.note_message(if replaced {
                "The previous next listen was replaced while the current client is still stopping. Capture keeps running."
            } else {
                "Playback stop requested. The next listen waits until this client exits. Capture keeps running."
            });
            return;
        }
        self.spawn(directory, &start, model);
    }

    fn pump(&mut self, directory: &Path, model: &mut Explorer) {
        if self.queued.is_none() || self.foreground_occupied() {
            return;
        }
        let Some(start) = self.queued.take() else {
            return;
        };
        self.spawn(directory, &start, model);
    }

    fn spawn(&mut self, directory: &Path, start: &Start, model: &mut Explorer) {
        let hold = start.is_retained() && self.retained.holds_protection();
        let started = match start {
            Start::Station(station) => self.live.start(directory, station),
            Start::Recording { id, request } => self.retained.start_file(directory, id, request),
            Start::Finding {
                monitor,
                finding,
                request,
            } => self
                .retained
                .start_finding(directory, monitor, finding, request),
        };
        match started {
            Ok(()) => {
                if hold {
                    model.note_message("A previous retained reader may still hold protection. This listen uses a new request id. Capture keeps running.");
                }
            }
            Err(error) => {
                if let Some(request) = start.request_id() {
                    model.set_retained_phase(request, "unresolved", None);
                }
                model.note_message(&start_failure(&error));
            }
        }
    }

    fn foreground_occupied(&mut self) -> bool {
        self.live.occupied() || self.retained.occupied()
    }

    fn stop_foreground(&mut self, runtime: &Runtime, directory: &Path, model: &mut Explorer) {
        let live = self.live.occupied();
        let retained = self.retained.occupied();
        if live {
            self.live.stop(runtime, directory, model);
        }
        if retained {
            self.retained.stop(runtime, directory, model);
        }
    }

    fn stop(
        &mut self,
        prefer_live: bool,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
    ) {
        let discarded = self.queued.take();
        if let Some(request) = discarded.as_ref().and_then(Start::request_id) {
            model.set_retained_phase(request, "unresolved", None);
        }
        let live = self.live.occupied();
        let retained = self.retained.occupied();
        if live || retained {
            self.stop_foreground(runtime, directory, model);
        } else if prefer_live {
            self.live.stop(runtime, directory, model);
        } else if self.retained.has_request() {
            self.retained.stop(runtime, directory, model);
        } else {
            model.note_message("No retained listen is active. Capture keeps running.");
        }
        if discarded.is_some() {
            model.note_message("The next listen was discarded. Playback stop requested. Closure is unresolved until this client exits. Capture keeps running.");
        }
    }
}

/// Operating-system spawn failures can contain a path. Keep those out of the status line.
fn start_failure(error: &std::io::Error) -> String {
    if error.raw_os_error().is_some() {
        "The listen client could not start. No path is shown. Capture keeps running.".into()
    } else {
        error.to_string()
    }
}
