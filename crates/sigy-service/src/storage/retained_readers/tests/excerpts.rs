use super::*;
use crate::storage::dvr::{GapCause, OPEN_SEGMENT_CEILING, SegmentOpen, SegmentSeal};

#[test]
fn legacy_json_and_digest_match_frozen_reference_and_excerpt_has_independent_hash() -> TestResult {
    const LEGACY: &str = r#"{"request_id":"reader","generation":1,"recording_id":"one","source_revision":"radio:v1","ordinal":0,"object_key":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","format":"wav","bytes":20,"timeline_start_us":30000000,"timeline_end_us":31000000,"file_seek_us":250000,"file_duration_us":1000000,"spec_sha256":"7e4233e45cdf0b23e24ee88dd96238f7c66937bf9729c2edf96422193497e1f2"}"#;
    let mut spec: RetainedReadSpec = serde_json::from_str(LEGACY)?;
    assert!(spec.excerpt.is_none());
    assert_eq!(serde_json::to_string(&spec)?, LEGACY);
    assert_eq!(spec.digest()?, spec.spec_sha256);
    assert_eq!(spec.playback_end_us()?, 1_000_000);
    assert_eq!(spec.playback_duration_us()?, 750_000);
    spec.excerpt = Some(RetainedExcerpt {
        version: 2,
        timeline_end_us: 30_375_000,
        citation: None,
    });
    // Independently encoded and hashed with the platform SHA-256 implementation.
    assert_eq!(
        spec.digest()?,
        "db0bd8b39625d57ae4a9c74007db16d591a2b619b1b1aede7e9dff1743f358fb"
    );
    assert_eq!(spec.playback_end_us()?, 375_000);
    assert_eq!(spec.playback_duration_us()?, 125_000);
    for end in [0, 30_250_000, 31_000_001, u64::MAX] {
        let mut changed = spec.clone();
        changed.excerpt.as_mut().ok_or("excerpt")?.timeline_end_us = end;
        assert!(changed.playback_duration_us().is_err());
        assert!(changed.digest().is_err());
    }
    spec.excerpt.as_mut().ok_or("excerpt")?.version = 3;
    assert!(spec.playback_end_us().is_err());
    Ok(())
}

#[test]
fn excerpt_admission_freezes_both_edges_and_conflicts_across_request_modes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    published(&mut store, "recording")?;
    let (spec, fresh) = store.admit_retained_range("range", "recording", 125_000, 375_000, 10)?;
    assert!(fresh);
    assert_eq!(
        (
            spec.timeline_start_us,
            spec.timeline_end_us,
            spec.file_duration_us,
            spec.bytes
        ),
        (0, 1_000_000, 1_000_000, 20)
    );
    assert_eq!(
        (
            spec.file_seek_us,
            spec.playback_end_us()?,
            spec.playback_duration_us()?
        ),
        (125_000, 375_000, 250_000)
    );
    assert_eq!(
        store.admit_retained_range("range", "recording", 125_000, 375_000, -1)?,
        (spec.clone(), false)
    );
    for (seek, end) in [(125_000, 375_001), (125_001, 375_000), (125_000, u64::MAX)] {
        assert!(matches!(
            store.admit_retained_range("range", "recording", seek, end, -1),
            Err(Error::IdempotencyConflict)
        ));
    }
    assert!(matches!(
        store.admit_retained_reader("range", "recording", 125_000, 11),
        Err(Error::IdempotencyConflict)
    ));
    store.admit_retained_reader("legacy", "recording", 0, 11)?;
    assert!(matches!(
        store.admit_retained_range("legacy", "recording", 0, 1_000_000, 12),
        Err(Error::IdempotencyConflict)
    ));
    let (edge, _) = store.admit_retained_range("edge", "recording", 750_000, 1_000_000, 12)?;
    assert_eq!(edge.playback_end_us()?, 1_000_000);
    for (seek, end) in [
        (375_000, 375_000),
        (375_000, 125_000),
        (0, 1_000_001),
        (1_000_000, 1_000_001),
        (0, u64::MAX),
    ] {
        assert!(
            store
                .admit_retained_range("invalid", "recording", seek, end, 13)
                .is_err()
        );
        assert!(matches!(
            store.retained_reader("invalid"),
            Err(Error::NotFound)
        ));
    }
    assert!(
        store
            .connection
            .execute(
                "UPDATE retained_readers SET excerpt_end_us=375001 WHERE id='range'",
                []
            )
            .is_err()
    );
    assert!(store.connection.execute("UPDATE retained_readers SET excerpt_version=2,excerpt_end_us=1000000 WHERE id='legacy'", []).is_err());
    store.admit_retained_range("fourth", "recording", 0, 125_000, 14)?;
    assert!(
        store
            .admit_retained_reader("fifth", "recording", 0, 15)
            .is_err()
    );
    assert!(store.begin_delete("recording", false).is_err());
    store.audit_retained_readers()?;
    Ok(())
}

#[test]
fn excerpt_checks_interior_gaps_and_accepts_gap_adjacent_exclusive_edges() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    published(&mut store, "recording")?;
    store.connection.execute(
        "INSERT INTO recording_gaps VALUES('recording',0,?1,250000,500000)",
        [GapCause::Disconnect.as_str()],
    )?;
    assert!(matches!(
        store.admit_retained_range("overlap", "recording", 125_000, 375_000, 10),
        Err(Error::InvalidInput(
            "retained excerpt overlaps a recording gap"
        ))
    ));
    assert!(store.retained_readers()?.is_empty());
    let (mut forged, _) = store.admit_retained_reader("point", "recording", 125_000, 10)?;
    forged.request_id = "forged".into();
    forged.excerpt = Some(RetainedExcerpt {
        version: 2,
        timeline_end_us: 375_000,
        citation: None,
    });
    forged.spec_sha256 = forged.digest()?;
    assert!(super::super::write::insert(&store.connection, &forged, 125_000, 10).is_err());
    assert!(matches!(
        store.retained_reader("forged"),
        Err(Error::NotFound)
    ));
    store.admit_retained_range("before", "recording", 0, 250_000, 11)?;
    store.admit_retained_range("after", "recording", 500_000, 1_000_000, 12)?;
    assert!(matches!(
        store.retained_reader("overlap"),
        Err(Error::NotFound)
    ));
    store.audit_retained_readers()?;
    Ok(())
}

#[test]
fn excerpt_origin_conversion_is_checked_and_adjacent_segments_are_not_stitched() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    let job = store
        .admit_recording("late", "radio:v1", 60, 100, Retention::Temporary, false)?
        .ok_or(Error::RequestState)?;
    let tx = store.connection.transaction()?;
    crate::storage::dvr::journal_prefix_gap(&tx, "late", 30_000_000)?;
    tx.commit()?;
    store.publish_recording(&job.version, &publication())?;
    let (spec, _) = store.admit_retained_range("late-range", "late", 30_125_000, 30_375_000, 10)?;
    assert_eq!(
        (spec.timeline_start_us, spec.timeline_end_us),
        (30_000_000, 31_000_000)
    );
    assert_eq!(
        (
            spec.file_seek_us,
            spec.playback_end_us()?,
            spec.playback_duration_us()?
        ),
        (125_000, 375_000, 250_000)
    );
    let job = store
        .admit_recording(
            "rolling",
            "radio:v1",
            60,
            OPEN_SEGMENT_CEILING * 3,
            Retention::Temporary,
            false,
        )?
        .ok_or(Error::RequestState)?;
    let mut version = store.connect_recording(&job.version)?;
    for hash in ['a', 'b'] {
        let SegmentOpen::Opened {
            version: opened, ..
        } = store.open_segment(&version)?
        else {
            return Err("segment not opened".into());
        };
        version = store.seal_segment(
            &opened,
            &SegmentSeal {
                bytes: 1000,
                sha256: hash.to_string().repeat(64),
                format: "wav",
                decoded_microseconds: 1_000_000,
            },
        )?;
    }
    assert!(matches!(
        store.admit_retained_range("cross", "rolling", 500_000, 1_500_000, 11),
        Err(Error::InvalidInput(
            "retained excerpt must fit one sealed interval"
        ))
    ));
    let (second, _) = store.admit_retained_range("second", "rolling", 1_000_000, 1_250_000, 12)?;
    assert_eq!(
        (
            second.ordinal,
            second.file_seek_us,
            second.playback_end_us()?
        ),
        (1, 0, 250_000)
    );
    assert!(matches!(
        store.retained_reader("cross"),
        Err(Error::NotFound)
    ));
    store.audit_retained_readers()?;
    Ok(())
}
