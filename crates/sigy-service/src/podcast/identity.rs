//! Episode identity is scoped to one subscription. Titles are never part of it.

use sha2::{Digest, Sha256};

use crate::sources::unsafe_display;

/// Publisher guid, or a labeled derivation from an enclosure URL and publication time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EpisodeIdentity {
    PublisherGuid(String),
    Derived { url: String, published_ms: i64 },
}

impl EpisodeIdentity {
    #[must_use]
    pub(crate) fn key(&self) -> String {
        let mut material = Vec::new();
        match self {
            Self::PublisherGuid(guid) => {
                material.extend_from_slice(b"v1\0publisher_guid\0");
                material.extend_from_slice(guid.as_bytes());
            }
            Self::Derived { url, published_ms } => {
                material.extend_from_slice(b"v1\0derived_enclosure\0");
                material.extend_from_slice(url.as_bytes());
                material.push(0);
                material.extend_from_slice(&published_ms.to_le_bytes());
            }
        }
        hex_encode(Sha256::digest(material).as_slice())
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::PublisherGuid(_) => "publisher_guid",
            Self::Derived { .. } => "derived_enclosure",
        }
    }
}

/// Syntactic HTTP(S) normalization. This does not resolve DNS or grant a fetch.
#[must_use]
pub(crate) fn normalize_http_url(value: &str) -> Option<String> {
    if value.len() > 2048
        || value
            .chars()
            .any(|character| character.is_whitespace() || unsafe_display(character))
        || value.contains('\\')
    {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() == Some(0)
        || url.as_str().len() > 2048
    {
        return None;
    }
    Some(url.as_str().to_owned())
}

/// Resolves one reference against the fetched document URL. `xml:base` is ignored.
#[must_use]
pub(crate) fn resolve_http_url(value: &str, base: Option<&reqwest::Url>) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let absolute = if let Some(base) = base {
        base.join(value).ok()?.as_str().to_owned()
    } else if value.contains("://") {
        value.to_owned()
    } else {
        return None;
    };
    normalize_http_url(&absolute)
}

pub(crate) fn document_sha256(bytes: &[u8]) -> String {
    hex_encode(Sha256::digest(bytes).as_slice())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{EpisodeIdentity, normalize_http_url};

    #[test]
    fn normalization_drops_credentials_and_default_ports() {
        assert_eq!(
            normalize_http_url("HTTP://Example.COM:80/a"),
            Some("http://example.com/a".to_owned())
        );
        assert!(normalize_http_url("http://user:secret@example.com/a").is_none());
        assert!(normalize_http_url("http://example.com/a#part").is_none());
    }

    #[test]
    fn titles_cannot_collide_with_either_identity() {
        let guid = EpisodeIdentity::PublisherGuid("Same title".to_owned()).key();
        let derived = EpisodeIdentity::Derived {
            url: "http://cdn.example/a.mp3".to_owned(),
            published_ms: 1,
        }
        .key();
        assert_ne!(guid, derived);
        assert_eq!(
            guid,
            EpisodeIdentity::PublisherGuid("Same title".to_owned()).key()
        );
    }
}
