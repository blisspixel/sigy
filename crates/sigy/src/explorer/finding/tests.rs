use super::{Browser, Effect, FindingPage, Key, Recording};
use sigy_service::monitor::FindingOriginal;

#[test]
fn wrapped_script_is_scrollable_without_splitting_graphemes() {
    let mut browser = Browser::default();
    let mut page = fixture();
    page.original_script = "明日e\u{301}の会議".repeat(20);
    assert!(browser.load("world-news", "one", &page));
    browser.lines(4, 20, 0);
    let rows = browser.wrapped_detail();
    assert!(
        rows.iter()
            .all(|row| ratatui::text::Line::raw(row.as_str()).width() <= 20)
    );
    assert!(!rows.iter().any(|row| row.starts_with('\u{301}')));
    let mut visited = String::new();
    for _ in 0..rows.len() {
        visited.push_str(&browser.lines(3, 20, 0).join("\n"));
        browser.handle(&Key::Down);
    }
    assert!(visited.contains("明日e\u{301}の会議"));
    assert!(visited.contains("work."));
    let wider = browser.lines(3, 80, 0);
    browser.handle(&Key::Up);
    assert_ne!(wider, browser.lines(3, 80, 0));
}

pub(crate) fn fixture() -> FindingPage {
    FindingPage {
        monitor_id: "world-news".into(),
        id: "one".into(),
        transcript_id: "original".into(),
        transcript_revision: 2,
        translation_revision: 3,
        cue_ordinal: 0,
        original: FindingOriginal::Retained,
        recording_id: "recording-one".into(),
        start_us: Some(100),
        end_us: Some(900),
        original_script: "明日の会議です。\u{1b}[2J".into(),
        english: Some("The meeting is tomorrow.".into()),
        untranslated_reason: None,
        stale_transcript: Some(true),
        stale_translation: None,
    }
}

fn recording() -> Result<Recording, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "id": "recording-one", "source_revision": "radio:v1", "state": "completed", "object_key": "fixture", "duration_seconds": 5, "maximum_bytes": 100,
        "retention": "temporary", "storage_state": "retained", "charged_bytes": 100,
        "media_bytes": 100, "sha256": "fixture", "format": "wav", "decoded_microseconds": 1000,
        "end_reason": "ended", "processing_receipt": null, "failure_detail": null, "profile": "radio",
        "escrow_bytes": 0, "open_ceiling": 0, "open_object_key": null, "lease_renewals": 0,
        "intervals": [{"ordinal": 0, "decoded_start_us": 0, "decoded_end_us": 1000, "byte_start": 0, "byte_end": 100, "object_key": "sealed", "sha256": "fixture", "format": "wav", "ceiling_bytes": 100, "released": false}],
        "gaps": []
    }))
}

#[test]
fn long_lookup_keeps_the_edited_tail_visible_without_shortening_ids() {
    let mut browser = Browser::default();
    let monitor = "m".repeat(128);
    let finding = format!("{}tail", "f".repeat(124));
    let query = format!("{monitor} {finding}");
    browser.handle(&Key::Char('/'));
    browser.handle(&Key::Paste(query.clone()));
    for width in [20, 40, 80] {
        let lines = browser.lines(2, width, 0);
        assert!(lines[0].len() <= width);
        assert!(lines[0].ends_with("l]"));
        if width >= 40 {
            assert!(lines[0].contains("..."));
            assert!(lines[0].ends_with("tail]"));
        }
        assert_eq!(browser.query(), query);
    }
    assert_eq!(
        browser.handle(&Key::Enter),
        Some(Effect::Finding { monitor, finding })
    );
    assert!(!browser.editing());
    assert!(browser.lines(2, 40, 0)[0].ends_with("tail]"));
}

#[test]
fn lookup_editing_is_bounded_and_named_reads_are_explicit() {
    let mut browser = Browser::default();
    assert_eq!(browser.handle(&Key::Char('o')), Some(Effect::None));
    browser.handle(&Key::Char('/'));
    browser.handle(&Key::Paste("world-news one".into()));
    browser.handle(&Key::Char('q'));
    assert!(browser.query().ends_with('q'));
    browser.handle(&Key::Backspace);
    assert_eq!(
        browser.handle(&Key::Enter),
        Some(Effect::Finding {
            monitor: "world-news".into(),
            finding: "one".into()
        })
    );
    browser.handle(&Key::Char('/'));
    browser.handle(&Key::Paste("x".repeat(300)));
    assert_eq!(browser.query().len(), 257);
    assert_eq!(browser.handle(&Key::Enter), Some(Effect::None));
    assert!(browser.lines(4, 80, 0).join("\n").contains("at most 128"));
    assert_eq!(browser.handle(&Key::Quit), Some(Effect::Detach));
    browser.handle(&Key::Escape);
    assert_eq!(browser.handle(&Key::Char('q')), None);
    assert!(browser.lines(1, 80, 0).len() <= 1);
}

#[test]
fn mismatched_reads_preserve_citation_and_historical_original_statement()
-> Result<(), serde_json::Error> {
    let mut browser = Browser::default();
    let page = fixture();
    assert!(browser.load("world-news", "one", &page));
    assert!(!browser.load("another", "one", &page));
    assert!(!browser.load("world-news", "two", &page));
    assert_eq!(
        browser.handle(&Key::Char('o')),
        Some(Effect::FindingOriginal {
            recording: "recording-one".into()
        })
    );
    let mut media = recording()?;
    assert!(browser.load_original(&media));
    assert!(browser.detail().join("\n").contains("available in catalog"));
    media.intervals[0].released = true;
    assert!(browser.load_original(&media));
    assert!(
        browser
            .detail()
            .join("\n")
            .contains("unavailable in catalog")
    );
    assert_eq!(
        browser.page.as_ref().map(|page| page.original),
        Some(FindingOriginal::Retained)
    );
    media.id = "another".into();
    assert!(!browser.load_original(&media));
    assert!(!browser.detail().join("\n").contains('\u{1b}'));
    assert!(browser.detail().join("\n").contains("明日の会議です。"));
    Ok(())
}

#[test]
fn current_interval_requires_one_available_segment_and_no_gap() -> Result<(), serde_json::Error> {
    let mut browser = Browser::default();
    assert!(!browser.load_original(&recording()?));
    let mut page = fixture();
    assert!(browser.load("world-news", "one", &page));
    let mut media = recording()?;
    media.storage_state = "deleted".into();
    assert!(browser.load_original(&media));
    assert!(browser.detail().join("\n").contains("unavailable"));
    media.storage_state = "retained".into();
    media.intervals[0].decoded_end_us = 500;
    let mut second = media.intervals[0].clone();
    second.ordinal = 1;
    second.decoded_start_us = 500;
    second.decoded_end_us = 1000;
    media.intervals.push(second);
    assert!(browser.load_original(&media));
    assert!(browser.detail().join("\n").contains("unavailable"));
    media.intervals[0].decoded_end_us = 1000;
    media.gaps = serde_json::from_value(
        serde_json::json!([{"ordinal":0, "start_us": 200, "end_us": 300, "cause":"late_start"}]),
    )?;
    assert!(browser.load_original(&media));
    assert!(browser.detail().join("\n").contains("unavailable"));
    page.original = FindingOriginal::Expired;
    page.start_us = None;
    page.end_us = None;
    page.english = None;
    page.untranslated_reason = Some("uncertain".into());
    assert!(browser.load("world-news", "one", &page));
    assert!(browser.load_original(&media));
    assert!(browser.detail().join("\n").contains("Cited interval: none"));
    assert!(
        browser
            .detail()
            .join("\n")
            .contains("untranslated: uncertain")
    );
    for _ in 0..50 {
        browser.handle(&Key::Down);
    }
    assert!(browser.offset < browser.wrapped_detail().len());
    for _ in 0..50 {
        browser.handle(&Key::Up);
    }
    assert_eq!(browser.offset, 0);
    Ok(())
}
