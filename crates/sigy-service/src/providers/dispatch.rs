//! Paid dispatch core proven against a fake transport. There is no HTTP client
//! and no production call path; this module is compiled only for tests.
//!
//! Order of effects for one attempt: validate the route and a fresh price
//! snapshot, compute the worst-case liability from proven unit bounds, reserve
//! it atomically with the attempt contract, persist `submitted`, and only then
//! hand the attempt to the transport. A reported cost settles (rounded up); any
//! other result keeps the full reservation as an uncertain liability.

use sha2::{Digest, Sha256};
use sigy_core::{
    money::Usd,
    pricing::{Dimension, Liability, ReportedCost, UnitBounds},
};

use super::{ProviderKind, Task};
use crate::{
    Error, Result,
    languages::normalize_tag,
    storage::{
        Store,
        ledger::{Change, RequestState},
        providers::attempts::{AttemptOutcome, AttemptRow},
        validate_key,
    },
};

/// Largest prompt text accepted for one attempt.
pub(crate) const MAX_INPUT_BYTES: usize = 32 * 1024;
/// Tokens added for the chat template around the text. Assumes every tokenizer
/// token covers at least one UTF-8 byte of content, which is a stated limitation.
pub(crate) const TEMPLATE_OVERHEAD_TOKENS: u32 = 256;
pub(crate) const MAX_COMPLETION_TOKENS: u32 = 32_768;
const MAX_RETRY_AFTER_SECONDS: u32 = 86_400;

/// What the transport receives. It names the secret variable; it never carries a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attempt {
    pub(crate) request_id: String,
    pub(crate) origin: String,
    pub(crate) model: String,
    pub(crate) secret_env: String,
    pub(crate) upstream_only: Vec<String>,
    pub(crate) allow_fallbacks: bool,
    pub(crate) max_prompt_usd_per_million: String,
    pub(crate) max_completion_usd_per_million: String,
    pub(crate) max_completion_tokens: u32,
    pub(crate) source_language: String,
    pub(crate) target_language: String,
    pub(crate) text: String,
}

/// A transport result. Costs are raw JSON number text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Sent {
    Completed {
        generation_id: Option<String>,
        cost: Option<String>,
    },
    TimedOut {
        generation_id: Option<String>,
    },
    /// HTTP 402. Without `Retry-After` the refusal is terminal.
    PaymentRequired {
        retry_after_seconds: Option<u32>,
    },
    Failed {
        generation_id: Option<String>,
    },
    /// Simulates the process dying after the request left: nothing more is recorded.
    ProcessLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Lookup {
    NotFound,
    Found { cost: String },
    Failed,
}

pub(crate) trait Transport {
    fn send(&mut self, attempt: &Attempt) -> Sent;
    fn lookup(&mut self, generation_id: &str) -> Lookup;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    Requested,
    /// Local work is behind. This never opens a paid route.
    LocalOverload,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DispatchRequest<'a> {
    pub(crate) id: &'a str,
    pub(crate) route_id: &'a str,
    pub(crate) snapshot_id: &'a str,
    pub(crate) task: Task,
    pub(crate) source_language: &'a str,
    pub(crate) target_language: &'a str,
    pub(crate) text: &'a str,
    pub(crate) max_completion_tokens: u32,
    pub(crate) budgets: &'a [&'a str],
    pub(crate) retry_of: Option<&'a str>,
    pub(crate) trigger: Trigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    UsageMissing,
    NoGenerationId,
    NotFoundYet,
    LookupFailed,
    TimedOut,
    Refused,
    RetryAfter { not_before_ms: i64 },
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dispatched {
    Settled {
        actual: Usd,
        breach: bool,
    },
    /// The full reservation stays as an uncertain liability.
    Pending(Pending),
    /// The request ID already exists. The transport was not called.
    Replayed(RequestState),
    ProcessLost,
}

/// An admitted attempt that has not yet been handed to the transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Admitted {
    pub(crate) attempt: Attempt,
    pub(crate) maximum: Usd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Admit {
    New(Box<Admitted>),
    Replayed(RequestState),
}

/// Admits and submits one attempt.
/// # Errors
/// Refuses overload, invalid or stale configuration, unbounded prices, and budget refusals
/// before any reservation; propagates catalog errors.
pub(crate) fn dispatch(
    store: &mut Store,
    transport: &mut impl Transport,
    request: &DispatchRequest<'_>,
    now_ms: i64,
) -> Result<Dispatched> {
    match admit(store, request, now_ms)? {
        Admit::Replayed(state) => Ok(Dispatched::Replayed(state)),
        Admit::New(admitted) => submit(store, transport, &admitted, now_ms),
    }
}

/// Validates and reserves. A replay of an existing ID is reported, never re-admitted.
/// # Errors
/// See [`dispatch`].
pub(crate) fn admit(
    store: &mut Store,
    request: &DispatchRequest<'_>,
    now_ms: i64,
) -> Result<Admit> {
    if request.trigger == Trigger::LocalOverload {
        return Err(Error::ProviderRefused(
            "local overload never opens a paid route",
        ));
    }
    let row = contract(request)?;
    if let Some(existing) = store.reservation(request.id)? {
        return if store.provider_attempt(request.id)?.map(|record| record.row) == Some(row) {
            Ok(Admit::Replayed(existing.state))
        } else {
            Err(Error::IdempotencyConflict)
        };
    }
    let route = store
        .provider_route(request.route_id)?
        .ok_or(Error::NotFound)?
        .route;
    if route.kind != ProviderKind::OpenRouter {
        return Err(Error::ProviderRefused("a local route is not a paid route"));
    }
    if route.task != request.task
        || !route
            .pairs
            .iter()
            .any(|pair| pair.source == row.source_language && pair.target == row.target_language)
    {
        return Err(Error::ProviderRefused(
            "the route does not authorize this task and language pair",
        ));
    }
    let price = store
        .price_snapshot(request.snapshot_id)?
        .ok_or(Error::NotFound)?
        .price;
    if price.route_id != route.id {
        return Err(Error::ProviderRefused(
            "the price snapshot belongs to another route",
        ));
    }
    if !price.fresh_at(now_ms) {
        return Err(Error::ProviderRefused(
            "the price snapshot is stale or the clock ran backward",
        ));
    }
    check_retry(store, &row, now_ms)?;
    let bounds = UnitBounds::new(
        u64::from(row.prompt_tokens),
        u64::from(row.completion_tokens),
    )?;
    let liability = Liability::worst_case(&price.rates.snapshot, bounds)?;
    let secret_env = route
        .secret_env
        .clone()
        .ok_or(Error::ProviderRefused("the route has no secret reference"))?;
    let attempt = Attempt {
        request_id: row.request_id.clone(),
        origin: route.origin,
        model: route.model,
        secret_env,
        upstream_only: route.upstreams,
        allow_fallbacks: false,
        max_prompt_usd_per_million: price
            .rates
            .snapshot
            .rate(Dimension::Prompt)
            .per_million_decimal()?,
        max_completion_usd_per_million: price
            .rates
            .snapshot
            .rate(Dimension::Completion)
            .per_million_decimal()?,
        max_completion_tokens: row.completion_tokens,
        source_language: row.source_language.clone(),
        target_language: row.target_language.clone(),
        text: request.text.to_owned(),
    };
    let context = context(&row)?;
    let admission = store.admit_provider_attempt(
        &row,
        &context,
        liability.reserve(),
        request.budgets,
        now_ms,
    )?;
    if !admission.newly_reserved {
        return Ok(Admit::Replayed(admission.reservation.state));
    }
    Ok(Admit::New(Box::new(Admitted {
        attempt,
        maximum: liability.reserve(),
    })))
}

/// Persists `submitted`, then calls the transport exactly once.
/// # Errors
/// Refuses a released or otherwise non-reserved request without calling the transport.
pub(crate) fn submit(
    store: &mut Store,
    transport: &mut impl Transport,
    admitted: &Admitted,
    now_ms: i64,
) -> Result<Dispatched> {
    let id = admitted.attempt.request_id.as_str();
    if store.mark_submitted(id)? == Change::Unchanged {
        return Ok(Dispatched::Replayed(RequestState::Submitted));
    }
    let sent = transport.send(&admitted.attempt);
    record(store, transport, id, sent, now_ms)
}

/// Reconciles an uncertain attempt by its generation ID. A settled attempt
/// returns its recorded charge without a lookup.
/// # Errors
/// Refuses a request that is not uncertain or settled.
pub(crate) fn reconcile(
    store: &mut Store,
    transport: &mut impl Transport,
    id: &str,
) -> Result<Dispatched> {
    let reservation = store.reservation(id)?.ok_or(Error::NotFound)?;
    match reservation.state {
        RequestState::Settled => {
            let actual = reservation.actual.ok_or(Error::LedgerIntegrity)?;
            return Ok(Dispatched::Settled {
                actual,
                breach: actual > reservation.maximum,
            });
        }
        RequestState::Uncertain => (),
        _ => return Err(Error::RequestState),
    }
    let attempt = store.provider_attempt(id)?.ok_or(Error::NotFound)?;
    let Some(generation) = attempt.generation_id else {
        return Ok(Dispatched::Pending(Pending::NoGenerationId));
    };
    Ok(match transport.lookup(&generation) {
        Lookup::NotFound => Dispatched::Pending(Pending::NotFoundYet),
        Lookup::Failed => Dispatched::Pending(Pending::LookupFailed),
        Lookup::Found { cost } => match ReportedCost::parse(&cost) {
            Ok(cost) => settle(store, id, cost.usd(), &generation)?,
            Err(_) => Dispatched::Pending(Pending::UsageMissing),
        },
    })
}

fn record(
    store: &mut Store,
    transport: &mut impl Transport,
    id: &str,
    sent: Sent,
    now_ms: i64,
) -> Result<Dispatched> {
    let (outcome, generation, retry_after_ms, pending) = match sent {
        Sent::ProcessLost => return Ok(Dispatched::ProcessLost),
        Sent::Completed {
            generation_id,
            cost,
        } => {
            let generation = usable(generation_id);
            let cost = cost.and_then(|text| ReportedCost::parse(&text).ok());
            let outcome = if cost.is_some() {
                AttemptOutcome::Completed
            } else {
                AttemptOutcome::UsageMissing
            };
            store.record_attempt_outcome(id, outcome, generation.as_deref(), None, now_ms)?;
            return match (generation, cost) {
                (Some(generation), Some(cost)) => settle(store, id, cost.usd(), &generation),
                (Some(_), None) => {
                    store.mark_uncertain(id)?;
                    reconcile(store, transport, id)
                }
                (None, _) => {
                    store.mark_uncertain(id)?;
                    Ok(Dispatched::Pending(Pending::NoGenerationId))
                }
            };
        }
        Sent::TimedOut { generation_id } => (
            AttemptOutcome::Timeout,
            usable(generation_id),
            None,
            Pending::TimedOut,
        ),
        Sent::PaymentRequired {
            retry_after_seconds: Some(seconds),
        } if seconds <= MAX_RETRY_AFTER_SECONDS => {
            let wait = i64::from(seconds) * 1000;
            (
                AttemptOutcome::RetryAfter,
                None,
                Some(wait),
                Pending::RetryAfter {
                    not_before_ms: now_ms.saturating_add(wait),
                },
            )
        }
        Sent::PaymentRequired { .. } => (AttemptOutcome::Refused, None, None, Pending::Refused),
        Sent::Failed { generation_id } => (
            AttemptOutcome::Failed,
            usable(generation_id),
            None,
            Pending::Failed,
        ),
    };
    store.record_attempt_outcome(id, outcome, generation.as_deref(), retry_after_ms, now_ms)?;
    store.mark_uncertain(id)?;
    Ok(Dispatched::Pending(pending))
}

fn settle(store: &mut Store, id: &str, actual: Usd, generation: &str) -> Result<Dispatched> {
    store.settle(id, actual, generation)?;
    let maximum = store.reservation(id)?.ok_or(Error::NotFound)?.maximum;
    Ok(Dispatched::Settled {
        actual,
        breach: actual > maximum,
    })
}

/// A generation ID is usable as billing evidence only if it is a valid ledger key.
fn usable(generation_id: Option<String>) -> Option<String> {
    generation_id.filter(|id| validate_key(id, "generation ID").is_ok())
}

fn contract(request: &DispatchRequest<'_>) -> Result<AttemptRow> {
    validate_key(request.id, "request ID")?;
    validate_key(request.route_id, "provider route ID")?;
    validate_key(request.snapshot_id, "price snapshot ID")?;
    if request.text.is_empty() || request.text.len() > MAX_INPUT_BYTES {
        return Err(Error::InvalidInput("provider input size"));
    }
    if !(1..=MAX_COMPLETION_TOKENS).contains(&request.max_completion_tokens) {
        return Err(Error::InvalidInput("completion token bound"));
    }
    if request.retry_of == Some(request.id) {
        return Err(Error::InvalidInput("a retry needs a new attempt ID"));
    }
    let content = u32::try_from(request.text.len())
        .map_err(|_| Error::InvalidInput("provider input size"))?;
    Ok(AttemptRow {
        request_id: request.id.to_owned(),
        route_id: request.route_id.to_owned(),
        snapshot_id: request.snapshot_id.to_owned(),
        source_language: normalize_tag(request.source_language)?,
        target_language: normalize_tag(request.target_language)?,
        input_sha256: sha256(request.text.as_bytes()),
        prompt_tokens: content + TEMPLATE_OVERHEAD_TOKENS,
        completion_tokens: request.max_completion_tokens,
        retry_of: request.retry_of.map(str::to_owned),
    })
}

fn check_retry(store: &Store, row: &AttemptRow, now_ms: i64) -> Result<()> {
    let Some(prior_id) = &row.retry_of else {
        return Ok(());
    };
    let prior = store.provider_attempt(prior_id)?.ok_or(Error::NotFound)?;
    if prior.row.retry_of.is_some() || store.provider_retry_exists(prior_id)? {
        return Err(Error::ProviderRefused("an attempt may be retried once"));
    }
    if prior.row.route_id != row.route_id
        || prior.row.input_sha256 != row.input_sha256
        || prior.row.source_language != row.source_language
        || prior.row.target_language != row.target_language
    {
        return Err(Error::ProviderRefused(
            "a retry must repeat the same route and input",
        ));
    }
    match (prior.outcome, prior.outcome_ms, prior.retry_after_ms) {
        (Some(AttemptOutcome::RetryAfter), Some(at), Some(wait))
            if now_ms >= at.saturating_add(wait) =>
        {
            Ok(())
        }
        (Some(AttemptOutcome::RetryAfter), ..) => Err(Error::ProviderRefused(
            "the provider retry time has not passed",
        )),
        (Some(AttemptOutcome::Refused), ..) => Err(Error::ProviderRefused(
            "the provider refused payment; that attempt is terminal",
        )),
        _ => Err(Error::ProviderRefused(
            "only a payment refusal with a retry time may be retried",
        )),
    }
}

fn context(row: &AttemptRow) -> Result<String> {
    let digest = sha256(&serde_json::to_vec(&(
        "sigy-provider-attempt-v1",
        &row.route_id,
        &row.snapshot_id,
        &row.source_language,
        &row.target_language,
        &row.input_sha256,
        row.prompt_tokens,
        row.completion_tokens,
        &row.retry_of,
    ))?);
    Ok(format!("provider-v1:{digest}"))
}

fn sha256(bytes: &[u8]) -> String {
    crate::storage::dvr::hex(&Sha256::digest(bytes))
}

mod tests;
