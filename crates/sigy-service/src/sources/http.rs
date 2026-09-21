//! Finite HTTP transport only. Received bytes are not verified media or a recording.

mod documents;
mod resolver;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use reqwest::{Client, StatusCode, header};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::Semaphore,
    time::{Instant, timeout_at},
};

use super::HttpSource;
use crate::{Error, Result};

pub const MAXIMUM_BODY_BYTES: u64 = 256 * 1024 * 1024;
pub const MAXIMUM_DURATION: Duration = Duration::from_mins(15);
const MAX_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, Copy)]
pub struct AcquisitionLimits {
    bytes: u64,
    duration: Duration,
}

impl AcquisitionLimits {
    /// # Errors
    /// Rejects zero or out-of-profile limits. Limits include connection setup.
    pub fn new(bytes: u64, duration: Duration) -> Result<Self> {
        if bytes == 0
            || bytes > MAXIMUM_BODY_BYTES
            || duration.is_zero()
            || duration > MAXIMUM_DURATION
        {
            return Err(Error::InvalidInput("HTTP acquisition limits"));
        }
        Ok(Self { bytes, duration })
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
    /// Copies a single direct HTTP(S) body to a caller-owned bounded sink.
    /// Does not create files, follow redirects, retry, decode, or finalize jobs.
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
            self.transfer(source, limits, sink, deadline, None),
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
        let _slot = self
            .attempts
            .try_acquire()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let deadline = Instant::now() + limits.duration;
        timeout_at(
            deadline + Duration::from_secs(5),
            self.transfer(source, limits, sink, deadline, Some(stop)),
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
            .header(header::ACCEPT_ENCODING, "identity")
            .header("icy-metadata", "0")
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
        if response.status().is_redirection() {
            return Err(Error::Acquisition(
                "redirect requires a separately authorized source revision",
            ));
        }
        if response.status() != StatusCode::OK {
            return Err(Error::Acquisition("source did not return HTTP 200"));
        }
        Ok(response)
    }

    async fn transfer<W: AsyncWrite + Unpin>(
        &self,
        source: &HttpSource,
        limits: AcquisitionLimits,
        sink: &mut W,
        deadline: Instant,
        mut stop: Option<&mut tokio::sync::watch::Receiver<bool>>,
    ) -> Result<TransferReceipt> {
        let mut response = self
            .open(
                source,
                limits,
                deadline,
                "audio/mpeg, audio/aac, audio/flac, audio/ogg, audio/wav, application/ogg",
            )
            .await?;
        let peer = response.remote_addr().ok_or(Error::DestinationDenied)?;
        let declared_content_type = validate_headers(response.headers())?;
        let mut bytes = 0;
        loop {
            if bytes == limits.bytes {
                sink.flush().await?;
                return Ok(TransferReceipt {
                    bytes,
                    end: TransferEnd::ByteLimit,
                    declared_content_type,
                    peer,
                });
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
                        if bytes == 0 { return Err(Error::Acquisition("stopped before receiving audio")); }
                        sink.flush().await?;
                        return Ok(TransferReceipt { bytes, end: TransferEnd::UserStop, declared_content_type, peer });
                    }
                    () = tokio::time::sleep_until(deadline) => None,
                    result = body => Some(result?),
                };
                if let Some(chunk) = end {
                    chunk
                } else {
                    if bytes == 0 {
                        return Err(Error::Acquisition("empty timed recording"));
                    }
                    sink.flush().await?;
                    return Ok(TransferReceipt {
                        bytes,
                        end: TransferEnd::DurationLimit,
                        declared_content_type,
                        peer,
                    });
                }
            } else {
                body.await?
            };
            let Some(chunk) = chunk else {
                if bytes == 0 {
                    return Err(Error::Acquisition("empty body"));
                }
                sink.flush().await?;
                return Ok(TransferReceipt {
                    bytes,
                    end: TransferEnd::EndOfBody,
                    declared_content_type,
                    peer,
                });
            };
            let remaining = usize::try_from(limits.bytes - bytes)
                .map_err(|_| Error::InvalidInput("body byte range"))?;
            let accepted = chunk.len().min(remaining);
            // No whole-body buffering, decompression or secondary URL fetching.
            sink.write_all(&chunk[..accepted]).await?;
            bytes += u64::try_from(accepted).map_err(|_| Error::InvalidInput("body byte range"))?;
        }
    }
}

fn validate_headers(headers: &header::HeaderMap) -> Result<AudioContentType> {
    let encoding = headers.get_all(header::CONTENT_ENCODING);
    for value in encoding {
        if !value.as_bytes().eq_ignore_ascii_case(b"identity") {
            return Err(Error::Acquisition("encoded HTTP body is unsupported"));
        }
    }
    if headers.contains_key("icy-metaint") {
        return Err(Error::Acquisition(
            "interleaved ICY metadata is unsupported",
        ));
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
