//! Finite HTTP transport only. Received bytes are not verified media or a recording.

mod documents;
mod resolver;

pub(crate) use documents::PlaylistKind;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use reqwest::{Client, StatusCode, header};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::Semaphore,
    time::{Instant, timeout_at},
};

use super::{
    HttpHop, HttpSource,
    icy::{IcyObservation, IcySplitter, MetadataPolicy, interval_header},
    redirects::{MAX_REDIRECTS, is_redirect},
};
use crate::{Error, Result};

pub const MAXIMUM_BODY_BYTES: u64 = 256 * 1024 * 1024;
pub const MAXIMUM_DURATION: Duration = Duration::from_mins(15);
/// Episode downloads reserve this whole ceiling. Radio acquisition cannot use it.
pub const EPISODE_BODY_BYTES: u64 = 512 * 1024 * 1024;
pub const EPISODE_DURATION: Duration = Duration::from_mins(30);
const MAX_ATTEMPTS: usize = 2;

/// Audio and directory reads stay identity-only. A feed may ask for compression.
#[derive(Clone, Copy)]
pub(crate) enum AcceptEncoding {
    Identity,
    Feed,
}

impl AcceptEncoding {
    const fn header(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Feed => "gzip, deflate, identity",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AcquisitionLimits {
    bytes: u64,
    duration: Duration,
    clean_end: bool,
}

impl AcquisitionLimits {
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }

    #[must_use]
    pub const fn clean_end(self) -> bool {
        self.clean_end
    }

    /// # Errors
    /// Rejects zero or out-of-profile limits. Limits include connection setup.
    /// Radio stays at 15 minutes and 256 MiB. Episode downloads use [`Self::episode`].
    pub fn new(bytes: u64, duration: Duration) -> Result<Self> {
        if bytes == 0
            || bytes > MAXIMUM_BODY_BYTES
            || duration.is_zero()
            || duration > MAXIMUM_DURATION
        {
            return Err(Error::InvalidInput("HTTP acquisition limits"));
        }
        Ok(Self {
            bytes,
            duration,
            clean_end: false,
        })
    }

    /// Fixed episode ceiling: 512 MiB and 30 minutes, and a clean end is required.
    #[must_use]
    pub const fn episode() -> Self {
        Self {
            bytes: EPISODE_BODY_BYTES,
            duration: EPISODE_DURATION,
            clean_end: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioContentType {
    Mpeg,
    Aac,
    Flac,
    Ogg,
    Wave,
}

impl AudioContentType {
    #[must_use]
    pub const fn format_name(self) -> &'static str {
        match self {
            Self::Mpeg => "mp3",
            Self::Aac => "aac",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Wave => "wav",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferEnd {
    EndOfBody,
    ByteLimit,
    DurationLimit,
    UserStop,
}

/// Transport observations, not evidence that decoding or publication succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferReceipt {
    pub bytes: u64,
    pub end: TransferEnd,
    pub declared_content_type: AudioContentType,
    pub peer: SocketAddr,
    pub route: Vec<HttpHop>,
    pub(crate) observations: Vec<IcyObservation>,
}

#[derive(Debug)]
struct Opened {
    response: reqwest::Response,
    route: Vec<HttpHop>,
    declared_content_type: AudioContentType,
    peer: SocketAddr,
    interval: Option<u64>,
}

/// One admitted audio response. Dropping it releases the shared attempt slot.
#[derive(Debug)]
pub(crate) struct AudioDownload {
    opened: Opened,
    limits: AcquisitionLimits,
    deadline: Instant,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl AudioDownload {
    #[must_use]
    pub(crate) fn format_name(&self) -> &'static str {
        self.opened.declared_content_type.format_name()
    }
}

/// Share one instance across acquisition workers. Clones share admission and
/// DNS slots. DNS lookups are asynchronous and cancellation does not retain workers.
#[derive(Debug, Clone)]
pub struct HttpAcquirer {
    attempts: Arc<Semaphore>,
    dns: Arc<Semaphore>,
}

impl Default for HttpAcquirer {
    fn default() -> Self {
        Self {
            attempts: Arc::new(Semaphore::new(MAX_ATTEMPTS)),
            dns: Arc::new(Semaphore::new(MAX_ATTEMPTS)),
        }
    }
}

impl HttpAcquirer {
    /// Copies one HTTP(S) body, following only the source's explicit redirect policy.
    /// Does not create files, retry, decode, or finalize jobs.
    /// A cancelled or failed call may have written a partial body; preserve it
    /// as unverified until the media publication/recovery layer reconciles it.
    /// # Errors
    /// Fails closed on capacity, destination policy, time, transport, unexpected
    /// response headers or sink failure. Diagnostics exclude source URLs.
    pub async fn acquire<W: AsyncWrite + Unpin>(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        sink: &mut W,
    ) -> Result<TransferReceipt> {
        let _slot = self
            .attempts
            .try_acquire()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let deadline = Instant::now() + limits.duration;
        timeout_at(
            deadline,
            self.transfer(source, limits, sink, deadline, None, MetadataPolicy::Off),
        )
        .await
        .map_err(|_| Error::Acquisition("deadline reached; partial body is unverified"))?
    }

    /// Record a bounded stream. An intentional time limit or stop preserves the
    /// received bytes; connection failures and stalls remain errors.
    /// # Errors
    /// Uses the same destination, header, byte and capacity checks as acquisition.
    pub async fn record<W: AsyncWrite + Unpin>(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        sink: &mut W,
        stop: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<TransferReceipt> {
        self.record_with(source, limits, sink, stop, MetadataPolicy::Off)
            .await
    }

    pub(crate) async fn record_with<W: AsyncWrite + Unpin>(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        sink: &mut W,
        stop: &mut tokio::sync::watch::Receiver<bool>,
        metadata: MetadataPolicy,
    ) -> Result<TransferReceipt> {
        let _slot = self
            .attempts
            .try_acquire()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let deadline = Instant::now() + limits.duration;
        timeout_at(
            deadline + Duration::from_secs(5),
            self.transfer(source, limits, sink, deadline, Some(stop), metadata),
        )
        .await
        .map_err(|_| {
            Error::Acquisition("recording sink deadline reached; partial body is unverified")
        })?
    }

    async fn open(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        deadline: Instant,
        accept: &'static str,
        encoding: AcceptEncoding,
        metadata: MetadataPolicy,
    ) -> Result<(reqwest::Response, Vec<HttpHop>)> {
        let mut current = source.clone();
        let mut visited = vec![current.url.clone()];
        let mut route = Vec::new();
        loop {
            let response = self
                .open_once(&current, limits, deadline, accept, encoding, metadata)
                .await?;
            route.push(HttpHop {
                origin: current.origin(),
                peer: response.remote_addr().ok_or(Error::DestinationDenied)?,
                status: response.status().as_u16(),
            });
            if response.status() == StatusCode::OK {
                return Ok((response, route));
            }
            if !is_redirect(response.status().as_u16()) {
                return Err(Error::Acquisition(
                    "source did not return HTTP 200 or supported redirect",
                ));
            }
            if route.len() > MAX_REDIRECTS {
                return Err(Error::Acquisition("redirect hop limit reached"));
            }
            let locations = response.headers().get_all(header::LOCATION);
            if locations.iter().count() != 1 {
                return Err(Error::Acquisition("missing or ambiguous redirect location"));
            }
            let location = locations
                .iter()
                .next()
                .and_then(|value| value.to_str().ok())
                .ok_or(Error::Acquisition("invalid redirect location"))?;
            let target = current.redirect_target(location)?;
            if visited.contains(&target.url) {
                return Err(Error::Acquisition("redirect loop"));
            }
            visited.push(target.url.clone());
            current = target;
            // Never consume a redirect body or carry cookies, auth or referer state.
            drop(response);
        }
    }

    async fn open_once(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        deadline: Instant,
        accept: &'static str,
        encoding: AcceptEncoding,
        metadata: MetadataPolicy,
    ) -> Result<reqwest::Response> {
        // Platform verification can retrieve certificate-supplied AIA/OCSP URLs
        // outside the source policy. Use offline WebPKI with explicit roots.
        let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|root| reqwest::Certificate::from_der(root.as_ref()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Acquisition("TLS trust store initialization"))?;
        let client = Client::builder()
            .tls_certs_only(roots)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http1_only()
            .http1_max_headers(64)
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(5).min(limits.duration))
            // Overall deadlines belong to the transfer. A shorter read timeout
            // would race an intentional recording limit and discard its receipt.
            .read_timeout(Duration::from_secs(5))
            .timeout(limits.duration + Duration::from_secs(5))
            .dns_resolver(resolver::CheckedResolver::new(
                source.network,
                self.dns.clone(),
            ))
            .user_agent("Sigy/0.1")
            .build()
            .map_err(|_| Error::Acquisition("HTTP client initialization"))?;
        let request = client
            .get(source.url.clone())
            .header(header::ACCEPT, accept)
            .header(header::ACCEPT_ENCODING, encoding.header())
            .header(
                "icy-metadata",
                match metadata {
                    MetadataPolicy::Off => "0",
                    MetadataPolicy::Requested => "1",
                },
            )
            .send();
        let response = timeout_at(deadline, request)
            .await
            .map_err(|_| Error::Acquisition("connection deadline reached"))?
            .map_err(|_| Error::Acquisition("connection or response headers"))?;
        let peer = response.remote_addr().ok_or(Error::DestinationDenied)?;
        if !source.network.permits(peer.ip())
            || Some(peer.port()) != source.url.port_or_known_default()
        {
            return Err(Error::DestinationDenied);
        }
        Ok(response)
    }

    /// Opens one direct audio response and keeps the shared attempt slot until copy finishes.
    /// Header rejection, including playlist types and ICY metadata, happens before a body copy.
    /// # Errors
    /// Uses the same destination, header, and capacity checks as recording.
    pub(crate) async fn open_audio(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        stop: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<AudioDownload> {
        let permit = self
            .attempts
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let deadline = Instant::now() + limits.duration;
        let mut signal = Some(stop);
        let opened = self
            .begin(source, limits, deadline, &mut signal, MetadataPolicy::Off)
            .await?;
        Ok(AudioDownload {
            opened,
            limits,
            deadline,
            _permit: permit,
        })
    }

    /// Copies an opened audio body. The caller supplies a local sink, never a source URL.
    /// # Errors
    /// Fails closed on transport, time, stop, or sink errors. A stop before any byte is an error.
    pub(crate) async fn copy_audio<W: AsyncWrite + Unpin>(
        download: AudioDownload,
        sink: &mut W,
        stop: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<TransferReceipt> {
        let deadline = download.deadline;
        let mut signal = Some(stop);
        timeout_at(
            deadline + Duration::from_secs(5),
            Self::pump(
                download.opened,
                download.limits,
                sink,
                deadline,
                &mut signal,
            ),
        )
        .await
        .map_err(|_| Error::Acquisition("listen deadline reached"))?
    }

    async fn transfer<W: AsyncWrite + Unpin>(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        sink: &mut W,
        deadline: Instant,
        mut stop: Option<&mut tokio::sync::watch::Receiver<bool>>,
        metadata: MetadataPolicy,
    ) -> Result<TransferReceipt> {
        let opened = self
            .begin(source, limits, deadline, &mut stop, metadata)
            .await?;
        Self::pump(opened, limits, sink, deadline, &mut stop).await
    }

    async fn begin(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        deadline: Instant,
        stop: &mut Option<&mut tokio::sync::watch::Receiver<bool>>,
        metadata: MetadataPolicy,
    ) -> Result<Opened> {
        let connecting = self.open(
            source,
            limits,
            deadline,
            "audio/mpeg, audio/aac, audio/flac, audio/ogg, audio/wav, application/ogg",
            AcceptEncoding::Identity,
            metadata,
        );
        let (response, route) = if let Some(signal) = stop.as_mut() {
            tokio::select! {
                biased;
                () = async { if !*signal.borrow() { let _ = signal.changed().await; } } => {
                    return Err(Error::Acquisition("stopped before receiving audio"));
                }
                result = connecting => result?,
            }
        } else {
            connecting.await?
        };
        let peer = response.remote_addr().ok_or(Error::DestinationDenied)?;
        let declared_content_type = validate_headers(response.headers())?;
        if limits.clean_end
            && response
                .content_length()
                .is_some_and(|declared| declared > limits.bytes)
        {
            return Err(Error::Acquisition(
                "declared length exceeds the episode ceiling",
            ));
        }
        let interval = icy_interval(response.headers(), metadata)?;
        Ok(Opened {
            response,
            route,
            declared_content_type,
            peer,
            interval,
        })
    }

    async fn pump<W: AsyncWrite + Unpin>(
        opened: Opened,
        limits: AcquisitionLimits,
        sink: &mut W,
        deadline: Instant,
        stop: &mut Option<&mut tokio::sync::watch::Receiver<bool>>,
    ) -> Result<TransferReceipt> {
        let Opened {
            mut response,
            route,
            declared_content_type,
            peer,
            interval,
        } = opened;
        let mut carried = Carried {
            bytes: 0,
            declared_content_type,
            peer,
            route,
            splitter: interval.map(IcySplitter::new).transpose()?,
        };
        loop {
            if carried.bytes == limits.bytes {
                if !limits.clean_end {
                    return close_transfer(sink, carried, TransferEnd::ByteLimit, false).await;
                }
                // A body that ends on the ceiling is complete. Further bytes are not.
                let clean = !next_chunk(&mut response, stop, deadline).await?;
                return close_transfer(
                    sink,
                    carried,
                    if clean {
                        TransferEnd::EndOfBody
                    } else {
                        TransferEnd::ByteLimit
                    },
                    clean,
                )
                .await;
            }
            let body = async {
                response
                    .chunk()
                    .await
                    .map_err(|_| Error::Acquisition("body interrupted; partial body is unverified"))
            };
            let chunk = if let Some(stop) = stop.as_mut() {
                let end = tokio::select! {
                    biased;
                    () = async { if !*stop.borrow() { let _ = stop.changed().await; } } => {
                        if carried.bytes == 0 {
                            return Err(Error::Acquisition("stopped before receiving audio"));
                        }
                        return close_transfer(sink, carried, TransferEnd::UserStop, false).await;
                    }
                    () = tokio::time::sleep_until(deadline) => None,
                    result = body => Some(result?),
                };
                if let Some(chunk) = end {
                    chunk
                } else if carried.bytes == 0 {
                    return Err(Error::Acquisition("empty timed recording"));
                } else {
                    return close_transfer(sink, carried, TransferEnd::DurationLimit, false).await;
                }
            } else {
                body.await?
            };
            let Some(chunk) = chunk else {
                if carried.bytes == 0 {
                    return Err(Error::Acquisition("empty body"));
                }
                return close_transfer(sink, carried, TransferEnd::EndOfBody, true).await;
            };
            let produced;
            let audio = if let Some(splitter) = carried.splitter.as_mut() {
                produced = splitter.push(&chunk)?;
                produced.as_slice()
            } else {
                chunk.as_ref()
            };
            let room = usize::try_from(limits.bytes - carried.bytes)
                .map_err(|_| Error::InvalidInput("body byte range"))?;
            let take = audio.len().min(room);
            if take > 0 {
                // Metadata bytes are not written and do not count toward the ceiling.
                sink.write_all(&audio[..take]).await?;
                carried.bytes +=
                    u64::try_from(take).map_err(|_| Error::InvalidInput("body byte range"))?;
            }
            if take < audio.len() {
                return close_transfer(sink, carried, TransferEnd::ByteLimit, false).await;
            }
        }
    }
}

async fn next_chunk(
    response: &mut reqwest::Response,
    stop: &mut Option<&mut tokio::sync::watch::Receiver<bool>>,
    deadline: Instant,
) -> Result<bool> {
    let body = async {
        response
            .chunk()
            .await
            .map_err(|_| Error::Acquisition("body interrupted; partial body is unverified"))
    };
    let extra = if let Some(stop) = stop.as_mut() {
        tokio::select! {
            biased;
            () = async { if !*stop.borrow() { let _ = stop.changed().await; } } => {
                return Err(Error::Acquisition("episode ended before a clean end"));
            }
            () = tokio::time::sleep_until(deadline) => {
                return Err(Error::Acquisition("episode ended before a clean end"));
            }
            result = body => result?,
        }
    } else {
        body.await?
    };
    Ok(extra.is_some())
}

struct Carried {
    bytes: u64,
    declared_content_type: AudioContentType,
    peer: SocketAddr,
    route: Vec<HttpHop>,
    splitter: Option<IcySplitter>,
}

async fn close_transfer<W: AsyncWrite + Unpin>(
    sink: &mut W,
    mut carried: Carried,
    end: TransferEnd,
    complete: bool,
) -> Result<TransferReceipt> {
    if complete && let Some(splitter) = carried.splitter.as_ref() {
        splitter.finish()?;
    }
    if let Some(splitter) = carried.splitter.as_mut() {
        splitter.retain_through(carried.bytes);
    }
    sink.flush().await?;
    Ok(TransferReceipt {
        bytes: carried.bytes,
        end,
        declared_content_type: carried.declared_content_type,
        peer: carried.peer,
        route: carried.route,
        observations: carried
            .splitter
            .take()
            .map_or(Vec::new(), IcySplitter::into_observations),
    })
}

fn icy_interval(headers: &header::HeaderMap, metadata: MetadataPolicy) -> Result<Option<u64>> {
    let values = headers.get_all("icy-metaint");
    let count = values.iter().count();
    if count == 0 {
        return Ok(None);
    }
    if metadata == MetadataPolicy::Off || count != 1 {
        return Err(Error::Acquisition(
            "interleaved ICY metadata is unsupported",
        ));
    }
    let value = values
        .iter()
        .next()
        .and_then(|item| item.to_str().ok())
        .ok_or(Error::Acquisition("ICY metadata interval is invalid"))?;
    Ok(Some(interval_header(value)?))
}

fn validate_headers(headers: &header::HeaderMap) -> Result<AudioContentType> {
    let encoding = headers.get_all(header::CONTENT_ENCODING);
    for value in encoding {
        if !value.as_bytes().eq_ignore_ascii_case(b"identity") {
            return Err(Error::Acquisition("encoded HTTP body is unsupported"));
        }
    }
    if headers.get_all(header::CONTENT_TYPE).iter().count() != 1 {
        return Err(Error::Acquisition(
            "missing or ambiguous audio content type",
        ));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .ok_or(Error::Acquisition("invalid audio content type"))?;
    match mime.to_ascii_lowercase().as_str() {
        "audio/mpeg" => Ok(AudioContentType::Mpeg),
        "audio/aac" | "audio/aacp" => Ok(AudioContentType::Aac),
        "audio/flac" | "audio/x-flac" => Ok(AudioContentType::Flac),
        "audio/ogg" | "application/ogg" => Ok(AudioContentType::Ogg),
        "audio/wav" | "audio/wave" | "audio/x-wav" => Ok(AudioContentType::Wave),
        _ => Err(Error::Acquisition("unsupported audio content type")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AcquisitionLimits, EPISODE_BODY_BYTES, EPISODE_DURATION, HttpAcquirer, MetadataPolicy,
        TransferEnd,
    };
    use crate::sources::{HttpSource, NetworkScope};
    use sha2::{Digest, Sha256};
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn radio_limits_stay_below_the_episode_ceiling() -> crate::Result<()> {
        assert!(AcquisitionLimits::new(EPISODE_BODY_BYTES, Duration::from_secs(60)).is_err());
        assert!(AcquisitionLimits::new(1024, EPISODE_DURATION).is_err());
        let episode = AcquisitionLimits::episode();
        assert_eq!(episode.bytes(), EPISODE_BODY_BYTES);
        assert_eq!(episode.duration(), EPISODE_DURATION);
        assert!(episode.clean_end());
        assert!(!AcquisitionLimits::new(1024, Duration::from_secs(60))?.clean_end());
        Ok(())
    }

    #[tokio::test]
    async fn requested_metadata_is_hashed_as_audio_only() -> crate::Result<()> {
        let audio = b"abcdefghijklmnop";
        let title = "StreamTitle='Owned';StreamUrl='http://evil.example/secret';";
        let mut block = title.as_bytes().to_vec();
        let size = block.len().div_ceil(16) * 16;
        block.resize(size, 0);
        let chunks = u8::try_from(
            size.checked_div(16)
                .ok_or(crate::Error::Acquisition("icy block"))?,
        )
        .map_err(|_| crate::Error::Acquisition("icy block"))?;
        let mut body = audio.to_vec();
        body.push(chunks);
        body.extend(block);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nicy-metaint: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            audio.len(),
            body.len()
        );
        let server = tokio::spawn(async move {
            let mut request = Vec::new();
            let accepted = tokio::time::timeout(Duration::from_secs(2), listener.accept()).await;
            let Ok(Ok((mut socket, _))) = accepted else {
                return request;
            };
            while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                match socket.read_u8().await {
                    Ok(byte) => request.push(byte),
                    Err(_) => return request,
                }
            }
            if socket.write_all(response.as_bytes()).await.is_err()
                || socket.write_all(&body).await.is_err()
            {
                return request;
            }
            request
        });
        let source = HttpSource::new(
            "Icy",
            &format!("http://fixture.invalid:{}/audio", address.port()),
            NetworkScope::PinnedAddress {
                address: address.ip(),
            },
        )?;
        let mut received = Vec::new();
        let (_stop, mut signal) = tokio::sync::watch::channel(false);
        let receipt = HttpAcquirer::default()
            .record_with(
                &source,
                AcquisitionLimits::new(1024, Duration::from_secs(2))?,
                &mut received,
                &mut signal,
                MetadataPolicy::Requested,
            )
            .await?;
        assert_eq!(received, audio);
        assert_eq!(
            receipt.bytes,
            u64::try_from(audio.len()).map_err(|_| crate::Error::StorageIntegrity)?
        );
        assert_eq!(receipt.end, TransferEnd::EndOfBody);
        assert_eq!(Sha256::digest(&received), Sha256::digest(audio));
        assert_eq!(receipt.observations.len(), 1);
        assert_eq!(receipt.observations[0].text, title);
        assert_eq!(receipt.observations[0].audio_offset, receipt.bytes);
        let request = server
            .await
            .map_err(|_| crate::Error::Acquisition("icy fixture failed"))?;
        let text = String::from_utf8_lossy(&request);
        assert!(text.to_ascii_lowercase().contains("icy-metadata: 1"));
        assert!(!text.contains("evil.example"));
        Ok(())
    }
}
