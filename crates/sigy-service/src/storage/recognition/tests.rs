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
    let coverages = work
        .input
        .chunks
        .iter()
        .map(|chunk| {
            let duration = chunk.end_us.saturating_sub(chunk.start_us);
            let samples = duration.saturating_mul(16_000) / 1_000_000;
            RecognitionCoverage {
                ordinal: chunk.ordinal,
                interval_ordinal: chunk.interval_ordinal,
                start_us: chunk.start_us,
                end_us: chunk.end_us,
                source_sha256: chunk.source_sha256.clone(),
                decoded_sha256: "c".repeat(64),
                sample_rate: 16_000,
                sample_count: samples.max(1),
            }
        })
        .collect();
    let cues = match (script.is_empty(), work.input.chunks.first()) {
        (false, Some(chunk)) => vec![RecognitionCue {
            ordinal: 0,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            script: script.into(),
        }],
        _ => Vec::new(),
    };
    RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages,
        cues,
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
    // Other jobs on the same transcript lineage queue behind the running one.
    let (other, dispatch) = store.admit_local_asr(&request("other", 0), 21)?;
    assert!(dispatch.is_none());
    assert_eq!(other.state, "queued");
    let (verify, dispatch) = store.admit_verification("verify-other", "pin", 1, 21)?;
    assert!(dispatch.is_none());
    assert_eq!(verify.state, "queued");
    assert!(matches!(
        store.admit_verification("asr", "pin", 1, 21),
        Err(Error::IdempotencyConflict)
    ));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    assert_eq!(work.input.byte_length()?, 100);
    assert_eq!(work.job.manifest_sha256, manifest(&work.input)?);
    Ok(())
}

#[test]
fn completed_recognition_reports_its_own_pace() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    assert!(matches!(
        store.recognition_pace()?,
        crate::storage::recognition::RecognitionPace::Unmeasured
    ));
    let work = admit(&mut store, "asr", 0)?;
    let job = store.finish_local_asr(&work, &proof(&work, "heard"), 21)?;
    assert_eq!(job.state, "succeeded");
    let pace = store.recognition_pace()?;
    let text = crate::storage::recognition::describe(&pace);
    let crate::storage::recognition::RecognitionPace::Observed {
        profiles,
        hidden_profiles,
        newest_limited,
    } = pace
    else {
        return Err("completed recognition stayed unmeasured".into());
    };
    assert!(!newest_limited);
    assert_eq!(hidden_profiles, 0);
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].profile, "synthetic-storage-fixture-v1");
    assert_eq!(profiles[0].jobs, 1);
    assert_eq!(profiles[0].wall_ms_per_audio_second, 1);
    assert_eq!(
        text,
        "synthetic-storage-fixture-v1: 1 job, 1 ms wall per audio second. Observed on this library."
    );
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
    assert!(!silence.coverages.is_empty());
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
            "source" => value.coverages[0].source_sha256 = "d".repeat(64),
            "decoded" => value.coverages[0].decoded_sha256 = "invalid".into(),
            "coverage" => value.coverages[0].end_us += 1,
            "clock" => value.coverages[0].sample_count -= 2,
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
        let expected = if crate::storage::job_pool::NATIVE_REQUEUE {
            "queued"
        } else {
            "interrupted"
        };
        assert_eq!(reopened.local_asr_job("asr")?.state, expected);
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

fn publish_wav(store: &mut Store, id: &str, bytes: u64, sha: String, decoded: u64) -> Result<()> {
    let recording = store
        .admit_recording(
            id,
            "radio:v1",
            120,
            80_000_000,
            super::super::dvr::Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    let mut publication = minute_publication(bytes, decoded, "wav");
    publication.sha256 = sha;
    store.publish_recording(&recording.version, &publication)?;
    store.admit_analysis(id, id, false, 20)?;
    store.publish_analysis(id, 1)?;
    Ok(())
}

#[test]
fn request_and_input_limits_refuse_before_admission() -> TestResult {
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
    publish_wav(&mut store, "large", 67_108_865, "d".repeat(64), 1_000_000)?;
    let mut large = request("large", 0);
    large.analysis_id = "large".into();
    assert!(matches!(
        store.admit_local_asr(&large, 21),
        Err(Error::Analysis("recognition-input-limit"))
    ));
    publish_wav(&mut store, "long", 100, "e".repeat(64), 60_000_001)?;
    let mut value = request("long", 0);
    value.analysis_id = "long".into();
    let work = store
        .admit_local_asr(&value, 21)?
        .1
        .ok_or(Error::StorageIntegrity)?;
    assert_eq!(work.input.chunks.len(), 3);
    assert_eq!(work.input.segments.len(), 1);
    let files: i64 = store.connection.query_row(
        "SELECT expected_files FROM analysis_jobs WHERE id = 'long'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(files, 1);
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

#[test]
fn a_ninety_second_file_publishes_three_chunks_and_skips_a_silent_span() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let recording = store
        .admit_recording(
            "show",
            "radio:v1",
            90,
            1_000,
            super::super::dvr::Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    store.publish_recording(
        &recording.version,
        &minute_publication(1_000, 90_000_000, "wav"),
    )?;
    store.admit_analysis("show", "show", false, 20)?;
    store.publish_analysis("show", 1)?;
    let mut request = request("show", 0);
    request.analysis_id = "show".into();
    let work = store
        .admit_local_asr(&request, 21)?
        .1
        .ok_or(Error::StorageIntegrity)?;
    assert_eq!(work.input.chunks.len(), 3);
    let mut value = output(&work, "bonjour");
    let third = work.input.chunks.get(2).ok_or(Error::StorageIntegrity)?;
    value.cues.push(RecognitionCue {
        ordinal: 1,
        start_us: third.start_us,
        end_us: third.end_us,
        script: "monde".into(),
    });
    let job = store.finish_local_asr(
        &work,
        &ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(value)),
        22,
    )?;
    assert_eq!(job.amount_usd, "0.000000");
    assert_eq!(counts(&store)?, (1, 2, 3, 1));
    let page = store.transcript_cues_page("show", 1, None)?;
    assert_eq!(page.transcript.outcome, "text");
    assert_eq!(page.coverages.len(), 3);
    let evidence = crate::recognizer::language_evidence(
        &job,
        &[
            crate::recognition::ChunkLanguage {
                ordinal: 0,
                interval_ordinal: work.input.chunks[0].interval_ordinal,
                start_us: work.input.chunks[0].start_us,
                end_us: work.input.chunks[0].end_us,
                code: "es".into(),
            },
            crate::recognition::ChunkLanguage {
                ordinal: 2,
                interval_ordinal: third.interval_ordinal,
                start_us: third.start_us,
                end_us: third.end_us,
                code: "fr".into(),
            },
        ],
    );
    store.publish_language_evidence(evidence, 23)?;
    let stored = store.language_evidence("show", 1)?;
    assert_eq!(stored.spans.len(), 2);
    assert_eq!(stored.spans[0].end_us, work.input.chunks[0].end_us);
    assert_eq!(stored.spans[1].start_us, third.start_us);
    assert_eq!(stored.spans[1].ordinal, 1);
    let paid: i64 = store
        .connection
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))?;
    assert_eq!(paid, 0);
    Ok(())
}

#[test]
fn a_cue_across_two_chunks_publishes_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let recording = store
        .admit_recording(
            "show",
            "radio:v1",
            90,
            1_000,
            super::super::dvr::Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    store.publish_recording(
        &recording.version,
        &minute_publication(1_000, 90_000_000, "wav"),
    )?;
    store.admit_analysis("show", "show", false, 20)?;
    store.publish_analysis("show", 1)?;
    let mut request = request("show", 0);
    request.analysis_id = "show".into();
    let work = admit_named(&mut store, &request)?;
    let mut value = output(&work, "");
    value.cues.push(RecognitionCue {
        ordinal: 0,
        start_us: 29_000_000,
        end_us: 31_000_000,
        script: "cross".into(),
    });
    let job = store.finish_local_asr(
        &work,
        &ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(value)),
        22,
    )?;
    assert_eq!(job.reason.as_deref(), Some("invalid-worker-result"));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    Ok(())
}

fn admit_named(store: &mut Store, request: &LocalAsrRequest) -> Result<LocalAsrWork> {
    store
        .admit_local_asr(request, 21)?
        .1
        .ok_or(Error::StorageIntegrity)
}

fn admit_show(store: &mut Store, seconds: u64, decoded_us: u64) -> Result<LocalAsrWork> {
    let recording = store
        .admit_recording(
            "show",
            "radio:v1",
            seconds,
            1_000,
            super::super::dvr::Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    store.publish_recording(
        &recording.version,
        &minute_publication(1_000, decoded_us, "wav"),
    )?;
    store.admit_analysis("show", "show", false, 20)?;
    store.publish_analysis("show", 1)?;
    let mut request = request("show", 0);
    request.analysis_id = "show".into();
    admit_named(store, &request)
}

fn coverage_row(
    ordinal: u32,
    start_us: u64,
    end_us: u64,
    samples: u64,
    source: &str,
) -> RecognitionCoverage {
    RecognitionCoverage {
        ordinal,
        interval_ordinal: 0,
        start_us,
        end_us,
        source_sha256: source.to_owned(),
        decoded_sha256: "c".repeat(64),
        sample_rate: 16_000,
        sample_count: samples,
    }
}

fn finish_custom(
    store: &mut Store,
    work: &LocalAsrWork,
    coverages: Vec<RecognitionCoverage>,
    cues: Vec<RecognitionCue>,
) -> Result<crate::recognition::LocalAsrJob> {
    let value = RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages,
        cues,
    };
    store.finish_local_asr(
        work,
        &ReapedLocalAsr::synthetic_fixture(work, LocalAsrOutcome::Succeeded(value)),
        22,
    )
}

#[test]
fn a_phrase_boundary_can_publish_more_coverages_than_the_plan() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let work = admit_show(&mut store, 60, 60_000_000)?;
    assert_eq!(work.input.chunks.len(), 2);
    let source = work.input.segments[0].source_sha256.clone();
    let coverages = vec![
        coverage_row(0, 0, 22_000_000, 352_000, &source),
        coverage_row(1, 22_000_000, 52_000_000, 480_000, &source),
        coverage_row(2, 52_000_000, 60_000_000, 128_000, &source),
    ];
    let cues = vec![
        RecognitionCue {
            ordinal: 0,
            start_us: 0,
            end_us: 1_000_000,
            script: "one".into(),
        },
        RecognitionCue {
            ordinal: 1,
            start_us: 22_000_000,
            end_us: 23_000_000,
            script: "two".into(),
        },
        RecognitionCue {
            ordinal: 2,
            start_us: 52_000_000,
            end_us: 53_000_000,
            script: "three".into(),
        },
    ];
    let job = finish_custom(&mut store, &work, coverages, cues)?;
    assert_eq!(job.state, "succeeded");
    assert_eq!(counts(&store)?, (1, 3, 3, 1));
    let page = store.transcript_cues_page("show", 1, None)?;
    assert_eq!(page.coverages[0].end_us, 22_000_000);
    assert_eq!(page.coverages[1].end_us, 52_000_000);
    assert_eq!(page.coverages[2].sample_count, 128_000);
    Ok(())
}

#[test]
fn a_cue_across_a_published_phrase_cut_publishes_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let work = admit_show(&mut store, 60, 60_000_000)?;
    let source = work.input.segments[0].source_sha256.clone();
    let rows = vec![
        coverage_row(0, 0, 22_000_000, 352_000, &source),
        coverage_row(1, 22_000_000, 52_000_000, 480_000, &source),
        coverage_row(2, 52_000_000, 60_000_000, 128_000, &source),
    ];
    let crossed = finish_custom(
        &mut store,
        &work,
        rows,
        vec![RecognitionCue {
            ordinal: 0,
            start_us: 21_500_000,
            end_us: 22_500_000,
            script: "cross".into(),
        }],
    )?;
    assert_eq!(crossed.reason.as_deref(), Some("invalid-worker-result"));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    Ok(())
}

#[test]
fn a_gap_between_published_coverages_publishes_nothing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    let work = admit_show(&mut store, 60, 60_000_000)?;
    let source = work.input.segments[0].source_sha256.clone();
    let gapped = finish_custom(
        &mut store,
        &work,
        vec![
            coverage_row(0, 0, 22_000_000, 352_000, &source),
            coverage_row(1, 30_000_000, 60_000_000, 480_000, &source),
        ],
        Vec::new(),
    )?;
    assert_eq!(gapped.reason.as_deref(), Some("invalid-worker-result"));
    assert_eq!(counts(&store)?, (0, 0, 0, 0));
    Ok(())
}

#[test]
fn abutting_segments_stay_two_chunks() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    publish_two_segments(&mut store)?;
    let mut request = request("roll", 0);
    request.analysis_id = "roll".into();
    let work = admit_named(&mut store, &request)?;
    assert_eq!(work.input.segments.len(), 2);
    assert_eq!(work.input.chunks.len(), 2);
    assert_eq!(work.input.chunks[0].interval_ordinal, 0);
    assert_eq!(work.input.chunks[1].interval_ordinal, 1);
    assert_eq!(work.input.chunks[0].end_us, work.input.chunks[1].start_us);
    let files: i64 = store.connection.query_row(
        "SELECT expected_files FROM analysis_jobs WHERE id = 'roll'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(files, 2);
    let job = store.finish_local_asr(&work, &proof(&work, ""), 22)?;
    assert_eq!(job.state, "succeeded");
    assert_eq!(
        store.transcript_cues_page("roll", 1, None)?.coverages.len(),
        2
    );
    Ok(())
}

fn publish_two_segments(store: &mut Store) -> Result<()> {
    use crate::storage::dvr::{SegmentOpen, SegmentSeal, hex};
    use sha2::{Digest, Sha256};
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        200_000_000,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    let job = store
        .admit_recording(
            "roll",
            "radio:v1",
            60,
            80_000_000,
            super::super::dvr::Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    let mut current = store.connect_recording(&job.version)?;
    let mut joined = Vec::new();
    for bytes in [b"aaaa".as_slice(), b"bbbb".as_slice()] {
        let SegmentOpen::Opened { version, .. } = store.open_segment(&current)? else {
            return Err(Error::StorageIntegrity);
        };
        joined.extend_from_slice(bytes);
        current = store.seal_segment(
            &version,
            &SegmentSeal {
                bytes: u64::try_from(bytes.len()).map_err(|_| Error::StorageIntegrity)?,
                sha256: hex(&Sha256::digest(bytes)),
                format: "wav",
                decoded_microseconds: 5_000_000,
            },
        )?;
    }
    let bytes = u64::try_from(joined.len()).map_err(|_| Error::StorageIntegrity)?;
    store.publish_recording(
        &current,
        &super::super::dvr::Publication {
            bytes,
            sha256: hex(&Sha256::digest(&joined)),
            format: "wav",
            decoded_microseconds: 10_000_000,
            end_reason: "end_of_body",
            http_route: minute_publication(bytes, 10_000_000, "wav").http_route,
            observations: Vec::new(),
            segments_sealed: true,
            gap: None,
        },
    )?;
    store.admit_analysis("roll", "roll", false, 20)?;
    store.publish_analysis("roll", 1)?;
    Ok(())
}

fn minute_publication(
    bytes: u64,
    decoded: u64,
    format: &'static str,
) -> super::super::dvr::Publication {
    use crate::sources::HttpHop;
    super::super::dvr::Publication {
        bytes,
        sha256: "d".repeat(64),
        format,
        decoded_microseconds: decoded,
        end_reason: "end_of_body",
        http_route: vec![HttpHop {
            origin: "https://example.com".into(),
            peer: ([8, 8, 8, 8], 443).into(),
            status: 200,
        }],
        observations: Vec::new(),
        segments_sealed: false,
        gap: None,
    }
}

#[test]
fn migration_keeps_a_full_minute_and_reopens_only_the_limit_skip() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    write_v33(&path)?;
    let mut store = Store::open(&path)?;
    let version: i64 = store
        .connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(version, i64::from(super::super::SCHEMA_VERSION));
    let ordinal: i64 = store.connection.query_row(
        "SELECT ordinal FROM transcript_coverage WHERE transcript_id = 'long'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(ordinal, 0);
    let table: String = store.connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE name = 'transcript_coverage'",
        [],
        |row| row.get(0),
    )?;
    assert!(table.contains("60000000"), "{table}");
    let trigger: String = store.connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE name = 'transcript_coverage_insert'",
        [],
        |row| row.get(0),
    )?;
    assert!(trigger.contains("30000000"), "{trigger}");
    assert!(store.claim_local_asr("stale-plan", "owner", 40)?.is_none());
    let stale = store.local_asr_job("stale-plan")?;
    assert_eq!(stale.reason.as_deref(), Some("input-no-longer-current"));
    assert_eq!(stale.manifest_sha256, "f".repeat(64));
    let mut statement = store
        .connection
        .prepare("SELECT reason FROM monitor_steps ORDER BY recording_id")?;
    let skips = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(skips, vec!["longer-than-daily-cap".to_owned()]);
    Ok(())
}

#[test]
fn an_interrupted_chunk_migration_stays_at_version_33() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    write_v33(&path)?;
    break_job_check(&path)?;
    assert!(Store::open(&path).is_err());
    let connection = rusqlite::Connection::open(&path)?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(version, 33);
    let rows: i64 =
        connection.query_row("SELECT count(*) FROM transcript_coverage", [], |row| {
            row.get(0)
        })?;
    assert!(rows >= 1);
    Ok(())
}

fn write_v33(path: &Path) -> Result<()> {
    let mut store = super::super::transcripts::migration_tests::setup_at_version(path, 25)?;
    store.connection.execute(
        "INSERT INTO transcripts VALUES ('pin', 1, 'pin', 1, 'one', ?1, 'original', 'local-unmeasured', 'published', 11)",
        ["a".repeat(64)],
    )?;
    store.connection.execute_batch("INSERT INTO transcript_cues VALUES ('pin', 1, 0, 0, 1000000, '', 'uncertain'); INSERT INTO analysis_decisions VALUES ('pin', 1, 0, NULL, 11);")?;
    let sha = publish_full_minute(&mut store)?;
    advance_to_v33(&mut store)?;
    insert_full_minute(&mut store, &sha)?;
    insert_stale_and_skips(&mut store)?;
    Ok(())
}

fn publish_full_minute(store: &mut Store) -> Result<String> {
    let recording = store
        .admit_recording(
            "long",
            "radio:v1",
            60,
            80,
            super::super::dvr::Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    store.publish_recording(
        &recording.version,
        &minute_publication(80, 60_000_000, "wav"),
    )?;
    store.admit_analysis("long", "long", false, 12)?;
    store.publish_analysis("long", 1)?;
    let (end, sha): (i64, String) = store.connection.query_row(
        "SELECT decoded_end_us, sha256 FROM recording_intervals WHERE recording_id = 'long'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let gaps: i64 = store.connection.query_row(
        "SELECT count(*) FROM recording_gaps WHERE recording_id = 'long'",
        [],
        |row| row.get(0),
    )?;
    if gaps != 0 || end != 60_000_000 {
        return Err(Error::StorageIntegrity);
    }
    Ok(sha)
}

fn advance_to_v33(store: &mut Store) -> Result<()> {
    let tx = store.connection.transaction()?;
    tx.execute_batch(include_str!("../026-transcript-revisions.sql"))?;
    tx.execute_batch(include_str!("../026-transcript-invariants.sql"))?;
    super::super::transcripts::audit_single_interval(&tx)?;
    super::super::analysis_jobs::audit(&tx)?;
    tx.execute_batch(include_str!("../027-recognition-profiles.sql"))?;
    tx.execute_batch(include_str!("../028-provider-routes.sql"))?;
    tx.execute_batch(include_str!("../029-translations.sql"))?;
    super::super::widen::migrate_030(&tx)?;
    tx.execute_batch(include_str!("../030-live-hls.sql"))?;
    super::super::job_pool::migrate_031(&tx)?;
    tx.execute_batch(include_str!("../032-monitors.sql"))?;
    tx.execute_batch(include_str!("../033-monitor-steps.sql"))?;
    tx.commit()?;
    Ok(())
}

fn insert_full_minute(store: &mut Store, sha: &str) -> Result<()> {
    let tx = store.connection.transaction()?;
    tx.execute("INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage, attempt) VALUES ('minute', 1, 'long', 1, 'long', 'fixture-profile', 'local_asr', ?1, 0, 'queued', 80, 1, ?2, 0, 20, 'long', 1)", params!["b".repeat(64), "c".repeat(64)])?;
    tx.execute("UPDATE analysis_jobs SET state = 'running', lease_owner = 'local-test', lease_expires_ms = 20, started_ms = 20 WHERE id = 'minute'", [])?;
    tx.execute("INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, kind, outcome, parent_revision, job_id, job_generation, profile_sha256, cue_count, text_bytes, state, created_ms) VALUES ('long', 1, 'long', 1, 'long', ?1, 'original', 'fixture-profile', 'recognition', 'no_text', NULL, 'minute', 1, ?2, 0, 0, 'published', 21)", params!["d".repeat(64), "b".repeat(64)])?;
    tx.execute("INSERT INTO transcript_coverage(transcript_id, revision, interval_ordinal, start_us, end_us, source_sha256, decoded_sha256, sample_rate, sample_count) VALUES ('long', 1, 0, 0, 60000000, ?1, ?2, 16000, 960000)", params![sha, "c".repeat(64)])?;
    tx.execute(
        "INSERT INTO analysis_decisions VALUES ('long', 1, 0, NULL, 21)",
        [],
    )?;
    tx.execute(
        "UPDATE analysis_jobs SET state = 'succeeded', finished_ms = 22 WHERE id = 'minute'",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

fn insert_stale_and_skips(store: &mut Store) -> Result<()> {
    store.connection.execute("INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage, attempt) VALUES ('stale-plan', 1, 'pin', 1, 'one', 'fixture-profile', 'local_asr', ?1, 1, 'queued', 100, 1, ?2, 0, 30, 'pin', 1)", params!["b".repeat(64), "f".repeat(64)])?;
    store.connection.execute(
        "INSERT INTO monitors(id, created_ms) VALUES ('desk', 1)",
        [],
    )?;
    store.connection.execute("INSERT INTO monitor_versions(monitor_id, version, spec_json, spec_sha256, created_ms) VALUES ('desk', 1, '{}', ?1, 1)", params!["a".repeat(64)])?;
    store.connection.execute("INSERT INTO monitor_steps(monitor_id, recording_id, stage, policy_version, decision, reason, analysis_id, job_id, audio_us, charged_day, created_ms) VALUES ('desk', 'one', 'recognition', 1, 'skipped', 'recognition-input-limit', NULL, NULL, 0, 0, 1)", [])?;
    store.connection.execute("INSERT INTO monitor_steps(monitor_id, recording_id, stage, policy_version, decision, reason, analysis_id, job_id, audio_us, charged_day, created_ms) VALUES ('desk', 'long', 'recognition', 1, 'skipped', 'longer-than-daily-cap', NULL, NULL, 0, 0, 2)", [])?;
    Ok(())
}

fn break_job_check(path: &Path) -> Result<()> {
    let connection = rusqlite::Connection::open(path)?;
    connection.pragma_update(None, "writable_schema", true)?;
    let changed = connection.execute(
        "UPDATE sqlite_schema SET sql = replace(sql, 'expected_files = 1 AND expected_bytes <= 67108864', 'expected_files = 9 AND expected_bytes <= 67108864') WHERE name = 'analysis_jobs'",
        [],
    )?;
    if changed != 1 {
        return Err(Error::StorageIntegrity);
    }
    connection.pragma_update(None, "writable_schema", false)?;
    Ok(())
}

mod translation {
    //! Translation storage fixtures over a published recognition transcript.
    use super::*;

    mod monitor_coverage;
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
        let (queued, dispatch) = store.admit_translation(&request("mt-2")?, 31)?;
        assert!(dispatch.is_none());
        assert_eq!(queued.state, "queued");
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
        let expected = if crate::storage::job_pool::NATIVE_REQUEUE {
            ("queued", 2, 2)
        } else {
            ("interrupted", 2, 1)
        };
        assert_eq!(
            (
                recovered.state.as_str(),
                recovered.generation,
                recovered.attempt
            ),
            expected
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
