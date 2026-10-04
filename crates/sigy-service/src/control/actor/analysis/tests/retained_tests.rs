use super::*;
use crate::control::RetainedOperation;
use tokio::io::AsyncReadExt;

type RetainedCompletion = (
    String,
    u64,
    Result<crate::recordings::retained::RetainedReadReceipt>,
);

fn start(id: &str) -> RetainedOperation {
    RetainedOperation::Start {
        id: id.into(),
        recording_id: "one".into(),
        seek_us: 0,
    }
}

fn fixture_sql(actor: &Actor, sql: &str) -> Result<()> {
    // This is deliberate catalog fault injection under the existing Library
    // owner, not a competing service or filesystem retention actor.
    let connection = rusqlite::Connection::open(actor.library.directory().join("catalog.sqlite3"))?;
    connection.execute_batch(sql)?;
    Ok(())
}

async fn ready(
    actor: &mut Actor,
    receiver: &mut mpsc::Receiver<Message>,
) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let Message::RetainedReady {
        id,
        generation,
        nonce,
    } = completion(receiver).await?
    else {
        return Err("reader did not publish an endpoint".into());
    };
    actor.ready_retained(&id, generation, nonce.clone())?;
    Ok(nonce)
}

async fn finished(
    receiver: &mut mpsc::Receiver<Message>,
) -> std::result::Result<RetainedCompletion, Box<dyn std::error::Error>> {
    let Message::RetainedFinished {
        id,
        generation,
        result,
    } = completion(receiver).await?
    else {
        return Err("reader did not publish completion".into());
    };
    Ok((id, generation, result))
}

#[tokio::test]
async fn retained_original_join_publishes_once_and_replay_never_reattaches() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    let before = serde_json::to_value(actor.library.store().recording("one")?)?;
    let first = actor
        .apply_retained(start("reader"))?
        .retained
        .ok_or("reader page")?;
    assert_eq!(first.newly_started, Some(true));
    let nonce = ready(&mut actor, &mut receiver).await?;
    let replay = actor
        .apply_retained(start("reader"))?
        .retained
        .ok_or("reader replay")?;
    assert_eq!(replay.newly_started, Some(false));
    assert!(replay.pipe_nonce.is_none());
    assert_eq!(actor.retained_workers.len(), 1);
    let mut input = crate::recordings::connect_retained_for_test(root.path(), &nonce).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), input.read_to_end(&mut bytes)).await??;
    assert_eq!(bytes, b"checksum fixture, not decoded speech");
    let (id, generation, result) = finished(&mut receiver).await?;
    actor.finish_retained(&id, generation, result)?;
    assert!(actor.idle());
    assert_eq!(
        actor.library.store().retained_reader("reader")?.state,
        "completed"
    );
    assert_eq!(
        serde_json::to_value(actor.library.store().recording("one")?)?,
        before
    );
    let replay = actor
        .apply_retained(start("reader"))?
        .retained
        .ok_or("terminal replay")?;
    assert_eq!(replay.newly_started, Some(false));
    assert!(replay.pipe_nonce.is_none());
    assert!(actor.retained_workers.is_empty());
    Ok(())
}

#[tokio::test]
async fn excerpt_actor_transfers_whole_original_and_replay_or_changed_end_never_dispatches()
-> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    let command = || RetainedOperation::StartRange {
        id: "excerpt".into(),
        recording_id: "one".into(),
        seek_us: 125_000,
        end_us: 375_000,
    };
    let first = actor
        .apply_retained(command())?
        .retained
        .ok_or("excerpt page")?;
    assert_eq!(first.newly_started, Some(true));
    let spec = first.entries.first().ok_or("excerpt receipt")?.spec.clone();
    assert_eq!(spec.playback_duration_us()?, 250_000);
    assert_eq!(
        spec.bytes,
        b"checksum fixture, not decoded speech".len() as u64
    );
    let nonce = ready(&mut actor, &mut receiver).await?;
    let replay = actor
        .apply_retained(command())?
        .retained
        .ok_or("excerpt replay")?;
    assert_eq!(replay.newly_started, Some(false));
    assert!(replay.pipe_nonce.is_none());
    assert!(matches!(
        actor.apply_retained(RetainedOperation::StartRange {
            id: "excerpt".into(),
            recording_id: "one".into(),
            seek_us: 125_000,
            end_us: 375_001,
        }),
        Err(Error::IdempotencyConflict)
    ));
    assert_eq!(actor.retained_workers.len(), 1);
    let mut input = crate::recordings::connect_retained_for_test(root.path(), &nonce).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), input.read_to_end(&mut bytes)).await??;
    assert_eq!(bytes, b"checksum fixture, not decoded speech");
    let (id, generation, result) = finished(&mut receiver).await?;
    actor.finish_retained(&id, generation, result)?;
    assert!(actor.idle());
    assert_eq!(
        actor.library.store().retained_reader("excerpt")?.state,
        "completed"
    );
    let terminal = actor
        .apply_retained(command())?
        .retained
        .ok_or("terminal replay")?;
    assert_eq!(terminal.newly_started, Some(false));
    assert!(terminal.pipe_nonce.is_none());
    assert!(actor.retained_workers.is_empty());
    Ok(())
}

#[tokio::test]
async fn retained_stop_commits_before_signal_and_stale_generation_does_nothing() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    actor.apply_retained(start("reader"))?;
    let nonce = ready(&mut actor, &mut receiver).await?;
    assert!(
        actor
            .apply_retained(RetainedOperation::Stop {
                id: "reader".into(),
                generation: 2
            })
            .is_err()
    );
    actor.finish_retained("reader", 2, Err(Error::Analysis("worker-panicked")))?;
    assert_eq!(
        actor.library.store().retained_reader("reader")?.state,
        "running"
    );
    assert_eq!(actor.retained_workers.len(), 1);
    fixture_sql(
        &actor,
        "CREATE TRIGGER fixture_reader_cancel_failure BEFORE UPDATE OF state ON retained_readers WHEN NEW.state='cancelling' BEGIN SELECT RAISE(ABORT,'injected cancel failure'); END;",
    )?;
    assert!(
        actor
            .apply_retained(RetainedOperation::Stop {
                id: "reader".into(),
                generation: 1
            })
            .is_err()
    );
    assert_eq!(
        actor.library.store().retained_reader("reader")?.state,
        "running"
    );
    fixture_sql(&actor, "DROP TRIGGER fixture_reader_cancel_failure;")?;
    // Successful original-byte transfer after the rejected mutation proves that
    // its failed stop did not signal the running reader.
    let mut input = crate::recordings::connect_retained_for_test(root.path(), &nonce).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), input.read_to_end(&mut bytes)).await??;
    assert_eq!(bytes, b"checksum fixture, not decoded speech");
    let (id, generation, result) = finished(&mut receiver).await?;
    actor.finish_retained(&id, generation, result)?;
    assert_eq!(
        actor.library.store().retained_reader("reader")?.state,
        "completed"
    );
    actor.apply_retained(start("cancelled-reader"))?;
    ready(&mut actor, &mut receiver).await?;
    actor.apply_retained(RetainedOperation::Stop {
        id: "cancelled-reader".into(),
        generation: 1,
    })?;
    assert_eq!(
        actor
            .library
            .store()
            .retained_reader("cancelled-reader")?
            .state,
        "cancelling"
    );
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete("one", false)
            .is_err()
    );
    let (id, generation, result) = finished(&mut receiver).await?;
    actor.finish_retained(&id, generation, result)?;
    let view = actor.library.store().retained_reader("cancelled-reader")?;
    assert_eq!(view.state, "failed");
    assert_eq!(view.completion_reason.as_deref(), Some("cancelled"));
    assert!(actor.idle());
    Ok(())
}

#[tokio::test]
async fn retained_completion_transaction_failure_is_visible_and_cannot_hang_idle() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    actor.apply_retained(start("reader"))?;
    let nonce = ready(&mut actor, &mut receiver).await?;
    let mut input = crate::recordings::connect_retained_for_test(root.path(), &nonce).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), input.read_to_end(&mut bytes)).await??;
    let (id, generation, result) = finished(&mut receiver).await?;
    fixture_sql(
        &actor,
        "CREATE TRIGGER fixture_reader_finish_failure BEFORE UPDATE OF state ON retained_readers WHEN NEW.state IN ('completed','failed') BEGIN SELECT RAISE(ABORT,'injected finish failure'); END;",
    )?;
    assert!(actor.finish_retained(&id, generation, result).is_err());
    let view = actor.library.store().retained_reader("reader")?;
    assert_eq!(view.state, "recovery_held");
    assert_eq!(
        view.recovery_reason.as_deref(),
        Some("completion-persistence-failed")
    );
    assert!(actor.idle());
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete("one", false)
            .is_err()
    );
    assert_eq!(actor.library.store().recording("one")?.state, "completed");
    assert!(
        actor
            .apply_retained(start("reader"))?
            .retained
            .ok_or("held replay")?
            .pipe_nonce
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn retained_completion_clock_failure_preserves_protection_without_blocking_idle() -> TestResult
{
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    actor.apply_retained(start("reader"))?;
    let before = actor.library.store().retained_reader("reader")?;
    let nonce = ready(&mut actor, &mut receiver).await?;
    let mut input = crate::recordings::connect_retained_for_test(root.path(), &nonce).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), input.read_to_end(&mut bytes)).await??;
    assert_eq!(bytes, b"checksum fixture, not decoded speech");
    let (id, generation, result) = finished(&mut receiver).await?;
    actor.finish_retained_at(
        &id,
        generation + 1,
        Err(Error::Analysis("worker-panicked")),
        Err(Error::InvalidInput("injected completion clock")),
    )?;
    assert_eq!(actor.retained_workers.len(), 1);
    assert!(
        actor
            .finish_retained_at(
                &id,
                generation,
                result,
                Err(Error::InvalidInput("injected completion clock")),
            )
            .is_err()
    );
    assert!(actor.retained_workers.is_empty());
    assert!(actor.idle());
    assert_eq!(actor.library.store().retained_reader("reader")?, before);
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete("one", false)
            .is_err()
    );
    actor
        .library
        .store_mut()
        .recover_retained_readers(before.updated_ms + 1)?;
    let recovered = actor.library.store().retained_reader("reader")?;
    assert_eq!(recovered.state, "recovery_held");
    assert_eq!(recovered.spec, before.spec);
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete("one", false)
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn retained_mismatched_closed_reader_receipt_holds_original_protection() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    actor.apply_retained(start("reader"))?;
    ready(&mut actor, &mut receiver).await?;
    let mut wrong = actor.library.store().retained_reader("reader")?.spec;
    wrong.object_key = "b".repeat(32);
    let (_stop, signal) = watch::channel(false);
    let receipt = crate::recordings::retained::stream_retained(
        root.path().to_owned(),
        wrong,
        actor.library.hold_ownership(),
        signal,
        |_| async { Ok(()) },
    )
    .await?;
    actor.finish_retained("reader", 1, Ok(receipt))?;
    let view = actor.library.store().retained_reader("reader")?;
    assert_eq!(view.state, "recovery_held");
    assert_eq!(
        view.recovery_reason.as_deref(),
        Some("reader-completion-unproven")
    );
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete("one", false)
            .is_err()
    );
    assert!(actor.retained_workers.is_empty());
    Ok(())
}
