use sigy_service::sources::{
    HttpSource, NetworkScope, RedirectPolicy,
    http::{AcquisitionLimits, HttpAcquirer},
};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Chain {
    address: SocketAddr,
    hits: Arc<AtomicUsize>,
    worker: JoinHandle<()>,
}

impl Chain {
    async fn start(responses: Vec<Vec<u8>>, stall: bool) -> std::io::Result<Self> {
        Self::start_delayed(responses, stall, Duration::ZERO).await
    }

    async fn start_delayed(
        responses: Vec<Vec<u8>>,
        stall: bool,
        delay: Duration,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let hits = Arc::new(AtomicUsize::new(0));
        let observed = hits.clone();
        let worker = tokio::spawn(async move {
            let work = async {
                for response in responses {
                    let (mut socket, _) = listener.accept().await?;
                    let mut request = Vec::new();
                    while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                        request.push(socket.read_u8().await?);
                    }
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    assert!(!text.contains("\r\ncookie:"));
                    assert!(!text.contains("\r\nreferer:"));
                    assert!(!text.contains("\r\nauthorization:"));
                    observed.fetch_add(1, Ordering::SeqCst);
                    if stall {
                        std::future::pending::<()>().await;
                    }
                    tokio::time::sleep(delay).await;
                    socket.write_all(&response).await?;
                }
                Ok::<_, std::io::Error>(())
            };
            let _ = timeout(Duration::from_secs(5), work).await;
        });
        Ok(Self {
            address,
            hits,
            worker,
        })
    }

    fn source(&self, policy: RedirectPolicy) -> sigy_service::Result<HttpSource> {
        HttpSource::new(
            "Redirect fixture",
            &format!(
                "http://fixture.invalid:{}/start?secret=hidden",
                self.address.port()
            ),
            NetworkScope::PinnedAddress {
                address: self.address.ip(),
            },
        )?
        .with_redirects(policy)
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn redirect(status: u16, location: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} Redirect\r\nLocation: {location}\r\nSet-Cookie: secret=do-not-forward\r\nContent-Length: 999999999\r\nConnection: close\r\n\r\n").into_bytes()
}

fn audio() -> Vec<u8> {
    b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_vec()
}

#[tokio::test]
async fn explicit_redirects_preserve_audio_and_observed_route_without_cookies() -> TestResult {
    for status in [301, 302, 303, 307, 308] {
        let chain = Chain::start(vec![redirect(status, "/audio"), audio()], false).await?;
        let mut sink = Vec::new();
        let result = HttpAcquirer::default()
            .acquire(
                &chain.source(RedirectPolicy::SameOrigin)?,
                AcquisitionLimits::new(32, Duration::from_secs(2))?,
                &mut sink,
            )
            .await?;
        assert_eq!(sink, b"abc");
        assert_eq!(result.route.len(), 2);
        assert_eq!(result.route[0].status, status);
        assert_eq!(result.route[1].status, 200);
        assert_eq!(result.route[1].peer, chain.address);
        assert_eq!(chain.hits.load(Ordering::SeqCst), 2);
        assert!(!format!("{:?}", result.route).contains("secret"));
    }
    Ok(())
}

#[tokio::test]
async fn redirect_denials_happen_before_secondary_contact_or_body_writes() -> TestResult {
    let target = Chain::start(vec![audio()], false).await?;
    let cases = [
        redirect(302, "/audio"),
        redirect(
            302,
            &format!("http://fixture.invalid:{}/audio", target.address.port()),
        ),
        redirect(302, "http://another.invalid/audio"),
        redirect(302, "http://169.254.169.254/audio"),
        b"HTTP/1.1 302 Redirect\r\nLocation: /one\r\nLocation: /two\r\n\r\n".to_vec(),
        b"HTTP/1.1 302 Redirect\r\nContent-Length: 0\r\n\r\n".to_vec(),
        redirect(304, "/audio"),
    ];
    for (index, response) in cases.into_iter().enumerate() {
        let chain = Chain::start(vec![response], false).await?;
        let policy = if index == 0 {
            RedirectPolicy::Deny
        } else {
            RedirectPolicy::SameOrigin
        };
        let mut sink = Vec::new();
        assert!(
            HttpAcquirer::default()
                .acquire(
                    &chain.source(policy)?,
                    AcquisitionLimits::new(32, Duration::from_secs(2))?,
                    &mut sink
                )
                .await
                .is_err()
        );
        assert!(sink.is_empty());
        assert_eq!(chain.hits.load(Ordering::SeqCst), 1);
        assert_eq!(target.hits.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[tokio::test]
async fn loops_and_excessive_chains_fail_without_resetting_limits() -> TestResult {
    for (responses, expected_hits, success) in [
        (vec![redirect(302, "/start?secret=hidden")], 1, false),
        (
            vec![
                redirect(302, "/one"),
                redirect(307, "/two"),
                redirect(308, "/three"),
                audio(),
            ],
            4,
            true,
        ),
        (
            vec![
                redirect(302, "/one"),
                redirect(307, "/two"),
                redirect(308, "/three"),
                redirect(301, "/four"),
                audio(),
            ],
            4,
            false,
        ),
    ] {
        let chain = Chain::start(responses, false).await?;
        let mut sink = Vec::new();
        let result = HttpAcquirer::default()
            .acquire(
                &chain.source(RedirectPolicy::SameOrigin)?,
                AcquisitionLimits::new(2, Duration::from_secs(2))?,
                &mut sink,
            )
            .await;
        assert_eq!(result.is_ok(), success);
        assert_eq!(chain.hits.load(Ordering::SeqCst), expected_hits);
        assert_eq!(sink, if success { b"ab".to_vec() } else { Vec::new() });
    }
    Ok(())
}

#[tokio::test]
async fn stop_cancels_waiting_for_redirect_headers() -> TestResult {
    let chain = Chain::start(vec![redirect(302, "/audio")], true).await?;
    let source = chain.source(RedirectPolicy::SameOrigin)?;
    let acquirer = HttpAcquirer::default();
    let (sender, mut stop) = tokio::sync::watch::channel(false);
    let mut sink = Vec::new();
    let record = acquirer.record(
        &source,
        AcquisitionLimits::new(32, Duration::from_secs(10))?,
        &mut sink,
        &mut stop,
    );
    let cancel = async {
        timeout(Duration::from_secs(2), async {
            while chain.hits.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await?;
        sender.send(true)?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, cancelled) = timeout(Duration::from_secs(3), async {
        tokio::join!(record, cancel)
    })
    .await?;
    cancelled?;
    assert!(result.is_err());
    assert!(sink.is_empty());
    Ok(())
}

#[tokio::test]
async fn redirect_waits_share_one_acquisition_deadline() -> TestResult {
    let chain = Chain::start_delayed(
        vec![redirect(302, "/one"), redirect(302, "/two"), audio()],
        false,
        Duration::from_millis(150),
    )
    .await?;
    let source = chain.source(RedirectPolicy::SameOrigin)?;
    let acquirer = HttpAcquirer::default();
    let mut sink = Vec::new();
    let result = timeout(
        Duration::from_secs(2),
        acquirer.acquire(
            &source,
            AcquisitionLimits::new(32, Duration::from_millis(250))?,
            &mut sink,
        ),
    )
    .await?;
    assert!(result.is_err());
    assert!(sink.is_empty());
    assert!(chain.hits.load(Ordering::SeqCst) <= 2);
    Ok(())
}
