//! Provider configuration. Routes and prices are stored; nothing is sent.

use serde::{Deserialize, Serialize};

use super::{Snapshot, snapshot};
use crate::{
    Error, Result,
    providers::{PriceDraft, PriceSpec, RouteDraft, RouteSpec},
    storage::{
        Store, now_ms,
        providers::{MAX_PAGE, PriceRecord, ProviderRoute},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderOperation {
    AddRoute { route: RouteSpec },
    ListRoutes { after: Option<String>, limit: u32 },
    ShowRoute { id: String },
    AddPrice { price: PriceSpec },
    ShowPrice { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPage {
    pub routes: Vec<ProviderRouteView>,
    pub prices: Vec<PriceSnapshotView>,
    pub next_after: Option<String>,
    pub newly_created: Option<bool>,
}

/// A route as shown to clients. `secret_env` is a variable name, never its value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRouteView {
    pub id: String,
    pub provider: String,
    pub endpoint_origin: String,
    pub model: String,
    pub upstream_providers: Vec<String>,
    pub allow_fallbacks: bool,
    pub task: String,
    pub secret_env: Option<String>,
    pub language_pairs: Vec<LanguagePairView>,
    pub dispatch_available: bool,
    pub created_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguagePairView {
    pub source: String,
    pub target: String,
    pub validation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceSnapshotView {
    pub id: String,
    pub route_id: String,
    pub retrieved_ms: i64,
    pub valid_until_ms: i64,
    pub fresh: bool,
    pub unit: String,
    pub rates: Vec<RateView>,
    pub source_note: String,
    pub created_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateView {
    pub dimension: String,
    pub usd: String,
}

impl From<ProviderRoute> for ProviderRouteView {
    fn from(record: ProviderRoute) -> Self {
        let route = record.route;
        Self {
            id: route.id,
            provider: route.kind.as_str().into(),
            endpoint_origin: route.origin,
            model: route.model,
            upstream_providers: route.upstreams,
            allow_fallbacks: false,
            task: route.task.as_str().into(),
            secret_env: route.secret_env,
            language_pairs: route
                .pairs
                .into_iter()
                .map(|pair| LanguagePairView {
                    source: pair.source,
                    target: pair.target,
                    validation: "unvalidated".into(),
                })
                .collect(),
            dispatch_available: false,
            created_ms: record.created_ms,
        }
    }
}

fn price_view(record: PriceRecord, now: i64) -> PriceSnapshotView {
    let price = record.price;
    PriceSnapshotView {
        fresh: price.fresh_at(now),
        rates: price
            .rates
            .values()
            .iter()
            .zip(crate::providers::Rates::NAMES)
            .map(|(rate, name)| RateView {
                dimension: name.into(),
                usd: rate.to_string(),
            })
            .collect(),
        id: price.id,
        route_id: price.route_id,
        retrieved_ms: price.retrieved_ms,
        valid_until_ms: price.valid_until_ms,
        unit: "usd_per_unit".into(),
        source_note: price.note,
        created_ms: record.created_ms,
    }
}

pub(super) fn apply(store: &mut Store, operation: ProviderOperation) -> Result<Snapshot> {
    let now = now_ms()?;
    let page = match operation {
        ProviderOperation::AddRoute { route } => {
            let draft = RouteDraft::from_spec(&route)?;
            let (record, created) = store.add_provider_route(&draft, now)?;
            ProviderPage {
                routes: vec![record.into()],
                prices: Vec::new(),
                next_after: None,
                newly_created: Some(created),
            }
        }
        ProviderOperation::ShowRoute { id } => {
            let record = store.provider_route(&id)?.ok_or(Error::NotFound)?;
            let prices = store
                .route_price_snapshots(&id)?
                .into_iter()
                .map(|price| price_view(price, now))
                .collect();
            ProviderPage {
                routes: vec![record.into()],
                prices,
                next_after: None,
                newly_created: None,
            }
        }
        ProviderOperation::ListRoutes { after, limit } => {
            let routes = store.provider_routes(after.as_deref(), limit.clamp(1, MAX_PAGE))?;
            let next_after = match routes.last() {
                Some(last) if !store.provider_routes(Some(&last.route.id), 1)?.is_empty() => {
                    Some(last.route.id.clone())
                }
                _ => None,
            };
            ProviderPage {
                routes: routes.into_iter().map(Into::into).collect(),
                prices: Vec::new(),
                next_after,
                newly_created: None,
            }
        }
        ProviderOperation::AddPrice { price } => {
            let draft = PriceDraft::from_spec(&price, now)?;
            let (record, created) = store.add_price_snapshot(&draft, now)?;
            ProviderPage {
                routes: Vec::new(),
                prices: vec![price_view(record, now)],
                next_after: None,
                newly_created: Some(created),
            }
        }
        ProviderOperation::ShowPrice { id } => {
            let record = store.price_snapshot(&id)?.ok_or(Error::NotFound)?;
            ProviderPage {
                routes: Vec::new(),
                prices: vec![price_view(record, now)],
                next_after: None,
                newly_created: None,
            }
        }
    };
    let mut view = snapshot(store)?;
    view.provider = Some(page);
    Ok(view)
}
