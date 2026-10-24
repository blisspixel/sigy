//! Start the next sealed segment after one whole-segment listen completes.
//!
//! The service receipt supplies the exclusive segment end. A cited excerpt does
//! not continue. The terminal does not prefetch, and it does not invent a gap
//! from the timeline projection. [`Decision::Jump`] arms a follow read before any
//! decoder starts. That read refuses a gap, the open tail, and a missing segment
//! without writing a reader.

use super::pause::Presence;

/// What a completed retained listen should do. Only [`Decision::Jump`] starts
/// another decoder, and only after the current child has already exited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Idle,
    Wait,
    MissingDecoder,
    Cited,
    Failed,
    Outside,
    Stale,
    Untracked,
    Jump { fallback_us: u64, target_us: u64 },
}

/// Why a finished listen must not continue, before the receipt is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Block {
    None,
    Live,
    Stopped,
    Stale,
    Untracked,
    Cited,
}

/// Service receipt observed for this request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Receipt {
    Pending,
    Completed,
    Failed,
}

/// Facts already stored for the current listen. This writes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Input {
    pub presence: Presence,
    pub origin_us: u64,
    pub decoder_us: Option<u64>,
    pub segment_end_us: Option<u64>,
    pub block: Block,
    pub receipt: Receipt,
}

/// Choose whether the finished segment continues. A running child stays put.
#[must_use]
pub(super) fn decide(input: &Input) -> Decision {
    if !matches!(input.presence, Presence::Exited) {
        return Decision::Idle;
    }
    match input.block {
        Block::Live | Block::Stopped => return Decision::Wait,
        Block::Untracked => return Decision::Untracked,
        Block::Stale => return Decision::Stale,
        Block::None | Block::Cited => {}
    }
    match input.receipt {
        Receipt::Failed => return Decision::Failed,
        Receipt::Pending => return Decision::Wait,
        Receipt::Completed => {}
    }
    if input.block == Block::Cited {
        return Decision::Cited;
    }
    let Some(end) = input.segment_end_us else {
        return Decision::Wait;
    };
    if end <= input.origin_us {
        return Decision::Outside;
    }
    if input.decoder_us.is_none() {
        return Decision::MissingDecoder;
    }
    Decision::Jump {
        fallback_us: end,
        target_us: end,
    }
}

/// Status text when no next decoder is started.
#[must_use]
pub(super) fn refusal(decision: &Decision) -> Option<&'static str> {
    match decision {
        Decision::MissingDecoder => Some(
            "No decoder output time is stored. The next segment was not started. Capture keeps running.",
        ),
        Decision::Cited => Some(
            "This cited listen stays inside its range. The next segment was not started. Capture keeps running.",
        ),
        Decision::Failed => Some(
            "The retained reader did not complete. The next segment was not started. Capture keeps running.",
        ),
        Decision::Outside => Some(
            "The next segment would not move forward. It was not started. Capture keeps running.",
        ),
        Decision::Stale => Some(
            "This listen no longer matches the running decoder. The next segment was not started. Capture keeps running.",
        ),
        Decision::Untracked => Some(
            "This decoder has no stored admission. The next segment was not started. Capture keeps running.",
        ),
        Decision::Idle | Decision::Wait | Decision::Jump { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::pause::{Presence, file_listen_args};
    use super::{Block, Decision, Input, Receipt, decide, refusal};

    fn exited() -> Input {
        Input {
            presence: Presence::Exited,
            origin_us: 0,
            decoder_us: Some(4_000_000),
            segment_end_us: Some(5_000_000),
            block: Block::None,
            receipt: Receipt::Completed,
        }
    }

    #[test]
    fn a_completed_segment_hands_off_at_its_exclusive_end() {
        let decision = decide(&exited());
        assert_eq!(
            decision,
            Decision::Jump {
                fallback_us: 5_000_000,
                target_us: 5_000_000,
            }
        );
        let args = file_listen_args("rec-kept", "reader-next", 5_000_000, None);
        assert!(args.iter().any(|arg| arg == "--seek-us"));
        assert!(args.iter().any(|arg| arg == "5000000"));
        assert!(!args.iter().any(|arg| arg == "--end-us"));
        assert!(refusal(&decision).is_none());
    }

    #[test]
    fn a_running_child_does_not_prefetch() {
        let mut input = exited();
        input.presence = Presence::Playing;
        assert_eq!(decide(&input), Decision::Idle);
        input.presence = Presence::Paused;
        assert_eq!(decide(&input), Decision::Idle);
        input.presence = Presence::Stopping;
        assert_eq!(decide(&input), Decision::Idle);
    }

    #[test]
    fn a_cited_or_unfinished_listen_does_not_continue() {
        let mut input = exited();
        input.block = Block::Cited;
        assert_eq!(decide(&input), Decision::Cited);
        input.block = Block::None;
        input.receipt = Receipt::Pending;
        assert_eq!(decide(&input), Decision::Wait);
        input.receipt = Receipt::Failed;
        assert_eq!(decide(&input), Decision::Failed);
        input.receipt = Receipt::Completed;
        input.decoder_us = None;
        assert_eq!(decide(&input), Decision::MissingDecoder);
        input.decoder_us = Some(1);
        input.segment_end_us = Some(0);
        assert_eq!(decide(&input), Decision::Outside);
    }

    #[test]
    fn a_live_listen_a_stop_and_a_stale_row_wait_or_refuse() {
        let mut input = exited();
        input.block = Block::Live;
        assert_eq!(decide(&input), Decision::Wait);
        assert!(refusal(&decide(&input)).is_none());
        input.block = Block::Stopped;
        assert_eq!(decide(&input), Decision::Wait);
        input.block = Block::Stale;
        assert_eq!(decide(&input), Decision::Stale);
        input.block = Block::Untracked;
        assert_eq!(decide(&input), Decision::Untracked);
    }
}
