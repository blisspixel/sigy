use super::*;

fn capture(ordinal: u32, state: &str, decoded_us: Option<u64>) -> CaptureFacts {
    CaptureFacts {
        ordinal,
        recording_id: format!("recording-{ordinal}"),
        capture_state: state.into(),
        retained: true,
        decoded_us,
        recognition: None,
        translation_recorded: false,
    }
}

fn facts(captures: Vec<CaptureFacts>) -> ProcessingFacts {
    ProcessingFacts {
        task_id: "task".into(),
        hold: None,
        remaining_audio_us: 60_000_000,
        recognition_profile: "asr".into(),
        translation_profile: Some("mt".into()),
        captures,
    }
}

fn recognized(state: &str, transcript: Option<(i64, &str)>) -> RecognitionFacts {
    RecognitionFacts {
        queued: true,
        analysis_id: Some("pin".into()),
        job_state: Some(state.into()),
        transcript: transcript.map(|(revision, outcome)| (revision, outcome.to_owned())),
    }
}

#[test]
fn spec_is_finite_and_refuses_embedded_authority() -> Result<()> {
    let spec = TaskProcessingSpec {
        recognition_profile: "asr".into(),
        translation_profile: Some("mt".into()),
        maximum_audio_seconds: MAX_PROCESSING_AUDIO_SECONDS,
    };
    spec.validate()?;
    assert_eq!(spec.maximum_audio_us(), 1_800_000_000);
    for invalid in [
        TaskProcessingSpec {
            maximum_audio_seconds: 0,
            ..spec.clone()
        },
        TaskProcessingSpec {
            maximum_audio_seconds: MAX_PROCESSING_AUDIO_SECONDS + 1,
            ..spec.clone()
        },
        TaskProcessingSpec {
            recognition_profile: "../asr".into(),
            ..spec.clone()
        },
        TaskProcessingSpec {
            translation_profile: Some(String::new()),
            ..spec.clone()
        },
    ] {
        assert!(invalid.validate().is_err());
    }
    let mut value = serde_json::to_value(&spec)?;
    value["paid_allowance"] = serde_json::json!("20.000000");
    assert!(serde_json::from_value::<TaskProcessingSpec>(value).is_err());
    Ok(())
}

#[test]
fn completed_recordings_are_recognized_in_collection_order_within_the_allowance() {
    let mut plan_facts = facts(vec![
        capture(0, "completed", Some(40_000_000)),
        capture(1, "completed", Some(30_000_000)),
    ]);
    assert_eq!(
        plan(&plan_facts),
        vec![
            ProcessingStep::Recognize {
                ordinal: 0,
                recording_id: "recording-0".into(),
                audio_us: 40_000_000,
            },
            ProcessingStep::SkipRecognition {
                ordinal: 1,
                recording_id: "recording-1".into(),
                reason: "task-audio-allowance".into(),
            },
        ]
    );
    plan_facts.remaining_audio_us = 70_000_000;
    assert_eq!(plan(&plan_facts).len(), 2);
    assert!(matches!(
        plan(&plan_facts)[1],
        ProcessingStep::Recognize { ordinal: 1, .. }
    ));
}

#[test]
fn running_captures_wait_and_unusable_recordings_are_refused_once() {
    let mut expired = capture(1, "completed", Some(1));
    expired.retained = false;
    let mut cases = vec![
        capture(0, "running", None),
        expired,
        capture(1, "completed", None),
        capture(1, "completed", Some(0)),
        capture(1, "failed", None),
        capture(1, "interrupted", Some(5)),
        capture(1, "cancelled", None),
    ];
    let expected = [
        None,
        Some("recording-not-retained"),
        Some("no-decoded-audio"),
        Some("no-decoded-audio"),
        Some("recording-failed"),
        Some("recording-interrupted"),
        Some("recording-cancelled"),
    ];
    for (case, reason) in cases.drain(..).zip(expected) {
        let steps = plan(&facts(vec![case]));
        match reason {
            None => assert!(steps.is_empty()),
            Some(reason) => assert!(
                matches!(&steps[..], [ProcessingStep::SkipRecognition { reason: found, .. }] if found == reason),
                "{reason}"
            ),
        }
    }
}

#[test]
fn translation_follows_only_the_task_recognition_result() {
    let mut done = capture(0, "completed", Some(1_000_000));
    let cases = [
        (recognized("running", None), None),
        (recognized("queued", None), None),
        (recognized("failed", None), Some("recognition-failed")),
        (
            recognized("interrupted", None),
            Some("recognition-interrupted"),
        ),
        (recognized("succeeded", None), Some("no-transcript")),
        (
            recognized("succeeded", Some((1, "no_text"))),
            Some("no-text"),
        ),
    ];
    for (recognition, reason) in cases {
        done.recognition = Some(recognition);
        let steps = plan(&facts(vec![done.clone()]));
        match reason {
            None => assert!(steps.is_empty()),
            Some(reason) => assert!(
                matches!(&steps[..], [ProcessingStep::SkipTranslation { reason: found, .. }] if found == reason),
                "{reason}"
            ),
        }
    }
    done.recognition = Some(recognized("succeeded", Some((2, "text"))));
    assert_eq!(
        plan(&facts(vec![done.clone()])),
        vec![ProcessingStep::Translate {
            ordinal: 0,
            recording_id: "recording-0".into(),
            analysis_id: "pin".into(),
            transcript_revision: 2,
        }]
    );
    let mut untranslated = facts(vec![done.clone()]);
    untranslated.translation_profile = None;
    assert!(matches!(
        &plan(&untranslated)[..],
        [ProcessingStep::SkipTranslation { reason, .. }] if reason == "no-translation-profile"
    ));
    done.translation_recorded = true;
    assert!(plan(&facts(vec![done.clone()])).is_empty());
    done.translation_recorded = false;
    done.recognition = Some(RecognitionFacts {
        queued: false,
        analysis_id: None,
        job_state: None,
        transcript: None,
    });
    assert!(plan(&facts(vec![done])).is_empty());
}

#[test]
fn every_hold_admits_nothing() {
    for hold in ["cancelled", "scope-changed", "clock"] {
        let mut held = facts(vec![capture(0, "completed", Some(1_000_000))]);
        held.hold = Some(hold);
        assert!(plan(&held).is_empty());
    }
}
