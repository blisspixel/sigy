use sigy_core::budget::BudgetError;

use super::*;
use crate::{
    control::{self, Operation, ProviderOperation},
    providers::{PriceSpec, RateSpec, RouteSpec},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn route_spec(secret_env: &str) -> RouteSpec {
    RouteSpec {
        id: "or-es-en".into(),
        provider: "openrouter".into(),
        endpoint_origin: "https://openrouter.ai".into(),
        model: "vendor/model".into(),
        upstream_providers: vec!["deepinfra".into()],
        task: "translate-text".into(),
        secret_env: Some(secret_env.into()),
        language_pairs: vec!["es:en".into(), "fr-CA:en".into()],
    }
}

fn price_spec() -> PriceSpec {
    PriceSpec {
        id: "or-price-1".into(),
        route_id: "or-es-en".into(),
        retrieved: "2020-01-01T00:00:00Z".into(),
        valid_hours: 24,
        rates: vec![
            RateSpec {
                dimension: "prompt".into(),
                usd: "0.00000015".into(),
            },
            RateSpec {
                dimension: "completion".into(),
                usd: "0.0000006".into(),
            },
        ],
        source_note: "fixture catalog, not a real price".into(),
    }
}

fn provider(store: &mut Store, command: ProviderOperation) -> Result<control::Snapshot> {
    control::apply(store, Operation::Provider { command })
}

/// Any variable already set in this process whose value is long enough that
/// an accidental match in JSON output is implausible. Setting one would need
/// `unsafe` in this edition, so the test reuses an inherited one.
fn inherited_secret() -> Option<(String, String)> {
    std::env::vars().find(|(name, value)| {
        value.len() >= 12
            && !value.contains('"')
            && !value.contains('\\')
            && crate::providers::validate_secret_name(name).is_ok()
    })
}

#[test]
fn snapshots_and_exports_name_the_variable_and_never_hold_its_value() -> TestResult {
    let Some((name, value)) = inherited_secret() else {
        return Err("no inherited environment variable to stand in for a secret".into());
    };
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    let mut outputs = vec![
        provider(
            &mut store,
            ProviderOperation::AddRoute {
                route: route_spec(&name),
            },
        )?,
        provider(
            &mut store,
            ProviderOperation::AddPrice {
                price: price_spec(),
            },
        )?,
    ];
    outputs.push(provider(
        &mut store,
        ProviderOperation::ShowRoute {
            id: "or-es-en".into(),
        },
    )?);
    outputs.push(provider(
        &mut store,
        ProviderOperation::ListRoutes {
            after: None,
            limit: 16,
        },
    )?);
    outputs.push(control::apply(&mut store, Operation::Status {})?);
    for output in &outputs {
        assert!(!output.provider_dispatch_available);
        let json = serde_json::to_string(output)?;
        assert!(!json.contains(&value), "a secret value reached the output");
    }
    let shown = outputs
        .get(2)
        .and_then(|view| view.provider.as_ref())
        .ok_or("page")?;
    let route = shown.routes.first().ok_or("route")?;
    assert_eq!(route.secret_env.as_deref(), Some(name.as_str()));
    assert!(!route.allow_fallbacks && !route.dispatch_available);
    assert!(
        route
            .language_pairs
            .iter()
            .all(|pair| pair.validation == "unvalidated")
    );
    assert_eq!(shown.prices.len(), 1);
    // The catalog file itself holds only the name.
    drop(store);
    let bytes = std::fs::read(directory.path().join("catalog.sqlite3"))?;
    let wal = std::fs::read(directory.path().join("catalog.sqlite3-wal")).unwrap_or_default();
    for file in [&bytes, &wal] {
        assert!(
            !file
                .windows(value.len())
                .any(|window| window == value.as_bytes()),
            "a secret value reached the catalog"
        );
    }
    Ok(())
}

#[test]
fn provider_requests_round_trip_on_current_ipc() -> TestResult {
    let request = control::Request::new(Operation::Provider {
        command: ProviderOperation::AddRoute {
            route: route_spec("OPENROUTER_API_KEY"),
        },
    });
    let text = serde_json::to_string(&request)?;
    assert!(
        text.contains(&format!("\"version\":{}", crate::control::PROTOCOL_VERSION)),
        "{text}"
    );
    let decoded: control::Request = serde_json::from_str(&text)?;
    assert!(matches!(
        decoded.operation,
        Operation::Provider {
            command: ProviderOperation::AddRoute { route }
        } if route == route_spec("OPENROUTER_API_KEY")
    ));
    let extra = text.replace("\"task\":", "\"secret_value\":\"x\",\"task\":");
    assert!(serde_json::from_str::<control::Request>(&extra).is_err());
    Ok(())
}

#[test]
fn a_configured_route_leaves_the_default_global_budget_disabled() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    provider(
        &mut store,
        ProviderOperation::AddRoute {
            route: route_spec("OPENROUTER_API_KEY"),
        },
    )?;
    provider(
        &mut store,
        ProviderOperation::AddPrice {
            price: price_spec(),
        },
    )?;
    assert_eq!(store.budget("global")?.limit(), sigy_core::money::Usd::ZERO);
    assert!(matches!(
        store.reserve("r1", "provider-v1:any", "0.1".parse()?, &[]),
        Err(Error::Budget(BudgetError::Disabled))
    ));
    store.audit_ledger()?;
    store.audit_providers()?;
    Ok(())
}

#[test]
fn routes_and_prices_are_immutable_and_replays_are_exact() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
    let route = RouteDraft::from_spec(&route_spec("OPENROUTER_API_KEY"))?;
    assert!(store.add_provider_route(&route, 1)?.1);
    assert!(!store.add_provider_route(&route, 2)?.1);
    let mut changed = route.clone();
    changed.model = "vendor/other".into();
    assert!(matches!(
        store.add_provider_route(&changed, 3),
        Err(Error::IdempotencyConflict)
    ));
    let price = PriceDraft::from_spec(&price_spec(), 1_600_000_000_000)?;
    assert!(store.add_price_snapshot(&price, 1)?.1);
    assert!(!store.add_price_snapshot(&price, 2)?.1);
    let mut repriced = price.clone();
    repriced.valid_until_ms += 1;
    assert!(matches!(
        store.add_price_snapshot(&repriced, 3),
        Err(Error::IdempotencyConflict)
    ));
    let mut orphan = price.clone();
    orphan.id = "orphan".into();
    orphan.route_id = "missing".into();
    assert!(matches!(
        store.add_price_snapshot(&orphan, 3),
        Err(Error::NotFound)
    ));
    for statement in [
        "UPDATE provider_routes SET model = 'x'",
        "UPDATE provider_routes SET secret_env = NULL",
        "DELETE FROM provider_routes",
        "UPDATE provider_route_pairs SET validation = 'validated'",
        "DELETE FROM provider_route_pairs",
        "UPDATE provider_price_snapshots SET prompt = '0'",
        "DELETE FROM provider_price_snapshots",
        "INSERT INTO provider_route_pairs(route_id, source_language, target_language, validation) VALUES ('or-es-en', 'de', 'en', 'validated')",
        "INSERT INTO provider_routes(id, provider, endpoint_origin, model, upstreams_json, task, secret_env, allow_fallbacks, created_ms) VALUES ('fallback', 'openrouter', 'https://openrouter.ai', 'm', '[\"a\"]', 'translate-text', 'KEY', 1, 0)",
        "INSERT INTO provider_routes(id, provider, endpoint_origin, model, upstreams_json, task, secret_env, created_ms) VALUES ('keyless', 'openrouter', 'https://openrouter.ai', 'm', '[\"a\"]', 'translate-text', NULL, 0)",
        "INSERT INTO provider_routes(id, provider, endpoint_origin, model, upstreams_json, task, secret_env, created_ms) VALUES ('open', 'openrouter', 'https://openrouter.ai', 'm', '[]', 'translate-text', 'KEY', 0)",
        "INSERT INTO provider_routes(id, provider, endpoint_origin, model, upstreams_json, task, secret_env, created_ms) VALUES ('value', 'openrouter', 'https://openrouter.ai', 'm', '[\"a\"]', 'translate-text', 'sk-or-v1-abc', 0)",
    ] {
        assert!(
            store.connection.execute(statement, []).is_err(),
            "accepted {statement}"
        );
    }
    let local = RouteDraft::from_spec(&RouteSpec {
        id: "local".into(),
        provider: "ollama".into(),
        endpoint_origin: "http://127.0.0.1:11434".into(),
        model: "llama3.2:3b".into(),
        upstream_providers: Vec::new(),
        task: "translate-text".into(),
        secret_env: None,
        language_pairs: vec!["es:en".into()],
    })?;
    store.add_provider_route(&local, 4)?;
    let mut local_price = price.clone();
    local_price.id = "local-price".into();
    local_price.route_id = "local".into();
    assert!(matches!(
        store.add_price_snapshot(&local_price, 5),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(store.provider_routes(None, 64)?.len(), 2);
    assert_eq!(store.provider_routes(Some("local"), 64)?.len(), 1);
    store.audit_providers()?;
    store.audit_ledger()?;
    Ok(())
}

#[test]
fn schema_28_applies_to_a_v27_catalog_and_rolls_back_on_failure() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite3");
    let mut store = Store::open(&path)?;
    store.set_budget_limit("global", "2".parse()?)?;
    store.reserve("kept", "model-v1", "0.5".parse()?, &[])?;
    drop(store);
    let downgrade = "DROP TABLE translation_cues; DROP TABLE translations; DROP TABLE translation_jobs; DROP TABLE translation_profiles; DROP TABLE provider_attempts; DROP TABLE provider_price_snapshots; DROP TABLE provider_route_pairs; DROP TABLE provider_routes; PRAGMA user_version = 27;";
    let mut connection = rusqlite::Connection::open(&path)?;
    crate::storage::job_pool::revert_031_for_tests(&mut connection)?;
    crate::storage::widen::revert_030_for_tests(&mut connection)?;
    connection.execute_batch(downgrade)?;
    // A conflicting object makes the migration fail part way through.
    connection.execute_batch("CREATE TABLE provider_attempts(x INTEGER);")?;
    drop(connection);
    assert!(Store::open(&path).is_err());
    let connection = rusqlite::Connection::open(&path)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let routes: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'provider_routes'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!((version, routes), (27, 0));
    connection.execute_batch("DROP TABLE provider_attempts;")?;
    drop(connection);
    let store = Store::open(&path)?;
    let version: i64 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, i64::from(crate::storage::SCHEMA_VERSION));
    assert_eq!(store.budget("global")?.reserved(), "0.5".parse()?);
    assert!(store.reservation("kept")?.is_some());
    store.audit_ledger()?;
    store.audit_providers()?;
    Ok(())
}
