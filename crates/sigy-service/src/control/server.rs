use std::{future::Future, path::Path, time::Duration};

use interprocess::local_socket::tokio::prelude::*;
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
    time::timeout,
};

use super::{
    Failure, MAX_CLIENTS, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Operation, PROTOCOL_VERSION,
    REQUEST_TIMEOUT, Request, Response, Snapshot,
    actor::{self, Message},
    endpoint::{self, Endpoint},
    frame,
};
use crate::{Error, Result, library::Library, local_stream::LocalStream};

/// Serves until a stop operation or caller shutdown signal. Client exit does
/// not own this lifetime. Library ownership remains held through final cleanup.
/// # Errors
/// Returns endpoint, recovery, listener, or catalog-thread errors.
pub async fn run(library: Library, shutdown: impl Future<Output = ()>) -> Result<()> {
    serve(library, shutdown, REQUEST_TIMEOUT).await
}

/// `deadline` bounds each whole client exchange, including the wait for its close.
async fn serve(
    mut library: Library,
    shutdown: impl Future<Output = ()>,
    deadline: Duration,
) -> Result<()> {
    // The actor may fail. Retain ownership until endpoint cleanup even if its
    // catalog connection has already unwound.
    let _ownership = library.hold_ownership();
    let directory = endpoint::directory(library.directory())?;
    while library.store_mut().recover_submitted()? != 0 {}
    while library.store_mut().recover_captures()? != 0 {}
    library.store_mut().recover_directory_refreshes()?;
    library.store_mut().recover_podcast_refreshes()?;
    library.store_mut().recover_publisher_text()?;
    library.store_mut().recover_playlist_resolves()?;
    library.store_mut().recover_clicks()?;
    library.store_mut().recover_listens()?;
    library.store_mut().recover_analysis_jobs()?;
    library.store_mut().recover_translation_jobs()?;
    crate::recognizer::clear_scratch(library.directory())?;
    crate::recordings::recover_deletions(&mut library)?;
    let endpoint = Endpoint::new()?;
    let listener = endpoint.listen(&directory)?;
    let publication = endpoint.publish(&directory)?;
    let (stopping, mut stopped) = watch::channel(false);
    let (catalog, worker) = actor::spawn(library, stopping)?;
    let mut clients = JoinSet::new();
    let mut retention_tick = tokio::time::interval(std::time::Duration::from_secs(60));
    retention_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Local due checks for recording schedules and a saved directory page.
    // The directory fetch starts only when that page's slot is due.
    let mut schedule_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    schedule_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tokio::pin!(shutdown);
    let result = loop {
        tokio::select! {
            _ = retention_tick.tick() => { let _ = catalog.try_send(Message::Sweep); },
            _ = schedule_tick.tick() => { let _ = catalog.try_send(Message::Schedules); },
            () = &mut shutdown => break Ok(()),
            _ = stopped.changed() => break Ok(()),
            joined = clients.join_next(), if !clients.is_empty() => {
                if joined.is_some_and(|result| result.is_err()) {
                    break Err(Error::Protocol("client handler failed"));
                }
            },
            accepted = listener.accept(), if clients.len() < MAX_CLIENTS => {
                match accepted {
                    Ok(stream) => {
                        let stream = LocalStream::new(stream);
                        let catalog = catalog.clone();
                        clients.spawn(async move {
                            // Per-client faults are isolated. An accepted operation
                            // remains owned by the catalog even after this deadline.
                            // Expiry drops the stream here, with no detached owner.
                            let _ = timeout(deadline, handle(stream, catalog)).await;
                        });
                    },
                    Err(error) => break Err(error.into()),
                }
            }
        }
    };
    drop(listener);
    // Every handler has a fixed deadline. Drain replies before dropping the actor.
    while clients.join_next().await.is_some() {}
    drop(publication);
    let _ = catalog.send(Message::Shutdown).await;
    drop(catalog);
    tokio::task::spawn_blocking(move || worker.join())
        .await
        .map_err(|_| Error::Protocol("catalog join task failed"))?
        .map_err(|_| Error::Protocol("catalog owner failed"))?;
    result
}

async fn handle(mut stream: LocalStream, catalog: mpsc::Sender<Message>) -> Result<()> {
    endpoint::verify_peer(stream.get())?;
    let request: Request = frame::read(&mut stream, MAX_REQUEST_BYTES).await?;
    let response = if request.version == PROTOCOL_VERSION {
        let (reply, response) = oneshot::channel();
        catalog
            .send(Message::Request {
                operation: request.operation,
                reply,
            })
            .await
            .map_err(|_| Error::ServiceStopped)?;
        response.await.map_err(|_| Error::ServiceStopped)?
    } else {
        Response {
            version: PROTOCOL_VERSION,
            result: Err(Failure {
                code: "protocol_version".into(),
                message: "unsupported control protocol version".into(),
            }),
        }
    };
    frame::write(&mut stream, &response, MAX_RESPONSE_BYTES).await?;
    // Close after the client has read the response and closed its end, within
    // the caller's deadline. Older clients also close after their response.
    stream.peer_closed().await
}

/// Sends one bounded local request. No mutation is retried automatically.
/// # Errors
/// Returns identity, protocol, timeout, transport, or application errors.
pub async fn request(directory: &Path, operation: Operation) -> Result<Snapshot> {
    let directory = endpoint::directory(directory)?;
    let endpoint = Endpoint::load(&directory)?;
    // Expiry or cancellation drops the stream inside this future.
    timeout(REQUEST_TIMEOUT, exchange(&endpoint, &directory, operation))
        .await
        .map_err(|_| Error::Timeout)?
}

async fn exchange(endpoint: &Endpoint, directory: &Path, operation: Operation) -> Result<Snapshot> {
    let mut stream = endpoint.connect(directory).await?;
    frame::write(&mut stream, &Request::new(operation), MAX_REQUEST_BYTES).await?;
    let response: Response = frame::read(&mut stream, MAX_RESPONSE_BYTES).await?;
    // The service responds only after reading the whole request, so nothing
    // remains to deliver. Closing now also ends the service's wait.
    drop(stream);
    if response.version != PROTOCOL_VERSION {
        return Err(Error::Protocol("unsupported service protocol version"));
    }
    response
        .result
        .map_err(|failure| Error::Remote(failure.message))
}

#[cfg(test)]
mod exchange_tests;

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bad_clients_and_versions_cannot_mutate_or_stop_the_service()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let directory = temporary.path().to_owned();
        let library = Library::open(&directory, true)?;
        let (shutdown, signal) = oneshot::channel();
        let service = tokio::spawn(run(library, async {
            let _ = signal.await;
        }));
        let endpoint = timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(endpoint) = Endpoint::load(&directory) {
                    break endpoint;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;

        let mut stream = endpoint.connect(&directory).await?;
        frame::write(
            &mut stream,
            &Request {
                version: PROTOCOL_VERSION + 1,
                operation: Operation::SetBudget {
                    scope: "global".into(),
                    limit_usd: "99".into(),
                },
            },
            MAX_REQUEST_BYTES,
        )
        .await?;
        let response: Response = frame::read(&mut stream, MAX_RESPONSE_BYTES).await?;
        assert_eq!(
            response.result.err().map(|error| error.code),
            Some("protocol_version".into())
        );
        drop(stream);

        let mut oversized = endpoint.connect(&directory).await?;
        oversized.write_u32(u32::MAX).await?;
        assert!(
            timeout(
                Duration::from_secs(2),
                frame::read::<Response>(&mut oversized, MAX_RESPONSE_BYTES)
            )
            .await?
            .is_err()
        );
        drop(oversized);

        let mut invalid = endpoint.connect(&directory).await?;
        frame::write(
            &mut invalid,
            &serde_json::json!({"version": PROTOCOL_VERSION, "operation": {"kind": "stop", "unexpected": true}}),
            MAX_REQUEST_BYTES,
        )
        .await?;
        assert!(
            timeout(
                Duration::from_secs(2),
                frame::read::<Response>(&mut invalid, MAX_RESPONSE_BYTES)
            )
            .await?
            .is_err()
        );
        drop(invalid);

        // A client holding a partial header cannot block another client's status.
        let mut stalled = endpoint.connect(&directory).await?;
        stalled.write_all(&[0]).await?;
        let snapshot = request(&directory, Operation::Status {}).await?;
        assert_eq!(snapshot.budgets[0].limit_usd, "0.000000");
        drop(stalled);
        let _ = shutdown.send(());
        timeout(Duration::from_secs(6), service).await???;
        assert!(!directory.join("service.json").exists());
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn client_rejects_a_discovery_record_with_the_wrong_process_identity()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let library = Library::open(temporary.path(), true)?;
        let endpoint = Endpoint::new()?;
        let _listener = endpoint.listen(library.directory())?;
        let _publication = endpoint.publish(library.directory())?;
        let path = library.directory().join("service.json");
        let mut record: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        record["process_id"] = serde_json::json!(u32::MAX);
        std::fs::write(path, serde_json::to_vec(&record)?)?;
        assert!(matches!(
            request(library.directory(), Operation::Stop {}).await,
            Err(Error::Protocol("service process identity mismatch"))
        ));
        Ok(())
    }
}
