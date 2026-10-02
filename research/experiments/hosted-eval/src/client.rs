//! Bounded `OpenRouter` transport over `rustls` with offline `WebPKI` roots.
//!
//! The client never retries, follows no redirects, uses no proxy and bounds
//! connect time, total request time and response bytes. A failure before a
//! connection exists is reported as not sent; any later failure is ambiguous
//! because the provider may already have accepted and billed the request.

use std::time::{Duration, Instant};

use reqwest::{
    Client,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};

use crate::Result;

pub const PRODUCTION_BASE: &str = "https://openrouter.ai/api/v1";
pub const KEY_VARIABLE: &str = "OPENROUTER_API_KEY";

#[derive(Debug)]
pub enum Sent {
    Completed {
        status: u16,
        body: Vec<u8>,
        elapsed_ms: u64,
    },
    Unsent(&'static str),
    Ambiguous(&'static str),
}

pub(crate) trait Transport {
    async fn post(&self, path: &str, body: Vec<u8>) -> Sent;
    async fn get(&self, path: &str) -> Sent;
}

/// An API key that never appears in `Debug`, errors or output.
pub struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(redacted)")
    }
}

impl Secret {
    /// Read the key from this process environment only.
    pub fn from_environment() -> Result<Self> {
        Self::new(
            std::env::var(KEY_VARIABLE).map_err(|_| "API key variable is absent or not Unicode")?,
        )
    }

    fn new(value: String) -> Result<Self> {
        if !(16..=512).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err("API key variable has an invalid shape".into());
        }
        Ok(Self(value))
    }

    fn header(&self) -> Option<HeaderValue> {
        let mut value = HeaderValue::from_str(&format!("Bearer {}", self.0)).ok()?;
        value.set_sensitive(true);
        Some(value)
    }
}

#[derive(Debug)]
pub struct OpenRouter {
    client: Client,
    base: String,
    key: Secret,
    max_body: usize,
}

impl OpenRouter {
    pub fn production(key: Secret, request_timeout: Duration) -> Result<Self> {
        Self::build(PRODUCTION_BASE, key, request_timeout, true)
    }

    fn build(base: &str, key: Secret, request_timeout: Duration, https_only: bool) -> Result<Self> {
        let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|root| reqwest::Certificate::from_der(root.as_ref()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let client = Client::builder()
            .tls_certs_only(roots)
            .https_only(https_only)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(10).min(request_timeout))
            .timeout(request_timeout)
            .user_agent("sigy-hosted-eval/0")
            .build()?;
        Ok(Self {
            client,
            base: base.into(),
            key,
            max_body: 1024 * 1024,
        })
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Sent {
        let Some(authorization) = self.key.header() else {
            return Sent::Unsent("authorization-header-invalid");
        };
        let started = Instant::now();
        let mut response = match request
            .header(AUTHORIZATION, authorization)
            .header("X-OpenRouter-Metadata", "enabled")
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) if error.is_connect() || error.is_builder() => {
                return Sent::Unsent("connect-failed");
            }
            Err(error) if error.is_timeout() => return Sent::Ambiguous("timeout-after-send"),
            Err(_) => return Sent::Ambiguous("request-failed-after-connect"),
        };
        let status = response.status().as_u16();
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len() + chunk.len() > self.max_body {
                        return Sent::Ambiguous("response-over-ceiling");
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(error) if error.is_timeout() => {
                    return Sent::Ambiguous("timeout-reading-response");
                }
                Err(_) => return Sent::Ambiguous("response-read-failed"),
            }
        }
        Sent::Completed {
            status,
            body,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        }
    }
}

impl Transport for OpenRouter {
    async fn post(&self, path: &str, body: Vec<u8>) -> Sent {
        let request = self
            .client
            .post(format!("{}{path}", self.base))
            .header(CONTENT_TYPE, "application/json")
            .body(body);
        self.send(request).await
    }

    async fn get(&self, path: &str) -> Sent {
        self.send(self.client.get(format!("{}{path}", self.base)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn key() -> Result<Secret> {
        Secret::new("fixture-key-0123456789".into())
    }

    async fn serve(mode: &'static str) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut request = vec![0_u8; 64 * 1024];
                let mut read = 0;
                // Read headers and any body that arrives promptly.
                while let Ok(Ok(count)) = tokio::time::timeout(
                    Duration::from_millis(200),
                    socket.read(&mut request[read..]),
                )
                .await
                {
                    if count == 0 {
                        break;
                    }
                    read += count;
                }
                let text = String::from_utf8_lossy(&request[..read]).to_string();
                let authorized = text.contains("authorization: Bearer fixture-key-0123456789")
                    && text.contains("x-openrouter-metadata: enabled");
                let reply = match mode {
                    "ok" if authorized => "HTTP/1.1 200 OK\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}".to_owned(),
                    "ok" => "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
                    "large" => format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", 2 * 1024 * 1024, "x".repeat(2 * 1024 * 1024)),
                    "truncated" => "HTTP/1.1 200 OK\r\ncontent-length: 100\r\nconnection: close\r\n\r\n{".to_owned(),
                    "hang" => {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        String::new()
                    }
                    _ => String::new(),
                };
                let _ = socket.write_all(reply.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Ok(format!("http://{address}"))
    }

    #[tokio::test]
    async fn loopback_contract_classifies_every_outcome() -> Result<()> {
        let timeout = Duration::from_millis(1500);
        let client = OpenRouter::build(&serve("ok").await?, key()?, timeout, false)?;
        match client.post("/chat", b"{}".to_vec()).await {
            Sent::Completed { status, body, .. } => {
                assert_eq!(status, 200);
                assert_eq!(body, b"{\"ok\":true}");
            }
            other => return Err(format!("unexpected {other:?}").into()),
        }
        let client = OpenRouter::build(&serve("large").await?, key()?, timeout, false)?;
        assert!(matches!(
            client.get("/key").await,
            Sent::Ambiguous("response-over-ceiling")
        ));
        let client = OpenRouter::build(&serve("truncated").await?, key()?, timeout, false)?;
        assert!(matches!(client.get("/key").await, Sent::Ambiguous(_)));
        let client = OpenRouter::build(&serve("hang").await?, key()?, timeout, false)?;
        assert!(matches!(
            client.post("/chat", b"{}".to_vec()).await,
            Sent::Ambiguous("timeout-after-send")
        ));
        let client = OpenRouter::build(&serve("close").await?, key()?, timeout, false)?;
        assert!(matches!(
            client.post("/chat", b"{}".to_vec()).await,
            Sent::Ambiguous(_)
        ));
        // A port with no listener refuses the connection before any request bytes.
        let closed = TcpListener::bind("127.0.0.1:0").await?;
        let address = closed.local_addr()?;
        drop(closed);
        // Windows retries a refused loopback SYN for about two seconds.
        let client = OpenRouter::build(
            &format!("http://{address}"),
            key()?,
            Duration::from_secs(20),
            false,
        )?;
        let refused = client.get("/key").await;
        assert!(
            matches!(refused, Sent::Unsent("connect-failed")),
            "{refused:?}"
        );
        // Production refuses plain HTTP before connecting.
        let client = OpenRouter::build(&format!("http://{address}"), key()?, timeout, true)?;
        assert!(matches!(client.get("/key").await, Sent::Unsent(_)));
        assert!(OpenRouter::production(key()?, timeout).is_ok());
        Ok(())
    }

    #[test]
    fn secrets_are_validated_and_never_printed() -> Result<()> {
        let secret = key()?;
        assert_eq!(format!("{secret:?}"), "Secret(redacted)");
        assert!(secret.header().is_some_and(|value| value.is_sensitive()));
        for bad in [
            "short",
            "has space in the middle of key",
            "line\nbreak-0123456789",
        ] {
            let error = Secret::new(bad.into())
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(!error.is_empty() && !error.contains(bad));
        }
        Ok(())
    }
}
