use super::LocalAsrWork;
use crate::recognition::{MAX_ASR_CUES, MAX_ASR_TEXT_BYTES, RecognitionOutput, is_sha256};

pub(super) fn output_valid(work: &LocalAsrWork, output: &RecognitionOutput) -> bool {
    let input = &work.input;
    let coverage = &output.coverage;
    if output.profile_sha256 != work.job.request.profile_sha256
        || output.manifest_sha256 != work.job.manifest_sha256
        || coverage.interval_ordinal != input.interval_ordinal
        || coverage.start_us != input.start_us
        || coverage.end_us != input.end_us
        || coverage.source_sha256 != input.source_sha256
        || !is_sha256(&coverage.decoded_sha256)
        || !(1..=384_000).contains(&coverage.sample_rate)
        || !(1..=23_040_000).contains(&coverage.sample_count)
        || coverage.sample_count > u64::from(coverage.sample_rate) * 60
        || output.cues.len() > MAX_ASR_CUES
    {
        return false;
    }
    // Quantized decoded duration must agree with the pinned media clock within one sample.
    // This checks a submitted relation, not the honesty of the decoder or its hash.
    let decoded = u128::from(coverage.sample_count) * 1_000_000;
    let pinned = u128::from(input.end_us - input.start_us) * u128::from(coverage.sample_rate);
    if decoded.abs_diff(pinned) > 1_000_000 {
        return false;
    }
    let mut bytes = 0;
    let mut end = input.start_us;
    for (ordinal, cue) in output.cues.iter().enumerate() {
        if usize::try_from(cue.ordinal).ok() != Some(ordinal)
            || cue.start_us < end
            || cue.end_us <= cue.start_us
            || cue.end_us > input.end_us
            || cue.script.is_empty()
            || cue.script.len() > 4096
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
