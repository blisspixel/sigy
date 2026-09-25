use super::*;

const MINUTE: u64 = 60_000_000;

fn candidate(id: &str, minutes: u64) -> Candidate {
    Candidate {
        recording_id: id.into(),
        audio_us: minutes * MINUTE,
    }
}

fn facts() -> Facts {
    Facts {
        monitor_id: "dam".into(),
        version: 1,
        paused: false,
        daily_cap_us: 30 * MINUTE,
        total_cap_us: 100 * MINUTE,
        used_today_us: 0,
        used_total_us: 0,
        recognition_profile: Some("asr".into()),
        translation_profile: Some("mt".into()),
        candidates: Vec::new(),
        recognized: Vec::new(),
    }
}

fn recognized(id: &str, state: &str, transcript: Option<(i64, &str)>) -> Recognized {
    Recognized {
        recording_id: id.into(),
        analysis_id: pin_id(id),
        job_state: state.into(),
        transcript: transcript.map(|(revision, outcome)| (revision, outcome.to_owned())),
    }
}

fn recognized_ids(steps: &[Step]) -> Vec<&str> {
    steps
        .iter()
        .filter_map(|step| match step {
            Step::Recognize { recording_id, .. } => Some(recording_id.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn recognition_is_oldest_first_and_stops_at_the_daily_cap() {
    let mut facts = facts();
    facts.used_today_us = 5 * MINUTE;
    facts.candidates = vec![
        candidate("a", 10),
        candidate("b", 10),
        candidate("c", 10),
        candidate("d", 1),
    ];
    let (steps, hold) = plan(&facts);
    // 5 + 10 + 10 = 25; c would make 35. The one-minute d does not jump ahead of c.
    assert_eq!(recognized_ids(&steps), vec!["a", "b"]);
    assert_eq!(hold, Hold::DailyCap);
}

#[test]
fn the_total_cap_holds_across_days() {
    let mut facts = facts();
    facts.used_total_us = 95 * MINUTE;
    facts.candidates = vec![candidate("a", 4), candidate("b", 4)];
    let (steps, hold) = plan(&facts);
    assert_eq!(recognized_ids(&steps), vec!["a"]);
    assert_eq!(hold, Hold::TotalCap);
}

#[test]
fn a_recording_longer_than_the_daily_cap_is_skipped_once_instead_of_blocking() {
    let mut facts = facts();
    facts.candidates = vec![candidate("huge", 31), candidate("a", 5)];
    let (steps, hold) = plan(&facts);
    assert_eq!(
        steps[0],
        Step::SkipRecognition {
            recording_id: "huge".into(),
            reason: "longer-than-daily-cap",
        }
    );
    assert_eq!(recognized_ids(&steps), vec!["a"]);
    assert_eq!(hold, Hold::None);
}

#[test]
fn a_pass_is_bounded_and_paused_monitors_take_no_steps() {
    let mut facts = facts();
    facts.daily_cap_us = 1000 * MINUTE;
    facts.total_cap_us = 1000 * MINUTE;
    facts.candidates = (0..10).map(|n| candidate(&format!("r{n}"), 1)).collect();
    let (steps, hold) = plan(&facts);
    assert_eq!(steps.len(), STEPS_PER_PASS);
    assert_eq!(hold, Hold::None);
    facts.paused = true;
    assert_eq!(plan(&facts), (Vec::new(), Hold::Paused));
}

#[test]
fn translation_follows_only_recognized_text() {
    let mut facts = facts();
    facts.recognized = vec![
        recognized("text", "succeeded", Some((2, "text"))),
        recognized("quiet", "succeeded", Some((1, "no_text"))),
        recognized("broken", "failed", None),
    ];
    let (steps, _) = plan(&facts);
    assert_eq!(
        steps[0],
        Step::Translate {
            recording_id: "text".into(),
            analysis_id: pin_id("text"),
            transcript_revision: 2,
            profile: "mt".into(),
        }
    );
    assert_eq!(
        steps[1],
        Step::SkipTranslation {
            recording_id: "quiet".into(),
            reason: "no-text".into(),
        }
    );
    assert_eq!(
        steps[2],
        Step::SkipTranslation {
            recording_id: "broken".into(),
            reason: "recognition-failed".into(),
        }
    );
    facts.translation_profile = None;
    facts.recognition_profile = None;
    let (steps, hold) = plan(&facts);
    assert_eq!(
        steps[0],
        Step::SkipTranslation {
            recording_id: "text".into(),
            reason: "no-translation-profile".into(),
        }
    );
    assert_eq!(hold, Hold::NoRecognitionProfile);
}

#[test]
fn derived_ids_are_stable_bounded_and_shared_across_monitors() {
    let pin = pin_id("recording-1");
    assert_eq!(pin, pin_id("recording-1"));
    assert_ne!(pin, pin_id("recording-2"));
    assert!(pin.len() <= 40 && pin.starts_with("mon-pin-"));
    let long = "r".repeat(128);
    assert!(recognition_job_id(&long, &"a".repeat(64)).len() <= 40);
    assert_ne!(
        recognition_job_id("recording-1", &"a".repeat(64)),
        recognition_job_id("recording-1", &"b".repeat(64))
    );
    assert_ne!(
        translation_job_id(&pin, 1, &"a".repeat(64)),
        translation_job_id(&pin, 2, &"a".repeat(64))
    );
}
