use super::*;
use crate::storage::dvr::{GapCause, OPEN_SEGMENT_CEILING, SegmentOpen, SegmentSeal};

#[test]
fn missing_unpublished_deleted_and_completed_edge_have_specific_atomic_refusals() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    assert!(matches!(
        store.admit_retained_reader("missing-reader", "missing-recording", 0, 10),
        Err(Error::NotFound)
    ));
    store
        .admit_recording(
            "unpublished",
            "radio:v1",
            1,
            100,
            Retention::Temporary,
            false,
        )?
        .ok_or(Error::RequestState)?;
    assert!(matches!(
        store.admit_retained_reader("unpublished-reader", "unpublished", 0, 10),
        Err(Error::InvalidInput(
            "recording has no verified retained media"
        ))
    ));
    published(&mut store, "completed")?;
    assert!(matches!(
        store.admit_retained_reader("edge-reader", "completed", 1_000_000, 10),
        Err(Error::InvalidInput("seek is outside the retained audio"))
    ));
    store.begin_delete("completed", false)?;
    store.finish_delete("completed")?;
    assert!(matches!(
        store.admit_retained_reader("deleted-reader", "completed", 0, 10),
        Err(Error::InvalidInput(
            "recording has no verified retained media"
        ))
    ));
    assert!(store.retained_readers()?.is_empty());
    Ok(())
}

#[test]
fn open_tail_requires_actual_running_open_reservation_and_creates_no_reader() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    let job = store
        .admit_recording(
            "segmented",
            "radio:v1",
            60,
            OPEN_SEGMENT_CEILING * 2,
            Retention::Temporary,
            false,
        )?
        .ok_or(Error::RequestState)?;
    let version = store.connect_recording(&job.version)?;
    let SegmentOpen::Opened { version, .. } = store.open_segment(&version)? else {
        return Err("segment not opened".into());
    };
    let version = store.seal_segment(
        &version,
        &SegmentSeal {
            bytes: 1000,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 1_000_000,
        },
    )?;
    assert!(matches!(
        store.admit_retained_reader("closed-tail", "segmented", 1_000_000, 10),
        Err(Error::InvalidInput("seek is outside the retained audio"))
    ));
    let SegmentOpen::Opened { .. } = store.open_segment(&version)? else {
        return Err("tail not opened".into());
    };
    for seek in [1_000_000, 1_500_000] {
        assert!(matches!(
            store.admit_retained_reader("open-tail", "segmented", seek, 10),
            Err(Error::InvalidInput("open tail is not readable"))
        ));
    }
    assert!(store.retained_readers()?.is_empty());
    let (spec, _) = store.admit_retained_reader("sealed", "segmented", 500_000, 11)?;
    assert_eq!(spec.file_seek_us, 500_000);
    assert_eq!(store.retained_readers()?.len(), 1);
    Ok(())
}

#[test]
fn gap_cause_refusals_preserve_original_wording_without_admitting_readers() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    let cases = [
        (GapCause::Disconnect, "seek is inside a disconnect gap"),
        (GapCause::Recovery, "seek is inside a recovery gap"),
        (GapCause::CodecChange, "seek is inside a codec change gap"),
        (
            GapCause::RefusedRenewal,
            "seek is inside a refused renewal gap",
        ),
        (GapCause::CapturePause, "seek is inside a capture pause gap"),
        (
            GapCause::BackwardClock,
            "seek is inside a backward clock gap",
        ),
        (
            GapCause::SequenceSkip,
            "seek is inside a skipped sequence gap",
        ),
        (
            GapCause::Discontinuity,
            "seek is inside a discontinuity gap",
        ),
        (
            GapCause::ReloadFailure,
            "seek is inside a reload failure gap",
        ),
    ];
    for (index, (cause, expected)) in cases.into_iter().enumerate() {
        let id = format!("gap-{index}");
        let job = store
            .admit_recording(&id, "radio:v1", 60, 100, Retention::Temporary, false)?
            .ok_or(Error::RequestState)?;
        let mut item = publication();
        item.gap = Some(cause);
        item.end_reason = "stream_gap";
        store.publish_recording(&job.version, &item)?;
        match store.admit_retained_reader(&format!("reader-{index}"), &id, 1_000_000, 10) {
            Err(Error::InvalidInput(reason)) => assert_eq!(reason, expected),
            other => return Err(format!("expected typed gap refusal: {other:?}").into()),
        }
    }
    assert!(store.retained_readers()?.is_empty());
    Ok(())
}
