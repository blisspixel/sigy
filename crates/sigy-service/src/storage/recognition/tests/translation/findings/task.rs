//! Canonical task publication preserves immutable historical media statements.

use super::*;
use crate::storage::findings::write_task_finding;
use crate::task::TaskCitation;

fn citation() -> TaskCitation {
    TaskCitation {
        source: "radio:v1".into(),
        recording_id: "one".into(),
        transcript_id: "pin".into(),
        transcript_revision: 1,
        translation_revision: Some(1),
        cue_ordinal: 0,
        start_us: 0,
        end_us: 1_000_000,
    }
}

#[test]
fn task_finding_replays_its_retained_history_after_delete_and_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = desk(&path)?;
    let before = activity(&store)?;
    let first = write_task_finding(&store.connection, "fair", "owned", &citation(), 40)?;
    assert_eq!(first.original, FindingOriginal::Retained);
    store.begin_delete("one", false)?;
    assert_eq!(
        write_task_finding(&store.connection, "fair", "owned", &citation(), 41)?,
        first
    );
    let expired = write_task_finding(&store.connection, "fair", "expired", &citation(), 42)?;
    assert_eq!(expired.original, FindingOriginal::Expired);
    assert_eq!((expired.start_us, expired.end_us), (None, None));
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.finding("fair", "owned")?, first);
    assert_eq!(store.finding("fair", "expired")?, expired);
    store.finish_delete("one")?;
    drop(store);
    let store = Store::open(&path)?;
    assert_eq!(store.finding("fair", "owned")?, first);
    assert_eq!(
        write_task_finding(&store.connection, "fair", "owned", &citation(), 43)?,
        first
    );
    assert_eq!(activity(&store)?, before);
    Ok(())
}

#[test]
fn a_missing_historical_finding_stays_valid_after_retention_changes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = desk(&directory.path().join("catalog"))?;
    // This focused finding fixture deliberately overlaps a gap; the whole catalog's
    // independent DVR audit is not exercised by that synthetic media inconsistency.
    store.connection.execute(
        "INSERT INTO recording_gaps VALUES ('one', 0, 'disconnect', 250000, 500000)",
        [],
    )?;
    let first = write_task_finding(&store.connection, "fair", "missing", &citation(), 40)?;
    assert_eq!(first.original, FindingOriginal::Missing);
    store.audit_findings()?;
    store.begin_delete("one", false)?;
    store.audit_findings()?;
    store.finish_delete("one")?;
    store.audit_findings()?;
    assert_eq!(
        write_task_finding(&store.connection, "fair", "missing", &citation(), 41)?,
        first
    );
    Ok(())
}

#[test]
fn task_finding_refuses_missing_translation_unsupported_cues_and_changed_replay() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = desk(&directory.path().join("catalog"))?;
    let mut ask = citation();
    ask.translation_revision = None;
    assert!(matches!(
        write_task_finding(&store.connection, "fair", "owned", &ask, 40),
        Err(Error::Analysis("task-translation-unavailable"))
    ));
    ask = citation();
    ask.cue_ordinal = 256;
    assert!(matches!(
        write_task_finding(&store.connection, "fair", "owned", &ask, 40),
        Err(Error::Analysis("task-cue-unsupported"))
    ));
    assert_eq!(stored(&store)?, 0);
    write_task_finding(&store.connection, "fair", "owned", &citation(), 40)?;
    for changed in ["source", "recording", "span", "translation"] {
        ask = citation();
        match changed {
            "source" => ask.source = "other:v1".into(),
            "recording" => ask.recording_id = "other".into(),
            "span" => ask.start_us = 1,
            _ => ask.translation_revision = Some(2),
        }
        assert!(write_task_finding(&store.connection, "fair", "owned", &ask, 41).is_err());
    }
    assert_eq!(stored(&store)?, 1);
    Ok(())
}

#[test]
fn task_finding_obeys_the_existing_monitor_capacity() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = desk(&directory.path().join("catalog"))?;
    store.connection.execute(
        "WITH RECURSIVE n(value) AS (VALUES(1) UNION ALL SELECT value + 1 FROM n WHERE value < 1024) INSERT INTO monitor_findings(monitor_id, id, transcript_id, transcript_revision, translation_revision, cue_ordinal, recording_id, original_state, start_us, end_us, created_ms) SELECT 'fair', 'full-' || value, 'pin', 1, 1, 0, 'one', 'retained', 0, 1000000, 40 FROM n",
        [],
    )?;
    assert!(matches!(
        write_task_finding(&store.connection, "fair", "overflow", &citation(), 41),
        Err(Error::Analysis("finding-limit"))
    ));
    assert_eq!(stored(&store)?, 1024);
    Ok(())
}

#[test]
fn released_segment_history_reopens_and_new_citations_truthfully_report_expired() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let store = segmented_desk(&path)?;
    let first = write_task_finding(&store.connection, "fair", "before-release", &citation(), 40)?;
    let before = store.recording("one")?;
    crate::recordings::checked_directory(directory.path(), true)?;
    for interval in &before.intervals {
        std::fs::write(
            crate::recordings::media_path(directory.path(), &interval.object_key)?,
            b"data",
        )?;
    }
    drop(store);
    let mut library = crate::library::Library::open(directory.path(), false)?;
    assert!(crate::recordings::release_segments_at(
        &mut library,
        crate::storage::now_ms()?,
        true
    )?);
    let store = library.store_mut();
    let after = store.recording("one")?;
    assert_eq!(after.storage_state, "retained");
    assert_eq!(after.charged_bytes, before.charged_bytes - 4);
    assert_eq!(after.media_bytes, before.media_bytes.map(|bytes| bytes - 4));
    assert!(after.intervals[0].released);
    store.audit_dvr()?;
    assert!(matches!(
        store.publish_finding(
            "fair",
            "false-retained",
            &cite(FindingOriginal::Retained),
            41
        ),
        Err(Error::Analysis("finding-range"))
    ));
    assert!(matches!(
        store.publish_finding("fair", "false-missing", &cite(FindingOriginal::Missing), 41),
        Err(Error::Analysis("finding-original"))
    ));
    let inserted = store.connection.execute(
        "INSERT INTO monitor_findings(monitor_id, id, transcript_id, transcript_revision, translation_revision, cue_ordinal, recording_id, original_state, start_us, end_us, created_ms) VALUES ('fair', 'false-sql', 'pin', 1, 1, 0, 'one', 'retained', 0, 1000000, 41)",
        [],
    );
    assert!(inserted.is_err());
    drop(library);
    let store = Store::open(&path)?;
    let expired = write_task_finding(&store.connection, "fair", "after-release", &citation(), 42)?;
    assert_eq!(expired.original, FindingOriginal::Expired);
    assert_eq!((expired.start_us, expired.end_us), (None, None));
    assert_eq!(store.finding("fair", "before-release")?, first);
    assert_eq!(
        write_task_finding(&store.connection, "fair", "before-release", &citation(), 43)?,
        first
    );
    drop(store);
    let store = Store::open(&path)?;
    assert_eq!(store.finding("fair", "after-release")?, expired);
    assert_eq!(store.finding("fair", "before-release")?, first);
    store.audit_dvr()?;
    Ok(())
}

fn segmented_desk(path: &std::path::Path) -> Result<Store> {
    use crate::storage::dvr::{
        OPEN_SEGMENT_CEILING, Publication, Retention, SegmentOpen, SegmentSeal,
    };
    let mut store = Store::open(path)?;
    store.configure_dvr(
        OPEN_SEGMENT_CEILING * 4,
        64 * 1024 * 1024,
        14,
        std::env::current_exe()?
            .to_str()
            .ok_or(Error::StorageIntegrity)?,
    )?;
    store.register_source(
        "radio:v1",
        &crate::sources::HttpSource::new(
            "Fixture",
            "https://example.com/audio",
            crate::sources::NetworkScope::PublicInternet {},
        )?,
    )?;
    let job = store
        .admit_recording(
            "one",
            "radio:v1",
            60,
            OPEN_SEGMENT_CEILING * 2,
            Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    let mut version = store.connect_recording(&job.version)?;
    for _ in 0..2 {
        let SegmentOpen::Opened {
            version: opened, ..
        } = store.open_segment(&version)?
        else {
            return Err(Error::StorageIntegrity);
        };
        version = store.seal_segment(
            &opened,
            &SegmentSeal {
                bytes: 4,
                sha256: "a".repeat(64),
                format: "wav",
                decoded_microseconds: 1_000_000,
            },
        )?;
    }
    store.publish_recording(
        &version,
        &Publication {
            bytes: 8,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 2_000_000,
            end_reason: "end_of_body",
            http_route: vec![crate::sources::HttpHop {
                origin: "https://example.com".into(),
                peer: std::net::SocketAddr::from(([8, 8, 8, 8], 443)),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: true,
            gap: None,
        },
    )?;
    store.admit_analysis("pin", "one", false, 10)?;
    store.publish_analysis("pin", 1)?;
    let work = admit(&mut store, "asr", 0)?;
    store.finish_local_asr(&work, &proof(&work, "Una feria mundial"), 21)?;
    store.add_translation_profile(&profile()?, 22)?;
    store.create_monitor("fair", &super::super::super::follow("radio:v1"), 25)?;
    let (job, work) = store.admit_translation(&request("mt-1")?, 30)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![translated(0, "A world fair")]),
    );
    store.finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 31)?;
    Ok(store)
}
