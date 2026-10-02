//! Archive search over real recognition, correction, translation and evidence rows.
//! Recognition and translation results are synthetic storage fixtures, not model output.

use std::collections::BTreeSet;
use std::path::Path;

use super::*;
use crate::{
    archive::{ArchiveFields, ArchiveRevisions},
    control::{AnalysisOperation, Operation, RecognitionView},
    languages::{
        LanguageEvidence, LanguageLabel, LanguageMethod, LanguageRoute, LanguageSpan,
        TranscriptReference,
    },
    monitor::{MonitorSpec, MonitorTerm},
    recognition::{
        LocalAsrOutcome, LocalAsrRequest, ReapedLocalAsr, RecognitionCoverage, RecognitionCue,
        RecognitionOutput,
    },
    sources::{HttpHop, HttpSource, NetworkScope},
    storage::dvr::{Publication, Retention},
    translation::{
        TranslatedCue, TranslationOutcome, TranslationProfile, TranslationRequest,
        TranslationResult,
    },
};

#[path = "populated.rs"]
mod populated;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

pub(super) fn profile() -> Result<TranslationProfile> {
    let root = std::env::temp_dir();
    let mut profile = TranslationProfile {
        id: "mt".into(),
        engine: crate::translation::LLAMA_CPP_COMPLETION.into(),
        template: crate::translation::HY_MT2_PLAIN.into(),
        runtime_dir: root.join("runtime").display().to_string(),
        executable: "llama-completion.exe".into(),
        runtime_sha256: "4".repeat(64),
        runtime_files: 3,
        runtime_bytes: 300,
        model_path: root.join("model.gguf").display().to_string(),
        model_sha256: "5".repeat(64),
        model_bytes: 1000,
        languages: "ar,es,fr,hi,zh".into(),
        threads: 2,
        memory_bytes: 1 << 30,
        cue_deadline_ms: 60_000,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    Ok(profile)
}

/// A current catalog with two sources, a DVR policy and one translation profile.
pub(super) fn library(path: &Path) -> Result<Store> {
    let mut store = Store::open(path)?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        1 << 30,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    for (id, name) in [("radio:v1", "Radio"), ("other:v1", "Other")] {
        store.register_source(
            id,
            &HttpSource::new(
                name,
                &format!("https://example.com/{name}"),
                NetworkScope::PublicInternet {},
            )?,
        )?;
    }
    store.add_translation_profile(&profile()?, 5)?;
    Ok(store)
}

/// One published one-second recording pinned under the same ID. Returns its capture start.
pub(super) fn record(store: &mut Store, id: &str, source: &str) -> Result<i64> {
    let job = store
        .admit_recording(id, source, 60, 600, Retention::Temporary, false)?
        .ok_or(Error::StorageIntegrity)?;
    store.publish_recording(
        &job.version,
        &Publication {
            bytes: 100,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![HttpHop {
                origin: "https://example.com".into(),
                peer: ([8, 8, 8, 8], 443).into(),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
            gap: None,
        },
    )?;
    store.admit_analysis(id, id, false, 10)?;
    store.publish_analysis(id, 1)?;
    Ok(store.connection.query_row(
        "SELECT starts_ms FROM capture_jobs WHERE id = ?1",
        [id],
        |row| row.get(0),
    )?)
}

/// Publish one recognition revision whose cues split the first coverage equally.
pub(super) fn recognize(store: &mut Store, pin: &str, scripts: &[&str]) -> Result<()> {
    let request = LocalAsrRequest {
        id: format!("asr-{pin}"),
        analysis_id: pin.into(),
        analysis_revision: 1,
        profile: "synthetic-storage-fixture-v1".into(),
        profile_sha256: "b".repeat(64),
        parent_revision: 0,
    };
    let work = store
        .admit_local_asr(&request, 20)?
        .1
        .ok_or(Error::StorageIntegrity)?;
    let coverages = work
        .input
        .chunks
        .iter()
        .map(|chunk| RecognitionCoverage {
            ordinal: chunk.ordinal,
            interval_ordinal: chunk.interval_ordinal,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            source_sha256: chunk.source_sha256.clone(),
            decoded_sha256: "c".repeat(64),
            sample_rate: 16_000,
            sample_count: (chunk.end_us - chunk.start_us) * 16_000 / 1_000_000,
        })
        .collect();
    let chunk = work.input.chunks.first().ok_or(Error::StorageIntegrity)?;
    let count = u64::try_from(scripts.len()).map_err(|_| Error::StorageIntegrity)?;
    let span = (chunk.end_us - chunk.start_us) / count;
    let cues = scripts
        .iter()
        .zip(0u32..)
        .map(|(script, ordinal)| RecognitionCue {
            ordinal,
            start_us: chunk.start_us + u64::from(ordinal) * span,
            end_us: chunk.start_us + (u64::from(ordinal) + 1) * span,
            script: (*script).into(),
        })
        .collect();
    let output = RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages,
        cues,
    };
    let job = store.finish_local_asr(
        &work,
        &ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(output)),
        21,
    )?;
    if job.state != "succeeded" {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

/// Publish one translation. `None` stores an untranslated cue with its reason.
pub(super) fn translate(
    store: &mut Store,
    job: &str,
    pin: &str,
    revision: i64,
    english: &[Option<&str>],
) -> Result<()> {
    let profile = profile()?;
    let request = TranslationRequest {
        id: job.into(),
        transcript_id: pin.into(),
        transcript_revision: revision,
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
    };
    let (admitted, work) = store.admit_translation(&request, 30)?;
    let work = work.ok_or(Error::StorageIntegrity)?;
    let cues = english
        .iter()
        .zip(0u32..)
        .map(|(text, ordinal)| match text {
            Some(text) => TranslatedCue {
                ordinal,
                state: "translated".into(),
                english: Some((*text).into()),
                reason: None,
            },
            None => TranslatedCue {
                ordinal,
                state: "untranslated".into(),
                english: None,
                reason: Some("unsupported-language".into()),
            },
        })
        .collect();
    let finished = store.finish_translation(
        &work,
        &TranslationOutcome::synthetic_fixture(&admitted, TranslationResult::Succeeded(cues)),
        31,
    )?;
    if finished.state != "succeeded" {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

/// Publish recognizer block labels bound to one transcript revision.
fn labels(
    store: &mut Store,
    id: &str,
    pin: &str,
    transcript_revision: i64,
    spans: &[(u64, u64, &str)],
) -> Result<()> {
    let evidence = LanguageEvidence {
        id: id.into(),
        revision: 1,
        analysis_id: pin.into(),
        analysis_revision: 1,
        transcript: Some(TranscriptReference {
            id: pin.into(),
            revision: transcript_revision,
        }),
        method: LanguageMethod {
            origin: "recognizer".into(),
            profile: "synthetic-storage-fixture-v1".into(),
            profile_sha256: "b".repeat(64),
            resolution: "block".into(),
            alias_map: "whisper-cpp-codes-v1".into(),
        },
        outcome: "succeeded".into(),
        reason: None,
        spans: spans
            .iter()
            .zip(0u32..)
            .map(|((start_us, end_us, tag), ordinal)| LanguageSpan {
                ordinal,
                interval_ordinal: 0,
                start_us: *start_us,
                end_us: *end_us,
                cue_ordinal: None,
                observation: "identified".into(),
                languages: vec![LanguageLabel {
                    tag: (*tag).into(),
                    provider_label: (*tag).into(),
                }],
                route: LanguageRoute {
                    task: "transcription".into(),
                    capability: "unevaluated".into(),
                    profile: "synthetic-storage-fixture-v1".into(),
                    profile_sha256: "b".repeat(64),
                    basis: "declared".into(),
                    basis_sha256: "b".repeat(64),
                },
            })
            .collect(),
    };
    store.publish_language_evidence(evidence, 25)?;
    Ok(())
}

fn query(term: &str) -> ArchiveQuery {
    ArchiveQuery::new(term)
}

/// Every hit as (transcript, revision, ordinal, field, translation revision).
fn keys(page: &ArchivePage) -> Vec<(String, i64, u32, ArchiveField, Option<i64>)> {
    page.hits
        .iter()
        .map(|hit| {
            (
                hit.transcript_id.clone(),
                hit.transcript_revision,
                hit.cue_ordinal,
                hit.field,
                hit.translation_revision,
            )
        })
        .collect()
}

/// Counts of every table a search must not write.
pub(super) fn writes(store: &Store) -> Result<Vec<i64>> {
    let mut counts = Vec::new();
    for table in [
        "analysis_jobs",
        "translation_jobs",
        "transcripts",
        "transcript_cues",
        "translations",
        "translation_cues",
        "language_evidence",
        "monitor_findings",
        "monitor_briefings",
        "ledger_events",
        "requests",
        "provider_attempts",
        "capture_jobs",
        "recordings",
    ] {
        counts.push(store.connection.query_row(
            &format!("SELECT count(*) FROM {table}"),
            [],
            |row| row.get(0),
        )?);
    }
    Ok(counts)
}

#[test]
fn an_empty_library_reads_nothing_and_writes_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = library(&directory.path().join("catalog"))?;
    let before = writes(&store)?;
    let changes = store.connection.total_changes();
    let page = store.archive_search(query("feria"))?;
    assert!(page.hits.is_empty());
    assert_eq!(page.stopped, None);
    assert_eq!(page.next, None);
    assert_eq!(
        (
            page.transcripts_scanned,
            page.rows_scanned,
            page.language_rows_scanned
        ),
        (0, 0, 0)
    );
    assert_eq!(
        page.query.limit,
        Some(crate::archive::ARCHIVE_DEFAULT_RESULTS)
    );
    assert_eq!(writes(&store)?, before);
    assert_eq!(store.connection.total_changes(), changes);
    assert!(matches!(
        store.archive_search(query(" padded")),
        Err(Error::InvalidInput("archive term"))
    ));
    Ok(())
}

#[test]
fn multilingual_scripts_match_as_written_ignoring_letter_case_only() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    record(&mut store, "one", "radio:v1")?;
    recognize(
        &mut store,
        "one",
        &[
            "سد النهضة الكبير",
            "बाँध परियोजना",
            "三峡大坝的水位",
            "Cafe\u{301} DEL MUNDO",
            "Café del mundo",
        ],
    )?;
    translate(
        &mut store,
        "mt-one",
        "one",
        1,
        &[
            Some("The great Renaissance dam"),
            Some("Dam project"),
            Some("Three Gorges Dam water level"),
            None,
            Some("Coffee of the world"),
        ],
    )?;
    let before = writes(&store)?;
    let found = |term: &str| -> Result<Vec<u32>> {
        Ok(store
            .archive_search(query(term))?
            .hits
            .iter()
            .map(|hit| hit.cue_ordinal)
            .collect())
    };
    assert_eq!(found("النهضة")?, vec![0]);
    assert_eq!(found("बाँध")?, vec![1]);
    // Anusvara is a different code point from candrabindu: no normalization.
    assert!(found("बांध")?.is_empty());
    assert_eq!(found("大坝")?, vec![2]);
    // Precomposed and decomposed accents stay distinct; letter case is ignored.
    assert_eq!(found("CAFÉ")?, vec![4]);
    assert_eq!(found("cafe\u{301}")?, vec![3]);
    assert_eq!(found("del mundo")?, vec![3, 4]);
    assert!(found("cafe del")?.is_empty());
    let dam = store.archive_search(query("DAM"))?;
    assert_eq!(keys(&dam).len(), 3);
    assert!(
        dam.hits
            .iter()
            .all(|hit| hit.field == ArchiveField::English)
    );
    let first = &dam.hits[0];
    assert_eq!(first.original, "سد النهضة الكبير");
    assert_eq!(first.english.as_deref(), Some("The great Renaissance dam"));
    assert_eq!(first.translation_revision, Some(1));
    assert_eq!((first.start_us, first.end_us), (0, 200_000));
    assert_eq!(first.source, "radio:v1");
    assert_eq!(first.recording_id, "one");
    assert_eq!(first.transcript_kind, "recognition");
    assert_eq!(first.media, ArchiveMedia::Retained);
    assert!(!first.stale_transcript && !first.stale_translation);
    let mut original_only = query("DAM");
    original_only.fields = ArchiveFields::Original;
    assert!(store.archive_search(original_only)?.hits.is_empty());
    let untranslated = store.archive_search(query("MUNDO"))?;
    assert_eq!(
        untranslated.hits[0].untranslated_reason.as_deref(),
        Some("unsupported-language")
    );
    assert_eq!(untranslated.hits[0].english, None);
    assert_eq!(writes(&store)?, before);
    Ok(())
}

type Citation = (String, i64, u32, String);

/// A monitor on `radio:v1` with these literal terms.
fn follow(store: &mut Store, terms: &[(&str, &str)]) -> Result<()> {
    store.create_monitor(
        "dam",
        &MonitorSpec {
            name: "Dams".into(),
            goal: "Follow dams.".into(),
            terms: terms
                .iter()
                .map(|(language, text)| MonitorTerm {
                    language: (*language).into(),
                    text: (*text).into(),
                })
                .collect(),
            sources: vec!["radio:v1".into()],
            candidate_sources: Vec::new(),
            schedules: Vec::new(),
            daily_audio_seconds: 3600,
            total_audio_seconds: 7200,
            recognition_profile: None,
            translation_profile: None,
            capture: None,
        },
        40,
    )?;
    Ok(())
}

/// The archive search a monitor term corresponds to, over the same source and window.
fn archive_citations(
    store: &Store,
    language: &str,
    text: &str,
    start: i64,
) -> Result<BTreeSet<Citation>> {
    let mut ask = query(text);
    ask.fields = if crate::monitor::searches_english(language) {
        ArchiveFields::Both
    } else {
        ArchiveFields::Original
    };
    ask.source = Some("radio:v1".into());
    ask.from_ms = Some(start);
    ask.to_ms = Some(start + 1);
    Ok(store
        .archive_search(ask)?
        .hits
        .iter()
        .map(|hit| {
            (
                hit.transcript_id.clone(),
                hit.transcript_revision,
                hit.cue_ordinal,
                match hit.field {
                    ArchiveField::Original => "original".to_owned(),
                    ArchiveField::English => "english".to_owned(),
                },
            )
        })
        .collect())
}

#[test]
fn library_search_agrees_with_monitor_matches_for_the_same_terms() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    let start = record(&mut store, "one", "radio:v1")?;
    recognize(
        &mut store,
        "one",
        &["سد النهضة", "बाँध परियोजना", "三峡大坝", "Café DEL MUNDO"],
    )?;
    translate(
        &mut store,
        "mt-one",
        "one",
        1,
        &[
            Some("The Renaissance dam"),
            Some("dam project"),
            None,
            Some("World coffee"),
        ],
    )?;
    let terms = [
        ("ar", "النهضة"),
        ("hi", "बाँध"),
        ("zh", "大坝"),
        ("es", "CAFÉ"),
        ("und", "del mundo"),
        ("en", "Dam"),
        ("und", "world"),
        ("es", "world"),
    ];
    follow(&mut store, &terms)?;
    let matches = store.monitor_matches("dam", start, start + 1)?;
    assert!(!matches.matches.is_empty());
    for (language, text) in terms {
        let monitor: BTreeSet<Citation> = matches
            .matches
            .iter()
            .filter(|found| found.term == text && found.term_language == language)
            .map(|found| {
                (
                    found.transcript_id.clone(),
                    found.transcript_revision,
                    found.cue_ordinal,
                    found.field.clone(),
                )
            })
            .collect();
        assert_eq!(
            archive_citations(&store, language, text, start)?,
            monitor,
            "{language}:{text}"
        );
    }
    // The window and source filters exclude everything outside them.
    let mut later = query("Dam");
    later.from_ms = Some(start + 1);
    later.to_ms = Some(start + 2);
    assert!(store.archive_search(later)?.hits.is_empty());
    let mut elsewhere = query("Dam");
    elsewhere.source = Some("other:v1".into());
    let page = store.archive_search(elsewhere)?;
    assert!(page.hits.is_empty());
    assert_eq!(page.transcripts_scanned, 1);
    assert_eq!(page.rows_scanned, 1);
    Ok(())
}

#[test]
fn corrections_and_retranslations_label_stale_revisions_without_rewriting() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    record(&mut store, "one", "radio:v1")?;
    recognize(&mut store, "one", &["Una feria mundial", "en la ciudad"])?;
    translate(
        &mut store,
        "mt-1",
        "one",
        1,
        &[Some("A world fair"), Some("in the city")],
    )?;
    assert_eq!(
        store.correct_transcript("one", 1, 0, "Una feria local", 40)?,
        2
    );
    let history = |term: &str| {
        let mut ask = query(term);
        ask.revisions = ArchiveRevisions::All;
        ask
    };
    // The corrected-away word is only in revision 1, which is stale.
    assert!(store.archive_search(query("mundial"))?.hits.is_empty());
    let old = store.archive_search(history("mundial"))?;
    assert_eq!(
        keys(&old),
        vec![("one".into(), 1, 0, ArchiveField::Original, Some(1))]
    );
    assert!(old.hits[0].stale_transcript);
    assert!(!old.hits[0].stale_translation);
    assert_eq!(old.hits[0].english.as_deref(), Some("A world fair"));
    let corrected = store.archive_search(query("LOCAL"))?;
    assert_eq!(
        keys(&corrected),
        vec![("one".into(), 2, 0, ArchiveField::Original, None)]
    );
    assert_eq!(corrected.hits[0].transcript_kind, "correction");
    assert!(!corrected.hits[0].stale_transcript);
    // A copied cue is current on revision 2 and stale on revision 1.
    assert_eq!(
        keys(&store.archive_search(query("ciudad"))?),
        vec![("one".into(), 2, 1, ArchiveField::Original, None)]
    );
    assert_eq!(
        keys(&store.archive_search(history("ciudad"))?),
        vec![
            ("one".into(), 1, 1, ArchiveField::Original, Some(1)),
            ("one".into(), 2, 1, ArchiveField::Original, None)
        ]
    );
    // The English of the stale revision is not current.
    assert!(store.archive_search(query("world"))?.hits.is_empty());
    // A second translation of revision 1 makes the first one stale.
    translate(
        &mut store,
        "mt-2",
        "one",
        1,
        &[Some("A global fair"), Some("in town")],
    )?;
    let english = store.archive_search(history("world"))?;
    assert_eq!(
        keys(&english),
        vec![("one".into(), 1, 0, ArchiveField::English, Some(1))]
    );
    assert!(english.hits[0].stale_transcript && english.hits[0].stale_translation);
    let global = store.archive_search(history("global"))?;
    assert_eq!(
        keys(&global),
        vec![("one".into(), 1, 0, ArchiveField::English, Some(2))]
    );
    assert!(!global.hits[0].stale_translation);
    // An original hit is reported once, beside the newest English of its revision.
    let mut both = history("feria");
    both.fields = ArchiveFields::Both;
    assert_eq!(
        keys(&store.archive_search(both)?),
        vec![
            ("one".into(), 1, 0, ArchiveField::Original, Some(2)),
            ("one".into(), 2, 0, ArchiveField::Original, None)
        ]
    );
    let mut english_only = history("fair");
    english_only.fields = ArchiveFields::English;
    assert_eq!(
        keys(&store.archive_search(english_only)?),
        vec![
            ("one".into(), 1, 0, ArchiveField::English, Some(2)),
            ("one".into(), 1, 0, ArchiveField::English, Some(1))
        ]
    );
    // Translating the correction makes its English current.
    translate(
        &mut store,
        "mt-3",
        "one",
        2,
        &[Some("A local fair"), Some("in the city")],
    )?;
    let current = store.archive_search(query("local fair"))?;
    assert_eq!(
        keys(&current),
        vec![("one".into(), 2, 0, ArchiveField::English, Some(1))]
    );
    // Stored text is unchanged by the searches.
    let page = store.translation_page("one", 1, 1, None)?;
    assert_eq!(page.pairs[0].english.as_deref(), Some("A world fair"));
    Ok(())
}

#[test]
fn media_state_follows_retention_release_gaps_and_checksums() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    for id in ["kept", "gapped", "released", "deleted", "mismatch"] {
        record(&mut store, id, "radio:v1")?;
        recognize(&mut store, id, &["presa del norte"])?;
    }
    store.connection.execute(
        "INSERT INTO recording_gaps VALUES ('gapped', 0, 'disconnect', 250000, 500000)",
        [],
    )?;
    store.connection.execute(
        "INSERT INTO recording_releases VALUES ('released', 0, 100)",
        [],
    )?;
    store.connection.execute(
        "UPDATE recordings SET sha256 = ?1 WHERE id = 'mismatch'",
        ["f".repeat(64)],
    )?;
    store.begin_delete("deleted", false)?;
    let media = |store: &Store| -> Result<Vec<(String, ArchiveMedia)>> {
        Ok(store
            .archive_search(query("presa"))?
            .hits
            .into_iter()
            .map(|hit| (hit.recording_id, hit.media))
            .collect())
    };
    assert_eq!(
        media(&store)?,
        vec![
            ("deleted".into(), ArchiveMedia::Expired),
            ("gapped".into(), ArchiveMedia::Missing),
            ("kept".into(), ArchiveMedia::Retained),
            ("mismatch".into(), ArchiveMedia::Unavailable),
            ("released".into(), ArchiveMedia::Released),
        ]
    );
    store.finish_delete("deleted")?;
    assert_eq!(media(&store)?[0], ("deleted".into(), ArchiveMedia::Expired));
    // Text of expired audio stays searchable; the label says the audio is gone.
    assert_eq!(media(&store)?.len(), 5);
    Ok(())
}

#[test]
fn language_filters_use_stored_labels_and_report_their_binding() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    record(&mut store, "one", "radio:v1")?;
    recognize(
        &mut store,
        "one",
        &[
            "radio uno",
            "radio deux",
            "radio Diné bizaad",
            "radio tlhIngan Hol",
        ],
    )?;
    labels(
        &mut store,
        "lid-one",
        "one",
        1,
        &[
            (0, 250_000, "es"),
            (250_000, 500_000, "fr-CA"),
            (500_000, 750_000, "nv"),
            (750_000, 1_000_000, "tlh"),
        ],
    )?;
    let filtered = |store: &Store, language: &str| -> Result<Vec<u32>> {
        let mut ask = query("radio");
        ask.language = Some(language.into());
        Ok(store
            .archive_search(ask)?
            .hits
            .iter()
            .map(|hit| hit.cue_ordinal)
            .collect())
    };
    assert_eq!(filtered(&store, "es")?, vec![0]);
    assert_eq!(filtered(&store, "fr")?, vec![1]);
    assert_eq!(filtered(&store, "FR-ca")?, vec![1]);
    assert_eq!(filtered(&store, "nv")?, vec![2]);
    assert_eq!(filtered(&store, "tlh")?, vec![3]);
    assert!(filtered(&store, "de")?.is_empty());
    let all = store.archive_search(query("radio"))?;
    assert_eq!(all.hits.len(), 4);
    let canadian = &all.hits[1].languages;
    assert_eq!(canadian.len(), 1);
    assert_eq!(canadian[0].tag, "fr-CA");
    assert_eq!(canadian[0].evidence_id, "lid-one");
    assert_eq!(canadian[0].evidence_revision, 1);
    assert_eq!(canadian[0].transcript_revision, Some(1));
    assert_eq!(canadian[0].capability, "unevaluated");
    assert!(!all.hits[1].more_languages);
    assert_eq!(all.language_rows_scanned, 4);
    // A correction keeps cue times, so the older binding still overlaps and says so.
    store.correct_transcript("one", 1, 1, "radio trois", 40)?;
    let after = store.archive_search(query("trois"))?;
    assert_eq!(after.hits[0].transcript_revision, 2);
    assert_eq!(after.hits[0].languages[0].transcript_revision, Some(1));
    assert_eq!(filtered(&store, "fr")?, vec![1]);
    Ok(())
}

#[test]
fn result_limits_stop_explicitly_and_cursors_resume_without_loss() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    for id in ["a", "b", "c"] {
        record(&mut store, id, "radio:v1")?;
        recognize(
            &mut store,
            id,
            &["feria uno", "nada", "otra feria", "FERIA final"],
        )?;
    }
    let full = store.archive_search(query("feria"))?;
    assert_eq!(full.hits.len(), 9);
    assert_eq!(full.stopped, None);
    let mut ask = query("feria");
    ask.limit = Some(2);
    let mut seen = Vec::new();
    let mut pages = 0;
    loop {
        let page = store.archive_search(ask.clone())?;
        pages += 1;
        seen.extend(keys(&page));
        match page.stopped {
            None => break,
            Some(stop) => assert_eq!(stop, ArchiveStop::Results),
        }
        ask.after = Some(page.next.ok_or("a stopped page has a cursor")?);
    }
    assert_eq!(pages, 5);
    assert_eq!(seen, keys(&full));
    // A limit equal to the hit count is complete, with no cursor.
    let mut exact = query("feria");
    exact.limit = Some(9);
    let page = store.archive_search(exact)?;
    assert_eq!((page.hits.len(), page.stopped, page.next), (9, None, None));
    Ok(())
}

#[test]
fn row_budgets_and_deadlines_always_advance_and_never_overrun() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    for id in ["a", "b", "c"] {
        record(&mut store, id, "radio:v1")?;
        recognize(&mut store, id, &["feria uno", "nada", "otra feria"])?;
    }
    store.correct_transcript("b", 1, 1, "nada mas", 40)?;
    let full = store.archive_search(query("feria"))?;
    assert_eq!(full.hits.len(), 6);
    for budget in [2, 3, 5] {
        let mut ask = query("feria");
        ask.scan_rows = Some(budget);
        let mut seen = Vec::new();
        let mut last = None;
        loop {
            let page = store.archive_search(ask.clone())?;
            assert!(page.rows_scanned <= budget);
            seen.extend(keys(&page));
            let Some(stop) = page.stopped else {
                break;
            };
            assert_eq!(stop, ArchiveStop::Rows);
            let next = page.next.ok_or("a stopped page has a cursor")?;
            assert_ne!(Some(&next), last.as_ref(), "the cursor advanced");
            last = Some(next.clone());
            ask.after = Some(next);
        }
        assert_eq!(seen, keys(&full), "budget {budget}");
    }
    // An expired deadline still reads one row per request, so the scan finishes.
    let mut ask = query("feria").validate()?;
    let mut seen = Vec::new();
    let mut requests = 0;
    loop {
        let page = search(&store.connection, ask.clone(), Instant::now())?;
        requests += 1;
        assert!(page.rows_scanned <= 2);
        seen.extend(keys(&page));
        let Some(stop) = page.stopped else {
            break;
        };
        assert_eq!(stop, ArchiveStop::Deadline);
        ask.after = page.next;
    }
    assert_eq!(seen, keys(&full));
    assert!(requests > 3);
    Ok(())
}

#[test]
fn oversized_hits_stop_at_the_page_byte_bound() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    record(&mut store, "one", "radio:v1")?;
    // Recognizer and translator text may hold control characters; JSON escapes them.
    let hostile = format!("{}zz", "\u{1}".repeat(4094));
    recognize(&mut store, "one", &[&hostile, &hostile, &hostile])?;
    translate(
        &mut store,
        "mt-one",
        "one",
        1,
        &[Some(&hostile), Some(&hostile), Some(&hostile)],
    )?;
    let mut ask = query("ZZ");
    ask.limit = Some(64);
    let mut seen = 0;
    loop {
        let page = store.archive_search(ask.clone())?;
        assert!(serde_json::to_vec(&page)?.len() <= ARCHIVE_PAGE_BYTES);
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].original, hostile);
        seen += 1;
        match page.stopped {
            None => break,
            Some(stop) => assert_eq!(stop, ArchiveStop::PageBytes),
        }
        ask.after = page.next;
    }
    assert_eq!(seen, 3);
    Ok(())
}

#[test]
fn the_largest_hit_fits_an_empty_page() -> TestResult {
    let tag = format!("x-{}", "a".repeat(126));
    let hit = ArchiveHit {
        source: "s".repeat(128),
        recording_id: "r".repeat(128),
        capture_start_ms: i64::MAX,
        transcript_id: "t".repeat(128),
        transcript_revision: 64,
        transcript_kind: "correction".into(),
        cue_ordinal: 255,
        start_us: u64::MAX,
        end_us: u64::MAX,
        field: ArchiveField::English,
        original: "\u{1}".repeat(4096),
        translation_revision: Some(64),
        english: Some("\u{1}".repeat(4096)),
        untranslated_reason: Some("r".repeat(128)),
        stale_transcript: true,
        stale_translation: true,
        media: ArchiveMedia::Unavailable,
        languages: vec![
            ArchiveLanguage {
                tag,
                evidence_id: "e".repeat(128),
                evidence_revision: 64,
                transcript_revision: Some(64),
                capability: "unevaluated".into(),
            };
            crate::archive::ARCHIVE_MAX_LANGUAGES
        ],
        more_languages: true,
    };
    let mut worst = query(&"\"".repeat(crate::monitor::MAX_TERM_CHARS));
    worst.source = Some("s".repeat(128));
    worst.from_ms = Some(i64::MAX - 1);
    worst.to_ms = Some(i64::MAX);
    worst.language = Some("x-aaaaaaaa".into());
    worst.revisions = ArchiveRevisions::All;
    worst.after = Some(ArchiveCursor {
        transcript_id: "t".repeat(128),
        transcript_revision: 65,
        pass: 64,
        ordinal: 256,
    });
    let page = ArchivePage {
        query: worst.validate()?,
        hits: Vec::new(),
        transcripts_scanned: 0,
        rows_scanned: 0,
        language_rows_scanned: 0,
        stopped: None,
        next: None,
    };
    let total = reserved_bytes(&page)? + serde_json::to_vec(&hit)?.len();
    assert!(total <= ARCHIVE_PAGE_BYTES, "{total}");
    Ok(())
}

#[test]
fn hostile_text_round_trips_exactly_through_the_control_operation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = library(&directory.path().join("catalog"))?;
    record(&mut store, "one", "radio:v1")?;
    let script = "alarma\u{1b}[2J\u{202e}txet\u{7}";
    recognize(&mut store, "one", &[script])?;
    translate(
        &mut store,
        "mt-one",
        "one",
        1,
        &[Some("alarm\u{1b}]0;title\u{7}")],
    )?;
    let before = writes(&store)?;
    let snapshot = crate::control::apply(
        &mut store,
        Operation::Analysis {
            command: AnalysisOperation::Search {
                query: query("ALARMA"),
            },
        },
    )?;
    let Some(view) = snapshot.recognition else {
        return Err("search returns a recognition view".into());
    };
    let RecognitionView::Search { page } = *view else {
        return Err("search returns a search page".into());
    };
    assert_eq!(page.hits[0].original, script);
    assert_eq!(
        page.hits[0].english.as_deref(),
        Some("alarm\u{1b}]0;title\u{7}")
    );
    let wire = serde_json::to_string(&page)?;
    let back: ArchivePage = serde_json::from_str(&wire)?;
    assert_eq!(back, *page);
    assert!(!wire.contains('\u{1b}'));
    let request: AnalysisOperation = serde_json::from_str(
        r#"{"action":"search","query":{"term":"x","fields":"english","revisions":"all"}}"#,
    )?;
    assert!(matches!(request, AnalysisOperation::Search { .. }));
    assert!(
        serde_json::from_str::<AnalysisOperation>(
            r#"{"action":"search","query":{"term":"x","publish":true}}"#
        )
        .is_err()
    );
    assert_eq!(writes(&store)?, before);
    Ok(())
}
