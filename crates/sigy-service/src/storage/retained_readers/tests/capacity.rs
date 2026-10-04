//! Synthetic catalog layout only. These rows are not physical close receipts.
//! Real completion capability is exercised independently by worker/actor tests.

use super::*;
use std::time::Instant;

#[test]
fn maximum_reader_history_layout_audit_and_cleanup_characterization() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    configure(&mut store)?;
    published(&mut store, "recording")?;
    let (base, _) = store.admit_retained_reader("layout-0000", "recording", 0, 10)?;
    let tx = store.connection.transaction()?;
    tx.execute("UPDATE retained_readers SET state='failed',completion_reason='input-unavailable' WHERE id='layout-0000'",[])?;
    for index in 1..4096 {
        let mut spec = base.clone();
        spec.request_id = format!("layout-{index:04}");
        if index % 2 == 1 {
            spec.excerpt = Some(RetainedExcerpt {
                version: 2,
                timeline_end_us: 500_000,
                citation: None,
            });
        }
        spec.spec_sha256 = spec.digest()?;
        super::super::write::insert(&tx, &spec, 0, 10)?;
        tx.execute("UPDATE retained_readers SET state='failed',completion_reason='input-unavailable' WHERE id=?1",[&spec.request_id])?;
    }
    tx.commit()?;
    let started = Instant::now();
    let audited = store.audit_retained_readers();
    eprintln!(
        "synthetic4096 reader history audit {:?}, accepted={}",
        started.elapsed(),
        audited.is_ok()
    );
    audited?;
    let started = Instant::now();
    assert_eq!(store.retained_readers()?.len(), 16);
    eprintln!(
        "synthetic4096 reader history bounded list {:?}",
        started.elapsed()
    );
    assert!(matches!(
        store.retained_reader("absent"),
        Err(Error::NotFound)
    ));
    assert!(
        store
            .admit_retained_reader("beyond-lifetime", "recording", 0, 11)
            .is_err()
    );
    assert_eq!(store.retained_reader("layout-4095")?.state, "failed");
    assert!(
        store
            .admit_retained_range("range-beyond-lifetime", "recording", 125_000, 375_000, 11)
            .is_err()
    );
    assert!(store.retained_reader("layout-4095")?.spec.excerpt.is_some());
    drop(store);
    let store = Store::open(&path)?;
    assert_eq!(store.retained_readers()?.len(), 16);
    Ok(())
}
