//! One retained-listen child. Service reader receipts stay separate from process exit.

use std::{
    io::Read,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
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
    timeline_end_us: u64,
    excerpt: bool,
}

/// Sealed-segment end from the service receipt. The timeline projection is not
/// this value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SegmentBound {
    pub end_us: u64,
    pub excerpt: bool,
    pub completed: bool,
    pub failed: bool,
}

struct Bound {
    request: String,
    end_us: u64,
    excerpt: bool,
    completed: bool,
    failed: bool,
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
    timeline: bool,
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
    bound: Option<Bound>,
    phase_label: String,
    stop_requested: bool,
    released: bool,
    notices: Notices,
    fence: u64,
    show: Lane,
    stop_lane: Lane,
    timeline: Arc<AtomicU64>,
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

    pub(super) fn stop_requested(&self) -> bool {
        self.stop_requested
    }

    /// Segment end for the request this client still holds.
    #[must_use]
    pub(super) fn segment_bound(&self) -> Option<SegmentBound> {
        let bound = self.bound.as_ref()?;
        if self.request.as_deref() != Some(bound.request.as_str()) {
            return None;
        }
        Some(SegmentBound {
            end_us: bound.end_us,
            excerpt: bound.excerpt,
            completed: bound.completed,
            failed: bound.failed,
        })
    }

    pub(super) fn start_file(
        &mut self,
        directory: &Path,
        recording: &str,
        request: &str,
        seek_us: u64,
        end_us: Option<u64>,
    ) -> std::io::Result<()> {
        let args = super::pause::file_listen_args(recording, request, seek_us, end_us);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.launch(directory, request, &borrowed)
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
        self.observe_timeline(model);
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
        self.timeline = Arc::new(AtomicU64::new(0));
        let timeline = Arc::clone(&self.timeline);
        let stdout_task = child
            .stdout
            .take()
            .map(|pipe| drain_pipe(pipe, Arc::clone(&diagnostic)));
        let stderr_task = child
            .stderr
            .take()
            .map(|pipe| drain_stderr(pipe, Arc::clone(&diagnostic), Arc::clone(&timeline)));
        self.child = Some(child);
        self.input = input;
        self.diagnostic = diagnostic;
        self.stdout_task = stdout_task;
        self.stderr_task = stderr_task;
        self.request = Some(request.to_owned());
        self.reader_generation = None;
        self.receipt_state = None;
        self.bound = None;
        self.phase_label = "pending".into();
        self.stop_requested = false;
        self.released = false;
        self.notices = Notices::default();
        Ok(())
    }

    fn observe_timeline(&mut self, model: &mut Explorer) {
        let time = self.timeline.load(Ordering::Relaxed);
        if time == 0 {
            return;
        }
        let Some(request) = self.request.clone() else {
            return;
        };
        let first = model.decoder_output_us().is_none();
        if model.set_decoder_output(&request, time) && first && !self.notices.timeline {
            self.notices.timeline = true;
            model.note_message(
                "Decoder output is measured from the admitted start. This does not prove audible output.",
            );
        }
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
                self.bound = Some(Bound {
                    request: receipt.request.clone(),
                    end_us: receipt.timeline_end_us,
                    excerpt: receipt.excerpt,
                    completed: receipt.state == "completed",
                    failed: receipt.state == "failed",
                });
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

fn drain_stderr(
    mut pipe: impl Read + Send + 'static,
    diagnostic: Arc<Mutex<Vec<u8>>>,
    timeline: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 1024];
        let mut lines = StderrLines::default();
        while let Ok(count) = pipe.read(&mut buffer) {
            if count == 0 {
                if let Ok(mut stored) = diagnostic.lock() {
                    lines.finish(&mut stored, &timeline);
                }
                break;
            }
            if let Ok(mut stored) = diagnostic.lock() {
                lines.push(&buffer[..count], &mut stored, &timeline);
            }
        }
    })
}

#[derive(Default)]
struct StderrLines {
    line: Vec<u8>,
    tainted: bool,
}

impl StderrLines {
    fn push(&mut self, bytes: &[u8], diagnostic: &mut Vec<u8>, timeline: &AtomicU64) {
        for &byte in bytes {
            if byte == b'\n' {
                if !self.tainted
                    && let Some(time) = decoder_timeline_us(&self.line)
                {
                    timeline.fetch_max(time, Ordering::Relaxed);
                } else {
                    store_diagnostic(diagnostic, &self.line);
                    store_diagnostic(diagnostic, b"\n");
                }
                self.line.clear();
                self.tainted = false;
            } else if self.line.len() == 80 {
                store_diagnostic(diagnostic, &self.line);
                self.line.clear();
                self.tainted = true;
                self.line.push(byte);
            } else {
                self.line.push(byte);
            }
        }
    }

    fn finish(&mut self, diagnostic: &mut Vec<u8>, timeline: &AtomicU64) {
        if self.line.is_empty() {
            return;
        }
        self.push(b"\n", diagnostic, timeline);
    }
}

fn store_diagnostic(diagnostic: &mut Vec<u8>, bytes: &[u8]) {
    let room = 8192_usize.saturating_sub(diagnostic.len());
    let count = bytes.len().min(room);
    diagnostic.extend_from_slice(&bytes[..count]);
}

pub(super) fn decoder_timeline_us(line: &[u8]) -> Option<u64> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let digits = line.strip_prefix(b"decoder-timeline-us ")?;
    if digits.is_empty() || digits.len() > 10 || digits.starts_with(b"0") {
        return None;
    }
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let time = std::str::from_utf8(digits).ok()?.parse::<u64>().ok()?;
    if time == 0 || time > sigy_service::recordings::DECODER_TIMELINE_MAX_US {
        None
    } else {
        Some(time)
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
    let bound = segment_bound_from(&view);
    Ok(Receipt {
        request: view.spec.request_id,
        state: view.state,
        generation: view.spec.generation,
        timeline_end_us: bound.end_us,
        excerpt: bound.excerpt,
    })
}

pub(super) fn segment_bound_from(view: &sigy_service::control::RetainedReadView) -> SegmentBound {
    SegmentBound {
        end_us: view.spec.timeline_end_us,
        excerpt: view.spec.excerpt.is_some(),
        completed: view.state == "completed",
        failed: view.state == "failed",
    }
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
    use std::sync::atomic::AtomicU64;

    use super::{StderrLines, decoder_timeline_us, phase};

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

    #[test]
    fn decoder_timeline_lines_keep_the_latest_sample_and_the_other_text() {
        assert_eq!(
            decoder_timeline_us(b"decoder-timeline-us 1500000"),
            Some(1_500_000)
        );
        assert_eq!(
            decoder_timeline_us(b"decoder-timeline-us 1500000\r"),
            Some(1_500_000)
        );
        assert_eq!(decoder_timeline_us(b"decoder-timeline-us 0"), None);
        assert_eq!(decoder_timeline_us(b"decoder-timeline-us 01"), None);
        assert_eq!(decoder_timeline_us(b"decoder-timeline-us 1800100001"), None);
        assert_eq!(decoder_timeline_us(b"decoder output 1"), None);
        let timeline = AtomicU64::new(0);
        let mut lines = StderrLines::default();
        let mut diagnostic = Vec::new();
        lines.push(
            b"decoder-timeline-us 100000\nhelper failed\ndecoder-timeline-us 1500000\n",
            &mut diagnostic,
            &timeline,
        );
        assert_eq!(
            timeline.load(std::sync::atomic::Ordering::Relaxed),
            1_500_000
        );
        assert_eq!(diagnostic, b"helper failed\n");
        lines.push(b"decoder-timeline-us 100000\n", &mut diagnostic, &timeline);
        assert_eq!(
            timeline.load(std::sync::atomic::Ordering::Relaxed),
            1_500_000
        );
        lines.push(b"trailing failure", &mut diagnostic, &timeline);
        lines.finish(&mut diagnostic, &timeline);
        assert_eq!(diagnostic, b"helper failed\ntrailing failure\n");
    }

    #[test]
    fn segment_bound_uses_the_service_object_end_and_excerpt_flag() {
        let whole = view(5_000_000, false, "completed");
        assert_eq!(
            super::segment_bound_from(&whole),
            super::SegmentBound {
                end_us: 5_000_000,
                excerpt: false,
                completed: true,
                failed: false,
            }
        );
        let cited = view(5_000_000, true, "running");
        let bound = super::segment_bound_from(&cited);
        assert!(bound.excerpt);
        assert!(!bound.completed);
        assert_eq!(bound.end_us, 5_000_000);
        let failed = view(5_000_000, false, "failed");
        assert!(super::segment_bound_from(&failed).failed);
    }

    fn view(end: u64, excerpt: bool, state: &str) -> sigy_service::control::RetainedReadView {
        sigy_service::control::RetainedReadView {
            spec: sigy_service::control::RetainedReadSpec {
                request_id: "reader-kept".into(),
                generation: 1,
                recording_id: "rec".into(),
                source_revision: "rev".into(),
                ordinal: 0,
                object_key: "object".into(),
                sha256: "ab".into(),
                format: "wav".into(),
                bytes: 100,
                timeline_start_us: 0,
                timeline_end_us: end,
                file_seek_us: 0,
                file_duration_us: end,
                spec_sha256: "cd".into(),
                excerpt: excerpt.then_some(sigy_service::control::RetainedExcerpt {
                    version: 2,
                    timeline_end_us: 1,
                    citation: None,
                }),
            },
            state: state.into(),
            seek_us: 0,
            admitted_ms: 0,
            updated_ms: 0,
            completion_reason: None,
            recovery_reason: None,
        }
    }
}
