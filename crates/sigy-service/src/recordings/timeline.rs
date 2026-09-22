//! Map one timeline position onto one sealed segment. The open tail is not a file.

use crate::storage::dvr::{GapCause, RecordingGap, RecordingInterval, blocking_gap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    Segment {
        ordinal: u32,
        object_key: String,
        format: String,
        file_seek_us: u64,
        file_decoded_us: u64,
    },
    Gap {
        cause: GapCause,
    },
    OpenTail {
        live_us: u64,
    },
    Outside {
        live_us: u64,
    },
    Unpublished,
}

#[must_use]
pub fn live_edge(intervals: &[RecordingInterval]) -> Option<u64> {
    intervals
        .iter()
        .map(|interval| interval.decoded_end_us)
        .max()
}

#[must_use]
pub fn earliest_retained(intervals: &[RecordingInterval]) -> Option<u64> {
    intervals
        .iter()
        .map(|interval| interval.decoded_start_us)
        .min()
}

#[must_use]
pub const fn position_expired(playhead_us: u64, earliest_us: Option<u64>) -> bool {
    match earliest_us {
        None => true,
        Some(earliest) => playhead_us < earliest,
    }
}

#[must_use]
pub fn locate(
    intervals: &[RecordingInterval],
    gaps: &[RecordingGap],
    tail_open: bool,
    seek_us: u64,
) -> Located {
    if let Some(gap) = blocking_gap(gaps, seek_us) {
        return Located::Gap { cause: gap.cause };
    }
    if let Some(interval) = intervals
        .iter()
        .find(|interval| seek_us >= interval.decoded_start_us && seek_us < interval.decoded_end_us)
    {
        let Some(file_seek_us) = seek_us.checked_sub(interval.decoded_start_us) else {
            return Located::Outside {
                live_us: interval.decoded_end_us,
            };
        };
        let Some(file_decoded_us) = interval
            .decoded_end_us
            .checked_sub(interval.decoded_start_us)
            .filter(|duration| *duration > file_seek_us)
        else {
            return Located::Outside {
                live_us: interval.decoded_end_us,
            };
        };
        return Located::Segment {
            ordinal: interval.ordinal,
            object_key: interval.object_key.clone(),
            format: interval.format.clone(),
            file_seek_us,
            file_decoded_us,
        };
    }
    let Some(live_us) = live_edge(intervals) else {
        return Located::Unpublished;
    };
    if tail_open && seek_us >= live_us {
        return Located::OpenTail { live_us };
    }
    Located::Outside { live_us }
}

#[cfg(test)]
mod tests {
    use super::{Located, earliest_retained, live_edge, locate, position_expired};
    use crate::storage::dvr::{GapCause, RecordingGap, RecordingInterval};

    fn interval(ordinal: u32, start: u64, end: u64, key: &str) -> RecordingInterval {
        RecordingInterval {
            ordinal,
            decoded_start_us: start,
            decoded_end_us: end,
            byte_start: start,
            byte_end: end,
            object_key: key.repeat(32),
            sha256: "ab".repeat(32),
            format: "wav".into(),
            ceiling_bytes: 32 * 1024 * 1024,
        }
    }

    fn gap(start: u64, end: u64) -> RecordingGap {
        RecordingGap {
            ordinal: 0,
            cause: GapCause::CapturePause,
            start_us: start,
            end_us: end,
        }
    }

    #[test]
    fn seek_maps_each_segment_and_refuses_the_hole_and_the_tail() -> Result<(), String> {
        let intervals = vec![
            interval(0, 0, 1_000_000, "a"),
            interval(1, 1_000_000, 2_000_000, "b"),
        ];
        let first = locate(&intervals, &[], false, 200_000);
        let Located::Segment {
            ordinal,
            file_seek_us,
            ..
        } = first
        else {
            return Err(format!("segment 0: {first:?}"));
        };
        if (ordinal, file_seek_us) != (0, 200_000) {
            return Err(format!("segment 0 offset {ordinal} {file_seek_us}"));
        }
        let second = locate(&intervals, &[], true, 1_200_000);
        let Located::Segment {
            ordinal,
            file_seek_us,
            object_key,
            ..
        } = second
        else {
            return Err(format!("segment 1: {second:?}"));
        };
        if (ordinal, file_seek_us) != (1, 200_000) || !object_key.starts_with('b') {
            return Err(format!("segment 1 offset {ordinal} {file_seek_us}"));
        }
        if !matches!(
            locate(&intervals, &[gap(2_000_000, 60_000_000)], false, 2_000_000),
            Located::Gap {
                cause: GapCause::CapturePause
            }
        ) {
            return Err("gap was playable".into());
        }
        if !matches!(
            locate(&intervals, &[], true, 2_500_000),
            Located::OpenTail { live_us: 2_000_000 }
        ) {
            return Err("open tail was playable".into());
        }
        if !matches!(
            locate(&intervals, &[], false, 2_000_000),
            Located::Outside { live_us: 2_000_000 }
        ) {
            return Err("finished edge was playable".into());
        }
        if !matches!(locate(&[], &[], true, 0), Located::Unpublished) {
            return Err("unpublished tail was playable".into());
        }
        if live_edge(&intervals) != Some(2_000_000) || earliest_retained(&intervals) != Some(0) {
            return Err("edge".into());
        }
        if !position_expired(200_000, Some(1_000_000))
            || position_expired(200_000, Some(0))
            || !position_expired(0, None)
        {
            return Err("expiry".into());
        }
        Ok(())
    }
}
