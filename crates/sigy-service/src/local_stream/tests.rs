use std::time::Duration;

use interprocess::local_socket::{
    ListenerOptions, Name,
    tokio::{Listener, prelude::*},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

use super::*;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// One listener and its unique name. Unix keeps the socket in a private directory.
struct TestEndpoint {
    listener: Listener,
    name: Name<'static>,
    #[cfg(windows)]
    pipe: String,
    _directory: tempfile::TempDir,
}

impl TestEndpoint {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| "random source")?;
        let nonce = format!("{:032x}", u128::from_be_bytes(random));
        #[cfg(windows)]
        let pipe = format!("sigy-test-{nonce}");
        #[cfg(windows)]
        let name = {
            use interprocess::local_socket::GenericNamespaced;
            pipe.clone().to_ns_name::<GenericNamespaced>()?
        };
        #[cfg(unix)]
        let name = {
            use interprocess::local_socket::GenericFilePath;
            directory
                .path()
                .join(format!("{nonce}.sock"))
                .to_fs_name::<GenericFilePath>()?
        };
        let listener = ListenerOptions::new().name(name.clone()).create_tokio()?;
        Ok(Self {
            listener,
            name,
            #[cfg(windows)]
            pipe,
            _directory: directory,
        })
    }

    async fn pair(
        &self,
    ) -> std::result::Result<(LocalStream, LocalStream), Box<dyn std::error::Error>> {
        let (accepted, connected) =
            tokio::join!(self.listener.accept(), Stream::connect(self.name.clone()));
        Ok((LocalStream::new(accepted?), LocalStream::new(connected?)))
    }
}

/// Writes one byte at a time until the peer's end is gone. A detached owner
/// that kept the peer's pipe open would keep these writes succeeding.
pub(crate) async fn peer_end_closes(stream: &mut LocalStream, within: Duration) -> bool {
    timeout(within, async {
        loop {
            if stream.write_all(&[0]).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

/// A synchronous Windows client end. Unlike a runtime stream it posts no read
/// of its own, so bytes written to it stay unread in the pipe buffer.
#[cfg(windows)]
pub(crate) mod synchronous {
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Write},
        time::{Duration, Instant},
    };

    pub(crate) async fn open(pipe: &str) -> std::io::Result<File> {
        let path = format!(r"\\.\pipe\{pipe}");
        tokio::task::spawn_blocking(move || OpenOptions::new().read(true).write(true).open(path))
            .await
            .map_err(std::io::Error::other)?
    }

    pub(crate) async fn write(mut file: File, bytes: Vec<u8>) -> std::io::Result<File> {
        tokio::task::spawn_blocking(move || file.write_all(&bytes).map(|()| file))
            .await
            .map_err(std::io::Error::other)?
    }

    /// Writes single bytes until the server end is gone. At most 200 bytes are
    /// sent, below the 512-byte pipe buffer, so a write never blocks.
    pub(crate) async fn server_end_closes(
        mut file: File,
        within: Duration,
    ) -> std::io::Result<(bool, File)> {
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + within;
            for _ in 0..200 {
                if file.write_all(&[0]).is_err() {
                    return (true, file);
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(within / 200);
            }
            (false, file)
        })
        .await
        .map_err(std::io::Error::other)
    }

    /// Reads what remains, then end of stream.
    pub(crate) async fn remaining(mut file: File) -> std::io::Result<Vec<u8>> {
        tokio::task::spawn_blocking(move || {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map(|_| bytes)
        })
        .await
        .map_err(std::io::Error::other)?
    }

    pub(crate) const PROBE_WINDOW: Duration = Duration::from_secs(5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_bytes_reach_a_late_reader_after_the_writer_is_dropped() -> TestResult {
    let endpoint = TestEndpoint::new()?;
    let (writer, mut reader) = endpoint.pair().await?;
    let payload: Vec<u8> = (0..(3 * 1024 * 1024 + 17))
        .map(|index: u32| u8::try_from(index % 251).unwrap_or(0))
        .collect();
    let received = deliver_then_drop(writer, &mut reader, payload.clone()).await?;
    assert_eq!(received.len(), payload.len());
    assert!(
        received == payload,
        "received bytes differ from the payload"
    );
    Ok(())
}

/// Writes `payload`, drops the writer, and reads to end of stream. Windows
/// accepts the whole buffer at once, so the native write is still pending when
/// the writer is dropped and the reader starts only afterwards. A Unix socket
/// delivers it while the reader drains.
pub(crate) async fn deliver_then_drop(
    mut writer: LocalStream,
    reader: &mut LocalStream,
    payload: Vec<u8>,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut writing = tokio::spawn(async move {
        writer.write_all(&payload).await?;
        drop(writer);
        Ok::<_, std::io::Error>(())
    });
    #[cfg(windows)]
    timeout(Duration::from_secs(10), &mut writing).await???;
    let mut received = Vec::new();
    timeout(Duration::from_secs(20), reader.read_to_end(&mut received)).await??;
    #[cfg(unix)]
    timeout(Duration::from_secs(10), &mut writing).await???;
    Ok(received)
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_writer_closes_although_its_peer_never_read() -> TestResult {
    let endpoint = TestEndpoint::new()?;
    let (accepted, file) = tokio::join!(
        endpoint.listener.accept(),
        synchronous::open(&endpoint.pipe)
    );
    let mut writer = LocalStream::new(accepted?);
    // Fits the pipe buffer: the native write completes and the bytes stay unread.
    timeout(Duration::from_secs(5), writer.write_all(b"unread response")).await??;
    drop(writer);
    // A detached flush would hold the writer's end open until these bytes were read.
    let (closed, file) = synchronous::server_end_closes(file?, synchronous::PROBE_WINDOW).await?;
    assert!(closed, "a detached owner still holds the dropped pipe");
    assert_eq!(synchronous::remaining(file).await?, b"unread response");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_writer_leaves_its_bytes_readable_then_ends() -> TestResult {
    let endpoint = TestEndpoint::new()?;
    let (mut writer, mut peer) = endpoint.pair().await?;
    timeout(Duration::from_secs(5), writer.write_all(b"unread request")).await??;
    drop(writer);
    assert!(peer_end_closes(&mut peer, Duration::from_secs(10)).await);
    let mut remaining = Vec::new();
    timeout(Duration::from_secs(5), peer.read_to_end(&mut remaining)).await??;
    assert_eq!(remaining, b"unread request");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pending_read_does_not_hold_a_dropped_stream_open() -> TestResult {
    let endpoint = TestEndpoint::new()?;
    let (mut server, mut client) = endpoint.pair().await?;
    timeout(Duration::from_secs(5), client.write_all(b"request")).await??;
    // The client is cancelled while its read is pending.
    let mut byte = [0_u8; 1];
    assert!(
        timeout(Duration::from_millis(100), client.read(&mut byte))
            .await
            .is_err()
    );
    drop(client);
    assert!(peer_end_closes(&mut server, Duration::from_secs(10)).await);
    let mut request = Vec::new();
    timeout(Duration::from_secs(5), server.read_to_end(&mut request)).await??;
    assert_eq!(request, b"request");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn peer_close_is_end_of_stream_and_extra_bytes_are_refused() -> TestResult {
    let endpoint = TestEndpoint::new()?;
    let (mut server, client) = endpoint.pair().await?;
    drop(client);
    timeout(Duration::from_secs(5), server.peer_closed()).await??;

    let (mut server, mut client) = endpoint.pair().await?;
    timeout(Duration::from_secs(5), client.write_all(b"x")).await??;
    assert!(matches!(
        timeout(Duration::from_secs(5), server.peer_closed()).await?,
        Err(Error::Protocol(_))
    ));
    drop(client);

    // A peer that neither closes nor sends is bounded only by the caller.
    let (mut server, _client) = endpoint.pair().await?;
    assert!(
        timeout(Duration::from_millis(100), server.peer_closed())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn flush_and_shutdown_delegate_without_closing_the_stream() -> TestResult {
    let endpoint = TestEndpoint::new()?;
    let (mut server, mut client) = endpoint.pair().await?;
    server.write_all(b"ok").await?;
    server.flush().await?;
    server.shutdown().await?;
    let mut received = [0_u8; 2];
    client.read_exact(&mut received).await?;
    assert_eq!(&received, b"ok");
    let _ = server.get().peer_creds()?;
    Ok(())
}
