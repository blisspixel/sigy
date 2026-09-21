use std::{net::SocketAddr, time::Duration};

use sigy_service::{
    Error,
    sources::{
        HttpSource, NetworkScope,
        http::{AcquisitionLimits, AudioContentType, HttpAcquirer, TransferEnd},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    address: SocketAddr,
    task: JoinHandle<()>,
    request: oneshot::Receiver<Vec<u8>>,
}

impl Fixture {
    async fn new(response: Vec<u8>, hold_open: bool) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (sender, request) = oneshot::channel();
        let task = tokio::spawn(async move {
            let work = async {
                let (mut socket, _) = listener.accept().await?;
                let mut request = Vec::new();
                while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                    let byte = socket.read_u8().await?;
                    request.push(byte);
                }
                let _ = sender.send(request);
                socket.write_all(&response).await?;
                if hold_open {
                    std::future::pending::<()>().await;
                }
                Ok::<_, std::io::Error>(())
            };
            let _ = timeout(Duration::from_secs(5), work).await;
        });
        Ok(Self {
            address,
            task,
            request,
        })
    }

    fn source(&self) -> sigy_service::Result<HttpSource> {
        // An unresolvable hostname proves that the explicit IP is used without
        // rewriting the HTTP authority. TLS uses the same hostname semantics.
        HttpSource::new(
            "Radio Québec",
            &format!(
                "http://fixture.invalid:{}/stream?secret=redacted",
                self.address.port()
            ),
            NetworkScope::PinnedAddress {
                address: self.address.ip(),
            },
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn limits(bytes: u64) -> sigy_service::Result<AcquisitionLimits> {
    AcquisitionLimits::new(bytes, Duration::from_secs(2))
}

#[tokio::test]
async fn intentional_recording_limits_preserve_received_bytes() -> TestResult {
    for expected in [TransferEnd::DurationLimit, TransferEnd::UserStop] {
        let fixture = Fixture::new(
            b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n\r\nabc".to_vec(),
            true,
        )
        .await?;
        let (sender, mut stop) = tokio::sync::watch::channel(false);
        let (mut sink, mut reader) = tokio::io::duplex(16);
        let source = fixture.source()?;
        let acquirer = HttpAcquirer::default();
        let capture = acquirer.record(
            &source,
            AcquisitionLimits::new(100, Duration::from_millis(300))?,
            &mut sink,
            &mut stop,
        );
        let control = async {
            let mut received = [0; 3];
            reader.read_exact(&mut received).await?;
            assert_eq!(&received, b"abc");
            if expected == TransferEnd::UserStop {
                sender.send(true)?;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        };
        let (receipt, control) = timeout(Duration::from_secs(2), async {
            tokio::join!(capture, control)
        })
        .await?;
        control?;
        let receipt = receipt?;
        assert_eq!(receipt.end, expected);
        assert_eq!(receipt.bytes, 3);
    }
    Ok(())
}

#[tokio::test]
async fn timed_recording_without_audio_cannot_be_published() -> TestResult {
    let fixture = Fixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n\r\n".to_vec(),
        true,
    )
    .await?;
    let (_sender, mut stop) = tokio::sync::watch::channel(false);
    let mut received = Vec::new();
    assert!(
        HttpAcquirer::default()
            .record(
                &fixture.source()?,
                AcquisitionLimits::new(100, Duration::from_millis(100))?,
                &mut received,
                &mut stop,
            )
            .await
            .is_err()
    );
    assert!(received.is_empty());
    Ok(())
}

#[tokio::test]
async fn finite_transfer_preserves_bytes_and_authority_without_trusting_audio_labels() -> TestResult
{
    let mut fixture = Fixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 6\r\n\r\nabcdef".to_vec(),
        false,
    )
    .await?;
    let acquirer = HttpAcquirer::default();
    let mut received = Vec::new();
    let receipt = acquirer
        .acquire(&fixture.source()?, limits(100)?, &mut received)
        .await?;
    assert_eq!(received, b"abcdef");
    assert_eq!(receipt.bytes, 6);
    assert_eq!(receipt.end, TransferEnd::EndOfBody);
    assert_eq!(receipt.declared_content_type, AudioContentType::Mpeg);
    assert_eq!(receipt.peer, fixture.address);
    let request = String::from_utf8(timeout(Duration::from_secs(1), &mut fixture.request).await??)?;
    assert!(request.starts_with("GET /stream?secret=redacted HTTP/1.1\r\n"));
    assert!(request.contains(&format!("host: fixture.invalid:{}", fixture.address.port())));
    assert!(request.contains("accept-encoding: identity"));
    assert!(request.contains("icy-metadata: 0"));
    // This fixture deliberately is not MPEG. A transfer receipt cannot qualify it.
    Ok(())
}

#[tokio::test]
async fn byte_limit_and_chunked_body_are_bounded_exactly() -> TestResult {
    for body in [
        b"Content-Length: 20\r\n\r\nabcdefghijklmnopqrst".as_slice(),
        b"Transfer-Encoding: chunked\r\n\r\n5\r\nabcde\r\n5\r\nfghij\r\n0\r\n\r\n".as_slice(),
    ] {
        let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n".to_vec();
        response.extend(body);
        let fixture = Fixture::new(response, true).await?;
        let mut received = Vec::new();
        let receipt = HttpAcquirer::default()
            .acquire(&fixture.source()?, limits(7)?, &mut received)
            .await?;
        assert_eq!(received, b"abcdefg");
        assert_eq!(receipt.bytes, 7);
        assert_eq!(receipt.end, TransferEnd::ByteLimit);
    }
    Ok(())
}

#[tokio::test]
async fn redirect_never_contacts_its_target_or_emits_sensitive_location() -> TestResult {
    let target = TcpListener::bind("127.0.0.1:0").await?;
    let response = format!(
        "HTTP/1.1 302 Found\r\nLocation: http://{}/private?token=secret\r\nContent-Length: 0\r\n\r\n",
        target.local_addr()?
    );
    let fixture = Fixture::new(response.into_bytes(), false).await?;
    let mut bytes = Vec::new();
    let result = HttpAcquirer::default()
        .acquire(&fixture.source()?, limits(100)?, &mut bytes)
        .await;
    let error = result.err().ok_or("redirect was followed")?;
    assert!(matches!(
        error,
        Error::Acquisition("redirect requires a separately authorized source revision")
    ));
    assert!(!error.to_string().contains("secret"));
    assert!(bytes.is_empty());
    assert!(
        timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn unsupported_and_ambiguous_headers_fail_before_body_writes() -> TestResult {
    for headers in [
        "Content-Type: text/html\r\n",
        "Content-Type: audio/x-mpegurl\r\n",
        "Content-Type: application/vnd.apple.mpegurl\r\n",
        "Content-Type: audio/mpeg\r\nContent-Encoding: gzip\r\n",
        "Content-Type: audio/mpeg\r\nContent-Type: text/html\r\n",
        "Content-Type: audio/mpeg\r\nicy-metaint: 100\r\n",
        "",
    ] {
        let response = format!("HTTP/1.1 200 OK\r\n{headers}Content-Length: 4\r\n\r\ntest");
        let fixture = Fixture::new(response.into_bytes(), false).await?;
        let mut bytes = Vec::new();
        assert!(
            HttpAcquirer::default()
                .acquire(&fixture.source()?, limits(100)?, &mut bytes)
                .await
                .is_err(),
            "accepted {headers:?}"
        );
        assert!(bytes.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn truncation_empty_body_and_excessive_headers_are_errors() -> TestResult {
    let mut large = b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nX-Large: ".to_vec();
    large.extend(vec![b'x'; 512 * 1024]);
    large.extend(b"\r\nContent-Length: 4\r\n\r\ntest");
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 5\r\n\r\nabc".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 0\r\n\r\n".to_vec(),
        large,
    ] {
        let fixture = Fixture::new(response, false).await?;
        let mut bytes = Vec::new();
        assert!(
            HttpAcquirer::default()
                .acquire(&fixture.source()?, limits(100)?, &mut bytes)
                .await
                .is_err()
        );
        assert!(bytes.len() <= 3);
    }
    Ok(())
}

#[tokio::test]
async fn deadline_includes_headers_body_and_a_stalled_sink() -> TestResult {
    for response in [
        Vec::new(),
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 10\r\n\r\nabc".to_vec(),
    ] {
        let fixture = Fixture::new(response, true).await?;
        let limits = AcquisitionLimits::new(100, Duration::from_millis(100))?;
        let mut bytes = Vec::new();
        let result = timeout(
            Duration::from_secs(1),
            HttpAcquirer::default().acquire(&fixture.source()?, limits, &mut bytes),
        )
        .await?;
        assert!(result.is_err());
        assert!(bytes.len() <= 3);
    }
    let fixture = Fixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 6\r\n\r\nabcdef".to_vec(),
        false,
    )
    .await?;
    let (mut sink, _unread) = tokio::io::duplex(1);
    let limits = AcquisitionLimits::new(100, Duration::from_millis(100))?;
    assert!(
        timeout(
            Duration::from_secs(1),
            HttpAcquirer::default().acquire(&fixture.source()?, limits, &mut sink)
        )
        .await?
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn clones_share_admission_and_cancellation_releases_the_attempt() -> TestResult {
    let mut first = Fixture::new(Vec::new(), true).await?;
    let mut second = Fixture::new(Vec::new(), true).await?;
    let third = Fixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
        false,
    )
    .await?;
    let acquirer = HttpAcquirer::default();
    let worker = |source: HttpSource| {
        let acquirer = acquirer.clone();
        tokio::spawn(async move {
            acquirer
                .acquire(&source, limits(100)?, &mut Vec::new())
                .await
        })
    };
    let first_attempt = worker(first.source()?);
    let second_attempt = worker(second.source()?);
    timeout(Duration::from_secs(2), &mut first.request).await??;
    timeout(Duration::from_secs(2), &mut second.request).await??;
    let result = acquirer
        .acquire(&third.source()?, limits(100)?, &mut Vec::new())
        .await;
    assert!(matches!(
        result,
        Err(Error::Acquisition("capacity reached"))
    ));
    first_attempt.abort();
    second_attempt.abort();
    assert!(first_attempt.await.is_err());
    assert!(second_attempt.await.is_err());
    let receipt = acquirer
        .acquire(&third.source()?, limits(100)?, &mut Vec::new())
        .await?;
    assert_eq!(receipt.bytes, 3);
    Ok(())
}

#[tokio::test]
async fn public_domain_resolving_to_loopback_cannot_connect() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let source = HttpSource::new(
        "Denied",
        &format!("http://localhost:{}/secret", listener.local_addr()?.port()),
        NetworkScope::PublicInternet {},
    )?;
    let result = HttpAcquirer::default()
        .acquire(&source, limits(100)?, &mut Vec::new())
        .await;
    assert!(result.is_err());
    assert!(
        timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn untrusted_tls_certificate_cannot_deliver_an_audio_body() -> TestResult {
    use std::sync::Arc;
    use tokio_rustls::{
        TlsAcceptor,
        rustls::{
            ServerConfig,
            pki_types::{CertificateDer, PrivatePkcs8KeyDer},
        },
    };
    let certificate =
        CertificateDer::from(include_bytes!("fixtures/tls/untrusted-cert.der").to_vec());
    let key = PrivatePkcs8KeyDer::from(include_bytes!("fixtures/tls/fixture-key.der").to_vec());
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key.into())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let source = HttpSource::new(
        "Untrusted TLS",
        &format!("https://fixture.invalid:{}/audio", address.port()),
        NetworkScope::PinnedAddress {
            address: address.ip(),
        },
    )?;
    let mut received = Vec::new();
    let server = async {
        let (socket, _) = listener.accept().await?;
        if let Ok(mut stream) = TlsAcceptor::from(Arc::new(config)).accept(socket).await {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 3\r\n\r\nabc",
                )
                .await?;
            return Err::<(), _>(std::io::Error::other("untrusted TLS handshake succeeded"));
        }
        Ok(())
    };
    let acquirer = HttpAcquirer::default();
    let client = acquirer.acquire(&source, limits(100)?, &mut received);
    let (server, client) = timeout(Duration::from_secs(3), async {
        tokio::join!(server, client)
    })
    .await?;
    server?;
    assert!(client.is_err());
    assert!(received.is_empty());
    Ok(())
}

#[tokio::test]
async fn limits_and_sink_failure_cannot_produce_a_success_receipt() -> TestResult {
    use sigy_service::sources::http::{MAXIMUM_BODY_BYTES, MAXIMUM_DURATION};
    assert!(AcquisitionLimits::new(0, Duration::from_secs(1)).is_err());
    assert!(AcquisitionLimits::new(MAXIMUM_BODY_BYTES + 1, Duration::from_secs(1)).is_err());
    assert!(AcquisitionLimits::new(1, Duration::ZERO).is_err());
    assert!(AcquisitionLimits::new(1, MAXIMUM_DURATION + Duration::from_secs(1)).is_err());
    let fixture = Fixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
        false,
    )
    .await?;
    let (mut sink, reader) = tokio::io::duplex(1);
    drop(reader);
    assert!(matches!(
        HttpAcquirer::default()
            .acquire(&fixture.source()?, limits(100)?, &mut sink)
            .await,
        Err(Error::Io(_))
    ));
    Ok(())
}

#[test]
fn environment_proxies_cannot_redirect_an_attempt() -> TestResult {
    use std::{
        process::{Command, Stdio},
        thread,
        time::Instant,
    };
    let proxy = std::net::TcpListener::bind("127.0.0.1:0")?;
    proxy.set_nonblocking(true)?;
    let endpoint = format!("http://{}", proxy.local_addr()?);
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            "finite_transfer_preserves_bytes_and_authority_without_trusting_audio_labels",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env(name, &endpoint);
    }
    command.env("NO_PROXY", "").env("no_proxy", "");
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                break Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "proxy fixture process deadline",
                ));
            }
            Err(error) => break Err(error),
        }
    };
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(outcome?.success());
    assert!(matches!(proxy.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
    Ok(())
}
