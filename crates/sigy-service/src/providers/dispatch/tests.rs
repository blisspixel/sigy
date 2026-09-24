use std::{collections::VecDeque, path::PathBuf};

use sigy_core::budget::BudgetError;

use super::*;
use crate::{
    providers::{PriceDraft, PriceSpec, RateSpec, RouteDraft, RouteSpec},
    storage::providers::attempts::AttemptOutcome,
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

const ROUTE: &str = "or-es-en";
const PRICE: &str = "or-price-1";
/// 2020-01-01T00:00:00Z, the retrieval time of the fixture price.
const RETRIEVED_MS: i64 = 1_577_836_800_000;
const NOW: i64 = RETRIEVED_MS + 3_600_000;
const SCOPE: &str = "provider:or-es-en";

/// A scripted transport. It records every send and the durable request state
/// seen through a second catalog connection at the moment of sending.
struct Fake {
    catalog: PathBuf,
    script: VecDeque<Sent>,
    lookups: VecDeque<Lookup>,
    sends: Vec<Attempt>,
    states_at_send: Vec<Option<RequestState>>,
    lookup_calls: usize,
}

impl Fake {
    fn new(catalog: PathBuf, script: impl IntoIterator<Item = Sent>) -> Self {
        Self {
            catalog,
            script: script.into_iter().collect(),
            lookups: VecDeque::new(),
            sends: Vec::new(),
            states_at_send: Vec::new(),
            lookup_calls: 0,
        }
    }
}

impl Transport for Fake {
    fn send(&mut self, attempt: &Attempt) -> Sent {
        let state = Store::open(&self.catalog)
            .ok()
            .and_then(|store| store.reservation(&attempt.request_id).ok().flatten())
            .map(|reservation| reservation.state);
        self.states_at_send.push(state);
        self.sends.push(attempt.clone());
        self.script.pop_front().unwrap_or(Sent::TimedOut {
            generation_id: None,
        })
    }

    fn lookup(&mut self, _generation_id: &str) -> Lookup {
        self.lookup_calls += 1;
        self.lookups.pop_front().unwrap_or(Lookup::Failed)
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    store: Store,
}

impl Fixture {
    fn new(
        global: &str,
        scope: Option<&str>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        Self::with_rates(
            global,
            scope,
            &[("prompt", "0.000001"), ("completion", "0.000002")],
        )
    }

    fn with_rates(
        global: &str,
        scope: Option<&str>,
        rates: &[(&str, &str)],
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog.sqlite3");
        let mut store = Store::open(&path)?;
        if global != "0" {
            store.set_budget_limit("global", global.parse()?)?;
        }
        if let Some(limit) = scope {
            store.set_budget_limit(SCOPE, limit.parse()?)?;
        }
        let route = RouteDraft::from_spec(&RouteSpec {
            id: ROUTE.into(),
            provider: "openrouter".into(),
            endpoint_origin: "https://openrouter.ai".into(),
            model: "vendor/model".into(),
            upstream_providers: vec!["deepinfra".into()],
            task: "translate-text".into(),
            secret_env: Some("SIGY_TEST_OPENROUTER_KEY".into()),
            language_pairs: vec!["es:en".into()],
        })?;
        store.add_provider_route(&route, RETRIEVED_MS)?;
        let price = PriceDraft::from_spec(
            &PriceSpec {
                id: PRICE.into(),
                route_id: ROUTE.into(),
                retrieved: "2020-01-01T00:00:00Z".into(),
                valid_hours: 24,
                rates: rates
                    .iter()
                    .map(|(dimension, usd)| RateSpec {
                        dimension: (*dimension).into(),
                        usd: (*usd).into(),
                    })
                    .collect(),
                source_note: "fixture catalog, not a real price".into(),
            },
            NOW,
        )?;
        store.add_price_snapshot(&price, RETRIEVED_MS)?;
        Ok(Self {
            _directory: directory,
            path,
            store,
        })
    }

    fn fake(&self, script: impl IntoIterator<Item = Sent>) -> Fake {
        Fake::new(self.path.clone(), script)
    }

    fn audit(&self) -> TestResult {
        self.store.audit_ledger()?;
        self.store.audit_providers()?;
        Ok(())
    }
}

fn request(id: &str) -> DispatchRequest<'_> {
    DispatchRequest {
        id,
        route_id: ROUTE,
        snapshot_id: PRICE,
        task: Task::TranslateText,
        source_language: "es",
        target_language: "en",
        text: "hola",
        max_completion_tokens: 100,
        budgets: &[],
        retry_of: None,
        trigger: Trigger::Requested,
    }
}

/// 4 content bytes + 256 template tokens at 1 USD per million, plus 100
/// completion tokens at 2 USD per million: 0.00046 USD.
fn expected_maximum() -> std::result::Result<Usd, Box<dyn std::error::Error>> {
    Ok("0.00046".parse()?)
}

fn completed(generation: &str, cost: &str) -> Sent {
    Sent::Completed {
        generation_id: Some(generation.into()),
        cost: Some(cost.into()),
    }
}

#[test]
fn settles_from_reported_cost_after_persisting_submission() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([completed("gen-1", "0.0003")]);
    let result = dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?;
    assert_eq!(
        result,
        Dispatched::Settled {
            actual: "0.0003".parse()?,
            breach: false
        }
    );
    assert_eq!(fake.states_at_send, [Some(RequestState::Submitted)]);
    let sent = fake.sends.first().ok_or("no send")?;
    assert!(!sent.allow_fallbacks);
    assert_eq!(sent.upstream_only, ["deepinfra"]);
    assert_eq!(sent.max_prompt_usd_per_million, "1");
    assert_eq!(sent.max_completion_usd_per_million, "2");
    assert_eq!(sent.secret_env, "SIGY_TEST_OPENROUTER_KEY");
    let reservation = fixture.store.reservation("r1")?.ok_or("missing")?;
    assert_eq!(reservation.maximum, expected_maximum()?);
    assert_eq!(reservation.evidence.as_deref(), Some("gen-1"));
    let global = fixture.store.budget("global")?;
    assert_eq!(global.settled(), "0.0003".parse()?);
    assert_eq!(global.reserved(), Usd::ZERO);
    fixture.audit()
}

#[test]
fn a_sub_micro_cost_rounds_up() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([completed("gen-1", "1.5e-7")]);
    let result = dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?;
    assert_eq!(
        result,
        Dispatched::Settled {
            actual: Usd::from_micros(1)?,
            breach: false
        }
    );
    fixture.audit()
}

#[test]
fn replay_of_a_request_id_never_calls_the_transport() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([completed("gen-1", "0.0003")]);
    dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?;
    for _ in 0..3 {
        assert_eq!(
            dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?,
            Dispatched::Replayed(RequestState::Settled)
        );
    }
    let changed = DispatchRequest {
        text: "adios",
        ..request("r1")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &changed, NOW),
        Err(Error::IdempotencyConflict)
    ));
    // A replay is answered even after the price snapshot went stale.
    assert_eq!(
        dispatch(
            &mut fixture.store,
            &mut fake,
            &request("r1"),
            NOW + 86_400_000
        )?,
        Dispatched::Replayed(RequestState::Settled)
    );
    assert_eq!(fake.sends.len(), 1);
    fixture.audit()
}

#[test]
fn insufficient_funds_reserve_nothing_and_send_nothing() -> TestResult {
    let mut fixture = Fixture::new("0.0004", None)?;
    let mut fake = fixture.fake([]);
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW),
        Err(Error::Budget(BudgetError::InsufficientFunds))
    ));
    let scoped = DispatchRequest {
        budgets: &[SCOPE],
        ..request("r2")
    };
    fixture.store.set_budget_limit("global", "1".parse()?)?;
    fixture.store.set_budget_limit(SCOPE, "0.0001".parse()?)?;
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &scoped, NOW),
        Err(Error::Budget(BudgetError::InsufficientFunds))
    ));
    assert!(fixture.store.reservation("r1")?.is_none());
    assert!(fixture.store.reservation("r2")?.is_none());
    assert!(fixture.store.provider_attempt("r2")?.is_none());
    assert_eq!(fixture.store.budget("global")?.reserved(), Usd::ZERO);
    assert!(fake.sends.is_empty());
    fixture.audit()
}

#[test]
fn a_configured_route_and_secret_with_a_zero_limit_cannot_submit() -> TestResult {
    let mut fixture = Fixture::new("0", None)?;
    let mut fake = fixture.fake([completed("gen-1", "0.0003")]);
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW),
        Err(Error::Budget(BudgetError::Disabled))
    ));
    assert!(fixture.store.reservation("r1")?.is_none());
    assert!(fake.sends.is_empty());
    fixture.audit()
}

#[test]
fn overload_never_opens_a_paid_route() -> TestResult {
    let mut fixture = Fixture::new("10", None)?;
    let mut fake = fixture.fake([completed("gen-1", "0.0003")]);
    let overloaded = DispatchRequest {
        trigger: Trigger::LocalOverload,
        ..request("r1")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &overloaded, NOW),
        Err(Error::ProviderRefused(_))
    ));
    assert!(fixture.store.reservation("r1")?.is_none());
    assert!(fake.sends.is_empty());
    assert_eq!(fixture.store.budget("global")?.reserved(), Usd::ZERO);
    fixture.audit()
}

#[test]
fn cost_above_the_reservation_is_recorded_and_freezes_every_scope() -> TestResult {
    let mut fixture = Fixture::new("1", Some("1"))?;
    let mut fake = fixture.fake([completed("gen-1", "0.5"), completed("gen-2", "0.0001")]);
    let scoped = DispatchRequest {
        budgets: &[SCOPE],
        ..request("r1")
    };
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &scoped, NOW)?,
        Dispatched::Settled {
            actual: "0.5".parse()?,
            breach: true
        }
    );
    for scope in ["global", SCOPE] {
        let balance = fixture.store.budget(scope)?;
        assert!(balance.frozen(), "{scope} is not frozen");
        assert_eq!(balance.settled(), "0.5".parse()?);
    }
    fixture.audit()?;
    // A frozen scope refuses new work before any reservation or send.
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &request("r2"), NOW),
        Err(Error::Budget(BudgetError::Frozen))
    ));
    assert!(fixture.store.reservation("r2")?.is_none());
    assert_eq!(fake.sends.len(), 1);
    fixture.audit()
}

#[test]
fn release_before_submit_frees_the_reservation_and_never_sends() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([completed("gen-1", "0.0003")]);
    let Admit::New(admitted) = admit(&mut fixture.store, &request("r1"), NOW)? else {
        return Err("expected a new admission".into());
    };
    assert_eq!(admitted.maximum, expected_maximum()?);
    assert_eq!(
        fixture.store.budget("global")?.reserved(),
        expected_maximum()?
    );
    assert_eq!(fixture.store.release_unsubmitted("r1")?, Change::Applied);
    assert!(matches!(
        submit(&mut fixture.store, &mut fake, &admitted, NOW),
        Err(Error::RequestState)
    ));
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?,
        Dispatched::Replayed(RequestState::Released)
    );
    assert!(fake.sends.is_empty());
    assert_eq!(fixture.store.budget("global")?.reserved(), Usd::ZERO);
    assert!(
        fixture
            .store
            .provider_attempt("r1")?
            .ok_or("missing attempt")?
            .outcome
            .is_none()
    );
    fixture.audit()
}

#[test]
fn a_crash_after_submit_keeps_an_uncertain_liability_across_reopen() -> TestResult {
    let Fixture {
        _directory,
        path,
        mut store,
    } = Fixture::new("1", None)?;
    let mut fake = Fake::new(path.clone(), [Sent::ProcessLost]);
    assert_eq!(
        dispatch(&mut store, &mut fake, &request("r1"), NOW)?,
        Dispatched::ProcessLost
    );
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(
        store.reservation("r1")?.ok_or("missing")?.state,
        RequestState::Submitted
    );
    assert_eq!(store.recover_submitted()?, 1);
    assert_eq!(store.recover_submitted()?, 0);
    let reservation = store.reservation("r1")?.ok_or("missing")?;
    assert_eq!(reservation.state, RequestState::Uncertain);
    assert_eq!(store.budget("global")?.reserved(), expected_maximum()?);
    assert_eq!(
        dispatch(&mut store, &mut fake, &request("r1"), NOW)?,
        Dispatched::Replayed(RequestState::Uncertain)
    );
    assert_eq!(
        reconcile(&mut store, &mut fake, "r1")?,
        Dispatched::Pending(Pending::NoGenerationId)
    );
    assert!(matches!(
        store.release_unsubmitted("r1"),
        Err(Error::RequestState)
    ));
    assert_eq!(fake.sends.len(), 1);
    assert_eq!(fake.lookup_calls, 0);
    store.audit_ledger()?;
    store.audit_providers()?;
    Ok(())
}

#[test]
fn a_timeout_without_a_generation_id_keeps_the_full_reservation() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([Sent::TimedOut {
        generation_id: None,
    }]);
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?,
        Dispatched::Pending(Pending::TimedOut)
    );
    assert_eq!(
        fixture.store.reservation("r1")?.ok_or("missing")?.state,
        RequestState::Uncertain
    );
    assert_eq!(
        fixture.store.budget("global")?.reserved(),
        expected_maximum()?
    );
    assert_eq!(
        reconcile(&mut fixture.store, &mut fake, "r1")?,
        Dispatched::Pending(Pending::NoGenerationId)
    );
    assert_eq!(fake.lookup_calls, 0);
    let attempt = fixture.store.provider_attempt("r1")?.ok_or("missing")?;
    assert_eq!(attempt.outcome, Some(AttemptOutcome::Timeout));
    fixture.audit()
}

#[test]
fn missing_usage_without_a_generation_id_stays_uncertain() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([
        Sent::Completed {
            generation_id: None,
            cost: Some("0.0001".into()),
        },
        Sent::Completed {
            generation_id: Some("gen with space".into()),
            cost: None,
        },
    ]);
    for id in ["r1", "r2"] {
        assert_eq!(
            dispatch(&mut fixture.store, &mut fake, &request(id), NOW)?,
            Dispatched::Pending(Pending::NoGenerationId)
        );
        assert_eq!(
            fixture.store.reservation(id)?.ok_or("missing")?.state,
            RequestState::Uncertain
        );
    }
    assert_eq!(
        fixture.store.budget("global")?.reserved(),
        Usd::from_micros(2 * expected_maximum()?.micros())?
    );
    assert_eq!(fake.lookup_calls, 0);
    fixture.audit()
}

#[test]
fn generation_lookup_404_then_success_settles_once() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([Sent::Completed {
        generation_id: Some("gen-9".into()),
        cost: None,
    }]);
    fake.lookups = VecDeque::from([
        Lookup::NotFound,
        Lookup::Found {
            cost: "0.00025".into(),
        },
    ]);
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?,
        Dispatched::Pending(Pending::NotFoundYet)
    );
    assert_eq!(
        fixture.store.budget("global")?.reserved(),
        expected_maximum()?
    );
    fixture.audit()?;
    let settled = Dispatched::Settled {
        actual: "0.00025".parse()?,
        breach: false,
    };
    assert_eq!(reconcile(&mut fixture.store, &mut fake, "r1")?, settled);
    // Idempotent settle: a second reconcile does not look up again.
    assert_eq!(reconcile(&mut fixture.store, &mut fake, "r1")?, settled);
    assert_eq!(fake.lookup_calls, 2);
    assert_eq!(
        fixture.store.settle("r1", "0.00025".parse()?, "gen-9")?,
        Change::Unchanged
    );
    assert!(matches!(
        fixture.store.settle("r1", "0.0003".parse()?, "gen-9"),
        Err(Error::IdempotencyConflict)
    ));
    let global = fixture.store.budget("global")?;
    assert_eq!(global.settled(), "0.00025".parse()?);
    assert_eq!(global.reserved(), Usd::ZERO);
    assert_eq!(fake.sends.len(), 1);
    fixture.audit()
}

#[test]
fn a_terminal_402_keeps_the_liability_and_cannot_be_retried() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([Sent::PaymentRequired {
        retry_after_seconds: None,
    }]);
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?,
        Dispatched::Pending(Pending::Refused)
    );
    assert_eq!(
        fixture.store.budget("global")?.reserved(),
        expected_maximum()?
    );
    let retry = DispatchRequest {
        retry_of: Some("r1"),
        ..request("r2")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &retry, NOW + 60_000),
        Err(Error::ProviderRefused(_))
    ));
    assert!(fixture.store.reservation("r2")?.is_none());
    assert_eq!(fake.sends.len(), 1);
    fixture.audit()
}

#[test]
fn a_402_retry_after_needs_a_new_attempt_and_a_new_reservation() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([
        Sent::PaymentRequired {
            retry_after_seconds: Some(30),
        },
        completed("gen-2", "0.0002"),
    ]);
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?,
        Dispatched::Pending(Pending::RetryAfter {
            not_before_ms: NOW + 30_000
        })
    );
    // The same ID is a replay, never a resend.
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW + 60_000)?,
        Dispatched::Replayed(RequestState::Uncertain)
    );
    let same_id = DispatchRequest {
        retry_of: Some("r1"),
        ..request("r1")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &same_id, NOW + 60_000),
        Err(Error::InvalidInput(_))
    ));
    let retry = DispatchRequest {
        retry_of: Some("r1"),
        ..request("r2")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &retry, NOW + 29_999),
        Err(Error::ProviderRefused(_))
    ));
    let changed_input = DispatchRequest {
        text: "otra cosa",
        ..retry
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &changed_input, NOW + 30_000),
        Err(Error::ProviderRefused(_))
    ));
    assert_eq!(
        fixture.store.budget("global")?.reserved(),
        expected_maximum()?
    );
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &retry, NOW + 30_000)?,
        Dispatched::Settled {
            actual: "0.0002".parse()?,
            breach: false
        }
    );
    // The first attempt stays an uncertain liability beside the settled retry.
    let global = fixture.store.budget("global")?;
    assert_eq!(global.reserved(), expected_maximum()?);
    assert_eq!(global.settled(), "0.0002".parse()?);
    let again = DispatchRequest {
        retry_of: Some("r1"),
        ..request("r3")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &again, NOW + 90_000),
        Err(Error::ProviderRefused(_))
    ));
    assert_eq!(fake.sends.len(), 2);
    fixture.audit()
}

#[test]
fn a_stale_or_future_price_snapshot_is_refused_before_reserve() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([]);
    for now in [RETRIEVED_MS - 1, RETRIEVED_MS + 24 * 3_600_000] {
        assert!(matches!(
            dispatch(&mut fixture.store, &mut fake, &request("r1"), now),
            Err(Error::ProviderRefused(_))
        ));
    }
    assert!(fixture.store.reservation("r1")?.is_none());
    assert!(fake.sends.is_empty());
    fixture.audit()
}

#[test]
fn an_unbounded_price_dimension_is_refused_before_reserve() -> TestResult {
    let mut fixture = Fixture::with_rates(
        "1",
        None,
        &[
            ("prompt", "0.000001"),
            ("completion", "0.000002"),
            ("web_search", "0.004"),
        ],
    )?;
    let mut fake = fixture.fake([]);
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW),
        Err(Error::Pricing(_))
    ));
    assert!(fixture.store.reservation("r1")?.is_none());
    assert!(fake.sends.is_empty());
    fixture.audit()
}

#[test]
fn unauthorized_pairs_tasks_and_bounds_are_refused_before_reserve() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([]);
    let wrong_pair = DispatchRequest {
        source_language: "fr",
        ..request("r1")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &wrong_pair, NOW),
        Err(Error::ProviderRefused(_))
    ));
    for invalid in [
        DispatchRequest {
            max_completion_tokens: 0,
            ..request("r1")
        },
        DispatchRequest {
            text: "",
            ..request("r1")
        },
    ] {
        assert!(matches!(
            dispatch(&mut fixture.store, &mut fake, &invalid, NOW),
            Err(Error::InvalidInput(_))
        ));
    }
    let large = "a".repeat(MAX_INPUT_BYTES + 1);
    let oversized = DispatchRequest {
        text: &large,
        ..request("r1")
    };
    assert!(matches!(
        dispatch(&mut fixture.store, &mut fake, &oversized, NOW),
        Err(Error::InvalidInput(_))
    ));
    assert!(fixture.store.reservation("r1")?.is_none());
    assert!(fake.sends.is_empty());
    fixture.audit()
}

#[test]
fn an_upstream_error_may_still_charge_and_is_reconciled_by_generation() -> TestResult {
    let mut fixture = Fixture::new("1", None)?;
    let mut fake = fixture.fake([Sent::Failed {
        generation_id: Some("gen-err".into()),
    }]);
    fake.lookups = VecDeque::from([
        Lookup::Failed,
        Lookup::Found {
            cost: "0.00001".into(),
        },
    ]);
    assert_eq!(
        dispatch(&mut fixture.store, &mut fake, &request("r1"), NOW)?,
        Dispatched::Pending(Pending::Failed)
    );
    assert_eq!(
        fixture.store.budget("global")?.reserved(),
        expected_maximum()?
    );
    assert_eq!(
        reconcile(&mut fixture.store, &mut fake, "r1")?,
        Dispatched::Pending(Pending::LookupFailed)
    );
    assert_eq!(
        reconcile(&mut fixture.store, &mut fake, "r1")?,
        Dispatched::Settled {
            actual: "0.00001".parse()?,
            breach: false
        }
    );
    assert_eq!(fixture.store.budget("global")?.reserved(), Usd::ZERO);
    assert_eq!(fake.sends.len(), 1);
    fixture.audit()
}

#[test]
fn a_consumed_lifetime_allowance_stays_consumed_across_restart_and_dates() -> TestResult {
    // The allowance covers exactly two worst-case attempts.
    let Fixture {
        _directory,
        path,
        mut store,
    } = Fixture::new("0.00092", None)?;
    let mut fake = Fake::new(
        path.clone(),
        [
            completed("gen-1", "0.00046"),
            Sent::TimedOut {
                generation_id: None,
            },
        ],
    );
    dispatch(&mut store, &mut fake, &request("r1"), NOW)?;
    assert_eq!(
        dispatch(&mut store, &mut fake, &request("r2"), NOW)?,
        Dispatched::Pending(Pending::TimedOut)
    );
    let consumed = store.budget("global")?;
    assert_eq!(consumed.available(), Usd::ZERO);
    assert!(matches!(
        dispatch(&mut store, &mut fake, &request("r3"), NOW),
        Err(Error::Budget(BudgetError::InsufficientFunds))
    ));
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.recover_submitted()?, 0);
    // A new day, a new month and a new year: a fresh price snapshot does not refill anything.
    for (index, (retrieved, now)) in [
        ("2020-01-02T00:00:00Z", RETRIEVED_MS + 86_400_000),
        ("2020-02-01T00:00:00Z", 1_580_515_200_000),
        ("2021-01-01T00:00:00Z", 1_609_459_200_000),
    ]
    .into_iter()
    .enumerate()
    {
        let snapshot = format!("later-{index}");
        let price = PriceDraft::from_spec(
            &PriceSpec {
                id: snapshot.clone(),
                route_id: ROUTE.into(),
                retrieved: retrieved.into(),
                valid_hours: 24,
                rates: vec![
                    RateSpec {
                        dimension: "prompt".into(),
                        usd: "0.000001".into(),
                    },
                    RateSpec {
                        dimension: "completion".into(),
                        usd: "0.000002".into(),
                    },
                ],
                source_note: "fixture catalog, not a real price".into(),
            },
            now,
        )?;
        store.add_price_snapshot(&price, now)?;
        let id = format!("later-request-{index}");
        let later = DispatchRequest {
            snapshot_id: &snapshot,
            ..request(&id)
        };
        assert!(matches!(
            dispatch(&mut store, &mut fake, &later, now + 1),
            Err(Error::Budget(BudgetError::InsufficientFunds))
        ));
        assert!(store.reservation(&id)?.is_none());
    }
    assert_eq!(store.budget("global")?, consumed);
    assert_eq!(fake.sends.len(), 2);
    store.audit_ledger()?;
    store.audit_providers()?;
    Ok(())
}
