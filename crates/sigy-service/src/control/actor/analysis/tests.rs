use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};

use tokio::sync::{mpsc, oneshot, watch};

use super::super::{Actor, Message, dispatch};
use crate::{
    Error, Result,
    control::{AnalysisOperation, Operation, playback::PlaySessions},
    library::Library,
    sources::{HttpHop, HttpSource, NetworkScope, http::HttpAcquirer},
    storage::dvr::{Publication, Retention, hex},
};
use sha2::{Digest, Sha256};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn actor(root: &Path, sender: &mpsc::Sender<Message>) -> Result<Actor> {
    let mut library = Library::open(root, true)?;
    let executable = std::env::current_exe()?;
    library.store_mut().configure_dvr(
        10_000,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    library.store_mut().register_source(
        "radio:v1",
        &HttpSource::new(
            "Fixture",
            "https://example.com/audio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    let bytes = b"checksum fixture, not decoded speech";
    let job = library
        .store_mut()
        .admit_recording("one", "radio:v1", 60, 600, Retention::Temporary, false)?
        .ok_or(Error::StorageIntegrity)?;
    library.store_mut().publish_recording(
        &job.version,
        &Publication {
            bytes: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![HttpHop {
                origin: "https://example.com".into(),
                peer: ([8, 8, 8, 8], 443).into(),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
        },
    )?;
    let recording = library.store().recording("one")?;
    crate::recordings::checked_directory(library.directory(), true)?;
    std::fs::write(
        crate::recordings::media_path(library.directory(), &recording.intervals[0].object_key)?,
        bytes,
    )?;
    library.store_mut().admit_analysis("pin", "one", false, 1)?;
    library.store_mut().publish_analysis("pin", 1)?;
    Ok(Actor {
        library,
        runtime: tokio::runtime::Handle::current(),
        sender: sender.downgrade(),
        workers: HashMap::new(),
        directory_worker: None,
        playlist_worker: None,
        click_worker: None,
        podcast_worker: None,
        text_worker: None,
        analysis_worker: None,
        listen_workers: HashMap::new(),
        playback: PlaySessions::default(),
        acquirer: HttpAcquirer::default(),
    })
}

async fn completion(
    receiver: &mut mpsc::Receiver<Message>,
) -> std::result::Result<Message, Box<dyn std::error::Error>> {
    Ok(
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await?
            .ok_or("worker did not deliver completion")?,
    )
}

fn deliver(actor: &mut Actor, message: Message) -> (bool, bool) {
    let (stopping, _) = watch::channel(false);
    let mut stopped = false;
    let mut shutdown = false;
    dispatch(
        actor,
        message,
        &stopping,
        &mut stopped,
        &mut shutdown,
        Instant::now(),
    );
    (stopped, *stopping.borrow())
}

#[tokio::test]
async fn stale_completion_cannot_clear_the_worker_and_replay_does_not_dispatch() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    actor.start_verification("verify", "pin", 1)?;
    assert!(!actor.idle());
    actor.finish_verification("verify", 2, Err(Error::Analysis("cancelled")))?;
    actor.finish_verification("other", 1, Err(Error::Analysis("cancelled")))?;
    assert!(actor.analysis_worker.is_some());
    actor.start_verification("verify", "pin", 1)?;
    assert!(matches!(
        actor.start_verification("second", "pin", 1),
        Err(Error::Analysis("worker-busy"))
    ));
    // A catalog read still works while the blocking checksum worker owns its slot.
    assert!(
        actor
            .apply(Operation::Analysis {
                command: AnalysisOperation::Show { id: "pin".into() }
            })?
            .analysis
            .is_some()
    );
    let message = completion(&mut receiver).await?;
    assert_eq!(deliver(&mut actor, message), (false, false));
    assert!(actor.idle());
    assert_eq!(
        actor.library.store().analysis_job("verify")?.state,
        "verified"
    );
    actor.start_verification("verify", "pin", 1)?;
    assert!(actor.idle());
    assert!(receiver.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn accepted_cancel_is_durable_until_actual_completion() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    actor.start_verification("verify", "pin", 1)?;
    actor.cancel_verification("verify", 1)?;
    assert!(!actor.idle());
    assert_eq!(
        actor.library.store().analysis_job("verify")?.state,
        "cancelling"
    );
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete("one", false)
            .is_err()
    );
    let message = completion(&mut receiver).await?;
    assert_eq!(deliver(&mut actor, message), (false, false));
    assert!(actor.idle());
    assert_eq!(
        actor.library.store().analysis_job("verify")?.state,
        "cancelled"
    );
    Ok(())
}

#[tokio::test]
async fn failed_spawn_compensates_the_admitted_lease() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    drop(sender);
    assert!(matches!(
        actor.start_verification("verify", "pin", 1),
        Err(Error::ServiceStopped)
    ));
    let job = actor.library.store().analysis_job("verify")?;
    assert_eq!(job.state, "failed");
    assert_eq!(job.reason.as_deref(), Some("worker-start-failed"));
    assert!(actor.idle());
    Ok(())
}

#[tokio::test]
async fn terminal_commit_failure_stops_admission_and_drains_shutdown() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    let fault = rusqlite::Connection::open(root.path().join("catalog.sqlite3"))?;
    fault.execute_batch(
        "CREATE TRIGGER reject_finish BEFORE UPDATE ON analysis_jobs
        WHEN NEW.state = 'verified' BEGIN SELECT RAISE(ABORT, 'injected terminal failure'); END;",
    )?;
    actor.start_verification("verify", "pin", 1)?;
    let message = completion(&mut receiver).await?;
    assert_eq!(deliver(&mut actor, message), (true, true));
    assert!(actor.idle());
    assert_eq!(
        actor.library.store().analysis_job("verify")?.state,
        "running"
    );
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete("one", false)
            .is_err()
    );
    let (reply, response) = oneshot::channel();
    let (stopping, _) = watch::channel(false);
    let mut stopped = true;
    let mut shutdown = false;
    dispatch(
        &mut actor,
        Message::Request {
            operation: Operation::Analysis {
                command: AnalysisOperation::Verify {
                    id: "second".into(),
                    input: "pin".into(),
                    revision: 1,
                },
            },
            reply,
        },
        &stopping,
        &mut stopped,
        &mut shutdown,
        Instant::now(),
    );
    assert!(response.await?.result.is_err());
    dispatch(
        &mut actor,
        Message::Shutdown,
        &stopping,
        &mut stopped,
        &mut shutdown,
        Instant::now(),
    );
    assert!(shutdown && actor.idle());
    fault.execute_batch("DROP TRIGGER reject_finish;")?;
    drop(fault);
    drop(actor);
    let mut reopened = Library::open(root.path(), false)?;
    reopened.store_mut().recover_analysis_jobs()?;
    let job = reopened.store().analysis_job("verify")?;
    assert_eq!(job.state, "interrupted");
    assert_eq!(job.generation, 2);
    assert!(
        reopened
            .store_mut()
            .admit_verification("verify", "pin", 1, 2)?
            .1
            .is_none()
    );
    Ok(())
}
