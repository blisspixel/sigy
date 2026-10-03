//! Rehashed corruptions must fail independent lineage checks, not just hash checks.

use super::*;
use crate::task::snapshot::TaskEvidenceSnapshot;

fn replace(store: &Store, snapshot: &TaskEvidenceSnapshot) -> Result<()> {
    store
        .connection
        .execute_batch("DROP TRIGGER IF EXISTS task_evidence_snapshot_no_update")?;
    let json = serde_json::to_string(snapshot)?;
    let digest = crate::recognition::sha256_hex(
        format!("[\"sigy-task-evidence-snapshot-v1\",{json}]").as_bytes(),
    );
    store.connection.execute("UPDATE task_evidence_snapshots SET payload_json=?1,payload_sha256=?2 WHERE task_id=?3 AND ordinal=?4",rusqlite::params![json,digest,snapshot.task_id,snapshot.ordinal])?;
    Ok(())
}

#[test]
fn legal_no_text_results_freeze_their_exact_coverage_and_reject_changed_outcome() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    store.start_task_processing("task", "grant", &spec(60, true), 0, START - 500)?;
    let recordings = collect(&mut store)?;
    let now = clock(&store)?;
    for (ordinal, recording) in recordings.iter().enumerate() {
        let request = recognition(&mut store, recording, now + 90_000)?;
        store.enqueue_task_recognition(
            &scope(
                u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?,
                recording,
                AUDIO[ordinal],
            ),
            &request,
            now + 90_000,
        )?;
        let work = store
            .claim_local_asr(&request.id, "fixture", now + 91_000)?
            .ok_or("work")?;
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
        let output = RecognitionOutput {
            profile_sha256: work.job.request.profile_sha256.clone(),
            manifest_sha256: work.job.manifest_sha256.clone(),
            coverages,
            cues: Vec::new(),
        };
        let proof = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(output));
        store.finish_local_asr(&work, &proof, now + 91_000)?;
    }
    let mut snapshot = store.freeze_task_evidence("task", "freeze", 0, now + 93_000)?;
    assert_eq!(snapshot.evidence.outcome, TaskOutcome::Partial);
    assert_eq!(snapshot.evidence.recognized_us, 60_000_000);
    assert!(snapshot.evidence.citations.is_empty());
    assert_eq!(store.task_evidence_snapshot("task", 1)?, snapshot);
    snapshot.evidence.entries[1].transcript_outcome = Some("text".into());
    replace(&store, &snapshot)?;
    assert!(store.task_evidence_snapshot("task", 1).is_err());
    Ok(())
}

#[test]
fn removing_a_literal_match_from_a_rehashed_complete_selection_is_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, _) = recognized(&directory.path().join("catalog"))?;
    let now = clock(&store)?;
    let mut snapshot = store.freeze_task_evidence("task", "freeze", 0, now + 93_000)?;
    assert!(!snapshot.evidence.more);
    assert!(!snapshot.evidence.citations.is_empty());
    snapshot.evidence.citations.clear();
    replace(&store, &snapshot)?;
    assert!(store.task_evidence_snapshot("task", 1).is_err());
    Ok(())
}

#[test]
fn swapping_valid_citations_in_a_rehashed_complete_selection_is_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"), false)?;
    super::super::bounded_evidence::populated(&mut store, 2, "agua", "quiet")?;
    let now = clock(&store)?;
    let snapshot = store.freeze_task_evidence("task", "freeze", 0, now + 1)?;
    assert_eq!(snapshot.evidence.outcome, TaskOutcome::Cited);
    assert!(!snapshot.evidence.more);
    assert_eq!(snapshot.evidence.citations.len(), 2);
    assert_ne!(
        snapshot.evidence.citations[0].cue_ordinal,
        snapshot.evidence.citations[1].cue_ordinal
    );
    assert_eq!(store.task_evidence_snapshot("task", 1)?, snapshot);
    store
        .connection
        .busy_timeout(std::time::Duration::from_millis(37))?;
    let mut swapped = snapshot.clone();
    swapped.evidence.citations.swap(0, 1);
    replace(&store, &swapped)?;
    assert!(matches!(
        store.task_evidence_snapshot("task", 1),
        Err(Error::StorageIntegrity)
    ));
    let wait: u32 = store
        .connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    assert_eq!(wait, 37);
    replace(&store, &snapshot)?;
    assert_eq!(store.task_evidence_snapshot("task", 1)?, snapshot);
    let wait: u32 = store
        .connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    assert_eq!(wait, 37);
    Ok(())
}

#[test]
fn a_rehashed_false_no_literal_match_is_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (mut store, recordings) = recognized(&directory.path().join("catalog"))?;
    let now = clock(&store)?;
    for (ordinal, recording) in recordings.iter().enumerate() {
        let request = translation(&store, recording, 1)?;
        store.enqueue_task_translation(
            &scope(
                u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?,
                recording,
                0,
            ),
            &request,
            now + 92_000,
        )?;
        translate(&mut store, &request.id, "No target term", now + 94_000)?;
    }
    let mut snapshot = store.freeze_task_evidence("task", "freeze", 0, now + 95_000)?;
    assert_eq!(snapshot.evidence.outcome, TaskOutcome::Cited);
    snapshot.evidence.citations.clear();
    snapshot.evidence.outcome = TaskOutcome::NoLiteralMatch;
    replace(&store, &snapshot)?;
    assert!(store.task_evidence_snapshot("task", 1).is_err());
    Ok(())
}

#[test]
fn rehashed_uncited_revision_and_self_consistent_counter_changes_are_refused() -> TestResult {
    for revision in [false, true] {
        let directory = tempfile::tempdir()?;
        let (mut store, _) = recognized(&directory.path().join("catalog"))?;
        let now = clock(&store)?;
        let mut snapshot = store.freeze_task_evidence("task", "freeze", 0, now + 93_000)?;
        // Entry 1 has no literal citation, so citation-only audits cannot catch it.
        let entry = snapshot.evidence.entries.get_mut(1).ok_or("entry")?;
        if revision {
            entry.transcript_revision = Some(63);
        } else {
            entry.recognized_us -= 1;
            entry.unprocessed_us += 1;
            snapshot.evidence.recognized_us -= 1;
            snapshot.evidence.unprocessed_us += 1;
        }
        replace(&store, &snapshot)?;
        assert!(store.task_evidence_snapshot("task", 1).is_err());
    }
    Ok(())
}

#[test]
fn rehashed_impossible_completion_state_and_time_are_refused() -> TestResult {
    for future in [false, true] {
        let directory = tempfile::tempdir()?;
        let (mut store, recordings) = recognized(&directory.path().join("catalog"))?;
        let now = clock(&store)?;
        let request = translation(&store, &recordings[0], 1)?;
        store.enqueue_task_translation(&scope(0, &recordings[0], 0), &request, now + 92_000)?;
        let mut snapshot = store.freeze_task_evidence("task", "freeze", 0, now + 93_000)?;
        if future {
            translate(&mut store, &request.id, "Report about water", now + 94_000)?;
        }
        let job = snapshot
            .jobs
            .iter_mut()
            .find(|j| j.stage == "translation")
            .ok_or("job")?;
        job.observed_state = "succeeded".into();
        snapshot
            .processing
            .as_mut()
            .ok_or("processing")?
            .steps
            .iter_mut()
            .find(|s| s.stage == "translation")
            .ok_or("step")?
            .job_state = Some("succeeded".into());
        snapshot.evidence.entries[0]
            .translation
            .as_mut()
            .ok_or("stage")?
            .job_state = Some("succeeded".into());
        replace(&store, &snapshot)?;
        assert!(store.task_evidence_snapshot("task", 1).is_err());
    }
    Ok(())
}

#[test]
fn oversized_original_or_target_citation_values_are_refused_and_next_query_works() -> TestResult {
    for target in [false, true] {
        let directory = tempfile::tempdir()?;
        let (mut store, recordings) = recognized(&directory.path().join("catalog"))?;
        let now = clock(&store)?;
        let request = translation(&store, &recordings[0], 1)?;
        store.enqueue_task_translation(&scope(0, &recordings[0], 0), &request, now + 92_000)?;
        translate(&mut store, &request.id, "Report about water", now + 94_000)?;
        store.freeze_task_evidence("task", "freeze", 0, now + 95_000)?;
        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")?;
        if target {
            store
                .connection
                .execute_batch("DROP TRIGGER translation_cue_no_update")?;
            store
                .connection
                .execute("UPDATE translation_cues SET english=?1", ["x".repeat(4097)])?;
        } else {
            store
                .connection
                .execute_batch("DROP TRIGGER transcript_cues_no_update")?;
            store
                .connection
                .execute("UPDATE transcript_cues SET script=?1", ["x".repeat(4097)])?;
        }
        assert!(store.task_evidence_snapshot("task", 1).is_err());
        assert_eq!(
            store
                .connection
                .query_row("SELECT 7", [], |r| r.get::<_, i64>(0))?,
            7
        );
    }
    Ok(())
}
