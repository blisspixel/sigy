//! Where one heard window becomes a published coverage.
//!
//! The admission plan is a fixed 30-second grid. A recognizer process hears at most
//! one grid window. When its last phrase still reaches that window's edge and the
//! segment continues, the phrase is not stored. The next window starts at the
//! phrase, so the phrase is heard intact and stored once. Silence, and a window
//! whose only phrase runs from the cursor to the edge, advance the full window.
//! That last case can still clip a word: the recognizer gave no earlier boundary.

use super::RecognitionCue;

/// A phrase ending within this distance of the heard edge is still unfinished.
/// whisper.cpp timestamps land on a 20 millisecond grid.
pub(crate) const EDGE_US: u64 = 20_000;
/// How far past the heard edge a cue end may extend before it is capped.
/// The cap keeps a rounding overrun visible without accepting an unbounded end.
pub(crate) const OVERRUN_US: u64 = 1_000_000;

/// The published end of one heard window, and the cue indexes that belong to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowCut {
    pub end_us: u64,
    pub keep: Vec<usize>,
}

/// Upper bound passed to the recognizer parser.
///
/// A window that finishes the segment clamps on its own end. Any earlier window
/// allows one second of overrun so a phrase that crosses the edge is visible.
#[must_use]
pub(crate) fn clamp_end(heard_end: u64, at_segment_end: bool) -> u64 {
    if at_segment_end {
        heard_end
    } else {
        heard_end.saturating_add(OVERRUN_US)
    }
}

/// Choose the published end of a window the recognizer has already heard.
#[must_use]
pub(crate) fn cut_window(
    cursor: u64,
    heard_end: u64,
    at_segment_end: bool,
    cues: &[RecognitionCue],
) -> WindowCut {
    let full = WindowCut {
        end_us: heard_end,
        keep: (0..cues.len()).collect(),
    };
    if at_segment_end || heard_end <= cursor {
        return full;
    }
    let Some(index) = cues.iter().position(|cue| {
        cue.start_us < heard_end && cue.end_us.saturating_add(EDGE_US) >= heard_end
    }) else {
        return full;
    };
    let boundary = cues[index].start_us;
    // A phrase that starts at the cursor has no earlier boundary to seek to.
    // A phrase that starts at the heard edge belongs to the next window.
    if boundary <= cursor || boundary >= heard_end {
        return full;
    }
    let keep = cues
        .iter()
        .enumerate()
        .take(index)
        .filter(|(_, cue)| {
            cue.start_us >= cursor && cue.end_us <= boundary && cue.end_us > cue.start_us
        })
        .map(|(index, _)| index)
        .collect();
    WindowCut {
        end_us: boundary,
        keep,
    }
}

/// Cues kept by `cut`, clamped so each one ends inside the published coverage.
///
/// `None` means a keep index does not name a cue.
#[must_use]
pub(crate) fn kept_cues(cues: &[RecognitionCue], cut: &WindowCut) -> Option<Vec<RecognitionCue>> {
    let mut kept = Vec::new();
    for index in &cut.keep {
        let cue = cues.get(*index)?;
        let end_us = cue.end_us.min(cut.end_us);
        if cue.start_us >= cut.end_us || end_us <= cue.start_us {
            continue;
        }
        kept.push(RecognitionCue {
            ordinal: u32::try_from(kept.len()).ok()?,
            start_us: cue.start_us,
            end_us,
            script: cue.script.clone(),
        });
    }
    Some(kept)
}

/// Sample count for a published prefix of the heard PCM.
///
/// The count matches `owned_us` at `sample_rate` within one sample, and it never
/// claims more samples than `pcm_samples`. `None` means there is nothing to own.
#[must_use]
pub(crate) fn owned_sample_count(owned_us: u64, sample_rate: u32, pcm_samples: u64) -> Option<u64> {
    if owned_us == 0 || sample_rate == 0 || pcm_samples == 0 {
        return None;
    }
    let count = owned_us.saturating_mul(u64::from(sample_rate)) / 1_000_000;
    Some(count.clamp(1, pcm_samples))
}

#[cfg(test)]
mod tests {
    use super::{EDGE_US, WindowCut, clamp_end, cut_window, kept_cues, owned_sample_count};
    use crate::recognition::RecognitionCue;

    fn cue(start_us: u64, end_us: u64, script: &str) -> RecognitionCue {
        RecognitionCue {
            ordinal: 0,
            start_us,
            end_us,
            script: script.into(),
        }
    }

    #[test]
    fn an_unfinished_phrase_rewinds_to_its_start() {
        let heard = 30_000_000;
        let cues = vec![
            cue(0, 10_000_000, "kept"),
            cue(29_700_000, heard + 250_000, "split"),
        ];
        let cut = cut_window(0, heard, false, &cues);
        assert_eq!(
            cut,
            WindowCut {
                end_us: 29_700_000,
                keep: vec![0],
            }
        );
        let kept = kept_cues(&cues, &cut).unwrap_or_else(|| panic!("cues"));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].script, "kept");
        assert_eq!(kept[0].end_us, 10_000_000);
    }

    #[test]
    fn a_phrase_that_fills_the_window_stays_until_the_edge() {
        let heard = 30_000_000;
        let cues = vec![cue(0, heard, "whole")];
        assert_eq!(
            cut_window(0, heard, false, &cues),
            WindowCut {
                end_us: heard,
                keep: vec![0],
            }
        );
    }

    #[test]
    fn silence_and_an_interior_phrase_advance_the_full_window() {
        let heard = 30_000_000;
        assert_eq!(
            cut_window(0, heard, false, &[]),
            WindowCut {
                end_us: heard,
                keep: Vec::new(),
            }
        );
        let cues = vec![cue(0, heard - EDGE_US - 1, "inside")];
        assert_eq!(cut_window(0, heard, false, &cues).end_us, heard);
        let edge = vec![cue(1_000_000, heard - EDGE_US, "near")];
        assert_eq!(cut_window(0, heard, false, &edge).end_us, 1_000_000);
    }

    #[test]
    fn the_final_window_keeps_its_edge_phrase() {
        let heard = 1_000_000;
        let cues = vec![
            cue(0, 400_000, "bonjour"),
            cue(500_000, heard + 250_000, "monde"),
        ];
        let cut = cut_window(0, heard, true, &cues);
        assert_eq!(cut.end_us, heard);
        let kept = kept_cues(&cues, &cut).unwrap_or_else(|| panic!("cues"));
        assert_eq!(kept[1].end_us, heard);
        assert_eq!(clamp_end(heard, true), heard);
        assert_eq!(clamp_end(heard, false), heard + 1_000_000);
    }

    #[test]
    fn a_cue_that_starts_on_the_heard_edge_is_not_a_rewind() {
        let heard = 30_000_000;
        let cues = vec![cue(heard, heard + 1_000_000, "next")];
        let cut = cut_window(0, heard, false, &cues);
        assert_eq!(cut.end_us, heard);
        let kept = kept_cues(&cues, &cut).unwrap_or_else(|| panic!("cues"));
        assert!(kept.is_empty());
    }

    #[test]
    fn owned_samples_match_the_published_prefix() {
        assert_eq!(owned_sample_count(1_500_000, 16_000, 32_000), Some(24_000));
        assert_eq!(owned_sample_count(1, 16_000, 16_000), Some(1));
        assert_eq!(owned_sample_count(0, 16_000, 16_000), None);
        assert_eq!(owned_sample_count(1_000, 16_000, 0), None);
    }
}
