use super::*;
use crate::{
    sources::{HttpSource, NetworkScope},
    storage::dvr::{Publication, Retention},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;
mod capacity;
mod completion;
mod migration;
mod refusals;
mod segments;

fn configure(store: &mut Store) -> Result<()> {
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        executable
            .to_str()
            .ok_or(Error::InvalidInput("test executable"))?,
    )?;
    store.register_source(
        "radio:v1",
        &HttpSource::new(
            "Reader fixture",
            "https://example.com/audio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    Ok(())
}

fn published(store: &mut Store, id: &str) -> Result<()> {
    let job = store
        .admit_recording(id, "radio:v1", 1, 100, Retention::Temporary, false)?
        .ok_or(Error::RequestState)?;
    store.publish_recording(&job.version, &publication())?;
    Ok(())
}

fn publication() -> Publication {
    Publication {
        bytes: 20,
        sha256: "a".repeat(64),
        format: "wav",
        decoded_microseconds: 1_000_000,
        end_reason: "end_of_body",
        http_route: vec![crate::sources::HttpHop {
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
fn frozen_spec_hash_matches_independent_big_endian_reference_and_refuses_overflow() -> TestResult {
    // Independently encoded and hashed with the platform SHA-256 implementation.
    let hash = spec_digest(
        [
            "reader",
            "one",
            "radio:v1",
            &"a".repeat(32),
            &"b".repeat(64),
            "wav",
        ],
        [1, 0, 20, 30_000_000, 31_000_000, 250_000, 1_000_000],
    )?;
    assert_eq!(
        crate::storage::dvr::hex(&hash.finalize()),
        "7e4233e45cdf0b23e24ee88dd96238f7c66937bf9729c2edf96422193497e1f2"
    );
    assert!(
        spec_digest(
            [
                &"a".repeat(1024),
                "one",
                "radio:v1",
                "object",
                "checksum",
                "wav"
            ],
            [1, 0, 20, 0, 1, 0, 1]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn exact_reader_replay_capacity_and_restart_keep_retention_protected() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    configure(&mut store)?;
    published(&mut store, "recording")?;
    let (spec, fresh) = store.admit_retained_reader("reader", "recording", 250_000, 10)?;
    assert!(fresh);
    assert_eq!(
        (spec.file_seek_us, spec.file_duration_us, spec.bytes),
        (250_000, 1_000_000, 20)
    );
    assert_eq!(
        store.admit_retained_reader("reader", "recording", 250_000, -1)?,
        (spec.clone(), false)
    );
    assert!(matches!(
        store.admit_retained_reader("reader", "recording", u64::MAX, 11),
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        store.admit_retained_reader("outside", "recording", 1_000_000, 11),
        Err(Error::InvalidInput("seek is outside the retained audio"))
    ));
    for id in ["reader2", "reader3", "reader4"] {
        store.admit_retained_reader(id, "recording", 0, 11)?;
    }
    let regressed = store.cancel_retained_reader("reader2", 1, 10)?;
    assert_eq!(regressed.updated_ms, 11);
    assert_eq!(regressed.state, "cancelling");
    assert_eq!(store.cancel_retained_reader("reader2", 1, -1)?, regressed);
    assert!(
        store
            .admit_retained_reader("fifth", "recording", 0, 12)
            .is_err()
    );
    assert!(matches!(
        store.retained_reader("fifth"),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.begin_delete("recording", false),
        Err(Error::RequestState)
    ));
    assert!(store.prune_candidates(true)?.is_empty());
    assert!(store.next_segment_release(true, 12)?.is_none());
    assert!(
        store
            .connection
            .execute(
                "UPDATE recordings SET storage_state='deleted' WHERE id='recording'",
                []
            )
            .is_err()
    );
    assert!(
        store
            .connection
            .execute(
                "INSERT INTO recording_releases VALUES('recording',0,20)",
                []
            )
            .is_err()
    );
    assert!(matches!(
        store.cancel_retained_reader("reader", 2, 12),
        Err(Error::RequestState)
    ));
    assert_eq!(
        store.cancel_retained_reader("reader", 1, 12)?.state,
        "cancelling"
    );
    let held = store.hold_retained_reader("reader", 1, "completion-unproven", 1)?;
    assert_eq!(held.state, "recovery_held");
    assert_eq!(held.updated_ms, 12);
    assert_eq!(held.recovery_reason.as_deref(), Some("completion-unproven"));
    assert!(
        store
            .admit_retained_reader("fifth", "recording", 0, 13)
            .is_err()
    );
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.recover_retained_readers(13)?, 3);
    assert_eq!(store.recover_retained_readers(1)?, 0);
    assert_eq!(store.retained_reader("reader")?.state, "recovery_held");
    assert_eq!(
        store.admit_retained_reader("reader", "recording", 250_000, 14)?,
        (spec, false)
    );
    assert!(store.begin_delete("recording", false).is_err());
    assert!(
        store
            .admit_retained_reader("fifth", "recording", 0, 14)
            .is_err()
    );
    store.audit_retained_readers()?;
    Ok(())
}

#[test]
fn corrupt_oversized_metadata_refuses_before_copy_and_guard_is_cleaned() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    configure(&mut store)?;
    published(&mut store, "recording")?;
    store.admit_retained_reader("reader", "recording", 0, 10)?;
    let original = store.retained_reader("reader")?.spec;
    store.connection.execute_batch(
        "DROP TRIGGER retained_readers_identity; PRAGMA ignore_check_constraints=ON;",
    )?;
    store.connection.execute(
        "UPDATE retained_readers SET sha256=?1 WHERE id='reader'",
        ["a".repeat(65)],
    )?;
    assert!(matches!(
        store.retained_reader("reader"),
        Err(Error::StorageIntegrity)
    ));
    store.connection.execute(
        "UPDATE retained_readers SET sha256=?1 WHERE id='reader'",
        ["a".repeat(64)],
    )?;
    store
        .connection
        .execute_batch("PRAGMA ignore_check_constraints=OFF;")?;
    assert_eq!(store.retained_reader("reader")?.state, "running");
    let mut forged = original.clone();
    forged.sha256 = "b".repeat(64);
    forged.spec_sha256 = forged.digest()?;
    store.connection.execute(
        "UPDATE retained_readers SET sha256=?1,spec_sha256=?2 WHERE id='reader'",
        rusqlite::params![forged.sha256, forged.spec_sha256],
    )?;
    assert!(matches!(
        store.audit_retained_readers(),
        Err(Error::StorageIntegrity)
    ));
    assert!(matches!(
        store.retained_reader("reader"),
        Err(Error::StorageIntegrity)
    ));
    store.connection.execute(
        "UPDATE retained_readers SET sha256=?1,spec_sha256=?2 WHERE id='reader'",
        rusqlite::params![original.sha256, original.spec_sha256],
    )?;
    store.audit_retained_readers()?;
    assert_eq!(store.retained_readers()?.len(), 1);
    Ok(())
}
