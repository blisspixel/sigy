//! Wait for the next sealed segment. The open tail is not a file.
//!
//! One recording show runs on the existing one-second clock. A sealed interval
//! starts one listen file. A gap, a released edge, a truncated map, or a capture
//! that is no longer running does not start a decoder. The wait ends after the
//! retained-read deadline. `H`, `[`, `]`, `S`, quit, and a new confirmed play
//! clear this slot before pause or seek can run.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use sigy_service::recordings::{RETAINED_READ_DEADLINE_SECONDS, locate};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

use super::state::{Explorer, SealMap};

pub(super) const WAITING: &str =
    "Waiting for the next sealed segment. The open tail was not started. Capture keeps running.";
pub(super) const GAVE_UP: &str = "The wait for the next sealed segment ended. The open tail was not started. Capture keeps running.";
pub(super) const MISSING: &str =
    "No sealed segment contains that end. Nothing was started. Capture keeps running.";
pub(super) const FAULT: &str =
    "The recording map could not be read. The open tail was not started. Capture keeps running.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Start,
    Wait,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Judgment {
    Stay,
    Start,
    Clear,
}

enum Sight {
    Idle,
    Fault,
    Chart(SealMap),
}

struct Reading {
    judgment: Judgment,
    message: Option<&'static str>,
    announced: bool,
    fault_noted: bool,
}

pub(super) struct StartAt {
    pub fallback_us: u64,
    pub target_us: u64,
}

struct Armed {
    recording: String,
    fallback_us: u64,
    target_us: u64,
    started: Instant,
    announced: bool,
    fault_noted: bool,
    pending: Option<Pending>,
}

struct Pending {
    generation: u64,
    receive: mpsc::Receiver<Result<SealMap, ()>>,
    task: JoinHandle<()>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
pub(super) struct Slot {
    armed: Option<Armed>,
    generation: u64,
}

impl Slot {
    pub(super) fn arm(&mut self, recording: String, fallback_us: u64, target_us: u64) {
        self.generation = self.generation.saturating_add(1);
        self.armed = Some(Armed {
            recording,
            fallback_us,
            target_us,
            started: Instant::now(),
            announced: false,
            fault_noted: false,
            pending: None,
        });
    }

    pub(super) fn clear(&mut self) {
        if self.armed.is_none() {
            return;
        }
        self.generation = self.generation.saturating_add(1);
        self.armed = None;
    }

    pub(super) fn request(&mut self, runtime: &Runtime, directory: &Path) {
        let generation = self.generation;
        let (recording, elapsed) = {
            let Some(armed) = self.armed.as_ref() else {
                return;
            };
            if armed.pending.is_some() {
                return;
            }
            (armed.recording.clone(), armed.started.elapsed())
        };
        if elapsed >= deadline() {
            return;
        }
        let directory = directory.to_path_buf();
        let (send, receive) = mpsc::channel(1);
        let task = runtime.spawn(async move {
            let result = read_map(&directory, &recording).await;
            let _ = send.send(result).await;
        });
        if let Some(armed) = self.armed.as_mut() {
            armed.pending = Some(Pending {
                generation,
                receive,
                task,
            });
        } else {
            task.abort();
        }
    }

    pub(super) fn poll(&mut self, model: &mut Explorer) -> Option<StartAt> {
        let sight = self.take_sight();
        self.apply(sight, model)
    }

    fn take_sight(&mut self) -> Sight {
        let generation = self.generation;
        let Some(armed) = self.armed.as_mut() else {
            return Sight::Idle;
        };
        if armed
            .pending
            .as_ref()
            .is_some_and(|pending| pending.generation != generation)
        {
            armed.pending = None;
            return Sight::Idle;
        }
        let (update, finished) = {
            let Some(pending) = armed.pending.as_mut() else {
                return Sight::Idle;
            };
            let update = match pending.receive.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::error::TryRecvError::Empty) => None,
                Err(mpsc::error::TryRecvError::Disconnected) => Some(Err(())),
            };
            let finished = pending.task.is_finished() && pending.receive.is_empty();
            (update, finished)
        };
        if finished {
            armed.pending = None;
        }
        match update {
            Some(Ok(chart)) => Sight::Chart(chart),
            Some(Err(())) => Sight::Fault,
            None => Sight::Idle,
        }
    }

    fn apply(&mut self, sight: Sight, model: &mut Explorer) -> Option<StartAt> {
        let armed = self.armed.as_ref()?;
        let reading = judge(
            armed.target_us,
            armed.started.elapsed(),
            armed.announced,
            armed.fault_noted,
            sight,
        );
        let start = StartAt {
            fallback_us: armed.fallback_us,
            target_us: armed.target_us,
        };
        if let Some(armed) = self.armed.as_mut() {
            armed.announced = reading.announced;
            armed.fault_noted = reading.fault_noted;
        }
        if let Some(message) = reading.message {
            model.note_message(message);
        }
        match reading.judgment {
            Judgment::Stay => None,
            Judgment::Start => {
                self.clear();
                Some(start)
            }
            Judgment::Clear => {
                self.clear();
                None
            }
        }
    }

    #[cfg(test)]
    fn started_ago(&mut self, elapsed: Duration) {
        let Some(armed) = self.armed.as_mut() else {
            return;
        };
        armed.started = Instant::now()
            .checked_sub(elapsed)
            .unwrap_or_else(Instant::now);
    }
}

fn deadline() -> Duration {
    Duration::from_secs(RETAINED_READ_DEADLINE_SECONDS)
}

/// Choose the next listen from one show. A truncated map never starts audio.
#[must_use]
pub(super) fn classify(at_us: u64, chart: &SealMap) -> Step {
    if chart.truncated {
        return Step::Stop;
    }
    let tail_open =
        chart.state == "running" && chart.open_ceiling > 0 && chart.open_object_key.is_some();
    match locate(&chart.intervals, &chart.gaps, tail_open, at_us) {
        sigy_service::recordings::Located::Segment { object_key, .. } => {
            if chart.open_object_key.as_deref() == Some(object_key.as_str()) {
                if tail_open { Step::Wait } else { Step::Stop }
            } else {
                Step::Start
            }
        }
        sigy_service::recordings::Located::OpenTail { .. } => {
            if tail_open {
                Step::Wait
            } else {
                Step::Stop
            }
        }
        sigy_service::recordings::Located::Gap { .. }
        | sigy_service::recordings::Located::Outside { .. }
        | sigy_service::recordings::Located::Unpublished => Step::Stop,
    }
}

fn judge(
    at_us: u64,
    elapsed: Duration,
    announced: bool,
    fault_noted: bool,
    sight: Sight,
) -> Reading {
    if elapsed >= deadline() {
        return Reading {
            judgment: Judgment::Clear,
            message: Some(GAVE_UP),
            announced,
            fault_noted,
        };
    }
    match sight {
        Sight::Idle => Reading {
            judgment: Judgment::Stay,
            message: None,
            announced,
            fault_noted,
        },
        Sight::Fault => Reading {
            judgment: Judgment::Stay,
            message: if fault_noted { None } else { Some(FAULT) },
            announced,
            fault_noted: true,
        },
        Sight::Chart(chart) => match classify(at_us, &chart) {
            Step::Start => Reading {
                judgment: Judgment::Start,
                message: None,
                announced,
                fault_noted,
            },
            Step::Wait => Reading {
                judgment: Judgment::Stay,
                message: if announced { None } else { Some(WAITING) },
                announced: true,
                fault_noted,
            },
            Step::Stop => Reading {
                judgment: Judgment::Clear,
                message: Some(MISSING),
                announced,
                fault_noted,
            },
        },
    }
}

async fn read_map(directory: &Path, recording: &str) -> Result<SealMap, ()> {
    let (_via, snapshot) =
        super::client::fetch(directory, super::client::original_operation(recording))
            .await
            .map_err(|_| ())?;
    let page = snapshot.recording_page.ok_or(())?;
    let record = page
        .entries
        .iter()
        .find(|entry| entry.id == recording)
        .ok_or(())?;
    Ok(SealMap::from_recording(record))
}

#[cfg(test)]
mod tests {
    use super::super::state::{Emphasis, Explorer, Modes, SealMap};
    use super::{
        FAULT, GAVE_UP, Judgment, MISSING, Sight, Slot, Step, WAITING, classify, deadline, judge,
    };
    use sigy_service::recordings::RETAINED_READ_DEADLINE_SECONDS;
    use sigy_service::storage::dvr::{GapCause, Recording, RecordingGap, RecordingInterval};
    use std::time::Duration;

    fn model() -> Explorer {
        Explorer::new(
            Modes {
                reduced_motion: true,
                linear: true,
                monochrome: true,
                emphasis: Emphasis::Standard,
            },
            10_000,
        )
    }

    fn segment(start: u64, end: u64, key: &str, released: bool) -> RecordingInterval {
        RecordingInterval {
            ordinal: 0,
            decoded_start_us: start,
            decoded_end_us: end,
            byte_start: 0,
            byte_end: 1,
            object_key: key.into(),
            sha256: "ab".repeat(32),
            format: "wav".into(),
            ceiling_bytes: 1,
            released,
        }
    }

    fn gap(start: u64, end: u64, cause: GapCause) -> RecordingGap {
        RecordingGap {
            ordinal: 0,
            cause,
            start_us: start,
            end_us: end,
        }
    }

    fn chart(
        state: &str,
        ceiling: u64,
        open_key: Option<&str>,
        intervals: Vec<RecordingInterval>,
        gaps: Vec<RecordingGap>,
    ) -> SealMap {
        SealMap {
            state: state.into(),
            open_ceiling: ceiling,
            open_object_key: open_key.map(str::to_owned),
            intervals,
            gaps,
            truncated: false,
        }
    }

    fn sealed_pair() -> SealMap {
        chart(
            "completed",
            0,
            None,
            vec![
                segment(0, 5_000_000, &"a".repeat(32), false),
                segment(5_000_000, 10_000_000, &"b".repeat(32), false),
            ],
            Vec::new(),
        )
    }

    fn open_tail() -> SealMap {
        chart(
            "running",
            1,
            Some(&"c".repeat(32)),
            vec![segment(0, 5_000_000, &"a".repeat(32), false)],
            Vec::new(),
        )
    }

    #[test]
    fn a_sealed_next_interval_starts_and_an_open_tail_waits() {
        assert_eq!(classify(5_000_000, &sealed_pair()), Step::Start);
        let mut still_capturing = sealed_pair();
        still_capturing.state = "running".into();
        still_capturing.open_ceiling = 1;
        still_capturing.open_object_key = Some("c".repeat(32));
        assert_eq!(classify(5_000_000, &still_capturing), Step::Start);
        assert_eq!(classify(5_000_000, &open_tail()), Step::Wait);
        let mut same_object = open_tail();
        same_object.intervals[0].object_key = "c".repeat(32);
        same_object.intervals[0].decoded_end_us = 6_000_000;
        assert_eq!(classify(5_000_000, &same_object), Step::Wait);
        same_object.state = "completed".into();
        assert_eq!(classify(5_000_000, &same_object), Step::Stop);
        let mut finished_edge = open_tail();
        finished_edge.state = "completed".into();
        finished_edge.open_ceiling = 0;
        finished_edge.open_object_key = None;
        assert_eq!(classify(5_000_000, &finished_edge), Step::Stop);
    }

    #[test]
    fn a_gap_a_released_edge_and_a_truncated_map_stop() {
        let codec = chart(
            "failed",
            0,
            None,
            vec![segment(0, 5_000_000, &"a".repeat(32), false)],
            vec![gap(5_000_000, 6_000_000, GapCause::CodecChange)],
        );
        assert_eq!(classify(5_000_000, &codec), Step::Stop);
        let released = chart(
            "completed",
            0,
            None,
            vec![segment(5_000_000, 10_000_000, &"b".repeat(32), true)],
            Vec::new(),
        );
        assert_eq!(classify(5_000_000, &released), Step::Stop);
        assert_eq!(
            classify(0, &chart("running", 0, None, Vec::new(), Vec::new())),
            Step::Stop
        );
        let mut partial = sealed_pair();
        partial.truncated = true;
        assert_eq!(classify(5_000_000, &partial), Step::Stop);
    }

    #[test]
    fn the_wait_is_announced_once_and_a_later_seal_starts() {
        let waiting = judge(
            5_000_000,
            Duration::from_secs(1),
            false,
            false,
            Sight::Chart(open_tail()),
        );
        assert_eq!(waiting.judgment, Judgment::Stay);
        assert_eq!(waiting.message, Some(WAITING));
        assert!(waiting.announced);
        let again = judge(
            5_000_000,
            Duration::from_secs(2),
            true,
            false,
            Sight::Chart(open_tail()),
        );
        assert_eq!(again.message, None);
        let start = judge(
            5_000_000,
            Duration::from_secs(3),
            true,
            false,
            Sight::Chart(sealed_pair()),
        );
        assert_eq!(start.judgment, Judgment::Start);
        assert_eq!(start.message, None);
        let idle = judge(5_000_000, Duration::ZERO, false, false, Sight::Idle);
        assert_eq!(idle.judgment, Judgment::Stay);
        assert_eq!(idle.message, None);
    }

    #[test]
    fn a_fault_is_noted_once_and_the_deadline_ignores_a_sealed_interval() {
        let first = judge(
            5_000_000,
            Duration::from_secs(1),
            false,
            false,
            Sight::Fault,
        );
        assert_eq!(first.message, Some(FAULT));
        assert!(first.fault_noted);
        let second = judge(5_000_000, Duration::from_secs(2), false, true, Sight::Fault);
        assert_eq!(second.message, None);
        let late = judge(
            5_000_000,
            deadline(),
            true,
            true,
            Sight::Chart(sealed_pair()),
        );
        assert_eq!(late.judgment, Judgment::Clear);
        assert_eq!(late.message, Some(GAVE_UP));
        let missing = judge(
            5_000_000,
            Duration::from_secs(1),
            false,
            false,
            Sight::Chart(chart("completed", 0, None, Vec::new(), Vec::new())),
        );
        assert_eq!(missing.judgment, Judgment::Clear);
        assert_eq!(missing.message, Some(MISSING));
        assert_ne!(MISSING, GAVE_UP);
        assert_ne!(MISSING, WAITING);
    }

    #[test]
    fn the_slot_starts_once_waits_once_and_then_gives_up() {
        let mut slot = Slot::default();
        let mut model = model();
        assert!(slot.poll(&mut model).is_none());
        slot.arm("rec".into(), 5_000_000, 5_000_000);
        let initial = model.status().to_owned();
        assert!(slot.apply(Sight::Idle, &mut model).is_none());
        assert_eq!(model.status(), initial);
        let start = slot.apply(Sight::Chart(open_tail()), &mut model);
        assert!(start.is_none());
        assert_eq!(model.status(), WAITING);
        model.note_message("marker");
        assert!(slot.apply(Sight::Chart(open_tail()), &mut model).is_none());
        assert_eq!(model.status(), "marker");
        let begun = slot.apply(Sight::Chart(sealed_pair()), &mut model);
        assert_eq!(
            begun
                .as_ref()
                .map(|item| (item.fallback_us, item.target_us)),
            Some((5_000_000, 5_000_000))
        );
        model.note_message("marker");
        assert!(slot.apply(Sight::Chart(open_tail()), &mut model).is_none());
        assert_eq!(model.status(), "marker");

        slot.arm("rec".into(), 1, 1);
        slot.started_ago(deadline() + Duration::from_secs(1));
        assert!(
            slot.apply(Sight::Chart(sealed_pair()), &mut model)
                .is_none()
        );
        assert_eq!(model.status(), GAVE_UP);
        assert_eq!(RETAINED_READ_DEADLINE_SECONDS, 30 * 60);
    }

    #[test]
    fn a_long_recording_is_truncated_before_it_can_choose_a_file() -> Result<(), serde_json::Error>
    {
        let recording: Recording = serde_json::from_value(serde_json::json!({
            "id": "rec",
            "source_revision": "radio:v1",
            "state": "running",
            "object_key": "fixture",
            "duration_seconds": 5,
            "maximum_bytes": 100,
            "retention": "temporary",
            "storage_state": "reserved",
            "charged_bytes": 0,
            "media_bytes": null,
            "sha256": null,
            "format": null,
            "decoded_microseconds": null,
            "end_reason": null,
            "processing_receipt": null,
            "failure_detail": null,
            "profile": "radio",
            "escrow_bytes": 0,
            "open_ceiling": 1,
            "open_object_key": "c".repeat(32),
            "lease_renewals": 0,
            "intervals": [],
            "gaps": []
        }))?;
        let mut recording = recording;
        let row = segment(0, 1, &"a".repeat(32), false);
        recording.intervals = vec![row; super::super::timeline::MAX_INTERVALS + 1];
        recording.intervals[0].decoded_start_us = 5_000_000;
        recording.intervals[0].decoded_end_us = 10_000_000;
        recording.intervals[0].object_key = "b".repeat(32);
        let map = SealMap::from_recording(&recording);
        assert!(map.truncated);
        assert_eq!(map.intervals.len(), super::super::timeline::MAX_INTERVALS);
        assert_eq!(map.state, "running");
        assert_eq!(classify(5_000_000, &map), Step::Stop);
        Ok(())
    }
}
