use super::LocalAsrWork;
use crate::recognition::{
    MAX_ASR_CUES, MAX_ASR_TEXT_BYTES, RecognitionCue, RecognitionOutput, is_sha256,
};

pub(super) fn output_valid(work: &LocalAsrWork, output: &RecognitionOutput) -> bool {
    output.profile_sha256 == work.job.request.profile_sha256
        && output.manifest_sha256 == work.job.manifest_sha256
        && coverages_match(work, output)
        && cues_fit(output)
}

fn coverages_match(work: &LocalAsrWork, output: &RecognitionOutput) -> bool {
    if output.coverages.len() != work.input.chunks.len() {
        return false;
    }
    output
        .coverages
        .iter()
        .zip(&work.input.chunks)
        .all(|(coverage, chunk)| {
            let duration = coverage.end_us.saturating_sub(coverage.start_us);
            let decoded = u128::from(coverage.sample_count) * 1_000_000;
            let pinned = u128::from(duration) * u128::from(coverage.sample_rate);
            coverage.ordinal == chunk.ordinal
                && coverage.interval_ordinal == chunk.interval_ordinal
                && coverage.start_us == chunk.start_us
                && coverage.end_us == chunk.end_us
                && coverage.source_sha256 == chunk.source_sha256
                && is_sha256(&coverage.decoded_sha256)
                && (1..=384_000).contains(&coverage.sample_rate)
                && (1..=23_040_000).contains(&coverage.sample_count)
                && coverage.sample_count <= u64::from(coverage.sample_rate) * 30
                && duration <= 30_000_000
                && duration > 0
                && decoded.abs_diff(pinned) <= 1_000_000
        })
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

fn inside_one_coverage(
    coverages: &[crate::recognition::RecognitionCoverage],
    cue: &RecognitionCue,
) -> bool {
    coverages
        .iter()
        .filter(|coverage| cue.start_us >= coverage.start_us && cue.end_us <= coverage.end_us)
        .count()
        == 1
}
