//! Bounded metadata reads through the same destination and transport policy.

use super::{AcquisitionLimits, HttpAcquirer};
use crate::{Error, Result, sources::HttpSource};
use std::time::Duration;
use tokio::time::{Instant, timeout, timeout_at};

pub(crate) const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

impl HttpAcquirer {
    pub(crate) async fn json_document(&self, source: &HttpSource) -> Result<Vec<u8>> {
        let _slot = self
            .attempts
            .try_acquire()
            .map_err(|_| Error::Acquisition("capacity reached"))?;
        let limits = AcquisitionLimits::new(MAX_DOCUMENT_BYTES as u64, Duration::from_secs(8))?;
        let deadline = Instant::now() + limits.duration;
        timeout_at(deadline, async {
            let (mut response, _) = self
                .open(source, limits, deadline, "application/json")
                .await?;
            for encoding in response
                .headers()
                .get_all(reqwest::header::CONTENT_ENCODING)
            {
                if !encoding.as_bytes().eq_ignore_ascii_case(b"identity") {
                    return Err(Error::Acquisition("encoded metadata is unsupported"));
                }
            }
            let types = response.headers().get_all(reqwest::header::CONTENT_TYPE);
            if types.iter().count() != 1
                || !types
                    .iter()
                    .next()
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.split(';').next())
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
            {
                return Err(Error::Acquisition("expected JSON metadata"));
            }
            if response
                .content_length()
                .is_some_and(|bytes| bytes > MAX_DOCUMENT_BYTES as u64)
            {
                return Err(Error::Acquisition("metadata size limit"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::Acquisition("metadata body interrupted"))?
            {
                if chunk.len() > MAX_DOCUMENT_BYTES - bytes.len() {
                    return Err(Error::Acquisition("metadata size limit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| Error::Acquisition("metadata deadline"))?
    }

    pub(crate) async fn service_hosts(&self, name: &str) -> Result<Vec<String>> {
        let _slot = self
            .dns
            .try_acquire()
            .map_err(|_| Error::Acquisition("DNS capacity reached"))?;
        timeout(Duration::from_secs(5), async {
            let resolver = hickory_resolver::Resolver::builder_tokio()
                .and_then(hickory_resolver::ResolverBuilder::build)
                .map_err(|_| Error::Acquisition("DNS configuration"))?;
            let lookup = resolver
                .srv_lookup(name)
                .await
                .map_err(|_| Error::Acquisition("directory mirror discovery"))?;
            let mut hosts = Vec::new();
            for record in lookup.answers().iter().take(17) {
                if let hickory_resolver::proto::rr::RData::SRV(srv) = &record.data {
                    hosts.push(srv.target.to_utf8().trim_end_matches('.').to_lowercase());
                }
            }
            if hosts.is_empty() || hosts.len() > 16 {
                return Err(Error::Acquisition("directory mirror count"));
            }
            Ok(hosts)
        })
        .await
        .map_err(|_| Error::Acquisition("directory DNS deadline"))?
    }
}
