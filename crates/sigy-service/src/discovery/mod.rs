//! Directory observations are candidates, never authority to acquire a stream.

pub(crate) mod radio_browser;

use crate::{
    Error, Result,
    sources::{HttpSource, NetworkScope, unsafe_display},
};
use serde::{Deserialize, Serialize};

pub const MAX_REFRESH_ROWS: u32 = 500;
pub const MAX_CACHED_STATIONS: u32 = 10_000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationFilter {
    pub name: String,
    pub country: String,
    pub language: String,
    pub tag: String,
    pub healthy_only: bool,
}

impl StationFilter {
    /// # Errors
    /// Rejects excessive or terminal-unsafe filters and malformed country codes.
    pub fn validate(&self) -> Result<()> {
        for text in [&self.name, &self.language, &self.tag] {
            validate_text(text, 128)?;
        }
        if !self.country.is_empty()
            && (self.country.len() != 2 || !self.country.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(Error::InvalidInput("two-letter uppercase country code"));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    pub filter: StationFilter,
    pub limit: u32,
    pub offset: u32,
    pub mirror: Option<String>,
    pub network: NetworkScope,
}

impl std::fmt::Debug for RefreshRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshRequest")
            .field("filter", &self.filter)
            .field("limit", &self.limit)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl RefreshRequest {
    /// # Errors
    /// Rejects unbounded requests, ambiguous mirror origins and implicit local grants.
    pub fn validate(&self) -> Result<()> {
        self.filter.validate()?;
        if !(1..=MAX_REFRESH_ROWS).contains(&self.limit) || self.offset > 100_000 {
            return Err(Error::InvalidInput("catalog page bounds"));
        }
        match &self.mirror {
            Some(origin) => {
                let source = HttpSource::new("Radio Browser", origin, self.network)?;
                let url = reqwest::Url::parse(source.endpoint())
                    .map_err(|_| Error::InvalidInput("mirror origin"))?;
                if url.path() != "/"
                    || url.query().is_some()
                    || (url.scheme() != "https"
                        && matches!(self.network, NetworkScope::PublicInternet {}))
                {
                    return Err(Error::InvalidInput(
                        "mirror must be an HTTPS origin, or an explicitly pinned HTTP origin",
                    ));
                }
            }
            None if self.network != NetworkScope::PublicInternet {} => {
                return Err(Error::InvalidInput(
                    "address pin requires an explicit mirror",
                ));
            }
            None => (),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Station {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub country: String,
    pub state: String,
    pub languages: Vec<String>,
    pub language_codes: Vec<String>,
    pub tags: Vec<String>,
    pub codec: String,
    pub bitrate_kbps: u32,
    pub hls: bool,
    pub last_check_ok: Option<bool>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub stream_origin: String,
    pub observed_ms: i64,
    pub refresh_id: String,
}

impl Station {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_station_id(&self.id)?;
        if self.provider != "radio_browser" || self.name.trim().is_empty() || self.observed_ms < 0 {
            return Err(Error::InvalidInput("station metadata"));
        }
        validate_text(&self.name, 256)?;
        validate_text(&self.state, 128)?;
        validate_text(&self.codec, 32)?;
        validate_text(&self.stream_origin, 2048)?;
        validate_text(&self.refresh_id, 128)?;
        for list in [&self.languages, &self.language_codes, &self.tags] {
            if list.len() > 32 {
                return Err(Error::InvalidInput("station label count"));
            }
            for text in list {
                validate_text(text, 128)?;
            }
        }
        StationFilter {
            country: self.country.clone(),
            ..StationFilter::default()
        }
        .validate()?;
        let coordinates_valid = match (self.latitude, self.longitude) {
            (None, None) => true,
            (Some(latitude), Some(longitude)) => {
                latitude.is_finite()
                    && longitude.is_finite()
                    && (-90.0..=90.0).contains(&latitude)
                    && (-180.0..=180.0).contains(&longitude)
            }
            _ => false,
        };
        if self.bitrate_kbps > 1_000_000 || !coordinates_valid {
            return Err(Error::InvalidInput("station measurements"));
        }
        Ok(())
    }
}

pub(crate) struct Candidate {
    pub station: Station,
    pub endpoint: String,
}
pub(crate) struct RefreshBatch {
    pub candidates: Vec<Candidate>,
    pub skipped: u32,
    pub origin: String,
}

pub(crate) fn validate_text(value: &str, maximum: usize) -> Result<()> {
    if value.len() > maximum || value.chars().any(unsafe_display) {
        return Err(Error::InvalidInput("directory text"));
    }
    Ok(())
}

pub(crate) fn validate_station_id(value: &str) -> Result<()> {
    if value.len() != 36
        || !value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
    {
        return Err(Error::InvalidInput("station UUID"));
    }
    Ok(())
}
