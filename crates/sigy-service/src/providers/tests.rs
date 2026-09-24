use super::*;

fn route(provider: &str, origin: &str) -> RouteSpec {
    RouteSpec {
        id: "route-1".into(),
        provider: provider.into(),
        endpoint_origin: origin.into(),
        model: "vendor/model:free".into(),
        upstream_providers: if provider == "openrouter" {
            vec!["deepinfra".into()]
        } else {
            Vec::new()
        },
        task: "translate-text".into(),
        secret_env: (provider == "openrouter").then(|| "OPENROUTER_API_KEY".into()),
        language_pairs: vec!["es:en".into()],
    }
}

fn price(rates: &[(&str, &str)]) -> PriceSpec {
    PriceSpec {
        id: "price-1".into(),
        route_id: "route-1".into(),
        retrieved: "2026-09-24T00:00:00Z".into(),
        valid_hours: 24,
        rates: rates
            .iter()
            .map(|(dimension, usd)| RateSpec {
                dimension: (*dimension).into(),
                usd: (*usd).into(),
            })
            .collect(),
        source_note: "OpenRouter model catalog, 2026-09-24".into(),
    }
}

const NOW: i64 = 1_790_208_000_000;

#[test]
fn origins_are_canonical_and_scoped_by_provider() -> Result<()> {
    for (provider, origin, expected) in [
        (
            "openrouter",
            "https://openrouter.ai",
            "https://openrouter.ai",
        ),
        (
            "openrouter",
            "https://OpenRouter.ai/",
            "https://openrouter.ai",
        ),
        ("ollama", "http://127.0.0.1:11434", "http://127.0.0.1:11434"),
        ("ollama", "http://localhost:11434", "http://localhost:11434"),
        ("ollama", "http://[::1]:11434", "http://[::1]:11434"),
    ] {
        assert_eq!(
            RouteDraft::from_spec(&route(provider, origin))?.origin,
            expected
        );
    }
    for (provider, origin) in [
        ("openrouter", "http://openrouter.ai"),
        ("openrouter", "https://openrouter.ai/api/v1"),
        ("openrouter", "https://key@openrouter.ai"),
        ("openrouter", "https://openrouter.ai/?key=1"),
        ("openrouter", "https://openrouter.ai/#x"),
        ("openrouter", " https://openrouter.ai"),
        ("ollama", "http://192.168.1.10:11434"),
        ("ollama", "https://127.0.0.1:11434"),
        ("ollama", "http://ollama.example:11434"),
        ("cloud", "https://openrouter.ai"),
    ] {
        assert!(
            RouteDraft::from_spec(&route(provider, origin)).is_err(),
            "accepted {provider} {origin}"
        );
    }
    Ok(())
}

#[test]
fn a_secret_is_a_variable_name_and_never_key_text() -> Result<()> {
    for name in ["OPENROUTER_API_KEY", "_KEY", "Path", "k1"] {
        validate_secret_name(name)?;
    }
    for name in [
        "",
        "sk-or-v1-0123456789abcdef",
        "1KEY",
        "A B",
        "KEY=1",
        "KEY\n",
        "\u{00c9}T\u{00c9}",
        &"K".repeat(129),
    ] {
        assert!(validate_secret_name(name).is_err(), "accepted {name:?}");
    }
    let mut hosted = route("openrouter", "https://openrouter.ai");
    hosted.secret_env = None;
    assert!(RouteDraft::from_spec(&hosted).is_err());
    let mut local = route("ollama", "http://127.0.0.1:11434");
    local.secret_env = Some("OPENROUTER_API_KEY".into());
    assert!(RouteDraft::from_spec(&local).is_err());
    Ok(())
}

#[test]
fn upstreams_models_and_pairs_are_bounded_and_canonical() -> Result<()> {
    let mut spec = route("openrouter", "https://openrouter.ai");
    spec.upstream_providers = vec!["together".into(), "deepinfra/fp8".into()];
    spec.language_pairs = vec!["fr-CA:en".into(), "es:EN".into()];
    let draft = RouteDraft::from_spec(&spec)?;
    assert_eq!(draft.upstreams, ["deepinfra/fp8", "together"]);
    assert_eq!(
        draft
            .pairs
            .iter()
            .map(|pair| format!("{}:{}", pair.source, pair.target))
            .collect::<Vec<_>>(),
        ["es:en", "fr-CA:en"]
    );
    for change in [
        |spec: &mut RouteSpec| spec.upstream_providers.clear(),
        |spec: &mut RouteSpec| spec.upstream_providers = vec!["a".into(), "a".into()],
        |spec: &mut RouteSpec| spec.upstream_providers = vec!["DeepInfra".into()],
        |spec: &mut RouteSpec| spec.upstream_providers = vec![String::new(); 17],
        |spec: &mut RouteSpec| spec.model = "/model".into(),
        |spec: &mut RouteSpec| spec.model = "model name".into(),
        |spec: &mut RouteSpec| spec.task = "transcribe-audio".into(),
        |spec: &mut RouteSpec| spec.language_pairs.clear(),
        |spec: &mut RouteSpec| spec.language_pairs = vec!["es".into()],
        |spec: &mut RouteSpec| spec.language_pairs = vec!["es:es".into()],
        |spec: &mut RouteSpec| spec.language_pairs = vec!["und:en".into()],
        |spec: &mut RouteSpec| spec.language_pairs = vec!["es:en".into(), "ES:en".into()],
        |spec: &mut RouteSpec| spec.language_pairs = vec!["es:en".into(); 17],
        |spec: &mut RouteSpec| spec.id = "route 1".into(),
    ] {
        let mut spec = route("openrouter", "https://openrouter.ai");
        change(&mut spec);
        assert!(RouteDraft::from_spec(&spec).is_err(), "accepted {spec:?}");
    }
    Ok(())
}

#[test]
fn prices_are_exact_dated_and_complete() -> Result<()> {
    let draft = PriceDraft::from_spec(
        &price(&[
            ("prompt", "0.00000015"),
            ("completion", "0.0000006"),
            ("web_search", "0.004"),
        ]),
        NOW,
    )?;
    assert_eq!(draft.retrieved_ms, NOW);
    assert_eq!(draft.valid_until_ms, NOW + 86_400_000);
    assert!(draft.fresh_at(NOW) && !draft.fresh_at(NOW - 1) && !draft.fresh_at(NOW + 86_400_000));
    let text: Vec<String> = draft
        .rates
        .values()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        text,
        [
            "0.00000015",
            "0.0000006",
            "0",
            "0",
            "0",
            "0",
            "0",
            "0",
            "0.004",
            "0"
        ]
    );
    for rates in [
        &[("prompt", "0.1")][..],
        &[("prompt", "0.1"), ("completion", "-0.1")],
        &[("prompt", "0.1"), ("completion", "1e-6")],
        &[("prompt", "0.1"), ("completion", "0.1"), ("prompt", "0.2")],
        &[("prompt", "0.1"), ("completion", "0.1"), ("tools", "0.2")],
        &[("prompt", "0.1"), ("completion", "0.0000000000000000001")],
    ] {
        assert!(
            PriceDraft::from_spec(&price(rates), NOW).is_err(),
            "accepted {rates:?}"
        );
    }
    let valid = [("prompt", "0.1"), ("completion", "0.1")];
    for change in [
        |spec: &mut PriceSpec| spec.valid_hours = 0,
        |spec: &mut PriceSpec| spec.valid_hours = 721,
        |spec: &mut PriceSpec| spec.retrieved = "2026-09-25T00:00:00Z".into(),
        |spec: &mut PriceSpec| spec.retrieved = "yesterday".into(),
        |spec: &mut PriceSpec| spec.source_note = String::new(),
        |spec: &mut PriceSpec| spec.source_note = "catalog\u{1b}[2J".into(),
        |spec: &mut PriceSpec| spec.source_note = "cat\u{00e1}logo".into(),
        |spec: &mut PriceSpec| spec.source_note = " padded".into(),
        |spec: &mut PriceSpec| spec.source_note = "n".repeat(257),
    ] {
        let mut spec = price(&valid);
        change(&mut spec);
        assert!(
            PriceDraft::from_spec(&spec, NOW).is_err(),
            "accepted {spec:?}"
        );
    }
    Ok(())
}
