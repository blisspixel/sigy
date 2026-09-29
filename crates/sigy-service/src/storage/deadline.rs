//! Whether one queued recognition job is still inside its measured delay.
//!
//! The deadline is the recording's last seal plus the retained audio at that
//! profile's observed pace. It is not stored, and it is not the worker's safety cap.

use crate::fairness::Class;

/// Microseconds in one second of audio. The pace is already milliseconds per second.
const US_PER_SECOND: u128 = 1_000_000;

/// The live deadline, when this job still qualifies.
///
/// `pace_ms_per_audio_second` is `None` when this profile has no completed sample.
/// Zero is a real measurement meaning under 1 ms per audio second, so the deadline
/// is the seal itself. A fraction of that integer pace rounds up. Overflow, a missing
/// seal, no retained audio, or a paused or absent monitor returns `None`, and the
/// caller keeps the job in the batch class.
#[must_use]
pub(crate) fn live_deadline(
    now_ms: i64,
    followed: bool,
    audio_us: u64,
    seal_ms: Option<i64>,
    pace_ms_per_audio_second: Option<u64>,
) -> Option<i64> {
    if !followed || audio_us == 0 {
        return None;
    }
    let pace = pace_ms_per_audio_second?;
    let seal_ms = seal_ms?;
    let lag_ms = lag_ms(audio_us, pace)?;
    let deadline_ms = seal_ms.checked_add(lag_ms)?;
    (now_ms <= deadline_ms).then_some(deadline_ms)
}

/// Class and the deadline the chooser should see. Batch keeps the enqueue time.
#[must_use]
pub(crate) fn classify(
    now_ms: i64,
    ready_ms: i64,
    followed: bool,
    audio_us: u64,
    seal_ms: Option<i64>,
    pace_ms_per_audio_second: Option<u64>,
) -> (Class, i64) {
    match live_deadline(
        now_ms,
        followed,
        audio_us,
        seal_ms,
        pace_ms_per_audio_second,
    ) {
        Some(deadline_ms) => (Class::Live, deadline_ms),
        None => (Class::Batch, ready_ms),
    }
}

/// Milliseconds of wall time for this audio at the already-rounded pace.
/// A non-zero remainder rounds up. The product has to fit in an `i64`.
fn lag_ms(audio_us: u64, pace_ms_per_audio_second: u64) -> Option<i64> {
    let product = u128::from(audio_us).checked_mul(u128::from(pace_ms_per_audio_second))?;
    let rounded = product.checked_add(US_PER_SECOND - 1)? / US_PER_SECOND;
    i64::try_from(rounded).ok()
}

#[cfg(test)]
mod tests {
    use super::live_deadline;

    #[test]
    fn classification_fails_closed() {
        assert_eq!(
            live_deadline(10, false, 1_000_000, Some(10), Some(1)),
            None,
            "an unfollowed source stays batch"
        );
        assert_eq!(
            live_deadline(10, true, 1_000_000, Some(10), None),
            None,
            "a profile outside the measured sample stays batch"
        );
        assert_eq!(
            live_deadline(10, true, 0, Some(10), Some(1)),
            None,
            "a pin with no retained audio stays batch"
        );
        assert_eq!(
            live_deadline(10, true, 1_000_000, None, Some(1)),
            None,
            "a recording with no seal stays batch"
        );
        assert_eq!(
            live_deadline(i64::MAX, true, u64::MAX, Some(0), Some(u64::MAX)),
            None,
            "an overflowing product stays batch"
        );
        assert_eq!(
            live_deadline(1, true, 1_000_000, Some(i64::MAX), Some(1)),
            None,
            "a deadline that does not fit stays batch"
        );
    }

    #[test]
    fn the_measured_window_ends_at_the_seal_plus_audio() {
        // 1.5 seconds at 1 ms per audio second is 1.5 ms, which rounds up to 2.
        assert_eq!(
            live_deadline(12, true, 1_500_000, Some(10), Some(1)),
            Some(12)
        );
        assert_eq!(live_deadline(13, true, 1_500_000, Some(10), Some(1)), None);
        // Two seconds at 1500 ms per audio second is an exact 3000 ms.
        assert_eq!(
            live_deadline(3_010, true, 2_000_000, Some(10), Some(1_500)),
            Some(3_010)
        );
        // Under 1 ms per audio second adds no time, so the window closes at the seal.
        assert_eq!(
            live_deadline(10, true, 2_000_000, Some(10), Some(0)),
            Some(10)
        );
        assert_eq!(live_deadline(11, true, 2_000_000, Some(10), Some(0)), None);
    }
}
