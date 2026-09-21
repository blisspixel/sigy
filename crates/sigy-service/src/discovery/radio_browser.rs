//! The first directory adapter. Mirror reads never report clicks or votes.

use super::{Candidate, RefreshBatch, RefreshRequest, Station};
use crate::{
    Error, Result,
    sources::{HttpSource, NetworkScope, http::HttpAcquirer},
};
use serde::Deserialize;
use std::{collections::HashSet, time::Duration};

pub(crate) async fn refresh(
    acquirer: &HttpAcquirer,
    request: &RefreshRequest,
) -> Result<RefreshBatch> {
    request.validate()?;
    tokio::time::timeout(Duration::from_secs(25), async {
        let mirrors = if let Some(mirror) = &request.mirror {
            vec![mirror.clone()]
        } else {
            let mut hosts = acquirer
                .service_hosts("_api._tcp.radio-browser.info.")
                .await?;
            hosts.retain(|name| {
                name.ends_with(".api.radio-browser.info")
                    && !name.contains('/')
                    && !name.contains(':')
            });
            hosts.sort();
            hosts.dedup();
            if hosts.is_empty() {
                return Err(Error::Acquisition("no supported directory mirrors"));
            }
            let mut random = [0_u8; 8];
            getrandom::fill(&mut random)
                .map_err(|_| Error::Acquisition("mirror selection entropy"))?;
            let index = usize::try_from(u64::from_le_bytes(random) % hosts.len() as u64)
                .map_err(|_| Error::Acquisition("mirror selection"))?;
            hosts.rotate_left(index);
            hosts
                .into_iter()
                .take(2)
                .map(|host| format!("https://{host}"))
                .collect()
        };
        let mut failure = Error::Acquisition("directory request failed");
        for (attempt, mirror) in mirrors.iter().enumerate() {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let source = query_source(mirror, request)?;
            match acquirer
                .json_document(&source)
                .await
                .and_then(|bytes| parse(&bytes, request.limit, source.origin()))
            {
                Ok(batch) => return Ok(batch),
                Err(error) => failure = error,
            }
        }
        Err(failure)
    })
    .await
    .map_err(|_| Error::Acquisition("directory refresh deadline"))?
}

fn query_source(mirror: &str, request: &RefreshRequest) -> Result<HttpSource> {
    let mut url = reqwest::Url::parse(mirror).map_err(|_| Error::InvalidInput("mirror URL"))?;
    url.set_path("/json/stations/search");
    for (key, value) in [
        ("name", &request.filter.name),
        ("countrycode", &request.filter.country),
        ("language", &request.filter.language),
        ("tag", &request.filter.tag),
    ] {
        if !value.is_empty() {
            url.query_pairs_mut().append_pair(key, value);
        }
    }
    if !request.filter.language.is_empty() {
        url.query_pairs_mut().append_pair("languageExact", "true");
    }
    if !request.filter.tag.is_empty() {
        url.query_pairs_mut().append_pair("tagExact", "true");
    }
    url.query_pairs_mut()
        .append_pair(
            "hidebroken",
            if request.filter.healthy_only {
                "true"
            } else {
                "false"
            },
        )
        .append_pair("order", "name")
        .append_pair("limit", &request.limit.to_string())
        .append_pair("offset", &request.offset.to_string());
    HttpSource::new("Radio Browser metadata", url.as_str(), request.network)
}

#[derive(Deserialize)]
struct RawStation {
    stationuuid: String,
    name: String,
    url: String,
    #[serde(default)]
    url_resolved: String,
    #[serde(default)]
    countrycode: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    languagecodes: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    codec: String,
    #[serde(default)]
    bitrate: u32,
    #[serde(default)]
    hls: u8,
    #[serde(default)]
    lastcheckok: Option<u8>,
    #[serde(default)]
    geo_lat: Option<f64>,
    #[serde(default)]
    geo_long: Option<f64>,
}

fn candidate(raw: RawStation) -> Result<Candidate> {
    let endpoint = if raw.url_resolved.is_empty() {
        raw.url
    } else {
        raw.url_resolved
    };
    // Directory entries cannot grant private-network access, even on a local mirror.
    let source = HttpSource::new(raw.name.trim(), &endpoint, NetworkScope::PublicInternet {})?;
    if raw.hls > 1 || raw.lastcheckok.is_some_and(|v| v > 1) {
        return Err(Error::InvalidInput("station flags"));
    }
    let split = |value: &str| -> Result<Vec<String>> {
        let mut labels = Vec::new();
        for label in value.split(',').map(str::trim).filter(|v| !v.is_empty()) {
            if labels.len() == 32 {
                return Err(Error::InvalidInput("station label count"));
            }
            super::validate_text(label, 128)?;
            labels.push(label.to_owned());
        }
        Ok(labels)
    };
    let station = Station {
        provider: "radio_browser".into(),
        id: raw.stationuuid.to_ascii_lowercase(),
        name: source.name().into(),
        country: raw.countrycode.to_ascii_uppercase(),
        state: raw.state,
        languages: split(&raw.language)?,
        language_codes: split(&raw.languagecodes)?,
        tags: split(&raw.tags)?,
        codec: raw.codec,
        bitrate_kbps: raw.bitrate,
        hls: raw.hls == 1,
        last_check_ok: raw.lastcheckok.map(|v| v == 1),
        latitude: raw.geo_lat,
        longitude: raw.geo_long,
        stream_origin: source.origin(),
        observed_ms: 0,
        refresh_id: String::new(),
    };
    station.validate()?;
    if serde_json::to_vec(&station)?.len() > 7500 {
        return Err(Error::InvalidInput("station metadata size"));
    }
    Ok(Candidate {
        station,
        endpoint: source.endpoint().into(),
    })
}

pub(crate) fn parse(bytes: &[u8], limit: u32, origin: String) -> Result<RefreshBatch> {
    use serde::Deserializer;
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let (candidates, skipped) = decoder
        .deserialize_seq(PageVisitor(limit))
        .map_err(|_| Error::Acquisition("invalid, duplicate or oversized directory page"))?;
    decoder
        .end()
        .map_err(|_| Error::Acquisition("trailing directory JSON"))?;
    Ok(RefreshBatch {
        candidates,
        skipped,
        origin,
    })
}

struct PageVisitor(u32);
impl<'de> serde::de::Visitor<'de> for PageVisitor {
    type Value = (Vec<Candidate>, u32);
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded station array")
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut sequence: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        use serde::de::{Error, IgnoredAny};
        let mut candidates = Vec::new();
        let mut skipped = 0;
        let mut ids = HashSet::new();
        for _ in 0..self.0 {
            let Some(entry) = sequence.next_element::<&serde_json::value::RawValue>()? else {
                return Ok((candidates, skipped));
            };
            if let Some(candidate) = serde_json::from_str::<RawStation>(entry.get())
                .ok()
                .and_then(|raw| candidate(raw).ok())
            {
                if !ids.insert(candidate.station.id.clone()) {
                    return Err(A::Error::custom("duplicate station identity"));
                }
                candidates.push(candidate);
            } else {
                skipped += 1;
            }
        }
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::custom("station page limit"));
        }
        Ok((candidates, skipped))
    }
}
