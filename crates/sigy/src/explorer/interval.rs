//! Choose one sealed interval from the loaded recording map.
//!
//! The choice writes nothing and starts no decoder. A later confirmed listen
//! passes these bounds to `listen file`.

use super::state::SealMap;

pub(super) const TRUNCATED: &str =
    "This recording map is partial. No interval was started. Capture keeps running.";
pub(super) const MISSING: &str =
    "No sealed interval is selected. Nothing was started. Capture keeps running.";
pub(super) const OPEN: &str =
    "That interval is the open tail. Nothing was started. Capture keeps running.";
pub(super) const RELEASED: &str =
    "That interval is released. Nothing was started. Capture keeps running.";
pub(super) const GAP: &str =
    "That interval overlaps a gap. Nothing was started. Capture keeps running.";
pub(super) const EMPTY: &str =
    "That interval has no positive length. Nothing was started. Capture keeps running.";
pub(super) const UNSTORED: &str =
    "This recording is not reserved or retained. Nothing was started. Capture keeps running.";

/// Half-open recording-timeline bounds of one sealed interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Bounds {
    pub start_us: u64,
    pub end_us: u64,
}

/// Why the loaded map cannot name a playable interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    Truncated,
    Missing,
    Open,
    Released,
    Gap,
    Empty,
}

#[must_use]
pub(super) const fn sentence(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Truncated => TRUNCATED,
        Refusal::Missing => MISSING,
        Refusal::Open => OPEN,
        Refusal::Released => RELEASED,
        Refusal::Gap => GAP,
        Refusal::Empty => EMPTY,
    }
}

/// Pick the newest playable interval. A truncated map never chooses a file.
///
/// An open object, a release, a gap, and an empty length are skipped when an
/// earlier sealed interval can play. When none can, the newest interval's
/// refusal is returned.
pub(super) fn latest_playable(chart: &SealMap) -> Result<Bounds, Refusal> {
    if chart.truncated {
        return Err(Refusal::Truncated);
    }
    let mut newest_refusal = Refusal::Missing;
    let mut saw_interval = false;
    for index in (0..chart.intervals.len()).rev() {
        match select(chart, index) {
            Ok(bounds) => return Ok(bounds),
            Err(refusal) => {
                if !saw_interval {
                    newest_refusal = refusal;
                    saw_interval = true;
                }
            }
        }
    }
    Err(newest_refusal)
}

/// Pick `index` from the loaded map. A truncated map never chooses a file.
pub(super) fn select(chart: &SealMap, index: usize) -> Result<Bounds, Refusal> {
    if chart.truncated {
        return Err(Refusal::Truncated);
    }
    let Some(interval) = chart.intervals.get(index) else {
        return Err(Refusal::Missing);
    };
    if interval.released {
        return Err(Refusal::Released);
    }
    if chart.open_object_key.as_deref() == Some(interval.object_key.as_str()) {
        return Err(Refusal::Open);
    }
    if interval.decoded_start_us >= interval.decoded_end_us {
        return Err(Refusal::Empty);
    }
    let start_us = interval.decoded_start_us;
    let end_us = interval.decoded_end_us;
    let overlaps = chart
        .gaps
        .iter()
        .any(|gap| gap.start_us < end_us && gap.end_us > start_us);
    if overlaps {
        return Err(Refusal::Gap);
    }
    Ok(Bounds { start_us, end_us })
}

/// One visible line for the interval under the cursor.
#[must_use]
pub(super) fn caption(chart: &SealMap, index: usize) -> String {
    if chart.truncated {
        return "Partial recording map. No sealed interval can be chosen.".to_owned();
    }
    let len = chart.intervals.len();
    if len == 0 {
        return "No sealed interval is on this map.".to_owned();
    }
    let shown = index.min(len - 1);
    match select(chart, shown) {
        Ok(bounds) => format!(
            "Sealed interval {} of {len}: [{}, {}).",
            shown + 1,
            bounds.start_us,
            bounds.end_us
        ),
        Err(refusal) => format!(
            "Sealed interval {} of {len}: {}",
            shown + 1,
            sentence(refusal)
        ),
    }
}

#[cfg(test)]
mod tests {
    use sigy_service::storage::dvr::{GapCause, RecordingGap, RecordingInterval};

    use super::super::pause::file_listen_args;
    use super::super::state::SealMap;
    use super::{Bounds, Refusal, TRUNCATED, latest_playable, select};

    fn interval(start: u64, end: u64, key: &str, released: bool) -> RecordingInterval {
        RecordingInterval {
            ordinal: 0,
            decoded_start_us: start,
            decoded_end_us: end,
            byte_start: 0,
            byte_end: 100,
            object_key: key.repeat(32),
            sha256: "a".repeat(64),
            format: "wav".into(),
            ceiling_bytes: 100,
            released,
        }
    }

    fn chart(intervals: Vec<RecordingInterval>, gaps: Vec<RecordingGap>) -> SealMap {
        SealMap {
            state: "completed".into(),
            open_ceiling: 0,
            open_object_key: None,
            intervals,
            gaps,
            truncated: false,
        }
    }

    #[test]
    fn one_sealed_interval_is_its_own_half_open_bounds() {
        let map = chart(vec![interval(0, 5_000_000, "a", false)], Vec::new());
        let bounds = Bounds {
            start_us: 0,
            end_us: 5_000_000,
        };
        assert_eq!(select(&map, 0), Ok(bounds));
        let args = file_listen_args("rec", "reader-1", bounds.start_us, Some(bounds.end_us));
        assert!(!args.iter().any(|arg| arg == "--seek-us"));
        assert_eq!(
            args.iter()
                .position(|arg| arg == "--end-us")
                .and_then(|index| args.get(index + 1))
                .map(String::as_str),
            Some("5000000")
        );
    }

    #[test]
    fn a_later_interval_keeps_seek_and_end() {
        let map = chart(
            vec![
                interval(0, 5_000_000, "a", false),
                interval(5_000_000, 9_000_000, "b", false),
            ],
            Vec::new(),
        );
        let bounds = Bounds {
            start_us: 5_000_000,
            end_us: 9_000_000,
        };
        assert_eq!(select(&map, 1), Ok(bounds));
        let args = file_listen_args("rec", "reader-1", bounds.start_us, Some(bounds.end_us));
        assert_eq!(
            args.iter()
                .position(|arg| arg == "--seek-us")
                .and_then(|index| args.get(index + 1))
                .map(String::as_str),
            Some("5000000")
        );
        assert_eq!(
            args.iter()
                .position(|arg| arg == "--end-us")
                .and_then(|index| args.get(index + 1))
                .map(String::as_str),
            Some("9000000")
        );
    }

    #[test]
    fn the_newest_playable_interval_skips_the_open_object_and_stores_no_end() {
        let mut map = chart(
            vec![
                interval(0, 5_000_000, "a", false),
                interval(5_000_000, 9_000_000, "b", false),
                interval(9_000_000, 12_000_000, "c", false),
            ],
            Vec::new(),
        );
        map.open_object_key = Some("c".repeat(32));
        let bounds = Bounds {
            start_us: 5_000_000,
            end_us: 9_000_000,
        };
        assert_eq!(latest_playable(&map), Ok(bounds));
        let args = file_listen_args("rec", "reader-1", bounds.start_us, None);
        assert_eq!(
            args.iter()
                .position(|arg| arg == "--seek-us")
                .and_then(|index| args.get(index + 1))
                .map(String::as_str),
            Some("5000000")
        );
        assert!(!args.iter().any(|arg| arg == "--end-us"));
        map.intervals.pop();
        map.open_object_key = Some("b".repeat(32));
        map.intervals[0].released = true;
        assert_eq!(latest_playable(&map), Err(Refusal::Open));
        map.truncated = true;
        assert_eq!(latest_playable(&map), Err(Refusal::Truncated));
        assert_eq!(
            latest_playable(&chart(Vec::new(), Vec::new())),
            Err(Refusal::Missing)
        );
        let gapped = chart(
            vec![
                interval(0, 4_000_000, "d", false),
                interval(4_000_000, 8_000_000, "e", false),
            ],
            vec![RecordingGap {
                ordinal: 0,
                cause: GapCause::CapturePause,
                start_us: 4_000_000,
                end_us: 8_000_000,
            }],
        );
        assert_eq!(
            latest_playable(&gapped),
            Ok(Bounds {
                start_us: 0,
                end_us: 4_000_000,
            })
        );
    }

    #[test]
    fn an_open_object_a_release_a_gap_and_a_partial_map_refuse() {
        let mut open = chart(vec![interval(0, 5_000_000, "a", false)], Vec::new());
        open.open_object_key = Some("a".repeat(32));
        assert_eq!(select(&open, 0), Err(Refusal::Open));
        let released = chart(vec![interval(0, 5_000_000, "b", true)], Vec::new());
        assert_eq!(select(&released, 0), Err(Refusal::Released));
        let gapped = chart(
            vec![interval(0, 5_000_000, "b", false)],
            vec![RecordingGap {
                ordinal: 0,
                cause: GapCause::CodecChange,
                start_us: 1_000_000,
                end_us: 2_000_000,
            }],
        );
        assert_eq!(select(&gapped, 0), Err(Refusal::Gap));
        let adjacent = chart(
            vec![interval(0, 5_000_000, "b", false)],
            vec![RecordingGap {
                ordinal: 1,
                cause: GapCause::CapturePause,
                start_us: 5_000_000,
                end_us: 6_000_000,
            }],
        );
        assert_eq!(
            select(&adjacent, 0),
            Ok(Bounds {
                start_us: 0,
                end_us: 5_000_000,
            })
        );
        let mut partial = chart(vec![interval(0, 5_000_000, "b", false)], Vec::new());
        partial.truncated = true;
        assert_eq!(select(&partial, 0), Err(Refusal::Truncated));
        assert_eq!(super::sentence(Refusal::Truncated), TRUNCATED);
        assert_eq!(
            select(&chart(Vec::new(), Vec::new()), 0),
            Err(Refusal::Missing)
        );
        let empty = chart(vec![interval(5, 5, "b", false)], Vec::new());
        assert_eq!(select(&empty, 0), Err(Refusal::Empty));
    }
}
