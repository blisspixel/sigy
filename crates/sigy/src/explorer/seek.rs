//! Heard seek for one retained listen.
//!
//! The step is ten seconds on the recording timeline. The metadata playhead is a
//! different control. A gap and the open tail are refused by retained-reader
//! admission, which writes no reader.

use super::pause::{PauseInput, Presence};

/// One fixed step. This is not a typed range editor.
pub(super) const STEP_US: u64 = 10_000_000;

/// What `[` or `]` should do. Only [`Decision::Jump`] stops the current child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Idle,
    MissingDecoder,
    OutsideRange,
    Untracked,
    Stale,
    Wait,
    LiveOccupied,
    Move { at_us: u64 },
    Jump { fallback_us: u64, target_us: u64 },
    Remember { at_us: u64 },
}

/// What to do with a seek whose child has already been spawned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Recovery {
    Wait,
    Commit,
    Restore,
    Forget,
}

/// Choose the next step. This writes nothing and starts no process.
#[must_use]
pub(super) fn decide(input: &PauseInput<'_>, forward: bool) -> Decision {
    if stale(input) {
        return Decision::Stale;
    }
    match input.presence {
        Presence::Absent => Decision::Idle,
        Presence::Stopping => Decision::Wait,
        Presence::Playing => jump(input, forward),
        Presence::Paused => move_hold(input, forward),
        Presence::Exited => remember(input, forward),
    }
}

/// Decide whether a spawned seek landed.
///
/// A decoder sample commits the new position. Exit with no sample restores the
/// previous position. The child staying up keeps the attempt.
#[must_use]
pub(super) fn recover(
    visible: Option<&str>,
    request: &str,
    decoder_present: bool,
    child_occupied: bool,
) -> Recovery {
    if visible != Some(request) {
        return Recovery::Forget;
    }
    if decoder_present {
        return Recovery::Commit;
    }
    if child_occupied {
        Recovery::Wait
    } else {
        Recovery::Restore
    }
}

/// Status text for a key that must not stop or start a decoder.
#[must_use]
pub(super) fn refusal(decision: &Decision) -> Option<&'static str> {
    match decision {
        Decision::Idle => Some("No retained listen is active. Capture keeps running."),
        Decision::MissingDecoder => {
            Some("No decoder output time is stored. Seek was not requested. Capture keeps running.")
        }
        Decision::OutsideRange => {
            Some("That step leaves the admitted range. The listen stays. Capture keeps running.")
        }
        Decision::Untracked => Some(
            "This decoder has no stored admission. Seek was not requested. Capture keeps running.",
        ),
        Decision::Stale => Some(
            "This listen no longer matches the running decoder. Seek was not requested. Capture keeps running.",
        ),
        Decision::Wait => Some(
            "The retained listen is still stopping. A second decoder was not started. Capture keeps running.",
        ),
        Decision::LiveOccupied => Some(
            "A station listen is active. The seek stays. A second decoder was not started. Capture keeps running.",
        ),
        Decision::Move { .. } | Decision::Jump { .. } | Decision::Remember { .. } => None,
    }
}

fn stale(input: &PauseInput<'_>) -> bool {
    if !matches!(
        input.presence,
        Presence::Playing | Presence::Exited | Presence::Paused
    ) {
        return false;
    }
    match input.request {
        Some(request) => input.visible_request != Some(request),
        None => false,
    }
}

fn jump(input: &PauseInput<'_>, forward: bool) -> Decision {
    if input.live_occupied {
        return Decision::LiveOccupied;
    }
    let current = match playing_us(input) {
        Ok(current) => current,
        Err(decision) => return decision,
    };
    let Some(target_us) = step(current, forward, input.origin_us, input.end_us) else {
        return Decision::OutsideRange;
    };
    Decision::Jump {
        fallback_us: current,
        target_us,
    }
}

fn move_hold(input: &PauseInput<'_>, forward: bool) -> Decision {
    let Some(current) = input.held_us else {
        return Decision::Untracked;
    };
    if input.recording.is_none() || input.request.is_none() {
        return Decision::Untracked;
    }
    let Some(at_us) = step(current, forward, input.origin_us, input.end_us) else {
        return Decision::OutsideRange;
    };
    Decision::Move { at_us }
}

fn remember(input: &PauseInput<'_>, forward: bool) -> Decision {
    let current = match playing_us(input) {
        Ok(current) => current,
        Err(decision) => return decision,
    };
    let Some(at_us) = step(current, forward, input.origin_us, input.end_us) else {
        return Decision::OutsideRange;
    };
    Decision::Remember { at_us }
}

fn playing_us(input: &PauseInput<'_>) -> Result<u64, Decision> {
    if input.recording.is_none() || input.request.is_none() {
        return Err(Decision::Untracked);
    }
    let Some(decoder_us) = input.decoder_us else {
        return Err(Decision::MissingDecoder);
    };
    input
        .origin_us
        .checked_add(decoder_us)
        .ok_or(Decision::OutsideRange)
}

fn step(current: u64, forward: bool, origin: u64, end: Option<u64>) -> Option<u64> {
    let target = if forward {
        current.checked_add(STEP_US)?
    } else if current < origin.checked_add(STEP_US)? {
        return None;
    } else {
        current - STEP_US
    };
    if target < origin {
        return None;
    }
    if end.is_some_and(|end| target >= end) {
        return None;
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::super::pause::{PauseInput, Presence, file_listen_args};
    use super::{Decision, Recovery, STEP_US, decide, recover, refusal};

    fn playing(decoder_us: u64, origin_us: u64, end_us: Option<u64>) -> PauseInput<'static> {
        PauseInput {
            presence: Presence::Playing,
            decoder_us: Some(decoder_us),
            recording: Some("rec-kept"),
            origin_us,
            end_us,
            request: Some("reader-kept"),
            visible_request: Some("reader-kept"),
            held_us: None,
            live_occupied: false,
        }
    }

    #[test]
    fn forward_and_back_stay_inside_a_whole_file() {
        let forward = decide(&playing(1_500_000, 0, None), true);
        assert_eq!(
            forward,
            Decision::Jump {
                fallback_us: 1_500_000,
                target_us: 1_500_000 + STEP_US,
            }
        );
        assert!(refusal(&forward).is_none());
        let back = decide(&playing(15_000_000, 0, None), false);
        assert_eq!(
            back,
            Decision::Jump {
                fallback_us: 15_000_000,
                target_us: 5_000_000,
            }
        );
        let Decision::Jump { target_us, .. } = back else {
            panic!("back seek");
        };
        let args = file_listen_args("rec-kept", "reader-next", target_us, None);
        assert!(args.contains(&"--seek-us".to_owned()));
        assert!(args.contains(&"5000000".to_owned()));
        assert!(!args.iter().any(|arg| arg == "--end-us"));
    }

    #[test]
    fn back_to_zero_omits_seek_and_a_short_step_refuses() {
        let back = decide(&playing(STEP_US, 0, None), false);
        assert_eq!(
            back,
            Decision::Jump {
                fallback_us: STEP_US,
                target_us: 0,
            }
        );
        let Decision::Jump { target_us, .. } = back else {
            panic!("zero seek");
        };
        let args = file_listen_args("rec-kept", "reader-next", target_us, None);
        assert!(!args.iter().any(|arg| arg == "--seek-us"));
        assert_eq!(
            decide(&playing(STEP_US - 1, 0, None), false),
            Decision::OutsideRange
        );
        assert!(refusal(&Decision::OutsideRange).is_some());
    }

    #[test]
    fn cited_interval_refuses_a_step_past_its_end_or_start() {
        let forward = decide(&playing(500_000, 1_000_000, Some(3_000_000)), true);
        assert_eq!(forward, Decision::OutsideRange);
        let back = decide(&playing(1_500_000, 1_000_000, Some(3_000_000)), false);
        assert_eq!(back, Decision::OutsideRange);
        let inside = decide(&playing(1_500_000, 1_000_000, Some(20_000_000)), true);
        assert_eq!(
            inside,
            Decision::Jump {
                fallback_us: 2_500_000,
                target_us: 12_500_000,
            }
        );
        let Decision::Jump { target_us, .. } = inside else {
            panic!("cited seek");
        };
        let args = file_listen_args("rec-kept", "reader-next", target_us, Some(20_000_000));
        assert!(args.contains(&"--end-us".to_owned()));
        assert!(args.contains(&"20000000".to_owned()));
    }

    #[test]
    fn missing_sample_stopping_and_live_listen_do_not_jump() {
        let mut missing = playing(1_500_000, 0, None);
        missing.decoder_us = None;
        assert_eq!(decide(&missing, true), Decision::MissingDecoder);
        missing.presence = Presence::Stopping;
        missing.decoder_us = Some(1_500_000);
        assert_eq!(decide(&missing, true), Decision::Wait);
        missing.presence = Presence::Playing;
        missing.live_occupied = true;
        assert_eq!(decide(&missing, true), Decision::LiveOccupied);
        missing.live_occupied = false;
        missing.visible_request = Some("reader-other");
        assert_eq!(decide(&missing, true), Decision::Stale);
        missing.presence = Presence::Absent;
        missing.request = None;
        missing.visible_request = None;
        assert_eq!(decide(&missing, true), Decision::Idle);
    }

    #[test]
    fn paused_seek_moves_the_hold_and_exit_does_not_start_audio() {
        let mut paused = playing(1_500_000, 0, None);
        paused.presence = Presence::Paused;
        paused.held_us = Some(15_000_000);
        paused.decoder_us = None;
        assert_eq!(decide(&paused, false), Decision::Move { at_us: 5_000_000 });
        paused.presence = Presence::Exited;
        paused.decoder_us = Some(15_000_000);
        paused.held_us = None;
        assert_eq!(
            decide(&paused, true),
            Decision::Remember {
                at_us: 15_000_000 + STEP_US
            }
        );
        assert!(refusal(&Decision::Remember { at_us: 1 }).is_none());
    }

    #[test]
    fn a_child_with_no_sample_restores_and_a_sample_commits() {
        assert_eq!(
            recover(Some("reader-next"), "reader-next", false, true),
            Recovery::Wait
        );
        assert_eq!(
            recover(Some("reader-next"), "reader-next", true, true),
            Recovery::Commit
        );
        assert_eq!(
            recover(Some("reader-next"), "reader-next", false, false),
            Recovery::Restore
        );
        assert_eq!(
            recover(Some("reader-old"), "reader-next", false, false),
            Recovery::Forget
        );
        assert_eq!(
            recover(Some("reader-next"), "reader-next", true, false),
            Recovery::Commit
        );
    }
}
