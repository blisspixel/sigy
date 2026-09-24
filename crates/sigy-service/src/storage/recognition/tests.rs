//! Storage fixtures only. Synthetic cleanup capabilities are not OS isolation evidence.

use std::path::Path;

use super::*;
use crate::recognition::{
    LocalAsrFailure, LocalAsrOutcome, ReapedLocalAsr, RecognitionCoverage, RecognitionCue,
    RecognitionOutput, TRANSCRIPT_PAGE_BYTES,
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn setup(path: &Path, legacy: bool) -> Result<Store> {
    let old = super::super::transcripts::migration_tests::setup_at_version(path, 25)?;
    if legacy {
        old.connection.execute("INSERT INTO transcripts VALUES ('pin', 1, 'pin', 1, 'one', ?1, 'original', 'local-unmeasured', 'published', 11)", ["a".repeat(64)])?;
        old.connection.execute_batch("INSERT INTO transcript_cues VALUES ('pin', 1, 0, 0, 1000000, '', 'uncertain'); INSERT INTO analysis_decisions VALUES ('pin', 1, 0, NULL, 11);")?;
    }
    drop(old);
    Store::open(path)
}

fn request(id: &str, parent: i64) -> LocalAsrRequest {
    LocalAsrRequest {
        id: id.into(),
        analysis_id: "pin".into(),
        analysis_revision: 1,
        profile: "synthetic-storage-fixture-v1".into(),
        profile_sha256: "b".repeat(64),
        parent_revision: parent,
    }
}

fn admit(store: &mut Store, id: &str, parent: i64) -> Result<LocalAsrWork> {
    store
        .admit_local_asr(&request(id, parent), 20)?
        .1
        .ok_or(Error::StorageIntegrity)
}

fn output(work: &LocalAsrWork, script: &str) -> RecognitionOutput {
    RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverage: RecognitionCoverage {
            interval_ordinal: work.input.interval_ordinal,
            start_us: work.input.start_us,
            end_us: work.input.end_us,
            source_sha256: work.input.source_sha256.clone(),
            decoded_sha256: "c".repeat(64),
            sample_rate: 16_000,
            sample_count: 16_000,
        },
        cues: if script.is_empty() {
            Vec::new()
        } else {
            vec![RecognitionCue {
                ordinal: 0,
                start_us: work.input.start_us,
                end_us: work.input.end_us,
                script: script.into(),
            }]
        },
    }
}

fn proof(work: &LocalAsrWork, script: &str) -> ReapedLocalAsr {
    ReapedLocalAsr::synthetic_fixture(work, LocalAsrOutcome::Succeeded(output(work, script)))
}

fn counts(store: &Store) -> Result<(u32, u32, u32, u32)> {
    Ok(store.connection.query_row(
        "SELECT (SELECT count(*) FROM transcripts), (SELECT count(*) FROM transcript_cues), (SELECT count(*) FROM transcript_coverage), (SELECT count(*) FROM analysis_decisions)", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?)
}

#[test]
fn admission_is_exact_replay_without_redispatch_and_shares_worker_slot() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let work = admit(&mut store, "asr", 0)?;
    let replay = store.admit_local_asr(&request("asr", 0), -1)?;
    assert_eq!(replay.0, work.job);
    assert!(replay.1.is_none());
    for change in ["profile", "digest", "parent", "pin"] {
        let mut changed = request("asr", 0);
        match change {
            "profile" => changed.profile = "different".into(),
            "digest" => changed.profile_sha256 = "d".repeat(64),
            "parent" => changed.parent_revision = 1,
            _ => changed.analysis_revision = 2,
        }
        assert!(matches!(
            store.admit_local_asr(&changed, 21),
            Err(Error::IdempotencyConflict)
        ));
    }
    assert!(matches!(
        store.admit_local_asr(&request("other", 0), 21),
        Err(Error::Analysis("worker-busy"))
    ));
    assert!(matches!(
        store.admit_verification("other", "pin", 1, 21),
        Err(Error::Analysis("worker-busy"))
    ));
    assert!(matches!(
        store.admit_verification("asr", "pin", 1, 21),
        Err(Error::IdempotencyConflict)
    ));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    assert_eq!(work.input.byte_length, 100);
    assert_eq!(work.job.manifest_sha256, manifest(&work.input)?);
    Ok(())
}

#[test]
fn unicode_success_is_atomic_zero_cost_and_replays_after_media_expiry() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    let work = admit(&mut store, "asr", 0)?;
    let text = "العربية हिंदी 中文 e\u{301} \"quoted\"\n";
    let completed = proof(&work, text);
    let job = store.finish_local_asr(&work, &completed, 21)?;
    assert_eq!(job.state, "succeeded");
    assert_eq!(job.amount_usd, "0.000000");
    let page = store.transcript_cues_page("pin", 1, None)?;
    assert_eq!(page.cues[0].script.as_bytes(), text.as_bytes());
    assert_eq!(page.transcript.text_bytes, u64::try_from(text.len())?);
    assert_eq!(page.transcript.outcome, "text");
    assert_eq!(page.next_after_ordinal, None);
    assert_eq!(counts(&store)?, (1, 1, 1, 1));
    let paid: (u32, u32) = store.connection.query_row(
        "SELECT (SELECT count(*) FROM requests), (SELECT count(*) FROM ledger_events)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(paid, (0, 0));
    assert!(matches!(
        store.finish_local_asr(&work, &proof(&work, "changed"), 22),
        Err(Error::IdempotencyConflict)
    ));
    store.begin_delete("one", false)?;
    store.finish_delete("one")?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert_eq!(reopened.transcript_cues_page("pin", 1, None)?, page);
    assert_eq!(reopened.finish_local_asr(&work, &completed, 99)?, job);
    assert!(
        reopened
            .admit_local_asr(&request("asr", 0), 99)?
            .1
            .is_none()
    );
    assert!(reopened.admit_local_asr(&request("new", 1), 99).is_err());
    Ok(())
}

#[test]
fn cancellation_holds_lease_and_restart_interrupts_with_new_generation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    let work = admit(&mut store, "asr", 0)?;
    assert!(matches!(
        store.cancel_local_asr("asr", 2),
        Err(Error::Analysis("stale-worker"))
    ));
    assert_eq!(store.cancel_local_asr("asr", 1)?.state, "cancelling");
    assert!(store.begin_delete("one", false).is_err());
    assert!(
        store
            .connection
            .execute("INSERT INTO recording_releases VALUES ('one', 0, 100)", [])
            .is_err()
    );
    drop(store);
    let mut reopened = Store::open(&path)?;
    // The previous service's contained group ended with its process. Restart
    // interrupts the job, advances its generation and never replays it.
    reopened.recover_analysis_jobs()?;
    let recovered = reopened.local_asr_job("asr")?;
    assert_eq!(
        (
            recovered.generation,
            recovered.state.as_str(),
            recovered.reason.as_deref()
        ),
        (2, "interrupted", Some("service-restarted"))
    );
    assert!(
        reopened
            .admit_local_asr(&request("asr", 0), 99)?
            .1
            .is_none()
    );
    let queued = proof(&work, "queued speech");
    assert!(matches!(
        reopened.finish_local_asr(&work, &queued, 22),
        Err(Error::Analysis("stale-worker"))
    ));
    assert_eq!(counts(&reopened)?, (0, 0, 0, 0));
    reopened.begin_delete("one", false)?;
    Ok(())
}

#[test]
fn legacy_and_zero_cue_revisions_stay_distinct_and_parent_is_exact() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, true)?;
    let legacy = store.transcript_cues_page("pin", 1, None)?;
    assert!(matches!(
        store.admit_local_asr(&request("wrong", 0), 20),
        Err(Error::Analysis("transcript-parent-conflict"))
    ));
    let first = admit(&mut store, "no-text", 1)?;
    store.finish_local_asr(&first, &proof(&first, ""), 21)?;
    let silence = store.transcript_cues_page("pin", 2, None)?;
    assert_eq!(silence.transcript.outcome, "no_text");
    assert!(silence.cues.is_empty());
    assert!(silence.coverage.is_some());
    assert_eq!(silence.next_after_ordinal, None);
    let second = admit(&mut store, "text", 2)?;
    store.finish_local_asr(&second, &proof(&second, "speech"), 22)?;
    assert_eq!(store.transcript_cues_page("pin", 1, None)?, legacy);
    let revisions = store.transcript_revisions("pin", 0)?;
    assert_eq!(revisions.revisions.len(), 3);
    assert_eq!(revisions.revisions[0].outcome, "legacy");
    assert_eq!(revisions.revisions[1].parent_revision, Some(1));
    assert_eq!(revisions.revisions[2].parent_revision, Some(2));
    drop(store);
    assert_eq!(
        Store::open(&path)?.transcript_cues_page("pin", 2, None)?,
        silence
    );
    Ok(())
}

#[test]
fn malformed_worker_outputs_fail_without_partial_or_empty_placeholder_rows() -> TestResult {
    for fault in [
        "digest",
        "manifest",
        "source",
        "decoded",
        "coverage",
        "clock",
        "ordinal",
        "range",
        "empty",
        "oversized",
        "count",
        "total",
        "overlap",
    ] {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog"), false)?;
        let work = admit(&mut store, "asr", 0)?;
        let mut value = output(&work, "speech");
        match fault {
            "digest" => value.profile_sha256 = "d".repeat(64),
            "manifest" => value.manifest_sha256 = "d".repeat(64),
            "source" => value.coverage.source_sha256 = "d".repeat(64),
            "decoded" => value.coverage.decoded_sha256 = "invalid".into(),
            "coverage" => value.coverage.end_us += 1,
            "clock" => value.coverage.sample_count -= 2,
            "ordinal" => value.cues[0].ordinal = 1,
            "range" => value.cues[0].end_us += 1,
            "empty" => value.cues[0].script.clear(),
            "oversized" => value.cues[0].script = "x".repeat(4097),
            "count" => {
                value.cues = (0..257)
                    .map(|ordinal| RecognitionCue {
                        ordinal,
                        start_us: u64::from(ordinal),
                        end_us: u64::from(ordinal) + 1,
                        script: "x".into(),
                    })
                    .collect();
            }
            "total" => {
                value.cues = (0..17)
                    .map(|ordinal| RecognitionCue {
                        ordinal,
                        start_us: u64::from(ordinal),
                        end_us: u64::from(ordinal) + 1,
                        script: "x".repeat(4096),
                    })
                    .collect();
            }
            _ => value.cues.push(RecognitionCue {
                ordinal: 1,
                start_us: 0,
                end_us: 1,
                script: "overlap".into(),
            }),
        }
        let completed = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(value));
        let job = store.finish_local_asr(&work, &completed, 21)?;
        assert_eq!(
            job.reason.as_deref(),
            Some("invalid-worker-result"),
            "{fault}"
        );
        assert_eq!(counts(&store)?, (0, 0, 0, 0), "{fault}");
        store.begin_delete("one", false)?;
    }
    Ok(())
}

#[test]
fn every_publication_stage_rolls_back_and_keeps_durable_lease_on_sql_failure() -> TestResult {
    for (table, operation) in [
        ("transcripts", "INSERT"),
        ("transcript_cues", "INSERT"),
        ("transcript_coverage", "INSERT"),
        ("analysis_decisions", "INSERT"),
        ("analysis_jobs", "UPDATE"),
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = setup(&path, false)?;
        let work = admit(&mut store, "asr", 0)?;
        let completed = proof(&work, "speech");
        store.connection.execute_batch(&format!("CREATE TRIGGER fixture_fault BEFORE {operation} ON {table} BEGIN SELECT RAISE(ABORT, 'synthetic storage fault'); END;"))?;
        assert!(
            store.finish_local_asr(&work, &completed, 21).is_err(),
            "{table}"
        );
        assert_eq!(counts(&store)?, (0, 0, 0, 0), "{table}");
        assert_eq!(store.local_asr_job("asr")?.state, "running");
        assert!(store.begin_delete("one", false).is_err());
        drop(store);
        // A failed commit keeps the lease. Restart interrupts the job with a new
        // generation, so the old completion can never publish afterward.
        let mut reopened = Store::open(&path)?;
        reopened
            .connection
            .execute_batch("DROP TRIGGER fixture_fault;")?;
        reopened.recover_analysis_jobs()?;
        assert_eq!(reopened.local_asr_job("asr")?.state, "interrupted");
        assert!(matches!(
            reopened.finish_local_asr(&work, &completed, 22),
            Err(Error::Analysis("stale-worker"))
        ));
        assert_eq!(counts(&reopened)?, (0, 0, 0, 0), "{table}");
    }
    Ok(())
}

#[test]
fn stale_worker_and_cleanup_capabilities_cannot_publish_or_release_lease() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let mut work = admit(&mut store, "asr", 0)?;
    let completed = proof(&work, "speech");
    work.job.generation = 2;
    assert!(matches!(
        store.finish_local_asr(&work, &completed, 21),
        Err(Error::Analysis("stale-worker"))
    ));
    let forged_fixture = proof(&work, "speech");
    assert!(matches!(
        store.finish_local_asr(&work, &forged_fixture, 21),
        Err(Error::Analysis("stale-worker"))
    ));
    work.job.generation = 1;
    work.input.timeline_sha256 = "d".repeat(64);
    assert!(matches!(
        store.finish_local_asr(&work, &completed, 21),
        Err(Error::Analysis("stale-worker"))
    ));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    assert!(store.begin_delete("one", false).is_err());
    Ok(())
}

#[test]
fn failed_worker_publishes_no_text_and_terminal_history_is_stable() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    let work = admit(&mut store, "asr", 0)?;
    let completed = ReapedLocalAsr::synthetic_fixture(
        &work,
        LocalAsrOutcome::Failed(LocalAsrFailure::Deadline),
    );
    let failed = store.finish_local_asr(&work, &completed, 19)?;
    assert_eq!(failed.reason.as_deref(), Some("deadline"));
    assert_eq!(failed.finished_ms, Some(20));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    assert_eq!(store.finish_local_asr(&work, &completed, 99)?, failed);
    drop(store);
    assert_eq!(Store::open(&path)?.local_asr_job("asr")?, failed);
    Ok(())
}

#[test]
fn cue_pagination_bounds_escaped_bytes_and_has_no_skips() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let work = admit(&mut store, "asr", 0)?;
    let mut value = output(&work, "");
    value.cues = (0..16)
        .map(|ordinal| RecognitionCue {
            ordinal,
            start_us: u64::from(ordinal),
            end_us: u64::from(ordinal) + 1,
            script: "\0".repeat(4096),
        })
        .collect();
    let completed =
        ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(value.clone()));
    store.finish_local_asr(&work, &completed, 21)?;
    let mut cursor = None;
    let mut collected = Vec::new();
    loop {
        let page = store.transcript_cues_page("pin", 1, cursor)?;
        assert!(serde_json::to_vec(&page)?.len() <= TRANSCRIPT_PAGE_BYTES);
        assert!(page.cues.len() <= 2);
        assert!(!page.cues.is_empty());
        collected.extend(page.cues);
        cursor = page.next_after_ordinal;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(collected, value.cues);
    assert!(
        store
            .transcript_cues_page("pin", 1, Some(15))?
            .cues
            .is_empty()
    );
    assert!(store.transcript_cues_page("pin", 0, None).is_err());
    assert!(
        store
            .transcript_cues_page("pin", 1, Some(1_000_001))
            .is_err()
    );
    Ok(())
}

#[test]
fn revision_pages_bind_history_and_keep_item_limits() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    for parent in 0..18 {
        let work = admit(&mut store, &format!("asr-{parent}"), parent)?;
        store.finish_local_asr(&work, &proof(&work, ""), 21)?;
    }
    let first = store.transcript_revisions("pin", 0)?;
    assert_eq!(first.revisions.len(), 16);
    assert_eq!(first.next_after_revision, Some(16));
    let rest = store.transcript_revisions("pin", 16)?;
    assert_eq!(
        rest.revisions
            .iter()
            .map(|row| row.revision)
            .collect::<Vec<_>>(),
        [17, 18]
    );
    assert_eq!(rest.next_after_revision, None);
    assert!(store.transcript_revisions("pin", -1).is_err());
    assert!(store.transcript_revisions("pin", 65).is_err());
    Ok(())
}

#[test]
fn replacement_refuses_active_native_work_and_stale_pin_completion_fails_closed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path, false)?;
    let work = admit(&mut store, "asr", 0)?;
    assert!(store.admit_analysis("pin", "one", true, 21).is_err());
    let (replay, pin) = store.admit_analysis("pin", "one", false, 21)?;
    assert_eq!(replay, super::super::analysis::AdmitOutcome::Unchanged);
    assert_eq!(pin.revision, 1);
    assert_eq!(store.local_asr_job("asr")?, work.job);
    // Simulate a newer pin written outside the service API. The replacement
    // transaction must not supersede it while the native lease is active.
    store.connection.execute_batch("INSERT INTO analysis_inputs SELECT id, 2, recording_id, media_sha256, timeline_json, 'admitted', 21 FROM analysis_inputs WHERE id = 'pin' AND revision = 1;")?;
    assert!(matches!(
        store.admit_analysis("pin", "one", true, 22),
        Err(Error::Analysis("native-worker-active"))
    ));
    assert_eq!(store.analysis_input("pin")?.state, "admitted");
    assert_eq!(store.analysis_input("pin")?.revision, 2);
    let completed = proof(&work, "stale result");
    let failed = store.finish_local_asr(&work, &completed, 23)?;
    assert_eq!(failed.reason.as_deref(), Some("input-no-longer-current"));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    let (outcome, replacement) = store.admit_analysis("pin", "one", true, 24)?;
    assert_eq!(outcome, super::super::analysis::AdmitOutcome::Replaced);
    assert_eq!(replacement.revision, 3);
    assert_eq!(store.analysis_revision("pin", 2)?.state, "superseded");
    assert!(store.admit_local_asr(&request("asr", 0), 99)?.1.is_none());
    drop(store);
    assert_eq!(Store::open(&path)?.local_asr_job("asr")?, failed);
    Ok(())
}

#[test]
fn sparse_legacy_cue_ordinals_page_without_truncation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let old = super::super::transcripts::migration_tests::setup_at_version(&path, 25)?;
    old.connection.execute("INSERT INTO transcripts VALUES ('pin', 1, 'pin', 1, 'one', ?1, 'original', 'local-unmeasured', 'published', 11)", ["a".repeat(64)])?;
    for index in 0..18 {
        old.connection.execute(
            "INSERT INTO transcript_cues VALUES ('pin', 1, ?1, ?2, ?3, '', 'uncertain')",
            params![100 + index * 10, index, index + 1],
        )?;
    }
    old.connection.execute(
        "INSERT INTO analysis_decisions VALUES ('pin', 1, 0, NULL, 11)",
        [],
    )?;
    drop(old);
    let store = Store::open(&path)?;
    let first = store.transcript_cues_page("pin", 1, None)?;
    assert_eq!(first.cues.len(), 16);
    assert_eq!(first.next_after_ordinal, Some(250));
    assert_eq!(first.transcript.wording, "uncertain");
    let second = store.transcript_cues_page("pin", 1, first.next_after_ordinal)?;
    assert_eq!(
        second
            .cues
            .iter()
            .map(|cue| cue.ordinal)
            .collect::<Vec<_>>(),
        [260, 270]
    );
    assert_eq!(second.next_after_ordinal, None);
    Ok(())
}

#[test]
fn request_and_input_limits_refuse_before_admission() -> TestResult {
    use crate::{
        sources::HttpHop,
        storage::dvr::{Publication, Retention},
    };

    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    for invalid in ["reserved", "digest", "revision", "parent"] {
        let mut value = request("invalid", 0);
        match invalid {
            "reserved" => value.profile = "local-unmeasured".into(),
            "digest" => value.profile_sha256 = "B".repeat(64),
            "revision" => value.analysis_revision = 0,
            _ => value.parent_revision = 64,
        }
        assert!(matches!(
            store.admit_local_asr(&value, 20),
            Err(Error::InvalidInput(_))
        ));
    }
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        200_000_000,
        67_108_864,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    for (name, bytes, duration) in [("large", 67_108_865, 1_000_000), ("long", 100, 60_000_001)] {
        let recording = store
            .admit_recording(
                name,
                "radio:v1",
                120,
                80_000_000,
                Retention::Temporary,
                false,
            )?
            .ok_or(Error::StorageIntegrity)?;
        store.publish_recording(
            &recording.version,
            &Publication {
                bytes,
                sha256: "d".repeat(64),
                format: "wav",
                decoded_microseconds: duration,
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
        store.admit_analysis(name, name, false, 20)?;
        store.publish_analysis(name, 1)?;
        let mut value = request(name, 0);
        value.analysis_id = name.into();
        assert!(matches!(
            store.admit_local_asr(&value, 21),
            Err(Error::Analysis("recognition-input-limit"))
        ));
    }
    let jobs: u32 =
        store
            .connection
            .query_row("SELECT count(*) FROM analysis_jobs", [], |row| row.get(0))?;
    assert_eq!(jobs, 0);
    Ok(())
}

fn stored_profile(id: &str, threads: u32) -> Result<crate::recognition::RecognitionProfile> {
    let root = std::env::temp_dir();
    let mut profile = crate::recognition::RecognitionProfile {
        id: id.into(),
        engine: crate::recognition::WHISPER_CPP_CLI.into(),
        runtime_dir: root.join("runtime").display().to_string(),
        executable: "whisper-cli.exe".into(),
        runtime_sha256: "1".repeat(64),
        runtime_files: 3,
        runtime_bytes: 300,
        model_path: root.join("model.bin").display().to_string(),
        model_sha256: "2".repeat(64),
        model_bytes: 1000,
        vad_path: root.join("vad.bin").display().to_string(),
        vad_sha256: "3".repeat(64),
        vad_bytes: 10,
        threads,
        memory_bytes: 1 << 30,
        deadline_ms: 60_000,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    Ok(profile)
}

#[test]
fn profiles_are_immutable_exact_replays_and_bounded() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let profile = stored_profile("cpu", 2)?;
    assert!(store.add_recognition_profile(&profile, 5)?);
    assert!(!store.add_recognition_profile(&profile, 6)?);
    assert_eq!(store.recognition_profile("cpu")?, profile);
    assert!(matches!(
        store.add_recognition_profile(&stored_profile("cpu", 3)?, 7),
        Err(Error::IdempotencyConflict)
    ));
    let mut forged = stored_profile("forged", 2)?;
    forged.memory_bytes *= 2;
    assert!(store.add_recognition_profile(&forged, 8).is_err());
    assert!(store.add_recognition_profile(&profile, -1).is_err());
    // The same file hashes and limits under another name is the same identity.
    assert!(
        store
            .add_recognition_profile(&stored_profile("alias", 2)?, 9)
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("UPDATE recognition_profiles SET threads = 8", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM recognition_profiles", [])
            .is_err()
    );
    assert!(matches!(
        store.recognition_profile("absent"),
        Err(Error::NotFound)
    ));
    assert_eq!(store.recognition_profiles()?.len(), 1);
    Ok(())
}

mod translation {
    //! Translation storage fixtures over a published recognition transcript.
    use super::*;
    use crate::translation::{
        TranslatedCue, TranslationOutcome, TranslationProfile, TranslationRequest,
        TranslationResult,
    };

    fn profile() -> Result<TranslationProfile> {
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
            languages: "ar,es,fr".into(),
            threads: 2,
            memory_bytes: 1 << 30,
            cue_deadline_ms: 60_000,
            profile_sha256: String::new(),
        };
        profile.profile_sha256 = profile.identity()?;
        Ok(profile)
    }

    fn transcribed(path: &Path) -> Result<Store> {
        let mut store = setup(path, false)?;
        let work = admit(&mut store, "asr", 0)?;
        store.finish_local_asr(&work, &proof(&work, "Una feria mundial"), 21)?;
        store.add_translation_profile(&profile()?, 22)?;
        Ok(store)
    }

    fn request(id: &str) -> Result<TranslationRequest> {
        let profile = profile()?;
        Ok(TranslationRequest {
            id: id.into(),
            transcript_id: "pin".into(),
            transcript_revision: 1,
            profile: profile.id,
            profile_sha256: profile.profile_sha256,
        })
    }

    fn translated(ordinal: u32, text: &str) -> TranslatedCue {
        TranslatedCue {
            ordinal,
            state: "translated".into(),
            english: Some(text.into()),
            reason: None,
        }
    }

    #[test]
    fn a_translation_maps_each_source_cue_once_with_zero_cost_and_replays() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = transcribed(&directory.path().join("catalog"))?;
        let (job, work) = store.admit_translation(&request("mt-1")?, 30)?;
        let work = work.ok_or("fresh admission has work")?;
        assert_eq!(work.cues.len(), 1);
        assert_eq!(work.cues[0].script, "Una feria mundial");
        assert!(store.admit_translation(&request("mt-1")?, 31)?.1.is_none());
        assert!(matches!(
            store.admit_translation(&request("mt-2")?, 31),
            Err(Error::Analysis("worker-busy"))
        ));
        let outcome = TranslationOutcome::synthetic_fixture(
            &job,
            TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
        );
        assert_eq!(
            store.finish_translation(&work, &outcome, 32)?.state,
            "succeeded"
        );
        let page = store.translation_page("pin", 1, 1, None)?;
        assert_eq!(page.pairs[0].original, "Una feria mundial");
        assert_eq!(page.pairs[0].english.as_deref(), Some("A world fair"));
        assert_eq!((page.cue_count, page.translated_count), (1, 1));
        assert_eq!(page.amount_usd, "0.000000");
        assert_eq!(
            store.finish_translation(&work, &outcome, 33)?.state,
            "succeeded"
        );
        assert_eq!(store.latest_translation_revision("pin", 1)?, 1);
        assert!(
            store
                .connection
                .execute("UPDATE translation_cues SET english = 'changed'", [])
                .is_err()
        );
        assert!(
            store
                .connection
                .execute("DELETE FROM translations", [])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn misaligned_or_hostile_results_fail_without_rows() -> TestResult {
        let cases = [
            Vec::new(),
            vec![translated(1, "wrong ordinal")],
            vec![translated(0, "one"), translated(1, "extra")],
            vec![translated(0, "")],
            vec![translated(0, &"x".repeat(4097))],
            vec![translated(0, "nul\0byte")],
            vec![TranslatedCue {
                ordinal: 0,
                state: "untranslated".into(),
                english: Some("both".into()),
                reason: Some("deadline".into()),
            }],
            vec![TranslatedCue {
                ordinal: 0,
                state: "partial".into(),
                english: None,
                reason: None,
            }],
        ];
        for (index, cues) in cases.into_iter().enumerate() {
            let directory = tempfile::tempdir()?;
            let mut store = transcribed(&directory.path().join("catalog"))?;
            let (job, work) = store.admit_translation(&request("mt")?, 30)?;
            let work = work.ok_or("work")?;
            let outcome =
                TranslationOutcome::synthetic_fixture(&job, TranslationResult::Succeeded(cues));
            let finished = store.finish_translation(&work, &outcome, 31)?;
            assert_eq!(
                (finished.state.as_str(), finished.reason.as_deref()),
                ("failed", Some("invalid-worker-output")),
                "case {index}"
            );
            assert_eq!(
                store.latest_translation_revision("pin", 1)?,
                0,
                "case {index}"
            );
        }
        Ok(())
    }

    #[test]
    fn untranslated_cues_keep_reasons_and_cancellation_wins() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = transcribed(&directory.path().join("catalog"))?;
        let (job, work) = store.admit_translation(&request("mt")?, 30)?;
        let work = work.ok_or("work")?;
        let skipped = TranslationOutcome::synthetic_fixture(
            &job,
            TranslationResult::Succeeded(vec![TranslatedCue {
                ordinal: 0,
                state: "untranslated".into(),
                english: None,
                reason: Some("unsupported-language".into()),
            }]),
        );
        store.finish_translation(&work, &skipped, 31)?;
        let page = store.translation_page("pin", 1, 1, None)?;
        assert_eq!(page.translated_count, 0);
        assert_eq!(
            page.pairs[0].reason.as_deref(),
            Some("unsupported-language")
        );

        let (job, work) = store.admit_translation(&request("mt-cancel")?, 40)?;
        let work = work.ok_or("work")?;
        assert_eq!(
            store.cancel_translation("mt-cancel", 1)?.state,
            "cancelling"
        );
        let late = TranslationOutcome::synthetic_fixture(
            &job,
            TranslationResult::Succeeded(vec![translated(0, "late")]),
        );
        assert_eq!(
            store.finish_translation(&work, &late, 41)?.state,
            "cancelled"
        );
        assert_eq!(store.latest_translation_revision("pin", 1)?, 1);
        Ok(())
    }

    #[test]
    fn restart_interrupts_and_a_stale_worker_cannot_publish() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = transcribed(&path)?;
        let (job, work) = store.admit_translation(&request("mt")?, 30)?;
        let work = work.ok_or("work")?;
        drop(store);
        let mut reopened = Store::open(&path)?;
        reopened.recover_translation_jobs()?;
        let recovered = reopened.translation_job("mt")?;
        assert_eq!(
            (recovered.state.as_str(), recovered.generation),
            ("interrupted", 2)
        );
        let outcome = TranslationOutcome::synthetic_fixture(
            &job,
            TranslationResult::Succeeded(vec![translated(0, "stale")]),
        );
        assert!(matches!(
            reopened.finish_translation(&work, &outcome, 31),
            Err(Error::Analysis("stale-worker"))
        ));
        assert_eq!(reopened.latest_translation_revision("pin", 1)?, 0);
        Ok(())
    }

    #[test]
    fn only_recognized_text_can_be_translated() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog"), true)?;
        store.add_translation_profile(&profile()?, 22)?;
        assert!(matches!(
            store.admit_translation(&request("mt")?, 30),
            Err(Error::Analysis("transcript-has-no-recognized-text"))
        ));
        Ok(())
    }
}
