use std::path::Path;

use sha2::{Digest, Sha256};

use super::*;
use crate::{
    domain::money::Usd,
    library::Library,
    sources::{HttpHop, HttpSource, NetworkScope},
    storage::dvr::{OPEN_SEGMENT_CEILING, Publication, Retention, SegmentOpen, SegmentSeal, hex},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

const AUDIO: &[u8] = b"retained verification fixture";

fn initialize(store: &mut Store) -> Result<()> {
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        10_000,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    store.register_source(
        "radio:v1",
        &HttpSource::new(
            "Fixture",
            "https://example.com/audio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    publish_recording(store, "one")?;
    store.admit_analysis("pin", "one", false, 10)?;
    store.publish_analysis("pin", 1)?;
    Ok(())
}

fn publish_recording(store: &mut Store, id: &str) -> Result<()> {
    let job = store
        .admit_recording(id, "radio:v1", 60, 600, Retention::Temporary, false)?
        .ok_or(Error::StorageIntegrity)?;
    store.publish_recording(
        &job.version,
        &Publication {
            bytes: AUDIO.len() as u64,
            sha256: hex(&Sha256::digest(AUDIO)),
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
    )
}

fn setup(path: &Path) -> Result<Store> {
    let mut store = Store::open(path)?;
    initialize(&mut store)?;
    Ok(store)
}

fn receipt(job: &AnalysisJob) -> VerificationReceipt {
    VerificationReceipt {
        bytes: job.expected_bytes,
        files: job.expected_files,
        manifest_sha256: job.manifest_sha256.clone(),
    }
}

fn assert_no_paid_work(store: &Store) -> Result<()> {
    store.audit_ledger()?;
    let counts: (i64, i64, i64) = store.connection.query_row(
        "SELECT (SELECT count(*) FROM requests), (SELECT coalesce(sum(reserved_micros), 0) FROM budgets), (SELECT coalesce(sum(settled_micros), 0) FROM budgets)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(counts, (0, 0, 0));
    let nonzero: bool = store.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM analysis_jobs WHERE amount_micros != 0)",
        [],
        |row| row.get(0),
    )?;
    assert!(!nonzero);
    Ok(())
}

fn assert_lease(store: &mut Store) -> Result<()> {
    assert!(store.begin_delete("one", false).is_err());
    assert!(store.begin_delete("one", true).is_err());
    assert!(store.prune_candidates(true)?.is_empty());
    assert!(store.prune_candidates(false)?.is_empty());
    let aged = Store::clock_ms()? + 15 * 86_400_000;
    assert!(store.next_segment_release(true, aged)?.is_none());
    assert!(store.next_segment_release(false, aged)?.is_none());
    assert_eq!(store.recording("one")?.storage_state, "retained");
    Ok(())
}

#[test]
fn admission_replay_conflict_and_busy_preserve_one_dispatch_and_zero_cost() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
    let original_budget = store.budget("global")?;
    assert_eq!(original_budget.limit(), Usd::ZERO);
    let (job, input) = store.admit_verification("verify", "pin", 1, 20)?;
    let input = input.ok_or("new job did not return work")?;
    assert_eq!((input.bytes, input.files.len()), (AUDIO.len() as u64, 1));
    assert_eq!(job.state, "running");
    assert_eq!(job.amount_usd, "0.000000");
    let (replay, dispatch) = store.admit_verification("verify", "pin", 1, 21)?;
    assert_eq!(replay, job);
    assert!(dispatch.is_none());
    for (pin, revision) in [("pin", 2), ("different", 1)] {
        assert!(matches!(
            store.admit_verification("verify", pin, revision, 21),
            Err(Error::IdempotencyConflict)
        ));
    }
    assert!(matches!(
        store.admit_verification("second", "pin", 1, 21),
        Err(Error::Analysis("worker-busy"))
    ));
    assert!(matches!(store.analysis_job("second"), Err(Error::NotFound)));
    assert!(store.finish_verification("verify", job.generation, Ok(receipt(&job)), 22)?);
    let verified = store.analysis_job("verify")?;
    assert_eq!(verified.state, "verified");
    assert_eq!(verified.verified_bytes, Some(input.bytes));
    assert_eq!(verified.finished_ms, Some(22));
    assert_eq!(
        store.cancel_analysis_job("verify", job.generation)?,
        verified
    );
    assert!(!store.finish_verification("verify", job.generation, Ok(receipt(&job)), 23)?);
    assert_eq!(store.budget("global")?, original_budget);
    assert_no_paid_work(&store)?;
    Ok(())
}

#[test]
fn concurrent_admission_cannot_create_two_active_readers() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let store = setup(&path)?;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut workers = Vec::new();
    for id in ["left", "right"] {
        let mut contender = Store::open(&path)?;
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            contender.admit_verification(id, "pin", 1, 20)
        }));
    }
    let mut admitted = 0;
    let mut refused = 0;
    for worker in workers {
        match worker.join().map_err(|_| "admission thread panicked")? {
            Ok((_, Some(_))) => admitted += 1,
            Err(Error::Analysis("worker-busy")) => refused += 1,
            other => return Err(format!("unexpected admission: {other:?}").into()),
        }
    }
    assert_eq!((admitted, refused), (1, 1));
    let rows: i64 =
        store
            .connection
            .query_row("SELECT count(*) FROM analysis_jobs", [], |row| row.get(0))?;
    assert_eq!(rows, 1);
    assert_no_paid_work(&store)?;
    Ok(())
}

#[test]
fn accepted_cancellation_wins_over_queued_success_and_keeps_the_lease() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
    let (job, _) = store.admit_verification("verify", "pin", 1, 20)?;
    store.acknowledge_processing("one", "cleanup-request")?;
    assert!(matches!(
        store.cancel_analysis_job("verify", job.generation + 1),
        Err(Error::Analysis("stale-worker"))
    ));
    assert_eq!(store.analysis_job("verify")?, job);
    let cancelling = store.cancel_analysis_job("verify", job.generation)?;
    assert_eq!(cancelling.state, "cancelling");
    assert!(cancelling.finished_ms.is_none());
    assert_eq!(
        store.cancel_analysis_job("verify", job.generation)?,
        cancelling
    );
    assert_lease(&mut store)?;
    assert!(store.finish_verification("verify", job.generation, Ok(receipt(&job)), 21)?);
    let cancelled = store.analysis_job("verify")?;
    assert_eq!(cancelled.state, "cancelled");
    assert_eq!(cancelled.reason.as_deref(), Some("cancelled"));
    assert!(cancelled.verified_bytes.is_none());
    assert_eq!(store.prune_candidates(false)?, vec!["one"]);
    assert_eq!(
        store.cancel_analysis_job("verify", job.generation)?,
        cancelled
    );
    assert_no_paid_work(&store)?;
    Ok(())
}

#[test]
fn restart_invalidates_workers_and_preserves_replay_after_expiry() -> TestResult {
    let directory = tempfile::tempdir()?;
    for cancelling in [false, true] {
        let path = directory
            .path()
            .join(format!("restart-{cancelling}.sqlite3"));
        let mut store = setup(&path)?;
        let (job, _) = store.admit_verification("verify", "pin", 1, 20)?;
        if cancelling {
            store.cancel_analysis_job("verify", job.generation)?;
        }
        drop(store);
        let mut store = Store::open(&path)?;
        assert_lease(&mut store)?;
        store.recover_analysis_jobs()?;
        let recovered = store.analysis_job("verify")?;
        assert_eq!(recovered.state, "interrupted");
        assert_eq!(recovered.generation, job.generation + 1);
        assert_eq!(recovered.reason.as_deref(), Some("service-restarted"));
        assert!(recovered.finished_ms.is_some());
        store.recover_analysis_jobs()?;
        assert_eq!(store.analysis_job("verify")?, recovered);
        assert!(!store.finish_verification("verify", job.generation, Ok(receipt(&job)), 22)?);
        assert!(matches!(
            store.cancel_analysis_job("verify", job.generation),
            Err(Error::Analysis("stale-worker"))
        ));
        store.begin_delete("one", false)?;
        store.finish_delete("one")?;
        let (replay, dispatch) = store.admit_verification("verify", "pin", 1, 23)?;
        assert_eq!(replay, recovered);
        assert!(dispatch.is_none());
        assert!(store.admit_verification("new", "pin", 1, 23).is_err());
        assert_eq!(store.analysis_job("verify")?, recovered);
        assert_no_paid_work(&store)?;
    }
    Ok(())
}

#[test]
fn receipt_mismatches_and_stale_inputs_cannot_be_verified() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
    for field in ["bytes", "files", "manifest"] {
        let (job, _) = store.admit_verification(field, "pin", 1, 20)?;
        let mut invalid = receipt(&job);
        match field {
            "bytes" => invalid.bytes += 1,
            "files" => invalid.files += 1,
            _ => invalid.manifest_sha256 = "b".repeat(64),
        }
        assert!(store.finish_verification(field, job.generation, Ok(invalid), 21)?);
        let failed = store.analysis_job(field)?;
        assert_eq!(failed.state, "failed");
        assert_eq!(failed.reason.as_deref(), Some("invalid-worker-result"));
        assert!(failed.verified_bytes.is_none());
    }
    let (job, _) = store.admit_verification("stale", "pin", 1, 22)?;
    store.connection.execute(
        "INSERT INTO analysis_inputs SELECT id, 2, recording_id, media_sha256, timeline_json, 'published', 23 FROM analysis_inputs WHERE id = 'pin' AND revision = 1",
        [],
    )?;
    assert!(store.finish_verification("stale", job.generation, Ok(receipt(&job)), 24)?);
    let failed = store.analysis_job("stale")?;
    assert_eq!(failed.state, "failed");
    assert_eq!(failed.reason.as_deref(), Some("input-no-longer-current"));
    assert!(store.admit_verification("old", "pin", 1, 25).is_err());
    assert_no_paid_work(&store)?;
    Ok(())
}

#[test]
fn terminal_sql_failure_rolls_back_the_result_and_retains_its_lease() -> TestResult {
    let directory = tempfile::tempdir()?;
    for cancelling in [false, true] {
        let path = directory
            .path()
            .join(format!("rollback-{cancelling}.sqlite3"));
        let mut store = setup(&path)?;
        let (job, _) = store.admit_verification("verify", "pin", 1, 20)?;
        if cancelling {
            store.cancel_analysis_job("verify", job.generation)?;
        }
        let before = store.analysis_job("verify")?;
        store.acknowledge_processing("one", "cleanup-request")?;
        store.connection.execute_batch(
            "CREATE TRIGGER fail_terminal AFTER UPDATE ON analysis_jobs WHEN NEW.finished_ms IS NOT NULL BEGIN SELECT RAISE(ABORT, 'fixture terminal failure'); END;",
        )?;
        assert!(
            store
                .finish_verification("verify", job.generation, Ok(receipt(&job)), 21)
                .is_err()
        );
        assert_eq!(store.analysis_job("verify")?, before);
        assert_lease(&mut store)?;
        assert_no_paid_work(&store)?;
        store
            .connection
            .execute_batch("DROP TRIGGER fail_terminal;")?;
        assert!(store.finish_verification("verify", job.generation, Ok(receipt(&job)), 22)?);
        assert!(!store.analysis_job("verify")?.active());
        assert_eq!(store.prune_candidates(false)?, vec!["one"]);
        store.audit_analysis_jobs()?;
    }
    Ok(())
}

#[test]
fn database_leases_block_direct_deletion_and_segment_release() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
    let (job, _) = store.admit_verification("verify", "pin", 1, 20)?;
    for cancelling in [false, true] {
        if cancelling {
            store.cancel_analysis_job("verify", job.generation)?;
        }
        for state in ["deleting", "deleted"] {
            assert!(
                store
                    .connection
                    .execute(
                        "UPDATE recordings SET storage_state = ?1 WHERE id = 'one'",
                        [state],
                    )
                    .is_err()
            );
        }
        assert!(store.connection.execute(
            "INSERT INTO recording_releases(recording_id, segment_ordinal, byte_length) SELECT recording_id, ordinal, byte_end - byte_start FROM recording_intervals WHERE recording_id = 'one'",
            [],
        ).is_err());
        assert_lease(&mut store)?;
    }
    assert!(store.finish_verification("verify", job.generation, Ok(receipt(&job)), 21)?);
    store.begin_delete("one", false)?;
    store.finish_delete("one")?;
    assert_eq!(store.recording("one")?.storage_state, "deleted");
    assert_eq!(store.dvr_status()?.charged_bytes, 0);
    assert_no_paid_work(&store)?;
    Ok(())
}

#[test]
fn retention_skips_live_readers_but_still_reclaims_unrelated_media() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut library = Library::open(directory.path(), true)?;
    initialize(library.store_mut())?;
    publish_recording(library.store_mut(), "other")?;
    let media = crate::recordings::checked_directory(directory.path(), true)?;
    let one = library.store().recording("one")?;
    let other = library.store().recording("other")?;
    let one_path = media.join(format!("{}.media", one.object_key));
    let other_path = media.join(format!("{}.media", other.object_key));
    std::fs::write(&one_path, AUDIO)?;
    std::fs::write(&other_path, AUDIO)?;
    let (job, _) = library
        .store_mut()
        .admit_verification("verify", "pin", 1, 20)?;
    library
        .store_mut()
        .acknowledge_processing("one", "cleanup-request")?;
    library
        .store_mut()
        .acknowledge_processing("other", "cleanup-request")?;
    assert_eq!(library.store().prune_candidates(true)?, vec!["other"]);
    assert_eq!(library.store().prune_candidates(false)?, vec!["other"]);
    assert!(crate::recordings::delete(&mut library, "one", false).is_err());
    crate::recordings::prune(&mut library)?;
    assert!(one_path.is_file());
    assert!(!other_path.exists());
    assert_eq!(
        library.store().recording("one")?.charged_bytes,
        AUDIO.len() as u64
    );
    assert_eq!(library.store().recording("other")?.storage_state, "deleted");
    library
        .store_mut()
        .cancel_analysis_job("verify", job.generation)?;
    let aged = Store::clock_ms()? + 15 * 86_400_000;
    for pressure in [false, true] {
        assert!(!crate::recordings::release_segments_at(
            &mut library,
            aged,
            pressure
        )?);
        assert!(one_path.is_file());
    }
    library
        .store_mut()
        .finish_verification("verify", job.generation, Ok(receipt(&job)), 21)?;
    assert!(crate::recordings::release_segments_at(
        &mut library,
        aged,
        false
    )?);
    assert!(!one_path.exists());
    assert_eq!(library.store().recording("one")?.storage_state, "deleted");
    assert_eq!(library.store().dvr_status()?.charged_bytes, 0);
    library.store().audit_dvr()?;
    assert_no_paid_work(library.store())?;
    drop(library);
    let reopened = Library::open(directory.path(), false)?;
    assert_eq!(reopened.store().recording("one")?.storage_state, "deleted");
    assert_eq!(reopened.store().dvr_status()?.charged_bytes, 0);
    Ok(())
}

fn publish_two_segments(store: &mut Store) -> Result<()> {
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        OPEN_SEGMENT_CEILING * 3,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    let job = store
        .admit_recording(
            "roll",
            "radio:v1",
            60,
            OPEN_SEGMENT_CEILING * 2,
            Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    let mut current = store.connect_recording(&job.version)?;
    for _ in 0..2 {
        let SegmentOpen::Opened { version, .. } = store.open_segment(&current)? else {
            return Err(Error::StorageIntegrity);
        };
        current = store.seal_segment(
            &version,
            &SegmentSeal {
                bytes: AUDIO.len() as u64,
                sha256: hex(&Sha256::digest(AUDIO)),
                format: "wav",
                decoded_microseconds: 1_000_000,
            },
        )?;
    }
    store.publish_recording(
        &current,
        &Publication {
            bytes: AUDIO.len() as u64 * 2,
            sha256: hex(&Sha256::digest(AUDIO.repeat(2))),
            format: "wav",
            decoded_microseconds: 2_000_000,
            end_reason: "end_of_body",
            http_route: vec![HttpHop {
                origin: "https://example.com".into(),
                peer: ([8, 8, 8, 8], 443).into(),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: true,
            gap: None,
        },
    )?;
    store.admit_analysis("roll-pin", "roll", false, 10)?;
    store.publish_analysis("roll-pin", 1)?;
    Ok(())
}

#[test]
fn expired_segments_release_partially_then_delete_the_final_recording() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut library = Library::open(directory.path(), true)?;
    initialize(library.store_mut())?;
    library
        .store_mut()
        .retain_recording("one", Retention::Kept)?;
    publish_two_segments(library.store_mut())?;
    let media = crate::recordings::checked_directory(directory.path(), true)?;
    let record = library.store().recording("roll")?;
    let mut paths = Vec::new();
    for interval in &record.intervals {
        let path = media.join(format!("{}.media", interval.object_key));
        std::fs::write(&path, AUDIO)?;
        paths.push(path);
    }
    let (job, _) = library
        .store_mut()
        .admit_verification("verify", "roll-pin", 1, 20)?;
    assert_eq!(job.expected_files, 2);
    let aged = Store::clock_ms()? + 15 * 86_400_000;
    assert!(!crate::recordings::release_segments_at(
        &mut library,
        aged,
        false
    )?);
    assert!(paths.iter().all(|path| path.is_file()));
    library
        .store_mut()
        .finish_verification("verify", job.generation, Ok(receipt(&job)), 21)?;
    assert!(crate::recordings::release_segments_at(
        &mut library,
        aged,
        false
    )?);
    let partial = library.store().recording("roll")?;
    assert_eq!(partial.storage_state, "retained");
    assert_eq!(partial.charged_bytes, AUDIO.len() as u64);
    assert!(partial.intervals[0].released);
    assert!(!partial.intervals[1].released);
    assert!(!paths[0].exists());
    assert!(paths[1].is_file());
    assert!(crate::recordings::release_segments_at(
        &mut library,
        aged,
        false
    )?);
    assert!(paths.iter().all(|path| !path.exists()));
    let deleted = library.store().recording("roll")?;
    assert_eq!(deleted.storage_state, "deleted");
    assert_eq!(deleted.charged_bytes, 0);
    assert!(deleted.intervals[0].released);
    assert!(!deleted.intervals[1].released);
    assert_eq!(
        library.store().dvr_status()?.charged_bytes,
        AUDIO.len() as u64
    );
    library.store().audit_dvr()?;
    assert_no_paid_work(library.store())?;
    drop(library);
    let reopened = Library::open(directory.path(), false)?;
    assert_eq!(reopened.store().recording("roll")?.storage_state, "deleted");
    assert_eq!(reopened.store().recording("roll")?.charged_bytes, 0);
    Ok(())
}
