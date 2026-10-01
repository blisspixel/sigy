use super::*;
use sigy_service::{
    recognition::LocalAsrRequest,
    translation::{TranslationPairView, TranslationRequest},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn translation() -> TranslationPage {
    TranslationPage {
        transcript_id: "pin".into(),
        transcript_revision: 2,
        revision: 3,
        job_id: "translation".into(),
        profile: "cpu".into(),
        profile_sha256: "ab".repeat(32),
        cue_count: 3,
        translated_count: 1,
        amount_usd: "0.000000".into(),
        created_ms: 0,
        pairs: vec![
            TranslationPairView {
                ordinal: 0,
                start_us: 60_123_999,
                end_us: 62_456_999,
                original: "تقارير عن سد\u{1b}]52;c;Zm9v\u{7}".into(),
                state: "translated".into(),
                english: Some("Reports about a dam\u{1b}[31m".into()),
                reason: None,
            },
            TranslationPairView {
                ordinal: 1,
                start_us: 62_457_000,
                end_us: 63_000_000,
                original: "Diné bizaad".into(),
                state: "untranslated".into(),
                english: None,
                reason: Some("unsupported-language\u{1b}[2J\nfalse success".into()),
            },
            TranslationPairView {
                ordinal: 2,
                start_us: 63_000_000,
                end_us: 64_000_000,
                original: "tlhIngan Hol".into(),
                state: "untranslated".into(),
                english: None,
                reason: None,
            },
        ],
        next_after_ordinal: Some(2),
        stale: None,
    }
}

#[test]
fn translation_output_keeps_scripts_times_and_untranslated_reasons_without_controls() -> TestResult
{
    let page = translation();
    let original = page.clone();
    let mut output = Vec::new();
    render_recognition(
        &mut output,
        &RecognitionView::Translation { page: page.clone() },
    )?;
    let text = String::from_utf8(output)?;
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\u{7}'));
    assert!(text.contains("تقارير عن سد"));
    assert!(text.contains("Diné bizaad"));
    assert!(text.contains("tlhIngan Hol"));
    assert!(text.contains("[01:00.123 - 01:02.456]"));
    assert!(text.contains("1 of 3 cues translated"));
    assert!(text.contains("untranslated: unsupported-language"));
    assert!(text.contains("en: (untranslated)"));
    assert!(text.contains("--transcript-revision 2 --revision 3 --after 2"));
    assert!(text.contains("unreviewed"));
    assert_eq!(page, original, "presentation changed stored evidence");
    Ok(())
}

#[test]
fn a_queued_translation_is_not_reported_as_a_completed_transcript() -> TestResult {
    let mut job = TranslationJob {
        request: TranslationRequest {
            id: "mt".into(),
            transcript_id: "pin".into(),
            transcript_revision: 2,
            profile: "cpu".into(),
            profile_sha256: "ab".repeat(32),
        },
        generation: 4,
        state: "queued".into(),
        reason: None,
        amount_usd: "0.000000".into(),
        created_ms: 0,
        finished_ms: None,
        attempt: 0,
        started_ms: None,
    };
    let mut output = Vec::new();
    render_translation_job(&mut output, &job)?;
    let text = String::from_utf8(output)?;
    assert!(text.contains("Queued. It starts when a translation slot is free"));
    assert!(!text.contains("Read it with:"));
    job.state = "failed".into();
    job.reason = Some("deadline\u{1b}[2J".into());
    let mut output = Vec::new();
    render_translation_job(&mut output, &job)?;
    let text = String::from_utf8(output)?;
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains("Queued."));
    assert!(text.contains("Reason deadline"));
    job.state = "succeeded".into();
    job.reason = None;
    let mut output = Vec::new();
    render_translation_job(&mut output, &job)?;
    assert!(String::from_utf8(output)?.contains("Read it with: sigy analysis translation pin"));
    Ok(())
}

#[test]
fn recognition_output_keeps_the_exact_parent_and_worker_generation() -> TestResult {
    let job = LocalAsrJob {
        request: LocalAsrRequest {
            id: "asr".into(),
            analysis_id: "pin".into(),
            analysis_revision: 3,
            profile: "cpu".into(),
            profile_sha256: "ab".repeat(32),
            parent_revision: 2,
        },
        generation: 7,
        recording_id: "recording".into(),
        state: "queued".into(),
        expected_bytes: 64,
        manifest_sha256: "cd".repeat(32),
        reason: Some("queued\u{1b}[31m".into()),
        amount_usd: "0.000000".into(),
        created_ms: 0,
        finished_ms: None,
        attempt: 0,
        started_ms: None,
    };
    let mut output = Vec::new();
    render_recognition_job(&mut output, &job)?;
    let text = String::from_utf8(output)?;
    assert!(text.contains("generation 7 attempt 0: queued"));
    assert!(text.contains("Input pin revision 3"));
    assert!(text.contains("follows transcript revision 2"));
    assert!(text.contains("a recognition slot is free"));
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains("Read the text with:"));
    Ok(())
}

#[test]
fn missing_results_and_empty_profiles_stay_distinct_and_writer_failure_propagates() -> TestResult {
    for view in [
        RecognitionView::Empty { id: "pin".into() },
        RecognitionView::Profiles { profiles: vec![] },
        RecognitionView::TranslationProfiles { profiles: vec![] },
    ] {
        let mut output = Vec::new();
        render_recognition(&mut output, &view)?;
        let text = String::from_utf8(output)?;
        assert!(!text.contains("succeeded"));
        assert!(!text.contains("0 of 0 cues translated"));
        assert!(render_recognition(&mut std::io::Cursor::new(&mut [0_u8; 1][..]), &view).is_err());
    }
    Ok(())
}
