//! Redirect authorization is part of the immutable source, not server authority.

use super::{HttpSource, unsafe_display};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, net::SocketAddr, str::FromStr};

pub(crate) const MAX_REDIRECTS: usize = 3;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedirectPolicy {
    #[default]
    Deny,
    SameOrigin,
    Public,
}

impl fmt::Display for RedirectPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Deny => "deny",
            Self::SameOrigin => "same-origin",
            Self::Public => "public",
        })
    }
}

impl FromStr for RedirectPolicy {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "deny" => Ok(Self::Deny),
            "same-origin" => Ok(Self::SameOrigin),
            "public" => Ok(Self::Public),
            _ => Err(Error::InvalidInput(
                "redirect policy: deny, same-origin, or public",
            )),
        }
    }
}

/// Successful response observations. Paths and queries are intentionally omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpHop {
    pub origin: String,
    pub peer: SocketAddr,
    pub status: u16,
}

impl HttpSource {
    pub(crate) fn redirect_target(&self, location: &str) -> Result<Self> {
        if self.redirects == RedirectPolicy::Deny {
            return Err(Error::Acquisition(
                "redirect requires a separately authorized source revision",
            ));
        }
        if location.is_empty()
            || location.len() > 2048
            || location
                .chars()
                .any(|c| c.is_whitespace() || unsafe_display(c))
            || location.contains('\\')
        {
            return Err(Error::Acquisition("invalid redirect location"));
        }
        let url = self
            .url
            .join(location)
            .map_err(|_| Error::Acquisition("invalid redirect location"))?;
        let target =
            Self::new(&self.name, url.as_str(), self.network)?.with_redirects(self.redirects)?;
        if (self.url.scheme() == "https" && target.url.scheme() != "https")
            || (self.redirects == RedirectPolicy::SameOrigin
                && self.url.origin() != target.url.origin())
        {
            return Err(Error::DestinationDenied);
        }
        Ok(target)
    }

    pub(crate) fn validate_route(&self, route: &[HttpHop]) -> Result<()> {
        if route.is_empty()
            || route.len() > MAX_REDIRECTS + 1
            || route[0].origin != self.origin()
            || (self.redirects == RedirectPolicy::Deny && route.len() != 1)
        {
            return Err(Error::SourceIntegrity);
        }
        let mut previous = self.clone();
        for (index, hop) in route.iter().enumerate() {
            let current =
                Self::new(&self.name, &hop.origin, self.network)?.with_redirects(self.redirects)?;
            if current.origin() != hop.origin
                || current.url.path() != "/"
                || current.url.query().is_some()
                || !self.network.permits(hop.peer.ip())
                || current.url.port_or_known_default() != Some(hop.peer.port())
            {
                return Err(Error::SourceIntegrity);
            }
            if index > 0 {
                previous.redirect_target(&hop.origin)?;
            }
            let last = index + 1 == route.len();
            if (last && hop.status != 200) || (!last && !is_redirect(hop.status)) {
                return Err(Error::SourceIntegrity);
            }
            previous = current;
        }
        Ok(())
    }
}

pub(crate) const fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::NetworkScope;

    #[test]
    fn redirects_preserve_scope_and_reject_unsafe_locations() -> Result<()> {
        let source = HttpSource::new(
            "Station",
            "https://radio.example/start",
            NetworkScope::PublicInternet {},
        )?;
        assert!(source.redirect_target("/next").is_err());
        let same = source.clone().with_redirects(RedirectPolicy::SameOrigin)?;
        assert_eq!(
            same.redirect_target("../audio")?.endpoint(),
            "https://radio.example/audio"
        );
        assert!(same.redirect_target("https://other.example/audio").is_err());
        let public = source.with_redirects(RedirectPolicy::Public)?;
        assert_eq!(
            public.redirect_target("//other.example/audio")?.origin(),
            "https://other.example"
        );
        for location in [
            "http://other.example/audio",
            "https://127.0.0.1/audio",
            "https://169.254.169.254/",
            "https://[::1]/",
            "https://2130706433/",
            "file:///tmp/audio",
            "https://user:password@other.example/",
            "/audio#fragment",
            "/audio\n",
            " /audio",
            "\\other.example\\audio",
            "",
        ] {
            assert!(
                public.redirect_target(location).is_err(),
                "accepted {location:?}"
            );
        }
        assert!(
            HttpSource::new(
                "Local",
                "http://fixture.invalid/",
                NetworkScope::PinnedAddress {
                    address: std::net::Ipv4Addr::LOCALHOST.into()
                }
            )?
            .with_redirects(RedirectPolicy::Public)
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn stored_route_cannot_claim_unpermitted_or_nonterminal_responses() -> Result<()> {
        let source = HttpSource::new(
            "Station",
            "https://radio.example/audio",
            NetworkScope::PublicInternet {},
        )?;
        let hop = HttpHop {
            origin: source.origin(),
            peer: SocketAddr::from(([8, 8, 8, 8], 443)),
            status: 200,
        };
        source.validate_route(std::slice::from_ref(&hop))?;
        assert!(source.validate_route(&[]).is_err());
        let mut bad = hop.clone();
        bad.peer = SocketAddr::from(([127, 0, 0, 1], 443));
        assert!(source.validate_route(&[bad]).is_err());
        let mut bad = hop.clone();
        bad.origin.push_str("/secret?token=hidden");
        assert!(source.validate_route(&[bad]).is_err());
        let mut bad = hop;
        bad.status = 302;
        assert!(source.validate_route(&[bad]).is_err());
        Ok(())
    }
}
