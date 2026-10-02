//! Transport lifetime of one control exchange, observed from the peer.

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::local_stream::tests::peer_end_closes;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

async fn published(directory: &Path) -> Result<Endpoint> {
    timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(endpoint) = Endpoint::load(directory) {
                break endpoint;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| Error::Timeout)
}

async fn send(stream: &mut LocalStream, operation: Operation) -> Result<()> {
    frame::write(stream, &Request::new(operation), MAX_REQUEST_BYTES).await
}

async fn read_snapshot(stream: &mut LocalStream) -> Result<Snapshot> {
    let response: Response = frame::read(stream, MAX_RESPONSE_BYTES).await?;
    assert_eq!(response.version, PROTOCOL_VERSION);
    response
        .result
        .map_err(|failure| Error::Remote(failure.message))
}

async fn end_of_stream(stream: &mut LocalStream) -> Result<Vec<u8>> {
    let mut rest = Vec::new();
    timeout(Duration::from_secs(10), stream.read_to_end(&mut rest))
        .await
        .map_err(|_| Error::Timeout)??;
    Ok(rest)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn complete_exchanges_end_on_client_close_not_on_the_deadline() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let directory = temporary.path().to_owned();
    let library = Library::open(&directory, true)?;
    // A handler that waited for this deadline would hold shutdown far beyond
    // the bounds below.
    let service = tokio::spawn(serve(
        library,
        std::future::pending(),
        Duration::from_secs(120),
    ));
    let endpoint = published(&directory).await?;

    let snapshot = request(&directory, Operation::Status {}).await?;
    assert_eq!(snapshot.budgets[0].limit_usd, "0.000000");

    // The unchanged v43 sequence: one request frame, one response frame, then
    // close. The service asks nothing more of an older client.
    let mut older = endpoint.connect(&directory).await?;
    send(&mut older, Operation::Status {}).await?;
    let snapshot = read_snapshot(&mut older).await?;
    assert!(snapshot.service.is_some());
    drop(older);

    let stopping = request(&directory, Operation::Stop {}).await?;
    assert!(stopping.service.is_some());
    timeout(Duration::from_secs(30), service).await???;
    assert!(!directory.join("service.json").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_and_hostile_peers_are_released_at_the_deadline() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let directory = temporary.path().to_owned();
    let library = Library::open(&directory, true)?;
    let service = tokio::spawn(serve(
        library,
        std::future::pending(),
        Duration::from_secs(3),
    ));
    let endpoint = published(&directory).await?;

    // Closes early with a partial frame.
    let mut early = endpoint.connect(&directory).await?;
    early.write_all(&[0]).await?;
    drop(early);

    // Appends a byte after its request: the response stays complete, then the
    // service refuses the extra byte and closes.
    let mut extra = endpoint.connect(&directory).await?;
    send(&mut extra, Operation::Status {}).await?;
    extra.write_all(b"x").await?;
    read_snapshot(&mut extra).await?;
    assert!(end_of_stream(&mut extra).await?.is_empty());

    // Reads only the length prefix of its response.
    let mut partial = endpoint.connect(&directory).await?;
    send(&mut partial, Operation::Status {}).await?;
    let length = usize::try_from(partial.read_u32().await?)?;

    // Reads its whole response but never closes.
    let mut lingering = endpoint.connect(&directory).await?;
    send(&mut lingering, Operation::Status {}).await?;
    read_snapshot(&mut lingering).await?;

    // Stalled peers do not block other clients.
    request(&directory, Operation::Status {}).await?;

    // The exact stop path, from a client that does not read its response.
    let mut silent = endpoint.connect(&directory).await?;
    send(&mut silent, Operation::Stop {}).await?;

    // Shutdown drains every handler, so completion means each one has ended.
    timeout(Duration::from_secs(30), service).await???;

    // A response larger than the pipe buffer is a pending native write. It
    // holds no task or thread and completes once the client reads it.
    let stopped = read_snapshot(&mut silent).await?;
    assert!(stopped.service.is_some());
    assert!(end_of_stream(&mut silent).await?.is_empty());

    assert!(peer_end_closes(&mut partial, Duration::from_secs(10)).await);
    let mut body = vec![0; length];
    partial.read_exact(&mut body).await?;
    let response: Response = serde_json::from_slice(&body)?;
    assert!(response.result.is_ok());
    assert!(end_of_stream(&mut partial).await?.is_empty());

    assert!(end_of_stream(&mut lingering).await?.is_empty());
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_never_reads_a_buffered_response_is_released() -> TestResult {
    use crate::local_stream::tests::synchronous;

    let temporary = tempfile::tempdir()?;
    let directory = temporary.path().to_owned();
    let library = Library::open(&directory, true)?;
    let service = tokio::spawn(serve(
        library,
        std::future::pending(),
        Duration::from_secs(2),
    ));
    published(&directory).await?;
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("service.json"))?)?;
    let nonce = record["nonce"].as_str().ok_or("endpoint nonce")?;
    let file = synchronous::open(&format!("sigy-{nonce}")).await?;

    // A refused version receives a refusal small enough for the pipe buffer.
    // The client posts no read, so the refusal stays unread in that buffer.
    let body = serde_json::to_vec(&Request {
        version: PROTOCOL_VERSION + 1,
        operation: Operation::Status {},
    })?;
    let mut bytes = u32::try_from(body.len())?.to_be_bytes().to_vec();
    bytes.extend(body);
    let file = synchronous::write(file, bytes).await?;

    request(&directory, Operation::Stop {}).await?;
    timeout(Duration::from_secs(30), service).await???;

    // A detached flush would hold the service end until the refusal was read.
    let (closed, file) = synchronous::server_end_closes(file, synchronous::PROBE_WINDOW).await?;
    assert!(
        closed,
        "a detached owner still holds the service end of an unread response"
    );
    let remaining = synchronous::remaining(file).await?;
    let (length, body) = remaining.split_at_checked(4).ok_or("response frame")?;
    let length: [u8; 4] = length.try_into()?;
    assert_eq!(usize::try_from(u32::from_be_bytes(length))?, body.len());
    let response: Response = serde_json::from_slice(body)?;
    assert_eq!(
        response.result.err().map(|failure| failure.code),
        Some("protocol_version".into())
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_client_closes_after_a_response_or_cancellation() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let mut library = Library::open(temporary.path(), true)?;
    let snapshot = super::super::apply_library(&mut library, Operation::Status {})?;
    let endpoint = Endpoint::new()?;
    let listener = endpoint.listen(library.directory())?;
    let _publication = endpoint.publish(library.directory())?;
    let directory = library.directory().to_owned();

    // This peer answers without reading the request.
    let answering = tokio::spawn(async move {
        let mut stream = LocalStream::new(listener.accept().await?);
        let response = Response {
            version: PROTOCOL_VERSION,
            result: Ok(snapshot),
        };
        frame::write(&mut stream, &response, MAX_RESPONSE_BYTES).await?;
        Ok::<_, Error>((listener, stream))
    });
    let answered = request(&directory, Operation::Status {}).await?;
    assert_eq!(answered.budgets[0].limit_usd, "0.000000");
    let (listener, mut stream) = answering.await??;
    assert!(peer_end_closes(&mut stream, Duration::from_secs(10)).await);
    // The request frame is the unchanged v43 encoding.
    let sent: Request = frame::read(&mut stream, MAX_REQUEST_BYTES).await?;
    assert_eq!(sent.version, PROTOCOL_VERSION);
    assert!(matches!(sent.operation, Operation::Status {}));
    assert!(end_of_stream(&mut stream).await?.is_empty());

    // Cancelled while its response read is pending.
    let silent =
        tokio::spawn(async move { Ok::<_, Error>(LocalStream::new(listener.accept().await?)) });
    assert!(
        timeout(
            Duration::from_millis(300),
            request(&directory, Operation::Stop {})
        )
        .await
        .is_err()
    );
    let mut stream = silent.await??;
    assert!(peer_end_closes(&mut stream, Duration::from_secs(10)).await);
    let sent: Request = frame::read(&mut stream, MAX_REQUEST_BYTES).await?;
    assert!(matches!(sent.operation, Operation::Stop {}));
    Ok(())
}
