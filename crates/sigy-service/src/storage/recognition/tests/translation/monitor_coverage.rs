//! Monitor coverage and passage matches over a published transcript and its translations.

use rusqlite::params;

use super::*;
use crate::{
    monitor::{MonitorSpec, MonitorTerm},
    sources::{HttpSource, NetworkScope},
};

fn term(language: &str, text: &str) -> MonitorTerm {
    MonitorTerm {
        language: language.into(),
        text: text.into(),
    }
}

fn spec() -> MonitorSpec {
    MonitorSpec {
        name: "World fair".into(),
        goal: "Follow the world fair.".into(),
        terms: vec![
            term("es", "FERIA"),
            term("en", "fair"),
            term("und", "mundial"),
            term("ar", "معرض"),
        ],
        sources: vec!["radio:v1".into()],
        candidate_sources: vec!["quiet:v1".into()],
        schedules: vec!["morning".into()],
        daily_audio_seconds: 3600,
        total_audio_seconds: 7200,
        recognition_profile: None,
        translation_profile: None,
    }
}

/// A daily rule with one admitted occurrence (the fixture recording) and one missed window.
fn schedule(store: &Store, start_ms: i64) -> Result<()> {
    store.connection.execute(
        "INSERT INTO schedule_rules VALUES ('morning', 'radio:v1', 'UTC', 'daily', NULL, NULL, 8, 0, 0, 60, 600, 0, 1, 1)",
        [],
    )?;
    store.connection.execute(
        "INSERT INTO schedule_occurrences VALUES ('morning-1', 'morning', 0, '2026-09-23', 8, 0, 0, ?1, ?2, 0, NULL, 60, 600, 'missed', 'elapsed', NULL, 1)",
        params![start_ms - 86_400_000 + 1, start_ms - 86_400_000 + 60_001],
    )?;
    store.connection.execute(
        "INSERT INTO schedule_occurrences VALUES ('morning-2', 'morning', 0, '2026-09-24', 8, 0, 0, ?1, ?2, 0, NULL, 60, 600, 'admitted', NULL, 'one', 1)",
        params![start_ms, start_ms + 60_000],
    )?;
    Ok(())
}

/// A monitored library with one recognized recording, a gap, a missed and an admitted
/// schedule occurrence, and a candidate source. Returns the store and a window around it.
fn monitored(path: &Path) -> Result<(Store, i64, i64)> {
    let mut store = transcribed(path)?;
    store.register_source(
        "quiet:v1",
        &HttpSource::new(
            "Quiet",
            "https://example.com/quiet",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    let start_ms: i64 = store.connection.query_row(
        "SELECT starts_ms FROM capture_jobs WHERE id = 'one'",
        [],
        |row| row.get(0),
    )?;
    schedule(&store, start_ms)?;
    store.connection.execute(
        "INSERT INTO recording_gaps VALUES ('one', 0, 'disconnect', 250000, 500000)",
        [],
    )?;
    store.create_monitor("fair", &spec(), 25)?;
    Ok((store, start_ms - 2 * 86_400_000, start_ms + 1))
}

fn translate(store: &mut Store, id: &str, cue: TranslatedCue, now: i64) -> TestResult {
    let (job, work) = store.admit_translation(&request(id)?, now)?;
    let outcome =
        TranslationOutcome::synthetic_fixture(&job, TranslationResult::Succeeded(vec![cue]));
    store.finish_translation(&work.ok_or("work")?, &outcome, now + 1)?;
    Ok(())
}

fn skipped() -> TranslatedCue {
    TranslatedCue {
        ordinal: 0,
        state: "untranslated".into(),
        english: None,
        reason: Some("unsupported-language".into()),
    }
}

#[test]
fn coverage_counts_each_stage_in_a_half_open_window() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, from, to) = monitored(&directory.path().join("catalog"))?;
    let coverage = store.monitor_coverage("fair", from, to)?;
    let radio = &coverage.sources[0];
    assert_eq!(
        (radio.captures, radio.published, radio.recorded_us),
        (1, 1, 1_000_000)
    );
    assert_eq!((radio.gaps, radio.gap_us), (1, 250_000));
    assert_eq!((radio.pinned, radio.transcribed, radio.no_text), (1, 1, 0));
    assert!(radio.transcribed_us > 0);
    assert_eq!(radio.cues_without_translation, 1);
    let schedule = &coverage.schedules[0];
    assert_eq!(
        (schedule.admitted, schedule.missed_elapsed, schedule.waiting),
        (1, 1, 0)
    );
    let later = store.monitor_coverage("fair", to, to + 86_400_000)?;
    assert_eq!(later.sources[0].captures, 0);
    assert_eq!(later.schedules[0].admitted, 0);
    assert!(store.monitor_coverage("fair", to, from).is_err());
    assert!(
        store
            .monitor_coverage("fair", 0, crate::monitor::MAX_WINDOW_MS + 1)
            .is_err()
    );
    assert!(matches!(
        store.monitor_coverage("missing", from, to),
        Err(Error::NotFound)
    ));
    // An untranslated language is counted with its reason, not as translated.
    translate(&mut store, "mt-1", skipped(), 30)?;
    let radio = store.monitor_coverage("fair", from, to)?.sources[0].clone();
    assert_eq!((radio.translated_cues, radio.untranslated_cues), (0, 1));
    assert_eq!(
        radio.untranslated_reasons,
        vec![("unsupported-language".to_owned(), 1)]
    );
    assert_eq!(radio.cues_without_translation, 0);
    // A source added later has its own counts; nothing from it is invented.
    store.propose_monitor_action(
        "fair",
        "add-quiet",
        crate::monitor::ActionOrigin::User,
        &crate::monitor::Proposal::AddSource {
            source: "quiet:v1".into(),
        },
        50,
    )?;
    let coverage = store.monitor_coverage("fair", from, to)?;
    assert_eq!(
        (
            coverage.sources[1].source.as_str(),
            coverage.sources[1].captures
        ),
        ("quiet:v1", 0)
    );
    Ok(())
}

#[test]
fn matches_cite_the_cue_and_use_the_latest_translation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, from, to) = monitored(&directory.path().join("catalog"))?;
    translate(&mut store, "mt-1", skipped(), 30)?;
    let matches = store.monitor_matches("fair", from, to)?;
    let fields: Vec<(&str, &str)> = matches
        .matches
        .iter()
        .map(|found| (found.term.as_str(), found.field.as_str()))
        .collect();
    assert_eq!(fields, vec![("FERIA", "original"), ("mundial", "original")]);
    assert_eq!(matches.matches[0].translation_revision, Some(1));
    assert_eq!(matches.matches[0].english, None);

    translate(&mut store, "mt-2", translated(0, "A world fair"), 40)?;
    let radio = store.monitor_coverage("fair", from, to)?.sources[0].clone();
    assert_eq!((radio.translated_cues, radio.untranslated_cues), (1, 0));
    let matches = store.monitor_matches("fair", from, to)?;
    assert_eq!((matches.transcripts_scanned, matches.more), (1, false));
    let english = matches
        .matches
        .iter()
        .find(|found| found.term == "fair")
        .ok_or("english match")?;
    assert_eq!(english.field, "english");
    assert_eq!(english.english.as_deref(), Some("A world fair"));
    assert_eq!(english.translation_revision, Some(2));
    assert_eq!(english.original, "Una feria mundial");
    assert_eq!(
        (
            english.recording_id.as_str(),
            english.transcript_id.as_str(),
            english.transcript_revision,
            english.cue_ordinal
        ),
        ("one", "pin", 1, 0)
    );
    assert!(english.end_us > english.start_us);
    // A term in another script does not match.
    assert!(matches.matches.iter().all(|found| found.term != "معرض"));
    assert_eq!(matches.matches.len(), 3);
    Ok(())
}

#[test]
fn reading_coverage_and_matches_writes_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (store, from, to) = monitored(&directory.path().join("catalog"))?;
    let count = |store: &Store| -> Result<u32> {
        Ok(store.connection.query_row(
            "SELECT (SELECT count(*) FROM monitor_actions) + (SELECT count(*) FROM monitor_versions) + (SELECT count(*) FROM translations) + (SELECT count(*) FROM analysis_jobs)",
            [],
            |row| row.get(0),
        )?)
    };
    let before = count(&store)?;
    store.monitor_coverage("fair", from, to)?;
    store.monitor_matches("fair", from, to)?;
    assert_eq!(count(&store)?, before);
    Ok(())
}
