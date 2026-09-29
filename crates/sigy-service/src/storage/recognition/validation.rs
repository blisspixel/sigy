use super::LocalAsrWork;
use crate::recognition::{
    MAX_ASR_CUES, MAX_ASR_TEXT_BYTES, RecognitionCoverage, RecognitionCue, RecognitionOutput,
    coverages_fit_one_page, is_sha256,
};

pub(super) fn output_valid(work: &LocalAsrWork, output: &RecognitionOutput) -> bool {
    output.profile_sha256 == work.job.request.profile_sha256
        && output.manifest_sha256 == work.job.manifest_sha256
        && coverages_match(work, output)
        && cues_fit(output)
}

fn coverages_match(work: &LocalAsrWork, output: &RecognitionOutput) -> bool {
    coverages_fit_one_page(output.coverages.len())
        && output
            .coverages
            .iter()
            .enumerate()
            .all(|(index, coverage)| coverage_sane(coverage, index))
        && tiles(work, output)
}

fn coverage_sane(coverage: &RecognitionCoverage, index: usize) -> bool {
    let duration = coverage.end_us.saturating_sub(coverage.start_us);
    let decoded = u128::from(coverage.sample_count) * 1_000_000;
    let pinned = u128::from(duration) * u128::from(coverage.sample_rate);
    usize::try_from(coverage.ordinal).ok() == Some(index)
        && is_sha256(&coverage.decoded_sha256)
        && (1..=384_000).contains(&coverage.sample_rate)
        && (1..=23_040_000).contains(&coverage.sample_count)
        && coverage.sample_count <= u64::from(coverage.sample_rate) * 30
        && (1..=30_000_000).contains(&duration)
        && decoded.abs_diff(pinned) <= 1_000_000
}

/// Coverages grouped by segment, abutting from each segment start to its end.
fn tiles(work: &LocalAsrWork, output: &RecognitionOutput) -> bool {
    let mut coverages = output.coverages.iter();
    let mut pending = coverages.next();
    for segment in &work.input.segments {
        let mut cursor = segment.start_us;
        let mut saw = false;
        while let Some(coverage) = pending {
            if coverage.interval_ordinal != segment.ordinal {
                break;
            }
            if coverage.start_us != cursor
                || coverage.end_us <= cursor
                || coverage.end_us > segment.end_us
                || coverage.source_sha256 != segment.source_sha256
            {
                return false;
            }
            cursor = coverage.end_us;
            saw = true;
            pending = coverages.next();
            if cursor == segment.end_us {
                break;
            }
        }
        if !saw || cursor != segment.end_us {
            return false;
        }
    }
    pending.is_none()
}

fn cues_fit(output: &RecognitionOutput) -> bool {
    if output.cues.len() > MAX_ASR_CUES {
        return false;
    }
    let mut bytes = 0;
    let mut end = 0_u64;
    for (ordinal, cue) in output.cues.iter().enumerate() {
        if usize::try_from(cue.ordinal).ok() != Some(ordinal)
            || cue.start_us < end
            || cue.end_us <= cue.start_us
            || cue.script.is_empty()
            || cue.script.len() > 4096
            || !inside_one_coverage(&output.coverages, cue)
        {
            return false;
        }
        bytes += cue.script.len();
        if bytes > MAX_ASR_TEXT_BYTES {
            return false;
        }
        end = cue.end_us;
    }
    true
}

fn inside_one_coverage(coverages: &[RecognitionCoverage], cue: &RecognitionCue) -> bool {
    coverages
        .iter()
        .filter(|coverage| cue.start_us >= coverage.start_us && cue.end_us <= coverage.end_us)
        .count()
        == 1
}
