//! Worst-case route pricing from a hashed `OpenRouter` endpoint snapshot.
//!
//! Every listed pricing key must be recognized. Prompt-class keys (text,
//! audio, cache reads and writes) are bounded by one prompt-token bound at the
//! highest listed rate; completion-class keys (visible output, reasoning, audio
//! or image output) by one completion-token bound at the highest rate. Every
//! endpoint matched by the route's provider slugs contributes, including
//! service-tier variants, and every override that could apply is included.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Result, money};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rates {
    pub prompt: u128,
    pub completion: u128,
    pub request: u128,
    pub image: u128,
    pub web_search: u128,
}

impl Rates {
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            prompt: self.prompt.max(other.prompt),
            completion: self.completion.max(other.completion),
            request: self.request.max(other.request),
            image: self.image.max(other.image),
            web_search: self.web_search.max(other.web_search),
        }
    }

    pub fn with_headroom(self, percent: u32) -> Result<Self> {
        Ok(Self {
            prompt: money::with_headroom(self.prompt, percent)?,
            completion: money::with_headroom(self.completion, percent)?,
            request: money::with_headroom(self.request, percent)?,
            image: money::with_headroom(self.image, percent)?,
            web_search: money::with_headroom(self.web_search, percent)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub images: u64,
    pub web_searches: u64,
}

/// Exact attodollar liability of one request under `rates` and `bounds`.
pub fn liability(rates: Rates, bounds: Bounds) -> Result<u128> {
    let parts = [
        (rates.request, 1),
        (rates.image, bounds.images),
        (rates.web_search, bounds.web_searches),
        (rates.prompt, bounds.prompt_tokens),
        (rates.completion, bounds.completion_tokens),
    ];
    let mut total = 0_u128;
    for (rate, units) in parts {
        total = rate
            .checked_mul(u128::from(units))
            .and_then(|part| total.checked_add(part))
            .ok_or("liability overflow")?;
    }
    Ok(total)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Prompt,
    Completion,
    Request,
    Image,
    WebSearch,
}

fn class(key: &str) -> Option<Class> {
    match key {
        "prompt"
        | "audio"
        | "input_audio_cache"
        | "input_cache_read"
        | "input_cache_write"
        | "input_cache_write_1h" => Some(Class::Prompt),
        "completion" | "internal_reasoning" | "audio_output" | "image_output" => {
            Some(Class::Completion)
        }
        "request" => Some(Class::Request),
        "image" => Some(Class::Image),
        "web_search" => Some(Class::WebSearch),
        _ => None,
    }
}

fn bounded_key(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_graphic)
        .take(48)
        .collect()
}

fn rates_of(keys: &BTreeMap<String, u128>) -> Rates {
    let mut rates = Rates::default();
    for (key, rate) in keys {
        let slot = match class(key) {
            Some(Class::Prompt) => &mut rates.prompt,
            Some(Class::Completion) => &mut rates.completion,
            Some(Class::Request) => &mut rates.request,
            Some(Class::Image) => &mut rates.image,
            Some(Class::WebSearch) => &mut rates.web_search,
            None => continue,
        };
        *slot = (*slot).max(*rate);
    }
    rates
}

fn discount(value: &Value) -> Result<()> {
    let valid = value
        .as_f64()
        .is_some_and(|number| (0.0..=1.0).contains(&number));
    if valid {
        Ok(())
    } else {
        Err("pricing discount must be a number between zero and one".into())
    }
}

#[derive(Debug)]
struct Override {
    min_prompt_tokens: Option<u64>,
    keys: BTreeMap<String, u128>,
}

fn parse_override(entry: &Value) -> Result<Override> {
    let object = entry
        .as_object()
        .ok_or("pricing override must be an object")?;
    let mut parsed = Override {
        min_prompt_tokens: None,
        keys: BTreeMap::new(),
    };
    for (key, value) in object {
        match key.as_str() {
            "min_prompt_tokens" => {
                parsed.min_prompt_tokens = Some(
                    value
                        .as_u64()
                        .ok_or("override threshold must be an integer")?,
                );
            }
            "utc_start" | "utc_end" => {
                value.as_u64().ok_or("override clock must be an integer")?;
            }
            "utc_days" => {
                value.as_array().ok_or("override days must be an array")?;
            }
            "discount" => discount(value)?,
            _ if class(key).is_some() => {
                let text = value.as_str().ok_or("override rate must be text")?;
                parsed.keys.insert(key.clone(), money::catalog_rate(text)?);
            }
            _ => {
                return Err(
                    format!("unrecognized pricing override key {}", bounded_key(key)).into(),
                );
            }
        }
    }
    Ok(parsed)
}

#[derive(Debug)]
pub struct Endpoint {
    pub tag: String,
    pub provider: String,
    pub context_length: u64,
    pub max_completion_tokens: u64,
    keys: BTreeMap<String, u128>,
    overrides: Vec<Override>,
}

fn parse_pricing(pricing: &Map<String, Value>) -> Result<(BTreeMap<String, u128>, Vec<Override>)> {
    let mut keys = BTreeMap::new();
    let mut overrides = Vec::new();
    for (key, value) in pricing {
        match key.as_str() {
            "discount" => discount(value)?,
            "overrides" => {
                for entry in value
                    .as_array()
                    .ok_or("pricing overrides must be an array")?
                {
                    overrides.push(parse_override(entry)?);
                }
            }
            _ if class(key).is_some() => {
                let text = value.as_str().ok_or("pricing rate must be text")?;
                keys.insert(key.clone(), money::catalog_rate(text)?);
            }
            _ => {
                return Err(format!("unrecognized pricing key {}", bounded_key(key)).into());
            }
        }
    }
    if !keys.contains_key("prompt") || !keys.contains_key("completion") {
        return Err("pricing must list prompt and completion rates".into());
    }
    Ok((keys, overrides))
}

impl Endpoint {
    fn parse(value: &Value) -> Result<Self> {
        let text = |field: &str| -> Result<String> {
            let found = value
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("endpoint {field} missing"))?;
            if found.is_empty() || found.len() > 128 || found.chars().any(char::is_control) {
                return Err(format!("endpoint {field} invalid").into());
            }
            Ok(found.to_owned())
        };
        let context_length = value
            .get("context_length")
            .and_then(Value::as_u64)
            .filter(|length| *length > 0)
            .ok_or("endpoint context length missing")?;
        // An endpoint without an output ceiling is bounded by its context.
        let max_completion_tokens = match value.get("max_completion_tokens") {
            Some(Value::Null) | None => context_length,
            Some(limit) => limit
                .as_u64()
                .filter(|limit| *limit > 0)
                .ok_or("endpoint output ceiling invalid")?,
        };
        let pricing = value
            .get("pricing")
            .and_then(Value::as_object)
            .ok_or("endpoint pricing missing")?;
        let (keys, overrides) = parse_pricing(pricing)?;
        Ok(Self {
            tag: text("tag")?,
            provider: text("provider_name")?,
            context_length,
            max_completion_tokens,
            keys,
            overrides,
        })
    }

    fn worst(&self, prompt_bound: u64) -> Rates {
        let mut worst = rates_of(&self.keys);
        for entry in &self.overrides {
            // An override with a prompt threshold applies only strictly above it.
            if entry
                .min_prompt_tokens
                .is_some_and(|threshold| prompt_bound <= threshold)
            {
                continue;
            }
            let mut merged = self.keys.clone();
            merged.extend(entry.keys.iter().map(|(key, rate)| (key.clone(), *rate)));
            worst = worst.max(rates_of(&merged));
        }
        worst
    }

    fn listed(&self, key: &str) -> u128 {
        self.keys.get(key).copied().unwrap_or(0)
    }
}

fn slug_matches(tag: &str, slug: &str) -> bool {
    tag == slug
        || tag
            .strip_prefix(slug)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The endpoints a route may reach, parsed from one endpoint snapshot.
#[derive(Debug)]
pub struct RoutePrices {
    pub endpoints: Vec<Endpoint>,
}

impl RoutePrices {
    pub fn parse(snapshot: &[u8], model: &str, only: &[String]) -> Result<Self> {
        let document: Value = serde_json::from_slice(snapshot)?;
        let data = document
            .get("data")
            .ok_or("endpoint snapshot has no data")?;
        if data.get("id").and_then(Value::as_str) != Some(model) {
            return Err("endpoint snapshot is for a different model".into());
        }
        let listed = data
            .get("endpoints")
            .and_then(Value::as_array)
            .ok_or("endpoint snapshot has no endpoints")?;
        if only.is_empty() {
            return Err("route must name at least one provider slug".into());
        }
        let mut endpoints = Vec::new();
        for value in listed {
            let tag = value.get("tag").and_then(Value::as_str).unwrap_or_default();
            if only.iter().any(|slug| slug_matches(tag, slug)) {
                endpoints.push(Endpoint::parse(value)?);
            }
        }
        if endpoints.is_empty() {
            return Err("no snapshot endpoint matches the route provider slugs".into());
        }
        Ok(Self { endpoints })
    }

    #[must_use]
    pub fn worst(&self, prompt_bound: u64) -> Rates {
        self.endpoints
            .iter()
            .fold(Rates::default(), |worst, endpoint| {
                worst.max(endpoint.worst(prompt_bound))
            })
    }

    /// Highest listed base prompt and completion rates, for routing `max_price`.
    #[must_use]
    pub fn listed_max(&self) -> (u128, u128) {
        self.endpoints
            .iter()
            .fold((0, 0), |(prompt, completion), endpoint| {
                (
                    prompt.max(endpoint.listed("prompt")),
                    completion.max(endpoint.listed("completion")),
                )
            })
    }

    #[must_use]
    pub fn context_length(&self) -> u64 {
        self.endpoints
            .iter()
            .map(|endpoint| endpoint.context_length)
            .max()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn max_completion_tokens(&self) -> u64 {
        self.endpoints
            .iter()
            .map(|endpoint| endpoint.max_completion_tokens)
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(endpoints: &Value) -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"data": {"id": "vendor/model", "endpoints": endpoints}}),
        )
        .unwrap_or_default()
    }

    fn endpoint(tag: &str, pricing: &Value) -> Value {
        serde_json::json!({"tag": tag, "provider_name": "Fixture", "context_length": 1000, "max_completion_tokens": 100, "pricing": pricing})
    }

    #[test]
    fn worst_case_covers_classes_tiers_and_applicable_overrides() -> Result<()> {
        let bytes = snapshot(&serde_json::json!([
            endpoint(
                "alpha",
                &serde_json::json!({"prompt": "0.000002", "completion": "0.00001", "audio": "0.000003", "input_cache_write_1h": "0.000004", "internal_reasoning": "0.00002", "web_search": "0.01", "discount": 0.5,
                "overrides": [{"min_prompt_tokens": 500, "prompt": "0.000009"}, {"utc_start": 30, "utc_end": 1630, "utc_days": ["monday"], "completion": "0.00003"}]})
            ),
            endpoint(
                "alpha/priority",
                &serde_json::json!({"prompt": "0.000005", "completion": "0.000001", "request": "0.001", "image": "0.002"})
            ),
            endpoint(
                "beta",
                &serde_json::json!({"prompt": "1", "completion": "1"})
            ),
        ]));
        let route = RoutePrices::parse(&bytes, "vendor/model", &["alpha".into()])?;
        assert_eq!(route.endpoints.len(), 2);
        let below = route.worst(500);
        assert_eq!(below.prompt, 5_000_000_000_000);
        assert_eq!(below.completion, 30_000_000_000_000);
        assert_eq!(below.request, 1_000_000_000_000_000);
        assert_eq!(below.image, 2_000_000_000_000_000);
        assert_eq!(below.web_search, 10_000_000_000_000_000);
        assert_eq!(route.worst(501).prompt, 9_000_000_000_000);
        assert_eq!(route.listed_max(), (5_000_000_000_000, 10_000_000_000_000));
        assert_eq!(route.context_length(), 1000);
        assert_eq!(route.max_completion_tokens(), 100);
        let exact = RoutePrices::parse(&bytes, "vendor/model", &["alpha/priority".into()])?;
        assert_eq!(exact.endpoints.len(), 1);
        Ok(())
    }

    #[test]
    fn liability_is_exact_and_checked() -> Result<()> {
        let rates = Rates {
            prompt: 2,
            completion: 10,
            request: 7,
            image: 3,
            web_search: 5,
        };
        let bounds = Bounds {
            prompt_tokens: 100,
            completion_tokens: 10,
            images: 1,
            web_searches: 2,
        };
        assert_eq!(liability(rates, bounds)?, 7 + 3 + 10 + 200 + 100);
        assert_eq!(rates.with_headroom(100)?.prompt, 4);
        let huge = Rates {
            prompt: u128::MAX,
            ..rates
        };
        assert!(liability(huge, bounds).is_err());
        assert!(huge.with_headroom(5).is_err());
        Ok(())
    }

    #[test]
    fn unknown_negative_or_malformed_pricing_makes_a_route_ineligible() {
        let cases = [
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "video": "0.1"}),
            serde_json::json!({"prompt": "-1", "completion": "-1"}),
            serde_json::json!({"prompt": "0.1"}),
            serde_json::json!({"prompt": 0.1, "completion": "0.1"}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "discount": 2}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": {}}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": [1]}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": [{"peak_hours": true}]}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": [{"min_prompt_tokens": "1"}]}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": [{"utc_start": "x"}]}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": [{"utc_days": "monday"}]}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": [{"prompt": 1}]}),
            serde_json::json!({"prompt": "0.1", "completion": "0.1", "overrides": [{"discount": "x"}]}),
        ];
        for pricing in cases {
            let bytes = snapshot(&serde_json::json!([endpoint("alpha", &pricing)]));
            assert!(
                RoutePrices::parse(&bytes, "vendor/model", &["alpha".into()]).is_err(),
                "{pricing}"
            );
        }
    }

    #[test]
    fn snapshot_identity_and_endpoint_fields_are_checked() {
        let good = serde_json::json!({"prompt": "0.1", "completion": "0.1"});
        let bytes = snapshot(&serde_json::json!([endpoint("alpha", &good)]));
        assert!(RoutePrices::parse(&bytes, "other/model", &["alpha".into()]).is_err());
        assert!(RoutePrices::parse(&bytes, "vendor/model", &[]).is_err());
        assert!(RoutePrices::parse(&bytes, "vendor/model", &["alp".into()]).is_err());
        assert!(RoutePrices::parse(b"[]", "vendor/model", &["alpha".into()]).is_err());
        assert!(
            RoutePrices::parse(
                b"{\"data\":{\"id\":\"vendor/model\"}}",
                "vendor/model",
                &["alpha".into()]
            )
            .is_err()
        );
        let mut unbounded = endpoint("alpha", &good);
        unbounded["max_completion_tokens"] = Value::Null;
        let parsed = RoutePrices::parse(
            &snapshot(&serde_json::json!([unbounded])),
            "vendor/model",
            &["alpha".into()],
        );
        assert_eq!(
            parsed.map(|route| route.max_completion_tokens()).ok(),
            Some(1000)
        );
        for (field, value) in [
            ("max_completion_tokens", serde_json::json!(0)),
            ("context_length", serde_json::json!(0)),
            ("provider_name", serde_json::json!("")),
            ("provider_name", serde_json::json!("bad\nname")),
            ("pricing", serde_json::json!("free")),
        ] {
            let mut broken = endpoint("alpha", &good);
            broken[field] = value;
            assert!(
                RoutePrices::parse(
                    &snapshot(&serde_json::json!([broken])),
                    "vendor/model",
                    &["alpha".into()]
                )
                .is_err(),
                "{field}"
            );
        }
    }
}
