//! Citation fixtures use the real storage admission paths, with synthetic model output.

use super::*;

fn partial_desk(path: &Path) -> Result<Store> {
    let mut store = setup(path, false)?;
    let work = admit(&mut store, "asr", 0)?;
    let mut recognized = output(&work, "Una feria mundial");
    recognized.cues[0].start_us = 125_000;
    recognized.cues[0].end_us = 375_000;
    let proof = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(recognized));
    store.finish_local_asr(&work, &proof, 21)?;
    store.add_translation_profile(&profile()?, 22)?;
    store.create_monitor("fair", &super::super::super::follow("radio:v1"), 25)?;
    let (job, work) = store.admit_translation(&request("mt-1")?, 30)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 31)?;
    store.publish_finding("fair", "world", &cite(FindingOriginal::Retained), 40)?;
    Ok(store)
}

async fn close_before_open(store: &mut Store, directory: &Path, id: &str) -> TestResult {
    let view = store.cancel_retained_reader(id, 1, 90)?;
    // The production worker observes cancellation before opening any original.
    // This exercises a real matching close receipt, not a fabricated completed row.
    let ownership = std::sync::Arc::new(std::fs::File::open(directory.join("catalog"))?);
    let (_stop, signal) = tokio::sync::watch::channel(true);
    let receipt = crate::recordings::retained::stream_retained(
        directory.to_owned(),
        view.spec,
        ownership,
        signal,
        |_| async { Ok(()) },
    )
    .await?;
    store.finish_retained_reader(&receipt, 91)?;
    Ok(())
}

#[tokio::test]
async fn finding_reader_freezes_exact_cue_and_replays_after_revision_and_retention_drift()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = partial_desk(&directory.path().join("catalog"))?;
    let before = activity(&store)?;
    let (spec, fresh) = store.admit_retained_finding("cited", "fair", "world", 50)?;
    assert!(fresh);
    assert_eq!(activity(&store)?, before);
    assert_eq!(
        (
            spec.recording_id.as_str(),
            spec.file_seek_us,
            spec.playback_end_us()?,
            spec.playback_duration_us()?
        ),
        ("one", 125_000, 375_000, 250_000)
    );
    let citation = spec
        .excerpt
        .as_ref()
        .and_then(|excerpt| excerpt.citation.as_ref())
        .ok_or("citation")?;
    assert_eq!(
        (
            citation.monitor_id.as_str(),
            citation.finding_id.as_str(),
            citation.transcript_id.as_str(),
            citation.transcript_revision,
            citation.translation_revision,
            citation.cue_ordinal
        ),
        ("fair", "world", "pin", 1, 1, 0)
    );
    for (monitor, finding) in [("fair", "other"), ("other", "world")] {
        assert!(matches!(
            store.admit_retained_finding("cited", monitor, finding, -1),
            Err(Error::IdempotencyConflict)
        ));
    }
    assert!(matches!(
        store.admit_retained_range("cited", "one", 125_000, 375_000, -1),
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        store.admit_retained_reader("cited", "one", 125_000, -1),
        Err(Error::IdempotencyConflict)
    ));
    store.correct_transcript("pin", 1, 0, "Una feria local", 60)?;
    let (job, work) = store.admit_translation(&request("mt-new")?, 70)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, "A local fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 71)?;
    let finding = store.finding("fair", "world")?;
    assert_eq!(
        (finding.stale_transcript, finding.stale_translation),
        (Some(true), Some(true))
    );
    assert_eq!(
        store.admit_retained_finding("cited", "fair", "world", -1)?,
        (spec.clone(), false)
    );
    let (fresh_spec, _) = store.admit_retained_finding("fresh", "fair", "world", 80)?;
    assert_eq!(fresh_spec.excerpt, spec.excerpt);
    assert_eq!(
        (fresh_spec.file_seek_us, fresh_spec.playback_end_us()?),
        (125_000, 375_000)
    );
    close_before_open(&mut store, directory.path(), "cited").await?;
    close_before_open(&mut store, directory.path(), "fresh").await?;
    store.begin_delete("one", false)?;
    store.finish_delete("one")?;
    assert_eq!(
        store.admit_retained_finding("cited", "fair", "world", -1)?,
        (spec, false)
    );
    assert!(matches!(
        store.admit_retained_finding("expired", "fair", "world", 100),
        Err(Error::InvalidInput("cited interval is no longer retained"))
    ));
    assert!(matches!(
        store.retained_reader("expired"),
        Err(Error::NotFound)
    ));
    assert_eq!(store.finding("fair", "world")?, finding);
    store.audit_retained_readers()?;
    Ok(())
}

#[test]
fn finding_reader_refuses_missing_expired_gap_and_foreign_citation_without_receipt() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = partial_desk(&directory.path().join("catalog"))?;
    assert!(matches!(
        store.admit_retained_finding("absent", "fair", "absent", 50),
        Err(Error::NotFound)
    ));
    store.connection.execute(
        "INSERT INTO recording_gaps VALUES('one',0,'disconnect',200000,250000)",
        [],
    )?;
    assert!(matches!(
        store.admit_retained_finding("gapped", "fair", "world", 50),
        Err(Error::InvalidInput("cited interval is no longer retained"))
    ));
    store.publish_finding("fair", "missing", &cite(FindingOriginal::Missing), 51)?;
    assert!(matches!(
        store.admit_retained_finding("missing", "fair", "missing", 52),
        Err(Error::InvalidInput(
            "finding has no retained cited interval"
        ))
    ));
    store.begin_delete("one", false)?;
    store.publish_finding("fair", "expired", &cite(FindingOriginal::Expired), 53)?;
    assert!(matches!(
        store.admit_retained_finding("expired", "fair", "expired", 54),
        Err(Error::InvalidInput(
            "finding has no retained cited interval"
        ))
    ));
    assert!(store.retained_readers()?.is_empty());
    Ok(())
}
