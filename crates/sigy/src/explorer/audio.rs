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
        recording: String,
        start_us: u64,
        end_us: u64,
    },
    Interval {
        id: String,
        request: String,
        start_us: u64,
        end_us: u64,
    },
}

struct Admission {
    recording: String,
    origin_us: u64,
    end_us: Option<u64>,
    request: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoldPhase {
    Stopping,
    Paused,
}

struct Hold {
    at_us: u64,
    pending_us: Option<u64>,
    phase: HoldPhase,
}

struct SeekAttempt {
    fallback_us: u64,
    request: String,
}

impl Start {
    fn request_id(&self) -> Option<&str> {
        match self {
            Self::Station(_) => None,
            Self::Recording { request, .. }
            | Self::Finding { request, .. }
            | Self::Interval { request, .. } => Some(request),
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
    admission: Option<Admission>,
    hold: Option<Hold>,
    seek_attempt: Option<SeekAttempt>,
    handoff_attempt: Option<SeekAttempt>,
    /// Request that already produced one handoff outcome. A later request can continue.
    explained: Option<String>,
    follow: super::follow::Slot,
}

impl Audio {
    pub(super) fn poll(&mut self, directory: &Path, model: &mut Explorer) {
        self.live.poll(model);
        self.retained.poll(model);
        self.settle_hold(directory, model);
        self.recover_seek(model);
        self.recover_handoff(model);
        if let Some(start) = self.follow.poll(model) {
            self.finish_handoff(directory, model, start.fallback_us, start.target_us);
        }
        self.advance_segment(model);
        self.pump(directory, model);
    }

    pub(super) fn request_status(&mut self, runtime: &Runtime, directory: &Path) {
        self.live.request_status(runtime, directory);
        self.retained.request_status(runtime, directory);
        self.follow.request(runtime, directory);
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
                self.end_hold(model);
                self.enqueue(Start::Station(station.clone()), runtime, directory, model);
            }
            Effect::PlayRecording { id, request } => {
                self.end_hold(model);
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
                recording,
                start_us,
                end_us,
            } => {
                self.end_hold(model);
                model.begin_retained(request);
                self.enqueue(
                    Start::Finding {
                        monitor: monitor.clone(),
                        finding: finding.clone(),
                        request: request.clone(),
                        recording: recording.clone(),
                        start_us: *start_us,
                        end_us: *end_us,
                    },
                    runtime,
                    directory,
                    model,
                );
            }
            Effect::PlayInterval {
                id,
                request,
                start_us,
                end_us,
            } => {
                self.end_hold(model);
                model.begin_retained(request);
                self.enqueue(
                    Start::Interval {
                        id: id.clone(),
                        request: request.clone(),
                        start_us: *start_us,
                        end_us: *end_us,
                    },
                    runtime,
                    directory,
                    model,
                );
            }
            Effect::HoldRetained => self.heard(runtime, directory, model),
            Effect::SeekRetained { forward } => self.nudge(runtime, directory, model, *forward),
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
            | Effect::FindingOriginal { .. }
            | Effect::ClaimList
            | Effect::ClaimShow { .. } => {}
        }
    }

    fn heard(&mut self, runtime: &Runtime, directory: &Path, model: &mut Explorer) {
        self.follow.clear();
        let decision = super::pause::decide(&self.pause_input(model));
        match decision {
            super::pause::Decision::Stop { at_us } => {
                self.begin_stop(runtime, directory, model, at_us);
            }
            super::pause::Decision::Remember { at_us } => self.remember(model, at_us),
            super::pause::Decision::Resume {
                recording,
                seek_us,
                end_us,
            } => self.resume(directory, model, recording, seek_us, end_us),
            other => {
                if let Some(message) = super::pause::refusal(&other) {
                    model.note_message(message);
                }
            }
        }
    }

    /// Close stdin and request a known stop. The caller does not wait for process exit.
    pub(super) fn quit(
        &mut self,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
    ) -> bool {
        self.end_hold(model);
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
            Start::Recording { id, request } => {
                self.retained.start_file(directory, id, request, 0, None)
            }
            Start::Finding {
                monitor,
                finding,
                request,
                ..
            } => self
                .retained
                .start_finding(directory, monitor, finding, request),
            Start::Interval {
                id,
                request,
                start_us,
                end_us,
            } => self
                .retained
                .start_file(directory, id, request, *start_us, Some(*end_us)),
        };
        match started {
            Ok(()) => {
                self.remember_spawn(start);
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
        let paused = self
            .hold
            .as_ref()
            .is_some_and(|hold| hold.phase == HoldPhase::Paused)
            && !self.retained.occupied();
        self.end_hold(model);
        if paused {
            self.discard_queued(model);
            model.note_message(
                "Heard pause cleared. No new listen was started. Capture keeps running.",
            );
            return;
        }
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

    fn end_hold(&mut self, model: &mut Explorer) {
        self.hold = None;
        self.admission = None;
        self.seek_attempt = None;
        self.handoff_attempt = None;
        self.explained = None;
        self.follow.clear();
        model.release_pause();
    }

    fn discard_queued(&mut self, model: &mut Explorer) -> bool {
        let discarded = self.queued.take();
        if let Some(request) = discarded.as_ref().and_then(Start::request_id) {
            model.set_retained_phase(request, "unresolved", None);
        }
        discarded.is_some()
    }

    fn pause_input<'a>(&'a mut self, model: &'a Explorer) -> super::pause::PauseInput<'a> {
        let child_running = self.retained.occupied();
        let live_occupied = self.live.occupied();
        let presence = match (&self.hold, child_running, self.admission.is_some()) {
            (Some(hold), _, _) if hold.phase == HoldPhase::Stopping => {
                super::pause::Presence::Stopping
            }
            (Some(hold), _, _) if hold.phase == HoldPhase::Paused => super::pause::Presence::Paused,
            (None, true, _) => super::pause::Presence::Playing,
            (None, false, true) => super::pause::Presence::Exited,
            _ => super::pause::Presence::Absent,
        };
        let held_us = self.hold.as_ref().map(|hold| hold.at_us);
        let admission = self.admission.as_ref();
        super::pause::PauseInput {
            presence,
            decoder_us: model.decoder_output_us(),
            recording: admission.map(|item| item.recording.as_str()),
            origin_us: admission.map_or(0, |item| item.origin_us),
            end_us: admission.and_then(|item| item.end_us),
            request: admission.map(|item| item.request.as_str()),
            visible_request: model.retained_label().map(|(_, request)| request),
            held_us,
            live_occupied,
        }
    }

    fn begin_stop(
        &mut self,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
        at_us: u64,
    ) {
        self.discard_queued(model);
        self.hold = Some(Hold {
            at_us,
            pending_us: None,
            phase: HoldPhase::Stopping,
        });
        self.seek_attempt = None;
        self.handoff_attempt = None;
        if self.retained.occupied() || self.retained.has_request() {
            self.retained.stop(runtime, directory, model);
        }
        model.note_message(&format!(
            "Pausing at {at_us} us. Waiting for this client to exit. This does not prove silence. Capture keeps running."
        ));
    }

    fn settle_hold(&mut self, directory: &Path, model: &mut Explorer) {
        if self
            .hold
            .as_ref()
            .is_none_or(|hold| hold.phase != HoldPhase::Stopping)
            || self.retained.occupied()
        {
            return;
        }
        let Some(at_us) = self.hold.as_ref().map(|hold| hold.at_us) else {
            return;
        };
        if let Some(target_us) = self.hold.as_ref().and_then(|hold| hold.pending_us) {
            self.finish_seek(directory, model, at_us, target_us);
            return;
        }
        let Some(request) = self.admission.as_ref().map(|item| item.request.clone()) else {
            return;
        };
        self.hold = Some(Hold {
            at_us,
            pending_us: None,
            phase: HoldPhase::Paused,
        });
        if model.show_paused(&request, at_us) {
            model.note_message(&format!(
                "Paused at {at_us} us. Press H to start one listen there. This does not prove silence. Capture keeps running."
            ));
        }
    }

    fn remember(&mut self, model: &mut Explorer, at_us: u64) {
        let Some(request) = self.admission.as_ref().map(|item| item.request.clone()) else {
            model.note_message("No retained listen is active. Capture keeps running.");
            return;
        };
        self.hold = Some(Hold {
            at_us,
            pending_us: None,
            phase: HoldPhase::Paused,
        });
        self.seek_attempt = None;
        self.handoff_attempt = None;
        model.show_paused(&request, at_us);
        model.note_message(&format!(
            "The client already exited. That is not a pause. Position kept at {at_us} us. Press H to listen there. Capture keeps running."
        ));
    }

    fn nudge(&mut self, runtime: &Runtime, directory: &Path, model: &mut Explorer, forward: bool) {
        self.follow.clear();
        let decision = super::seek::decide(&self.pause_input(model), forward);
        match decision {
            super::seek::Decision::Jump {
                fallback_us,
                target_us,
            } => self.begin_jump(runtime, directory, model, fallback_us, target_us),
            super::seek::Decision::Move { at_us } => self.remember_seek(model, at_us, false),
            super::seek::Decision::Remember { at_us } => self.remember_seek(model, at_us, true),
            other => {
                if let Some(message) = super::seek::refusal(&other) {
                    model.note_message(message);
                }
            }
        }
    }

    fn begin_jump(
        &mut self,
        runtime: &Runtime,
        directory: &Path,
        model: &mut Explorer,
        fallback_us: u64,
        target_us: u64,
    ) {
        self.discard_queued(model);
        self.seek_attempt = None;
        self.handoff_attempt = None;
        self.hold = Some(Hold {
            at_us: fallback_us,
            pending_us: Some(target_us),
            phase: HoldPhase::Stopping,
        });
        if self.retained.occupied() || self.retained.has_request() {
            self.retained.stop(runtime, directory, model);
        }
        model.note_message(&format!(
            "Seeking to {target_us} us. Waiting for this client to exit. This does not prove silence. Capture keeps running."
        ));
    }

    fn finish_seek(
        &mut self,
        directory: &Path,
        model: &mut Explorer,
        fallback_us: u64,
        target_us: u64,
    ) {
        if self.live.occupied() || self.retained.occupied() {
            self.park(
                model,
                fallback_us,
                &format!(
                    "A listen is still running. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            );
            return;
        }
        let Some(admission) = self.admission.as_ref() else {
            self.park(
                model,
                fallback_us,
                "This decoder has no stored admission. Seek was not requested. Capture keeps running.",
            );
            return;
        };
        let recording = admission.recording.clone();
        let end_us = admission.end_us;
        let Some(request) = model.allocate_retained() else {
            self.park(
                model,
                fallback_us,
                &format!(
                    "The seek wrote no reader. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            );
            return;
        };
        match self
            .retained
            .start_file(directory, &recording, &request, target_us, end_us)
        {
            Ok(()) => {
                self.hold = None;
                self.admission = Some(Admission {
                    recording,
                    origin_us: target_us,
                    end_us,
                    request: request.clone(),
                });
                self.seek_attempt = Some(SeekAttempt {
                    fallback_us,
                    request: request.clone(),
                });
                self.handoff_attempt = None;
                model.begin_retained(&request);
                model.note_message(&format!(
                    "Starting one retained listen at {target_us} us. Request {request}. Capture keeps running."
                ));
            }
            Err(_) => self.park(
                model,
                fallback_us,
                &format!(
                    "The seek wrote no reader. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            ),
        }
    }

    fn recover_seek(&mut self, model: &mut Explorer) {
        let Some(attempt) = self.seek_attempt.as_ref() else {
            return;
        };
        let request = attempt.request.clone();
        let fallback_us = attempt.fallback_us;
        let visible = model.retained_label().map(|(_, request)| request);
        let recovery = super::seek::recover(
            visible,
            &request,
            model.decoder_output_us().is_some(),
            self.retained.occupied(),
        );
        match recovery {
            super::seek::Recovery::Wait => {}
            super::seek::Recovery::Commit | super::seek::Recovery::Forget => {
                self.seek_attempt = None;
            }
            super::seek::Recovery::Restore => self.park(
                model,
                fallback_us,
                &format!(
                    "The seek wrote no reader. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            ),
        }
    }

    fn remember_seek(&mut self, model: &mut Explorer, at_us: u64, exited: bool) {
        let Some(request) = self.admission.as_ref().map(|item| item.request.clone()) else {
            model.note_message(
                "This decoder has no stored admission. Seek was not requested. Capture keeps running.",
            );
            return;
        };
        self.seek_attempt = None;
        self.handoff_attempt = None;
        self.hold = Some(Hold {
            at_us,
            pending_us: None,
            phase: HoldPhase::Paused,
        });
        model.show_paused(&request, at_us);
        if exited {
            model.note_message(&format!(
                "The client already exited. That is not a seek. Position kept at {at_us} us. Press H to listen there. Capture keeps running."
            ));
        } else {
            model.note_message(&format!(
                "Paused at {at_us} us. Press H to start one listen there. Capture keeps running."
            ));
        }
    }

    fn park(&mut self, model: &mut Explorer, at_us: u64, message: &str) {
        self.follow.clear();
        let request = self.admission.as_ref().map(|item| item.request.clone());
        self.seek_attempt = None;
        self.handoff_attempt = None;
        self.hold = Some(Hold {
            at_us,
            pending_us: None,
            phase: HoldPhase::Paused,
        });
        if let Some(request) = request.as_deref() {
            model.show_paused(request, at_us);
        }
        model.note_message(message);
    }

    fn resume(
        &mut self,
        directory: &Path,
        model: &mut Explorer,
        recording: String,
        seek_us: u64,
        end_us: Option<u64>,
    ) {
        if self.live.occupied() || self.retained.occupied() {
            model.note_message(
                "A listen is still running. The pause stays. A second decoder was not started. Capture keeps running.",
            );
            return;
        }
        let Some(request) = model.allocate_retained() else {
            return;
        };
        match self
            .retained
            .start_file(directory, &recording, &request, seek_us, end_us)
        {
            Ok(()) => {
                self.hold = None;
                self.admission = Some(Admission {
                    recording,
                    origin_us: seek_us,
                    end_us,
                    request: request.clone(),
                });
                model.begin_retained(&request);
                model.note_message(&format!(
                    "Starting one retained listen at {seek_us} us. Request {request}. Capture keeps running."
                ));
            }
            Err(error) => model.note_message(&start_failure(&error)),
        }
    }

    fn recover_handoff(&mut self, model: &mut Explorer) {
        let Some(attempt) = self.handoff_attempt.as_ref() else {
            return;
        };
        let request = attempt.request.clone();
        let fallback_us = attempt.fallback_us;
        let visible = model.retained_label().map(|(_, request)| request);
        let recovery = super::seek::recover(
            visible,
            &request,
            model.decoder_output_us().is_some(),
            self.retained.occupied(),
        );
        match recovery {
            super::seek::Recovery::Wait => {}
            super::seek::Recovery::Commit | super::seek::Recovery::Forget => {
                self.handoff_attempt = None;
            }
            super::seek::Recovery::Restore => self.park(
                model,
                fallback_us,
                &format!(
                    "The next segment wrote no reader. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            ),
        }
    }

    fn advance_segment(&mut self, model: &mut Explorer) {
        if self.seek_attempt.is_some() || self.handoff_attempt.is_some() || self.hold.is_some() {
            return;
        }
        let decision = super::handoff::decide(&self.handoff_input(model));
        if matches!(
            decision,
            super::handoff::Decision::Idle | super::handoff::Decision::Wait
        ) {
            return;
        }
        let Some(request) = self.admission.as_ref().map(|item| item.request.clone()) else {
            return;
        };
        if self.explained.as_deref() == Some(request.as_str()) {
            return;
        }
        self.explained = Some(request);
        match decision {
            super::handoff::Decision::Jump {
                fallback_us,
                target_us,
            } => self.arm_follow(model, fallback_us, target_us),
            other => {
                if let Some(message) = super::handoff::refusal(&other) {
                    model.note_message(message);
                }
            }
        }
    }

    fn arm_follow(&mut self, model: &mut Explorer, fallback_us: u64, target_us: u64) {
        let Some(recording) = self.admission.as_ref().map(|item| item.recording.clone()) else {
            self.park(
                model,
                fallback_us,
                "This decoder has no stored admission. The next segment was not started. Capture keeps running.",
            );
            return;
        };
        self.follow.arm(recording, fallback_us, target_us);
    }

    fn finish_handoff(
        &mut self,
        directory: &Path,
        model: &mut Explorer,
        fallback_us: u64,
        target_us: u64,
    ) {
        if self.live.occupied() || self.retained.occupied() {
            self.park(
                model,
                fallback_us,
                &format!(
                    "A listen is still running. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            );
            return;
        }
        let Some(admission) = self.admission.as_ref() else {
            self.park(
                model,
                fallback_us,
                "This decoder has no stored admission. The next segment was not started. Capture keeps running.",
            );
            return;
        };
        let recording = admission.recording.clone();
        let Some(request) = model.allocate_retained() else {
            self.park(
                model,
                fallback_us,
                &format!(
                    "The next segment wrote no reader. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            );
            return;
        };
        match self
            .retained
            .start_file(directory, &recording, &request, target_us, None)
        {
            Ok(()) => {
                self.hold = None;
                self.admission = Some(Admission {
                    recording,
                    origin_us: target_us,
                    end_us: None,
                    request: request.clone(),
                });
                self.seek_attempt = None;
                self.handoff_attempt = Some(SeekAttempt {
                    fallback_us,
                    request: request.clone(),
                });
                model.begin_retained(&request);
                model.note_message(&format!(
                    "Starting the next sealed segment at {target_us} us. Request {request}. Capture keeps running."
                ));
            }
            Err(_) => self.park(
                model,
                fallback_us,
                &format!(
                    "The next segment wrote no reader. Position kept at {fallback_us} us. Press H to listen there. Capture keeps running."
                ),
            ),
        }
    }

    fn handoff_input(&mut self, model: &Explorer) -> super::handoff::Input {
        let (presence, origin_us, cited, decoder_us, live_occupied, stale, tracked) = {
            let input = self.pause_input(model);
            let stale = input.request.is_some() && input.visible_request != input.request;
            (
                input.presence,
                input.origin_us,
                input.end_us.is_some(),
                input.decoder_us,
                input.live_occupied,
                stale,
                input.recording.is_some() && input.request.is_some(),
            )
        };
        let bound = self.retained.segment_bound();
        let stop_requested = self.retained.stop_requested();
        let excerpt = bound.is_some_and(|item| item.excerpt);
        let block = if live_occupied {
            super::handoff::Block::Live
        } else if stop_requested {
            super::handoff::Block::Stopped
        } else if !tracked {
            super::handoff::Block::Untracked
        } else if stale {
            super::handoff::Block::Stale
        } else if cited || excerpt {
            super::handoff::Block::Cited
        } else {
            super::handoff::Block::None
        };
        let receipt = if bound.is_some_and(|item| item.failed) {
            super::handoff::Receipt::Failed
        } else if bound.is_some_and(|item| item.completed) {
            super::handoff::Receipt::Completed
        } else {
            super::handoff::Receipt::Pending
        };
        super::handoff::Input {
            presence,
            origin_us,
            decoder_us,
            segment_end_us: bound.map(|item| item.end_us),
            block,
            receipt,
        }
    }

    fn remember_spawn(&mut self, start: &Start) {
        self.hold = None;
        self.seek_attempt = None;
        self.handoff_attempt = None;
        self.explained = None;
        self.admission = match start {
            Start::Station(_) => None,
            Start::Recording { id, request } => Some(Admission {
                recording: id.clone(),
                origin_us: 0,
                end_us: None,
                request: request.clone(),
            }),
            Start::Finding {
                recording,
                start_us,
                end_us,
                request,
                ..
            } => Some(Admission {
                recording: recording.clone(),
                origin_us: *start_us,
                end_us: Some(*end_us),
                request: request.clone(),
            }),
            Start::Interval {
                id,
                request,
                start_us,
                end_us,
            } => Some(Admission {
                recording: id.clone(),
                origin_us: *start_us,
                end_us: Some(*end_us),
                request: request.clone(),
            }),
        };
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
