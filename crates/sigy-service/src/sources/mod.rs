//! Validated source configurations. Discovery data never grants network access.

pub(crate) mod hls;
pub mod http;
pub(crate) mod playlist;
mod policy;
mod redirects;

pub use redirects::{HttpHop, RedirectPolicy};

use std::{fmt, net::IpAddr};

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// An exact address grant is an explicit local-user choice, not an adapter hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkScope {
    PublicInternet {},
    PinnedAddress { address: IpAddr },
}

impl NetworkScope {
    pub(crate) fn permits(self, address: IpAddr) -> bool {
        match self {
            Self::PublicInternet {} => policy::is_public(address),
            Self::PinnedAddress { address: expected } => {
                address == expected && policy::is_pinnable(address)
            }
        }
    }
}

/// The first adapter accepts direct HTTP audio bodies. Other signal kinds will
/// have their own configurations rather than inheriting audio assumptions.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpSource {
    name: String,
    url: Url,
    network: NetworkScope,
    redirects: RedirectPolicy,
}

impl fmt::Debug for HttpSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // URL paths and queries can contain credentials even without userinfo.
        f.debug_struct("HttpSource")
            .field("name", &self.name)
            .field("origin", &self.origin())
            .field("network", &self.network)
            .field("redirects", &self.redirects)
            .finish_non_exhaustive()
    }
}

impl HttpSource {
    /// Parses once and validates the authority before storage or execution.
    /// # Errors
    /// Rejects unsafe display text, unsupported URLs, credentials, fragments,
    /// denied literal destinations, and unsupported pinned-address classes.
    pub fn new(name: &str, endpoint: &str, network: NetworkScope) -> Result<Self> {
        if name.trim().is_empty() || name.len() > 256 || name.chars().any(unsafe_display) {
            return Err(Error::InvalidInput("source name"));
        }
        if endpoint.len() > 2048
            || endpoint.chars().any(char::is_whitespace)
            || endpoint.chars().any(unsafe_display)
            || endpoint.contains('\\')
        {
            return Err(Error::InvalidInput("source URL"));
        }
        let url = Url::parse(endpoint).map_err(|_| Error::InvalidInput("source URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.port_or_known_default() == Some(0)
            || url.as_str().len() > 2048
        {
            return Err(Error::InvalidInput("direct HTTP(S) source URL"));
        }
        if let NetworkScope::PinnedAddress { address } = network
            && !policy::is_pinnable(address)
        {
            return Err(Error::DestinationDenied);
        }
        if let Some(address) = literal_address(&url)
            && !network.permits(address)
        {
            return Err(Error::DestinationDenied);
        }
        Ok(Self {
            name: name.into(),
            url,
            network,
            redirects: RedirectPolicy::Deny,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The origin omits paths and queries from ordinary diagnostics.
    #[must_use]
    pub fn origin(&self) -> String {
        self.url.origin().ascii_serialization()
    }

    #[must_use]
    pub const fn network(&self) -> NetworkScope {
        self.network
    }

    /// # Errors
    /// Cross-origin redirects require public-internet scope, never an address pin.
    pub fn with_redirects(mut self, policy: RedirectPolicy) -> Result<Self> {
        if policy == RedirectPolicy::Public && self.network != (NetworkScope::PublicInternet {}) {
            return Err(Error::InvalidInput(
                "public redirects require public-internet scope",
            ));
        }
        self.redirects = policy;
        Ok(self)
    }

    #[must_use]
    pub const fn redirects(&self) -> RedirectPolicy {
        self.redirects
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.url.as_str()
    }
}

fn literal_address(url: &Url) -> Option<IpAddr> {
    // URL normalization covers shortened, integer, hex and octal IPv4 forms.
    url.host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

pub(crate) fn unsafe_display(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}
