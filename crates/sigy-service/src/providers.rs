//! Provider routes and dated price evidence. Nothing in this module sends a
//! request, reads a credential, or enables dispatch.

use std::{net::IpAddr, str::FromStr};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sigy_core::pricing::{Dimension, PriceSnapshot, Rate};

use crate::{Error, Result, storage::validate_key};

#[cfg(test)]
pub(crate) mod dispatch;

/// Longest accepted price validity window.
const MAX_VALID_HOURS: u32 = 720;
/// Largest clock skew tolerated for a retrieval time in the future.
const MAX_FUTURE_SKEW_MS: i64 = 300_000;
const MAX_UPSTREAMS: usize = 16;
const MAX_PAIRS: usize = 16;

/// One route as requested by a client. Validated into a [`RouteDraft`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSpec {
    pub id: String,
    pub provider: String,
    pub endpoint_origin: String,
    pub model: String,
    #[serde(default)]
    pub upstream_providers: Vec<String>,
    pub task: String,
    /// Name of an environment variable. The value is never stored or read here.
    #[serde(default)]
    pub secret_env: Option<String>,
    /// `source:target` BCP 47 pairs, for example `es:en`.
    pub language_pairs: Vec<String>,
}

/// One dated price snapshot as requested by a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceSpec {
    pub id: String,
    pub route_id: String,
    /// RFC 3339 instant at which the prices were read from their source.
    pub retrieved: String,
    pub valid_hours: u32,
    /// Exact USD per unit for each dimension name. Prompt and completion are required.
    pub rates: Vec<RateSpec>,
    pub source_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateSpec {
    pub dimension: String,
    pub usd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderKind {
    OpenRouter,
    Ollama,
}

impl ProviderKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Ollama => "ollama",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "openrouter" => Ok(Self::OpenRouter),
            "ollama" => Ok(Self::Ollama),
            _ => Err(Error::InvalidInput("provider kind")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    TranslateText,
}

impl Task {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TranslateText => "translate-text",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "translate-text" => Ok(Self::TranslateText),
            _ => Err(Error::InvalidInput("provider task")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanguagePair {
    pub(crate) source: String,
    pub(crate) target: String,
}

/// A validated immutable route. Fallback is always off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteDraft {
    pub(crate) id: String,
    pub(crate) kind: ProviderKind,
    pub(crate) origin: String,
    pub(crate) model: String,
    pub(crate) upstreams: Vec<String>,
    pub(crate) task: Task,
    pub(crate) secret_env: Option<String>,
    pub(crate) pairs: Vec<LanguagePair>,
}

impl RouteDraft {
    pub(crate) fn from_spec(spec: &RouteSpec) -> Result<Self> {
        validate_key(&spec.id, "provider route ID")?;
        let kind = ProviderKind::parse(&spec.provider)?;
        let origin = canonical_origin(kind, &spec.endpoint_origin)?;
        validate_model(&spec.model)?;
        let mut upstreams = spec.upstream_providers.clone();
        for upstream in &upstreams {
            validate_upstream(upstream)?;
        }
        upstreams.sort();
        upstreams.dedup();
        if upstreams.len() != spec.upstream_providers.len() || upstreams.len() > MAX_UPSTREAMS {
            return Err(Error::InvalidInput("upstream provider list"));
        }
        let secret_env = spec.secret_env.clone();
        if let Some(name) = &secret_env {
            validate_secret_name(name)?;
        }
        match kind {
            ProviderKind::OpenRouter if upstreams.is_empty() => {
                return Err(Error::InvalidInput(
                    "a hosted route needs an explicit upstream provider list",
                ));
            }
            ProviderKind::OpenRouter if secret_env.is_none() => {
                return Err(Error::InvalidInput(
                    "a hosted route needs a secret environment variable name",
                ));
            }
            ProviderKind::Ollama if !upstreams.is_empty() || secret_env.is_some() => {
                return Err(Error::InvalidInput(
                    "a local route has no upstream providers or secret",
                ));
            }
            _ => (),
        }
        Ok(Self {
            id: spec.id.clone(),
            kind,
            origin,
            model: spec.model.clone(),
            upstreams,
            task: Task::parse(&spec.task)?,
            secret_env,
            pairs: parse_pairs(&spec.language_pairs)?,
        })
    }
}

/// Canonical per-unit rates for every dimension a snapshot records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rates {
    pub(crate) snapshot: PriceSnapshot,
    pub(crate) unrecognized: Rate,
}

impl Rates {
    /// Column and dimension names in storage order.
    pub(crate) const NAMES: [&'static str; 10] = [
        "prompt",
        "completion",
        "request",
        "internal_reasoning",
        "input_cache_read",
        "input_cache_write",
        "image",
        "audio",
        "web_search",
        "unrecognized",
    ];

    pub(crate) fn from_named(values: &[(String, String)]) -> Result<Self> {
        let mut rates = [None; 10];
        for (name, text) in values {
            let index = Self::NAMES
                .iter()
                .position(|known| known == name)
                .ok_or(Error::InvalidInput("price dimension"))?;
            let slot = rates.get_mut(index).ok_or(Error::StorageIntegrity)?;
            if slot.replace(Rate::parse(text)?).is_some() {
                return Err(Error::InvalidInput("duplicate price dimension"));
            }
        }
        let [prompt, completion, rest @ ..] = rates;
        let (Some(prompt), Some(completion)) = (prompt, completion) else {
            return Err(Error::InvalidInput(
                "prompt and completion rates are required",
            ));
        };
        let [
            request,
            reasoning,
            cache_read,
            cache_write,
            image,
            audio,
            search,
            unknown,
        ] = rest.map(Option::unwrap_or_default);
        Ok(Self {
            snapshot: PriceSnapshot::new(prompt, completion)
                .with(Dimension::Request, request)
                .with(Dimension::InternalReasoning, reasoning)
                .with(Dimension::InputCacheRead, cache_read)
                .with(Dimension::InputCacheWrite, cache_write)
                .with(Dimension::Image, image)
                .with(Dimension::Audio, audio)
                .with(Dimension::WebSearch, search)
                .with_unrecognized_charge(unknown),
            unrecognized: unknown,
        })
    }

    pub(crate) fn values(&self) -> [Rate; 10] {
        let rate = |dimension| self.snapshot.rate(dimension);
        [
            rate(Dimension::Prompt),
            rate(Dimension::Completion),
            rate(Dimension::Request),
            rate(Dimension::InternalReasoning),
            rate(Dimension::InputCacheRead),
            rate(Dimension::InputCacheWrite),
            rate(Dimension::Image),
            rate(Dimension::Audio),
            rate(Dimension::WebSearch),
            self.unrecognized,
        ]
    }
}

/// A validated immutable price snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PriceDraft {
    pub(crate) id: String,
    pub(crate) route_id: String,
    pub(crate) retrieved_ms: i64,
    pub(crate) valid_until_ms: i64,
    pub(crate) rates: Rates,
    pub(crate) note: String,
}

impl PriceDraft {
    pub(crate) fn from_spec(spec: &PriceSpec, now_ms: i64) -> Result<Self> {
        validate_key(&spec.id, "price snapshot ID")?;
        validate_key(&spec.route_id, "provider route ID")?;
        let retrieved_ms = jiff::Timestamp::from_str(&spec.retrieved)
            .map_err(|_| Error::InvalidInput("price retrieval time"))?
            .as_millisecond();
        if retrieved_ms < 0 || retrieved_ms > now_ms.saturating_add(MAX_FUTURE_SKEW_MS) {
            return Err(Error::InvalidInput("price retrieval time"));
        }
        if !(1..=MAX_VALID_HOURS).contains(&spec.valid_hours) {
            return Err(Error::InvalidInput("price validity hours"));
        }
        let valid_until_ms = retrieved_ms
            .checked_add(i64::from(spec.valid_hours) * 3_600_000)
            .ok_or(Error::InvalidInput("price validity hours"))?;
        if spec.rates.len() > Rates::NAMES.len() {
            return Err(Error::InvalidInput("price dimension count"));
        }
        let named: Vec<(String, String)> = spec
            .rates
            .iter()
            .map(|rate| (rate.dimension.clone(), rate.usd.clone()))
            .collect();
        validate_note(&spec.source_note)?;
        Ok(Self {
            id: spec.id.clone(),
            route_id: spec.route_id.clone(),
            retrieved_ms,
            valid_until_ms,
            rates: Rates::from_named(&named)?,
            note: spec.source_note.clone(),
        })
    }

    /// A snapshot is usable only inside its window on a clock that did not run backward.
    pub(crate) const fn fresh_at(&self, now_ms: i64) -> bool {
        now_ms >= self.retrieved_ms && now_ms < self.valid_until_ms
    }
}

fn canonical_origin(kind: ProviderKind, text: &str) -> Result<String> {
    if text.is_empty()
        || text.len() > 256
        || text
            .bytes()
            .any(|byte| !byte.is_ascii_graphic() || byte == b'\\')
    {
        return Err(Error::InvalidInput("provider endpoint origin"));
    }
    let url = Url::parse(text).map_err(|_| Error::InvalidInput("provider endpoint origin"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.port_or_known_default() == Some(0)
    {
        return Err(Error::InvalidInput(
            "provider endpoint must be an origin without path, query, or credentials",
        ));
    }
    let host = url
        .host_str()
        .ok_or(Error::InvalidInput("provider endpoint origin"))?;
    let permitted = match kind {
        ProviderKind::OpenRouter => url.scheme() == "https",
        ProviderKind::Ollama => url.scheme() == "http" && is_loopback(host),
    };
    if !permitted {
        return Err(Error::InvalidInput(match kind {
            ProviderKind::OpenRouter => "a hosted route requires an https origin",
            ProviderKind::Ollama => "a local route requires an http loopback origin",
        }));
    }
    Ok(url.origin().ascii_serialization())
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn validate_model(model: &str) -> Result<()> {
    let first = model
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric());
    if !first
        || model.len() > 128
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'))
    {
        return Err(Error::InvalidInput("provider model ID"));
    }
    Ok(())
}

fn validate_upstream(slug: &str) -> Result<()> {
    let first = slug
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if !first
        || slug.len() > 64
        || !slug.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-' | b'/')
        })
    {
        return Err(Error::InvalidInput("upstream provider slug"));
    }
    Ok(())
}

/// An environment variable name: ASCII letters, digits and underscores, not
/// starting with a digit. Typical key text contains hyphens and is refused.
pub(crate) fn validate_secret_name(name: &str) -> Result<()> {
    let first = name
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    if !first || name.len() > 128 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(Error::InvalidInput("secret environment variable name"));
    }
    Ok(())
}

fn validate_note(note: &str) -> Result<()> {
    if note.is_empty()
        || note.len() > 256
        || note.trim() != note
        || !note.bytes().all(|b| (b' '..=b'~').contains(&b))
    {
        return Err(Error::InvalidInput("price source note"));
    }
    Ok(())
}

fn parse_pairs(values: &[String]) -> Result<Vec<LanguagePair>> {
    if values.is_empty() || values.len() > MAX_PAIRS {
        return Err(Error::InvalidInput("language pair count"));
    }
    let mut pairs: Vec<LanguagePair> = Vec::with_capacity(values.len());
    for value in values {
        let (source, target) = value
            .split_once(':')
            .ok_or(Error::InvalidInput("language pair"))?;
        let pair = LanguagePair {
            source: crate::languages::normalize_tag(source)?,
            target: crate::languages::normalize_tag(target)?,
        };
        if pair.source == pair.target || pairs.contains(&pair) {
            return Err(Error::InvalidInput("language pair"));
        }
        pairs.push(pair);
    }
    pairs.sort_by(|a, b| (&a.source, &a.target).cmp(&(&b.source, &b.target)));
    Ok(pairs)
}

#[cfg(test)]
mod tests;
