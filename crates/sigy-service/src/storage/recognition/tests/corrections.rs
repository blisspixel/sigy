//! A correction appends one revision. The previous revision stays readable.

use super::*;
use crate::languages::{
    LanguageEvidence, LanguageLabel, LanguageMethod, LanguageRoute, LanguageSpan,
    TranscriptReference,
};
use crate::recognition::RecognitionCue;

struct Heard {
    start: u64,
    mid: u64,
    end: u64,
}

fn heard(path: &Path) -> Result<(Store, Heard)> {
    let mut store = setup(path, false)?;
    let work = admit(&mut store, "asr", 0)?;
    let chunk = work.input.chunks.first().ok_or(Error::StorageIntegrity)?;
    let start = chunk.start_us;
    let end = chunk.end_us;
    let mid = start + (end - start) / 2;
    if mid <= start || mid >= end {
        return Err(Error::StorageIntegrity);
    }
    let coverages = output(&work, "").coverages;
    finish_custom(
        &mut store,
        &work,
        coverages,
        vec![
            RecognitionCue {
                ordinal: 0,
                start_us: start,
                end_us: mid,
                script: "alpha beta".into(),
            },
            RecognitionCue {
                ordinal: 1,
                start_us: mid,
                end_us: end,
                script: "gamma delta".into(),
            },
        ],
    )?;
    Ok((store, Heard { start, mid, end }))
}

fn activity(store: &Store) -> Result<(i64, i64, i64, i64, i64)> {
    Ok(store.connection.query_row(
        "SELECT (SELECT count(*) FROM analysis_jobs), (SELECT count(*) FROM translation_jobs), (SELECT count(*) FROM provider_attempts), (SELECT count(*) FROM requests), (SELECT count(*) FROM ledger_events)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?)
}

fn evidence(id: &str, transcript: Option<i64>, heard: &Heard) -> LanguageEvidence {
    LanguageEvidence {
        id: id.into(),
        revision: 1,
        analysis_id: "pin".into(),
        analysis_revision: 1,
        transcript: transcript.map(|revision| TranscriptReference {
            id: "pin".into(),
            revision,
        }),
        method: LanguageMethod {
            origin: "acoustic".into(),
            profile: "fixture-detector".into(),
            profile_sha256: "b".repeat(64),
            resolution: "block".into(),
            alias_map: "fixture-v1".into(),
        },
        outcome: "succeeded".into(),
        reason: None,
        spans: vec![LanguageSpan {
            ordinal: 0,
            interval_ordinal: 0,
            start_us: heard.start,
            end_us: heard.end,
            cue_ordinal: None,
            observation: "identified".into(),
            languages: vec![LanguageLabel {
                tag: "es".into(),
                provider_label: "es".into(),
            }],
            route: LanguageRoute {
                task: "translation_en".into(),
                capability: "unsupported".into(),
                profile: "fixture-translator".into(),
                profile_sha256: "c".repeat(64),
                basis: "declared".into(),
                basis_sha256: "d".repeat(64),
            },
        }],
    }
}

#[test]
fn a_correction_keeps_the_previous_revision_and_does_not_dispatch() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let (mut store, heard) = heard(&path)?;
    let show = admit_show(&mut store, 1, 1_000_000)?;
    store.finish_local_asr(&show, &proof(&show, "untouched"), 23)?;
    let show_page = store.transcript_cues_page("show", 1, None)?;
    store.set_budget_limit("global", "0".parse()?)?;
    store.set_budget_limit("paid", "0".parse()?)?;
    let before = activity(&store)?;
    let revision = store.correct_transcript("pin", 1, 0, "alpha corrected", 40)?;
    assert_eq!(revision, 2);
    assert_eq!(activity(&store)?, before);
    let old = store.transcript_cues_page("pin", 1, None)?;
    let new = store.transcript_cues_page("pin", 2, None)?;
    assert_eq!(old.cues[0].script, "alpha beta");
    assert_eq!(old.cues[1].script, "gamma delta");
    assert_eq!(
        (old.cues[1].start_us, old.cues[1].end_us),
        (heard.mid, heard.end)
    );
    assert!(!old.coverages.is_empty());
    assert_eq!(old.transcript.kind, "recognition");
    assert_eq!(new.transcript.kind, "correction");
    assert_eq!(new.transcript.profile, "user-correction-v1");
    assert_eq!(new.transcript.parent_revision, Some(1));
    assert_eq!(new.transcript.wording, "uncertain");
    assert_eq!(new.transcript.amount_usd, "0.000000");
    assert!(new.transcript.job_id.is_none());
    assert!(new.transcript.profile_sha256.is_none());
    assert!(new.coverages.is_empty());
    assert_eq!(new.cues[0].script, "alpha corrected");
    assert_eq!(new.cues[1].script, "gamma delta");
    assert_eq!(
        (new.cues[0].start_us, new.cues[0].end_us),
        (heard.start, heard.mid)
    );
    assert_eq!(store.transcript_cues_page("show", 1, None)?, show_page);
    let next = store.correct_transcript("pin", 2, 1, "gamma corrected", 50)?;
    assert_eq!(next, 3);
    let third = store.transcript_cues_page("pin", 3, None)?;
    assert_eq!(third.transcript.parent_revision, Some(2));
    assert_eq!(third.cues[0].script, "alpha corrected");
    assert_eq!(
        (third.cues[1].start_us, third.cues[1].end_us),
        (heard.mid, heard.end)
    );
    drop(store);
    let opened = Store::open(&path)?;
    let kept = opened.transcript_cues_page("pin", 1, None)?;
    assert_eq!(kept.cues[0].script, "alpha beta");
    assert_eq!(kept.cues[1].script, "gamma delta");
    assert_eq!(
        opened.transcript_cues_page("pin", 3, None)?.cues[0].script,
        "alpha corrected"
    );
    Ok(())
}

#[test]
fn the_same_revision_conflicts_and_a_bad_script_writes_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, _) = heard(&directory.path().join("catalog"))?;
    let rows = counts(&store)?;
    let unchanged = store.correct_transcript("pin", 1, 0, "alpha beta", 40);
    assert!(matches!(
        unchanged,
        Err(Error::Analysis("correction-unchanged"))
    ));
    for script in ["", "line\nbreak", "nul\0byte", "   \u{0001}"] {
        assert!(matches!(
            store.correct_transcript("pin", 1, 0, script, 40),
            Err(Error::InvalidInput("correction text"))
        ));
    }
    assert!(matches!(
        store.correct_transcript("pin", 1, 9, "other", 40),
        Err(Error::InvalidInput("transcript cue"))
    ));
    assert!(matches!(
        store.correct_transcript("missing", 1, 0, "other", 40),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.correct_transcript("pin", 2, 0, "other", 40),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.correct_transcript("pin", 0, 0, "other", 40),
        Err(Error::InvalidInput("transcript revision"))
    ));
    assert!(matches!(
        store.correct_transcript("pin", 1, 0, "other", 0),
        Err(Error::InvalidInput("correction clock"))
    ));
    assert_eq!(counts(&store)?, rows);
    store.correct_transcript("pin", 1, 0, "alpha corrected", 40)?;
    assert!(matches!(
        store.correct_transcript("pin", 1, 1, "again", 41),
        Err(Error::Analysis("revision-conflict"))
    ));
    let duplicate = store.connection.execute(
        "INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, kind, outcome, parent_revision, job_id, job_generation, profile_sha256, cue_count, text_bytes, state, created_ms) SELECT id, 2, analysis_id, analysis_revision, recording_id, media_sha256, role, 'user-correction-v1', 'correction', 'text', 1, NULL, NULL, NULL, cue_count, text_bytes, state, created_ms + 1 FROM transcripts WHERE id = 'pin' AND revision = 1",
        [],
    );
    let Err(error) = duplicate else {
        return Err("duplicate correction was stored".into());
    };
    let message = error.to_string();
    assert!(
        message.contains("transcript revision conflicts"),
        "{message}"
    );
    assert_eq!(
        counts(&store)?,
        (rows.0 + 1, rows.1 + 2, rows.2, rows.3 + 1)
    );
    Ok(())
}

#[test]
fn legacy_silence_and_expired_input_stay_on_the_previous_revision() -> TestResult {
    let legacy_dir = tempfile::tempdir()?;
    let mut legacy = setup(&legacy_dir.path().join("catalog"), true)?;
    assert!(matches!(
        legacy.correct_transcript("pin", 1, 0, "speech", 40),
        Err(Error::Analysis("correction-unavailable"))
    ));
    assert_eq!(
        legacy.transcript_cues_page("pin", 1, None)?.transcript.kind,
        "legacy_placeholder"
    );

    let silent_dir = tempfile::tempdir()?;
    let mut silent = setup(&silent_dir.path().join("catalog"), true)?;
    let work = admit(&mut silent, "no-text", 1)?;
    silent.finish_local_asr(&work, &proof(&work, ""), 21)?;
    assert!(matches!(
        silent.correct_transcript("pin", 2, 0, "speech", 40),
        Err(Error::Analysis("correction-unavailable"))
    ));
    assert_eq!(silent.transcript_revisions("pin", 0)?.revisions.len(), 2);

    let expired_dir = tempfile::tempdir()?;
    let (mut expired, _) = heard(&expired_dir.path().join("catalog"))?;
    expired.begin_delete("one", false)?;
    assert!(matches!(
        expired.correct_transcript("pin", 1, 0, "alpha corrected", 40),
        Err(Error::Analysis("input-expired"))
    ));
    let page = expired.transcript_cues_page("pin", 1, None)?;
    assert_eq!(page.cues[0].script, "alpha beta");
    assert_eq!(page.transcript.kind, "recognition");
    let latest: i64 = expired.connection.query_row(
        "SELECT max(revision) FROM transcripts WHERE id = 'pin'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(latest, 1);
    Ok(())
}

#[test]
fn bound_language_evidence_goes_stale_and_unbound_evidence_does_not() -> TestResult {
    let loose_dir = tempfile::tempdir()?;
    let (mut loose, loose_heard) = heard(&loose_dir.path().join("catalog"))?;
    loose.publish_language_evidence(evidence("loose", None, &loose_heard), 30)?;
    loose.correct_transcript("pin", 1, 0, "alpha corrected", 40)?;
    let loose_page = loose.transcript_cues_page("pin", 2, None)?;
    assert_eq!(loose_page.stale_language_of, None);
    assert_eq!(loose_page.stale_translation_of, None);

    let bound_dir = tempfile::tempdir()?;
    let (mut bound, bound_heard) = heard(&bound_dir.path().join("catalog"))?;
    bound.publish_language_evidence(evidence("lid", Some(1), &bound_heard), 30)?;
    bound.correct_transcript("pin", 1, 0, "alpha corrected", 40)?;
    let current = bound.transcript_cues_page("pin", 2, None)?;
    assert_eq!(current.stale_language_of, Some(1));
    assert_eq!(current.cues[0].script, "alpha corrected");
    let previous = bound.transcript_cues_page("pin", 1, None)?;
    assert_eq!(previous.stale_language_of, None);
    assert_eq!(previous.cues[0].script, "alpha beta");
    assert_eq!(
        bound
            .language_evidence("lid", 1)?
            .transcript
            .map(|row| row.revision),
        Some(1)
    );
    Ok(())
}
