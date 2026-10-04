use super::*;
use crate::storage::dvr::{OPEN_SEGMENT_CEILING, SegmentOpen, SegmentSeal};

#[test]
fn prefix_gap_refuses_zero_and_freezes_nonzero_timeline_without_inventing_audio() -> TestResult {
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
    assert!(matches!(
        store.admit_retained_reader("gap", "late", 0, 10),
        Err(Error::InvalidInput("seek is inside a late start gap"))
    ));
    assert!(matches!(store.retained_reader("gap"), Err(Error::NotFound)));
    let (spec, _) = store.admit_retained_reader("reader", "late", 30_250_000, 11)?;
    assert_eq!(
        (spec.timeline_start_us, spec.timeline_end_us),
        (30_000_000, 31_000_000)
    );
    assert_eq!(
        (spec.file_seek_us, spec.file_duration_us),
        (250_000, 1_000_000)
    );
    assert!(matches!(
        store.admit_retained_reader("end", "late", 31_000_000, 12),
        Err(Error::InvalidInput("seek is outside the retained audio"))
    ));
    assert_eq!(store.retained_readers()?.len(), 1);
    store.audit_retained_readers()?;
    Ok(())
}

#[test]
fn reader_protects_exact_sealed_segment_not_an_unrelated_interval_or_open_tail() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    let job = store
        .admit_recording(
            "segmented",
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
    let (spec, _) = store.admit_retained_reader("reader", "segmented", 500_000, 10)?;
    assert_eq!(
        (spec.ordinal, spec.timeline_start_us, spec.timeline_end_us),
        (0, 0, 1_000_000)
    );
    let release = store
        .next_segment_release(false, Store::clock_ms()? + 15 * 86_400_000)?
        .ok_or("unrelated interval not eligible")?;
    assert_eq!(release.ordinal, 1);
    store.mark_segment_released(&release)?;
    assert!(
        store
            .admit_retained_reader("released", "segmented", 1_500_000, 11)
            .is_err()
    );
    assert!(
        store
            .admit_retained_reader("tail", "segmented", 2_500_000, 11)
            .is_err()
    );
    store.cancel_retained_reader("reader", 1, 12)?;
    store.recover_retained_readers(13)?;
    assert!(
        store
            .next_segment_release(false, Store::clock_ms()? + 15 * 86_400_000)?
            .is_none()
    );
    assert!(
        store
            .connection
            .execute(
                "INSERT INTO recording_releases VALUES('segmented',0,1000)",
                []
            )
            .is_err()
    );
    store.audit_retained_readers()?;
    store.audit_dvr()?;
    Ok(())
}
