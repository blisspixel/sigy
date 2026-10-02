//! A failed quota reclamation cannot strand an earlier committed schedule admission.

use super::*;
use crate::storage::schedules::ScheduleDraft;

#[tokio::test]
async fn committed_schedule_is_supervised_before_later_reclamation_fails() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(directory.path(), &sender)?;
    let decoder = std::env::current_exe()?;
    actor.library.store_mut().configure_dvr(
        64 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        decoder.to_str().ok_or("decoder path")?,
    )?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    actor.library.store_mut().register_source(
        "local:v1",
        &HttpSource::new(
            "Local",
            &format!("http://{}/audio", listener.local_addr()?),
            NetworkScope::PinnedAddress {
                address: "127.0.0.1".parse()?,
            },
        )?,
    )?;
    let now = crate::storage::now_ms()?;
    let clock = jiff::Timestamp::from_millisecond(now - 1000)?.to_zoned(jiff::tz::TimeZone::UTC);
    for (id, maximum_bytes) in [("a-first", 1_048_576), ("z-full", 64 * 1024 * 1024)] {
        actor.library.store_mut().create_schedule_at(
            &ScheduleDraft {
                id: id.into(),
                source_revision: "local:v1".into(),
                zone: "Etc/UTC".into(),
                recurrence: "once".into(),
                civil_date: Some(clock.date().to_string()),
                weekday: None,
                hour: i64::from(clock.hour()),
                minute: i64::from(clock.minute()),
                second: i64::from(clock.second()),
                duration_seconds: 60,
                maximum_bytes,
            },
            now,
        )?;
    }
    assert!(matches!(
        actor.reconcile_schedules(),
        Err(Error::StorageQuota)
    ));
    let first = actor.library.store().schedule_occurrences("a-first")?;
    let id = first[0]
        .recording_id
        .as_ref()
        .ok_or("first schedule not admitted")?;
    assert_eq!(first[0].state, "admitted");
    assert!(
        actor.workers.contains_key(id),
        "a committed launch must have its supervisor before reclaim fails"
    );
    let second = actor.library.store().schedule_occurrences("z-full")?;
    assert_eq!(second[0].state, "waiting");
    assert!(second[0].recording_id.is_none());
    actor.stop();
    let message = completion(&mut receiver).await?;
    assert!(matches!(message, Message::Finished { .. }));
    let (stopped, _) = deliver(&mut actor, message);
    assert!(!stopped);
    assert!(actor.workers.is_empty());
    assert!(matches!(
        actor.library.store().recording(id)?.state.as_str(),
        "failed" | "interrupted" | "cancelled"
    ));
    Ok(())
}

#[tokio::test]
async fn settlement_fault_is_visible_after_remaining_committed_launches_are_visited() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let (sender, _) = mpsc::channel(8);
    let mut actor = actor(directory.path(), &sender)?;
    let decoder = directory.path().join("fixture-decoder.exe");
    std::fs::write(&decoder, b"fixture file, never executed")?;
    actor.library.store_mut().configure_dvr(
        10_000,
        64 * 1024 * 1024,
        14,
        decoder.to_str().ok_or("decoder path")?,
    )?;
    let now = crate::storage::now_ms()?;
    let clock = jiff::Timestamp::from_millisecond(now - 1000)?.to_zoned(jiff::tz::TimeZone::UTC);
    for id in ["a-first", "b-second"] {
        actor.library.store_mut().create_schedule_at(
            &ScheduleDraft {
                id: id.into(),
                source_revision: "radio:v1".into(),
                zone: "Etc/UTC".into(),
                recurrence: "once".into(),
                civil_date: Some(clock.date().to_string()),
                weekday: None,
                hour: i64::from(clock.hour()),
                minute: i64::from(clock.minute()),
                second: i64::from(clock.second()),
                duration_seconds: 60,
                maximum_bytes: 1000,
            },
            now,
        )?;
    }
    let batch = actor
        .library
        .store_mut()
        .reconcile_schedules_at(now, true)?;
    assert_eq!(batch.launches.len(), 2);
    let first = batch.launches[0].job.version.id().to_owned();
    let second = batch.launches[1].job.version.id().to_owned();
    let before = actor.library.store().dvr_status()?.available_bytes;
    std::fs::remove_file(&decoder)?;
    let fault = rusqlite::Connection::open(directory.path().join("catalog.sqlite3"))?;
    fault.execute_batch("CREATE TRIGGER schedule_settlement_fault BEFORE INSERT ON capture_events WHEN NEW.event = 'fail' AND NEW.job_id LIKE 'a-first:%' BEGIN SELECT RAISE(ABORT, 'injected schedule settlement fault'); END")?;
    assert!(matches!(
        actor.spawn_schedule_launches(batch.launches),
        Err(Error::Database(_))
    ));
    assert!(actor.workers.is_empty());
    assert_eq!(actor.library.store().recording(&first)?.state, "starting");
    assert_eq!(actor.library.store().recording(&second)?.state, "failed");
    assert_eq!(actor.library.store().dvr_status()?.available_bytes, before);
    fault.execute_batch("DROP TRIGGER schedule_settlement_fault")?;
    drop(fault);
    drop(actor);
    let mut reopened = Library::open(directory.path(), false)?;
    assert_eq!(reopened.store_mut().recover_captures()?, 1);
    assert_eq!(reopened.store().recording(&first)?.state, "interrupted");
    assert_eq!(reopened.store().recording(&second)?.state, "failed");
    assert_eq!(reopened.store().dvr_status()?.available_bytes, before);
    assert!(
        reopened
            .store_mut()
            .reconcile_schedules_at(now + 1, true)?
            .launches
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn spawn_setup_failure_already_settled_is_not_a_second_transition() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (sender, _) = mpsc::channel(8);
    let mut actor = actor(directory.path(), &sender)?;
    let decoder = std::env::current_exe()?;
    let floor = fs4::available_space(directory.path())?
        .checked_add(1 << 30)
        .ok_or("free-space floor")?;
    actor.library.store_mut().configure_dvr(
        10_000,
        floor,
        14,
        decoder.to_str().ok_or("decoder path")?,
    )?;
    let now = crate::storage::now_ms()?;
    let clock = jiff::Timestamp::from_millisecond(now - 1000)?.to_zoned(jiff::tz::TimeZone::UTC);
    actor.library.store_mut().create_schedule_at(
        &ScheduleDraft {
            id: "setup-fail".into(),
            source_revision: "radio:v1".into(),
            zone: "Etc/UTC".into(),
            recurrence: "once".into(),
            civil_date: Some(clock.date().to_string()),
            weekday: None,
            hour: i64::from(clock.hour()),
            minute: i64::from(clock.minute()),
            second: i64::from(clock.second()),
            duration_seconds: 60,
            maximum_bytes: 1000,
        },
        now,
    )?;
    let batch = actor
        .library
        .store_mut()
        .reconcile_schedules_at(now, true)?;
    assert_eq!(batch.launches.len(), 1);
    let token = batch.launches[0].job.version.clone();
    let before = actor.library.store().dvr_status()?.available_bytes;
    actor.spawn_schedule_launches(batch.launches)?;
    assert!(actor.workers.is_empty());
    let stored = actor
        .library
        .store()
        .capture(token.id())?
        .ok_or("missing capture")?;
    assert_eq!(stored.state, sigy_core::capture::CaptureState::Failed);
    assert_eq!(stored.version.generation(), token.generation());
    assert_eq!(
        Some(stored.version.revision()),
        token.revision().checked_add(1)
    );
    let receipts = actor
        .library
        .store()
        .capture_history(token.id(), Some(token.revision()), 8)?;
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0].event,
        Some(sigy_core::capture::CaptureEvent::Fail)
    );
    assert_eq!(actor.library.store().dvr_status()?.available_bytes, before);
    Ok(())
}
