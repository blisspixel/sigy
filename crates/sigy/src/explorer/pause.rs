//! Heard pause for one retained listen.
//!
//! The in-memory playback playhead is a different control. This decision uses the
//! decoder sample already shown on the retained row. A recording gap and the open
//! tail are refused by retained-reader admission, which writes no reader row.

/// Where the retained decoder is, from the explorer's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Presence {
    Absent,
    Playing,
    Stopping,
    Paused,
    Exited,
}

/// Facts the key needs. The child process stays in the audio client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PauseInput<'a> {
    pub presence: Presence,
    pub decoder_us: Option<u64>,
    pub recording: Option<&'a str>,
    pub origin_us: u64,
    pub end_us: Option<u64>,
    pub request: Option<&'a str>,
    pub visible_request: Option<&'a str>,
    pub held_us: Option<u64>,
    pub live_occupied: bool,
}

/// What capital H should do. Only [`Decision::Resume`] starts a decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Idle,
    MissingDecoder,
    OutsideRange,
    Untracked,
    Stale,
    Wait,
    LiveOccupied,
    Stop {
        at_us: u64,
    },
    Remember {
        at_us: u64,
    },
    Resume {
        recording: String,
        seek_us: u64,
        end_us: Option<u64>,
    },
}

/// Choose the next step. This writes nothing and starts no process.
#[must_use]
pub(super) fn decide(input: &PauseInput<'_>) -> Decision {
    if stale(input) {
        return Decision::Stale;
    }
    match input.presence {
        Presence::Absent => Decision::Idle,
        Presence::Stopping => Decision::Wait,
        Presence::Playing => position(input, true),
        Presence::Exited => position(input, false),
        Presence::Paused => resume(input),
    }
}

/// Arguments for one `listen file` child, before the parent pid.
///
/// A zero seek and no end match a whole-file listen. A positive seek is the
/// recording timeline. An end keeps the new listen inside that cited interval.
#[must_use]
pub(super) fn file_listen_args(
    recording: &str,
    request: &str,
    seek_us: u64,
    end_us: Option<u64>,
) -> Vec<String> {
    let mut args = vec![
        "listen".to_owned(),
        "file".to_owned(),
        recording.to_owned(),
        "--request".to_owned(),
        request.to_owned(),
        "--destination".to_owned(),
        "system".to_owned(),
    ];
    if seek_us > 0 {
        args.push("--seek-us".to_owned());
        args.push(seek_us.to_string());
    }
    if let Some(end_us) = end_us {
        args.push("--end-us".to_owned());
        args.push(end_us.to_string());
    }
    args.push("--cancel-on-stdin".to_owned());
    args.push("--parent-pid".to_owned());
    args
}

/// Launch arguments for a resume, or nothing when this key must not start audio.
#[cfg(test)]
fn resume_args(decision: &Decision, request: &str) -> Option<Vec<String>> {
    match decision {
        Decision::Resume {
            recording,
            seek_us,
            end_us,
        } => Some(file_listen_args(recording, request, *seek_us, *end_us)),
        Decision::Idle
        | Decision::MissingDecoder
        | Decision::OutsideRange
        | Decision::Untracked
        | Decision::Stale
        | Decision::Wait
        | Decision::LiveOccupied
        | Decision::Stop { .. }
        | Decision::Remember { .. } => None,
    }
}

/// Status text for a key that must not stop or start a decoder.
#[must_use]
pub(super) fn refusal(decision: &Decision) -> Option<&'static str> {
    match decision {
        Decision::Idle => Some("No retained listen is active. Capture keeps running."),
        Decision::MissingDecoder => Some(
            "No decoder output time is stored. Pause was not requested. Capture keeps running.",
        ),
        Decision::OutsideRange => Some(
            "That position is outside the admitted range. Pause was not requested. Capture keeps running.",
        ),
        Decision::Untracked => Some(
            "This decoder has no stored admission. Pause was not requested. Capture keeps running.",
        ),
        Decision::Stale => Some(
            "This listen no longer matches the running decoder. Pause was not requested. Capture keeps running.",
        ),
        Decision::Wait => Some(
            "The retained listen is still stopping. A second decoder was not started. Capture keeps running.",
        ),
        Decision::LiveOccupied => {
            Some("A station listen is active. The retained pause stays. Capture keeps running.")
        }
        Decision::Stop { .. } | Decision::Remember { .. } | Decision::Resume { .. } => None,
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

fn position(input: &PauseInput<'_>, stop: bool) -> Decision {
    if input.recording.is_none() || input.request.is_none() {
        return Decision::Untracked;
    }
    let Some(decoder_us) = input.decoder_us else {
        return Decision::MissingDecoder;
    };
    let Some(at_us) = input.origin_us.checked_add(decoder_us) else {
        return Decision::OutsideRange;
    };
    if input.end_us.is_some_and(|end| at_us >= end) {
        return Decision::OutsideRange;
    }
    if stop {
        Decision::Stop { at_us }
    } else {
        Decision::Remember { at_us }
    }
}

fn resume(input: &PauseInput<'_>) -> Decision {
    if input.live_occupied {
        return Decision::LiveOccupied;
    }
    let Some(recording) = input.recording else {
        return Decision::Untracked;
    };
    let Some(at_us) = input.held_us else {
        return Decision::Untracked;
    };
    if input.end_us.is_some_and(|end| at_us >= end) {
        return Decision::OutsideRange;
    }
    Decision::Resume {
        recording: recording.to_owned(),
        seek_us: at_us,
        end_us: input.end_us,
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, PauseInput, Presence, decide, file_listen_args, refusal, resume_args};

    fn sample() -> PauseInput<'static> {
        PauseInput {
            presence: Presence::Playing,
            decoder_us: Some(1_500_000),
            recording: Some("rec-kept"),
            origin_us: 0,
            end_us: None,
            request: Some("reader-kept"),
            visible_request: Some("reader-kept"),
            held_us: None,
            live_occupied: false,
        }
    }

    #[test]
    fn whole_file_pause_remembers_decoder_time_and_resume_seeks_there() {
        let first = decide(&sample());
        assert_eq!(first, Decision::Stop { at_us: 1_500_000 });
        assert!(resume_args(&first, "reader-next").is_none());
        let mut paused = sample();
        paused.presence = Presence::Paused;
        paused.held_us = Some(1_500_000);
        paused.decoder_us = None;
        let second = decide(&paused);
        assert_eq!(
            second,
            Decision::Resume {
                recording: "rec-kept".into(),
                seek_us: 1_500_000,
                end_us: None,
            }
        );
        assert_eq!(
            resume_args(&second, "reader-next"),
            Some(file_listen_args("rec-kept", "reader-next", 1_500_000, None))
        );
        let args = file_listen_args("rec-kept", "reader-next", 1_500_000, None);
        assert!(args.contains(&"--seek-us".to_owned()));
        assert!(args.contains(&"1500000".to_owned()));
        assert!(!args.iter().any(|arg| arg == "--end-us"));
        assert_eq!(
            file_listen_args("rec-kept", "reader-next", 0, None),
            vec![
                "listen",
                "file",
                "rec-kept",
                "--request",
                "reader-next",
                "--destination",
                "system",
                "--cancel-on-stdin",
                "--parent-pid",
            ]
        );
    }

    #[test]
    fn cited_interval_resume_keeps_the_same_end() {
        let mut playing = sample();
        playing.origin_us = 1_000_000;
        playing.end_us = Some(3_000_000);
        playing.decoder_us = Some(500_000);
        assert_eq!(decide(&playing), Decision::Stop { at_us: 1_500_000 });
        let mut paused = playing;
        paused.presence = Presence::Paused;
        paused.held_us = Some(1_500_000);
        let decision = decide(&paused);
        let Some(args) = resume_args(&decision, "reader-next") else {
            panic!("resume builds one listen");
        };
        assert!(args.contains(&"--seek-us".to_owned()));
        assert!(args.contains(&"1500000".to_owned()));
        assert!(args.contains(&"--end-us".to_owned()));
        assert!(args.contains(&"3000000".to_owned()));
    }

    #[test]
    fn missing_sample_past_end_and_overflow_build_no_listen() {
        let mut missing = sample();
        missing.decoder_us = None;
        let missing = decide(&missing);
        assert_eq!(missing, Decision::MissingDecoder);
        assert!(resume_args(&missing, "reader-next").is_none());
        assert!(refusal(&missing).is_some());

        let mut past = sample();
        past.origin_us = 1_000_000;
        past.end_us = Some(2_000_000);
        past.decoder_us = Some(1_000_000);
        let past = decide(&past);
        assert_eq!(past, Decision::OutsideRange);
        assert!(resume_args(&past, "reader-next").is_none());

        let mut overflow = sample();
        overflow.origin_us = u64::MAX - 10;
        overflow.decoder_us = Some(20);
        assert_eq!(decide(&overflow), Decision::OutsideRange);
    }

    #[test]
    fn stopping_waits_and_child_exit_does_not_start_a_decoder() {
        let mut stopping = sample();
        stopping.presence = Presence::Stopping;
        let stopping = decide(&stopping);
        assert_eq!(stopping, Decision::Wait);
        assert!(resume_args(&stopping, "reader-next").is_none());

        let mut exited = sample();
        exited.presence = Presence::Exited;
        let exited = decide(&exited);
        assert_eq!(exited, Decision::Remember { at_us: 1_500_000 });
        assert!(resume_args(&exited, "reader-next").is_none());
        assert!(refusal(&exited).is_none());
    }

    #[test]
    fn stale_live_and_idle_keys_write_nothing() {
        let mut stale = sample();
        stale.visible_request = Some("reader-other");
        assert_eq!(decide(&stale), Decision::Stale);
        assert!(resume_args(&decide(&stale), "reader-next").is_none());

        let mut paused = sample();
        paused.presence = Presence::Paused;
        paused.held_us = Some(1_500_000);
        paused.live_occupied = true;
        let paused = decide(&paused);
        assert_eq!(paused, Decision::LiveOccupied);
        assert!(resume_args(&paused, "reader-next").is_none());

        let mut idle = sample();
        idle.presence = Presence::Absent;
        idle.recording = None;
        idle.request = None;
        idle.visible_request = None;
        idle.decoder_us = None;
        assert_eq!(decide(&idle), Decision::Idle);
        assert!(refusal(&Decision::Idle).is_some_and(|text| text.contains("Capture")));
    }
}
