//! Recording metadata only. No samples, playback, capture or subscription.

use sigy_service::storage::dvr::Recording;

#[cfg(test)]
pub(super) use tests::{fixture, truncated_fixture};

pub(super) const MAX_INTERVALS: usize = 1_024;
const MAX_CELLS: usize = 120;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeline {
    planned_us: u64,
    intervals: Vec<Interval>,
    gaps: Vec<(u64, u64)>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Interval {
    start_us: u64,
    end_us: u64,
    available: bool,
}

impl Timeline {
    pub fn from_recording(recording: &Recording) -> Self {
        let available = matches!(recording.storage_state.as_str(), "reserved" | "retained");
        Self {
            planned_us: recording.duration_seconds.saturating_mul(1_000_000),
            intervals: recording
                .intervals
                .iter()
                .take(MAX_INTERVALS)
                .map(|interval| Interval {
                    start_us: interval.decoded_start_us,
                    end_us: interval.decoded_end_us,
                    available: available && !interval.released,
                })
                .collect(),
            gaps: recording
                .gaps
                .iter()
                .take(MAX_INTERVALS)
                .map(|gap| (gap.start_us, gap.end_us))
                .collect(),
            truncated: recording.intervals.len() > MAX_INTERVALS
                || recording.gaps.len() > MAX_INTERVALS,
        }
    }

    fn extent_us(&self) -> u64 {
        self.intervals
            .iter()
            .map(|interval| interval.end_us)
            .chain(self.gaps.iter().map(|(_, end)| *end))
            .fold(self.planned_us, u64::max)
    }

    pub fn summary(&self) -> String {
        let retained = self
            .intervals
            .iter()
            .filter(|interval| interval.available)
            .count();
        format!(
            "{}Published intervals: {retained} available, {} unavailable; gaps {}",
            if self.truncated {
                "Partial counts: "
            } else {
                ""
            },
            self.intervals.len() - retained,
            self.gaps.len()
        )
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn axis(&self) -> String {
        if self.truncated {
            return "Timeline unavailable: metadata result limit reached.".into();
        }
        format!(
            "{} to {}  planned {}",
            clock_label(0),
            clock_label(self.extent_us()),
            clock_label(self.planned_us)
        )
    }

    /// Exact media interval for the detail view. JSON keeps the same microseconds.
    pub fn detail(&self) -> String {
        if self.truncated {
            return "Exact interval unavailable: metadata result limit reached.".into();
        }
        format!("[0, {})us; planned {}us", self.extent_us(), self.planned_us)
    }

    pub fn compact_axis(&self) -> String {
        if self.truncated {
            return "Timeline unavailable: metadata limit.".into();
        }
        format!("{} to {}", clock_label(0), clock_label(self.extent_us()))
    }

    pub fn cells(&self, requested_width: usize) -> String {
        self.cells_marked(requested_width, false)
    }

    /// Unicode marks and the ASCII fallback classify every cell the same way.
    pub fn cells_marked(&self, requested_width: usize, marks: bool) -> String {
        if self.truncated {
            return String::new();
        }
        let extent = self.extent_us();
        let width = requested_width
            .min(MAX_CELLS)
            .min(usize::try_from(extent).unwrap_or(MAX_CELLS));
        let width = u64::try_from(width).unwrap_or(0);
        if width == 0 {
            return String::new();
        }
        let boundary = |index| extent / width * index + extent % width * index / width;
        (0..width)
            .map(|index| Self::glyph(self.mask_at(boundary(index), boundary(index + 1)), marks))
            .collect()
    }

    fn mask_at(&self, start: u64, end: u64) -> u8 {
        let mut mask = 0u8;
        let mut covered = 0u64;
        for interval in &self.intervals {
            let overlap = overlap(start, end, interval.start_us, interval.end_us);
            if overlap > 0 {
                mask |= if interval.available { 1 } else { 2 };
                covered = covered.saturating_add(overlap);
            }
        }
        for (gap_start, gap_end) in &self.gaps {
            let overlap = overlap(start, end, *gap_start, *gap_end);
            if overlap > 0 {
                mask |= 4;
                covered = covered.saturating_add(overlap);
            }
        }
        if covered < end - start {
            mask |= 8;
        }
        mask
    }

    fn glyph(mask: u8, marks: bool) -> char {
        match mask {
            1 => {
                if marks {
                    '█'
                } else {
                    '#'
                }
            }
            2 => {
                if marks {
                    '░'
                } else {
                    'x'
                }
            }
            4 => {
                if marks {
                    '╳'
                } else {
                    '!'
                }
            }
            0 | 8 => {
                if marks {
                    '·'
                } else {
                    '.'
                }
            }
            _ => '+',
        }
    }

    pub fn legend(marks: bool) -> &'static str {
        if marks {
            "█ retained  ░ unavailable  ╳ gap  · not yet published  + mixed"
        } else {
            "# retained, x unavailable, ! gap, . not yet published, + mixed"
        }
    }

    /// Narrow terminals keep every state name on one line. `open` is the unpublished tail.
    pub fn legend_for(marks: bool, width: usize) -> &'static str {
        let full = Self::legend(marks);
        if width == 0 || full.chars().count() <= width {
            return full;
        }
        if marks {
            "█ retained ░ unavail ╳ gap · open +mix"
        } else {
            "# retained x unavail ! gap . open +mix"
        }
    }
}

/// `m:ss`, or `h:mm:ss` once the duration reaches an hour. Integer seconds, rounded down.
fn clock_label(us: u64) -> String {
    let seconds = us / 1_000_000;
    let minutes = seconds / 60;
    let secs = seconds % 60;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}:{:02}:{:02}", minutes % 60, secs)
    } else {
        format!("{minutes}:{secs:02}")
    }
}

fn overlap(start: u64, end: u64, other_start: u64, other_end: u64) -> u64 {
    end.min(other_end).saturating_sub(start.max(other_start))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_of(cell: char) -> u8 {
        match cell {
            '#' | '█' => 1,
            'x' | '░' => 2,
            '!' | '╳' => 4,
            '.' | '·' => 8,
            '+' => 16,
            _ => 0,
        }
    }

    #[test]
    fn narrow_legends_name_every_state_on_one_line() {
        assert_eq!(Timeline::legend_for(false, 80), Timeline::legend(false));
        assert_eq!(Timeline::legend_for(true, 0), Timeline::legend(true));
        assert_eq!(
            Timeline::legend_for(false, 40),
            "# retained x unavail ! gap . open +mix"
        );
        assert_eq!(
            Timeline::legend_for(true, 40),
            "█ retained ░ unavail ╳ gap · open +mix"
        );
        assert!(Timeline::legend_for(false, 40).chars().count() <= 40);
        assert!(Timeline::legend_for(true, 40).chars().count() <= 40);
    }

    #[test]
    fn clock_labels_use_integer_seconds() {
        for (us, label) in [
            (0, "0:00"),
            (1_999_999, "0:01"),
            (59_999_999, "0:59"),
            (60_000_000, "1:00"),
            (3_599_000_000, "59:59"),
            (3_660_000_000, "1:01:00"),
        ] {
            assert_eq!(clock_label(us), label);
        }
        assert!(clock_label(u64::MAX).contains(':'));
    }

    pub(crate) fn fixture() -> Timeline {
        Timeline {
            planned_us: 10_000_000,
            intervals: vec![
                Interval {
                    start_us: 2_000_000,
                    end_us: 4_000_000,
                    available: true,
                },
                Interval {
                    start_us: 4_000_000,
                    end_us: 6_000_000,
                    available: false,
                },
            ],
            gaps: vec![(0, 2_000_000), (6_000_000, 8_000_000)],
            truncated: false,
        }
    }

    pub(crate) fn truncated_fixture() -> Timeline {
        Timeline {
            truncated: true,
            ..fixture()
        }
    }

    #[test]
    fn published_gap_released_and_unpublished_times_stay_separate() {
        let timeline = fixture();
        assert_eq!(timeline.cells(10), "!!##xx!!..");
        assert_eq!(timeline.cells(5), "!#x!.");
        assert_eq!(timeline.cells(2), "++");
        assert!(
            timeline
                .summary()
                .contains("1 available, 1 unavailable; gaps 2")
        );
        assert_eq!(timeline.axis(), "0:00 to 0:10  planned 0:10");
        assert_eq!(timeline.detail(), "[0, 10000000)us; planned 10000000us");
        assert_eq!(timeline.cells_marked(10, true), "╳╳██░░╳╳··");
        assert_eq!(
            timeline.cells(10).chars().map(class_of).collect::<Vec<_>>(),
            timeline
                .cells_marked(10, true)
                .chars()
                .map(class_of)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn partial_cells_never_claim_the_whole_cell_is_available() {
        let mut timeline = fixture();
        timeline.intervals[0].end_us = 3_000_001;
        assert_eq!(timeline.cells(5), "!+x!.");
        assert_eq!(timeline.cells(0), "");
        assert_eq!(timeline.cells(usize::MAX).len(), MAX_CELLS);
    }

    #[test]
    fn published_media_can_extend_beyond_the_planned_duration() {
        let mut timeline = fixture();
        timeline.planned_us = 1_000_000;
        assert_eq!(timeline.axis(), "0:00 to 0:08  planned 0:01");
        assert!(timeline.detail().contains("[0, 8000000)us"));
        assert_eq!(timeline.cells(4), "!#x!");
    }

    #[test]
    fn extreme_clock_and_tiny_view_are_bounded_without_floats() {
        let timeline = Timeline {
            planned_us: u64::MAX,
            ..Timeline::default()
        };
        assert_eq!(timeline.cells(120), ".".repeat(120));
        let tiny = Timeline {
            planned_us: 1,
            ..Timeline::default()
        };
        assert_eq!(tiny.cells(120), ".");
    }

    #[test]
    fn snapshot_availability_tracks_release_and_whole_file_deletion()
    -> Result<(), serde_json::Error> {
        let mut recording: Recording = serde_json::from_value(serde_json::json!({
            "id": "one", "source_revision": "radio:v1", "state": "running", "object_key": "fixture", "duration_seconds": 5, "maximum_bytes": 100,
            "retention": "temporary", "storage_state": "reserved", "charged_bytes": 100,
            "media_bytes": null, "sha256": null, "format": null, "decoded_microseconds": null,
            "end_reason": null, "processing_receipt": null, "failure_detail": null, "profile": "radio",
            "escrow_bytes": 0, "open_ceiling": 100, "open_object_key": "open", "lease_renewals": 1,
            "intervals": [{"ordinal": 0, "decoded_start_us": 0, "decoded_end_us": 1_000_000, "byte_start": 0, "byte_end": 100, "object_key": "sealed", "sha256": "fixture", "format": "wav", "ceiling_bytes": 100, "released": false}],
            "gaps": []
        }))?;
        assert_eq!(Timeline::from_recording(&recording).cells(5), "#....");
        recording.intervals[0].released = true;
        assert_eq!(Timeline::from_recording(&recording).cells(5), "x....");
        recording.intervals[0].released = false;
        recording.storage_state = "deleted".into();
        assert_eq!(Timeline::from_recording(&recording).cells(5), "x....");
        recording.intervals = vec![recording.intervals[0].clone(); MAX_INTERVALS + 1];
        let oversized = Timeline::from_recording(&recording);
        assert!(oversized.truncated);
        assert_eq!(oversized.intervals.len(), MAX_INTERVALS);
        assert!(oversized.summary().starts_with("Partial counts:"));
        assert!(oversized.axis().contains("Timeline unavailable"));
        assert!(oversized.compact_axis().contains("Timeline unavailable"));
        assert_eq!(oversized.cells(120), "");
        // The omitted last interval extends beyond the plan. Its absence must not
        // produce either an understated axis or a fabricated unpublished region.
        recording.intervals[MAX_INTERVALS].decoded_start_us = 10_000_000;
        recording.intervals[MAX_INTERVALS].decoded_end_us = 20_000_000;
        let omitted_publication = Timeline::from_recording(&recording);
        assert_eq!(omitted_publication.cells(120), "");
        assert!(!omitted_publication.axis().contains("5000000"));
        assert!(!omitted_publication.compact_axis().contains("5000000"));
        Ok(())
    }
}
