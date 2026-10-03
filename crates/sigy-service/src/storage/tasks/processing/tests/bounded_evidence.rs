//! Legal published outputs exercise scan limits independently from corruption checks.

use super::*;
use crate::{
    storage::{
        query_work::{Limits, QueryWork},
        tasks::evidence::Bounds,
    },
    task::evidence::TaskOutcome,
};
use std::time::{Duration, Instant};

fn hear_cues(store: &mut Store, job: &str, count: u32, script: &str, now: i64) -> Result<()> {
    let work = store
        .claim_local_asr(job, "fixture", now)?
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
    let width = (chunk.end_us - chunk.start_us) / u64::from(count);
    let cues = (0..count)
        .map(|ordinal| RecognitionCue {
            ordinal,
            start_us: chunk.start_us + u64::from(ordinal) * width,
            end_us: chunk.start_us + u64::from(ordinal + 1) * width,
            script: script.into(),
        })
        .collect();
    let output = RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages,
        cues,
    };
    let proof = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(output));
    store.finish_local_asr(&work, &proof, now)?;
    Ok(())
}

pub(super) fn populated(
    store: &mut Store,
    count: u32,
    script: &str,
    english: &str,
) -> Result<Vec<String>> {
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(store)?;
    for (ordinal, recording) in recordings.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?;
        let now = START + 100_000 + i64::from(ordinal) * 10;
        let request = recognition(store, recording, now)?;
        store.enqueue_task_recognition(
            &scope(ordinal, recording, AUDIO[ordinal as usize]),
            &request,
            now,
        )?;
        let cues = count;
        hear_cues(
            store,
            &request.id,
            cues,
            if ordinal == 0 { script } else { "silencio" },
            now + 1,
        )?;
        let request = translation(store, recording, 1)?;
        store.enqueue_task_translation(&scope(ordinal, recording, 0), &request, now + 2)?;
        let work = store
            .claim_translation(&request.id, "fixture", now + 3)?
            .ok_or(Error::StorageIntegrity)?;
        let result = TranslationResult::Succeeded(
            (0..cues)
                .map(|ordinal| TranslatedCue {
                    ordinal,
                    state: "translated".into(),
                    english: Some(english.into()),
                    reason: None,
                })
                .collect(),
        );
        let outcome = TranslationOutcome::synthetic_fixture(&work.job, result);
        store.finish_translation(&work, &outcome, now + 3)?;
    }
    Ok(recordings)
}

#[test]
fn exactly_full_citations_are_complete_but_the_next_match_is_partial() -> TestResult {
    for (count, more, outcome) in [
        (64, false, TaskOutcome::Cited),
        (65, true, TaskOutcome::Partial),
    ] {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog"), false)?;
        let recordings = populated(&mut store, count, "agua", "quiet")?;
        let evidence = store.task_evidence("task")?.ok_or("evidence")?;
        assert_eq!(evidence.citations.len(), 64);
        assert_eq!(evidence.more, more);
        assert_eq!(evidence.outcome, outcome);
        assert_eq!(evidence.citations[0].recording_id, recordings[0]);
        assert_eq!(evidence.citations[0].cue_ordinal, 0);
        assert_eq!(evidence.citations[63].cue_ordinal, 63);
        assert_eq!(
            evidence.citations[63].start_us,
            63 * (30_000_000 / u64::from(count))
        );
        assert_eq!(
            evidence.reasons,
            if more {
                vec!["citations-truncated"]
            } else {
                Vec::new()
            }
        );
    }
    Ok(())
}

#[test]
fn bounded_no_match_is_partial_and_original_plus_target_bytes_are_charged() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    populated(&mut store, 256, "silencio", "quiet")?;
    for (bounds, reason) in [
        (
            Bounds {
                rows: 2,
                text_bytes: 4 * 1_024 * 1_024,
            },
            "evidence-rows-truncated",
        ),
        (
            Bounds {
                rows: 4_096,
                text_bytes: 12,
            },
            "evidence-text-truncated",
        ),
    ] {
        let evidence = store
            .task_evidence_bounded("task", bounds)?
            .ok_or("evidence")?;
        assert_eq!(evidence.outcome, TaskOutcome::Partial);
        assert!(evidence.citations.is_empty());
        assert!(evidence.more);
        assert_eq!(evidence.reasons, [reason]);
    }
    let evidence = store.task_evidence("task")?.ok_or("evidence")?;
    assert_eq!(evidence.outcome, TaskOutcome::NoLiteralMatch);
    assert!(!evidence.more);
    Ok(())
}

#[test]
fn operation_can_share_an_outer_transaction_guard_and_clear_after_refusal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    populated(&mut store, 256, "silencio", "quiet")?;
    let unrelated = rusqlite::Connection::open_in_memory()?;
    let unrelated_work = QueryWork::start(
        &unrelated,
        Limits {
            wall: Duration::from_secs(1),
            vm_ops: 4_000_000,
            lock_wait: Duration::from_millis(1),
        },
    )?;
    assert!(matches!(
        store.task_evidence_in_work("task", &unrelated_work),
        Err(Error::InvalidInput("query work connection"))
    ));
    unrelated_work.finish()?;
    store.connection.execute_batch("BEGIN IMMEDIATE")?;
    let work = QueryWork::start(
        &store.connection,
        Limits {
            wall: Duration::from_secs(1),
            vm_ops: 1,
            lock_wait: Duration::from_millis(1),
        },
    )?;
    assert!(store.task_evidence_in_work("task", &work).is_err());
    work.finish()?;
    store.connection.execute_batch("ROLLBACK")?;
    assert_eq!(
        store.task_evidence("task")?.ok_or("evidence")?.outcome,
        TaskOutcome::NoLiteralMatch
    );
    store.connection.execute_batch("BEGIN IMMEDIATE")?;
    let work = QueryWork::start(
        &store.connection,
        Limits {
            wall: Duration::from_millis(100),
            vm_ops: 4_000_000,
            lock_wait: Duration::from_millis(10),
        },
    )?;
    assert!(store.task_evidence_in_work("task", &work)?.is_some());
    work.finish()?;
    store.connection.execute_batch("COMMIT")?;
    Ok(())
}

#[test]
fn corrupt_oversize_translation_is_refused_and_cleanup_preserves_next_query() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    populated(&mut store, 1, "agua", "quiet")?;
    // Deliberately bypass guards to model a damaged catalog, not a legal publication.
    store.connection.execute_batch(
        "PRAGMA ignore_check_constraints=ON; DROP TRIGGER translation_cue_no_update",
    )?;
    store
        .connection
        .execute("UPDATE translation_cues SET english=?1", ["x".repeat(4097)])?;
    assert!(matches!(
        store.task_evidence("task"),
        Err(Error::StorageIntegrity)
    ));
    assert_eq!(
        store
            .connection
            .query_row("SELECT 7", [], |row| row.get::<_, i64>(0))?,
        7
    );
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))?,
        5000
    );
    store
        .connection
        .execute("UPDATE translation_cues SET english='quiet'", [])?;
    assert!(store.task_evidence("task")?.is_some());
    Ok(())
}

#[test]
fn legal_boundary_measurement_keeps_work_far_below_vm_ceiling() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    populated(&mut store, 256, &"z".repeat(256), &"z".repeat(4096))?;
    let mut elapsed = Vec::new();
    let mut largest_ops = 0;
    for _ in 0..16 {
        let started = Instant::now();
        let work = QueryWork::start(
            &store.connection,
            Limits {
                wall: Duration::from_millis(100),
                vm_ops: 4_000_000,
                lock_wait: Duration::from_millis(10),
            },
        )?;
        let evidence = store
            .task_evidence_in_work("task", &work)?
            .ok_or("evidence")?;
        assert_eq!(evidence.outcome, TaskOutcome::NoLiteralMatch);
        largest_ops = largest_ops.max(work.checkpoint_ops());
        work.finish()?;
        elapsed.push(started.elapsed().as_micros());
    }
    elapsed.sort_unstable();
    println!(
        "bounded evidence: 512 legal cues, 2,164,736 examined original/English bytes; p50={}us max={}us VM checkpoint ops={largest_ops}",
        elapsed[8], elapsed[15]
    );
    assert!(largest_ops < 4_000_000);
    Ok(())
}
