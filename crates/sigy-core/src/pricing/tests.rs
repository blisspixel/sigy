use super::*;
use crate::budget::{Balance, BudgetError};

type TestResult = Result<(), PricingError>;

fn rate(text: &str) -> Result<Rate, PricingError> {
    Rate::parse(text)
}

fn micros(value: i64) -> Usd {
    Usd::from_micros(value).unwrap_or(Usd::ZERO)
}

#[test]
fn catalog_rates_parse_exactly_and_display_canonically() -> TestResult {
    for (input, attodollars, display) in [
        ("0", 0, "0"),
        ("1", ATTO_PER_DOLLAR, "1"),
        ("0.00000015", 150_000_000_000, "0.00000015"),
        ("0.0000006", 600_000_000_000, "0.0000006"),
        ("0.000000000000000001", 1, "0.000000000000000001"),
        ("1.50", 1_500_000_000_000_000_000, "1.5"),
        ("2.000", 2 * ATTO_PER_DOLLAR, "2"),
        ("0.0", 0, "0"),
        (
            "340282366920938463463.374607431768211455",
            u128::MAX,
            "340282366920938463463.374607431768211455",
        ),
    ] {
        let parsed = rate(input)?;
        assert_eq!(parsed.attodollars(), attodollars, "{input}");
        assert_eq!(parsed.to_string(), display, "{input}");
        assert_eq!(rate(&parsed.to_string())?, parsed, "{input}");
    }
    Ok(())
}

#[test]
fn hostile_rate_text_is_rejected_with_a_typed_error() {
    let long = format!("0.{}", "0".repeat(MAX_TEXT_BYTES));
    for (input, expected) in [
        ("", PricingError::Empty),
        ("-0", PricingError::Negative),
        ("-1", PricingError::Negative),
        ("-0.00000015", PricingError::Negative),
        ("+1", PricingError::InvalidDecimal),
        (" 1", PricingError::InvalidDecimal),
        ("1 ", PricingError::InvalidDecimal),
        ("\t1", PricingError::InvalidDecimal),
        ("1,0", PricingError::InvalidDecimal),
        ("1_0", PricingError::InvalidDecimal),
        ("1e-7", PricingError::InvalidDecimal),
        ("1E2", PricingError::InvalidDecimal),
        ("1e999", PricingError::InvalidDecimal),
        ("NaN", PricingError::InvalidDecimal),
        ("inf", PricingError::InvalidDecimal),
        ("0x10", PricingError::InvalidDecimal),
        ("1.", PricingError::InvalidDecimal),
        (".1", PricingError::InvalidDecimal),
        ("01", PricingError::InvalidDecimal),
        ("00.1", PricingError::InvalidDecimal),
        ("1.2.3", PricingError::InvalidDecimal),
        ("1.-2", PricingError::InvalidDecimal),
        ("\u{ff11}", PricingError::InvalidDecimal),
        ("0.0000000000000000001", PricingError::TooPrecise),
        ("0.0000000000000000000", PricingError::TooPrecise),
        (
            "340282366920938463463.374607431768211456",
            PricingError::Overflow,
        ),
        ("340282366920938463464", PricingError::Overflow),
        (
            "99999999999999999999999999999999999999999",
            PricingError::Overflow,
        ),
        (long.as_str(), PricingError::TooLong),
    ] {
        assert_eq!(rate(input), Err(expected), "{input:?}");
    }
}

#[test]
fn per_token_rates_convert_to_exact_per_million_prices() -> TestResult {
    for (per_token, per_million) in [
        ("0", "0"),
        ("0.00000015", "0.15"),
        ("0.0000025", "2.5"),
        ("0.000000000000000001", "0.000000000001"),
        ("0.000000123456789012", "0.123456789012"),
        ("3", "3000000"),
        ("0.01", "10000"),
    ] {
        let parsed = rate(per_token)?;
        let converted = parsed.per_million_decimal()?;
        assert_eq!(converted, per_million, "{per_token}");
        assert_eq!(
            rate(&converted)?.attodollars(),
            parsed.attodollars() * 1_000_000,
            "{per_token}"
        );
    }
    assert_eq!(
        Rate::from_attodollars(u128::MAX).per_million_decimal(),
        Err(PricingError::Overflow)
    );
    Ok(())
}

#[test]
fn worst_case_liability_is_exact_before_rounding() -> TestResult {
    let snapshot = PriceSnapshot::new(rate("0.00000015")?, rate("0.0000006")?);
    let liability = Liability::worst_case(&snapshot, UnitBounds::new(1_000, 500)?)?;
    assert_eq!(liability.attodollars(), 450_000_000_000_000);
    assert_eq!(liability.reserve(), micros(450));

    let with_fee = snapshot.with(Dimension::Request, rate("0.005")?);
    let liability = Liability::worst_case(&with_fee, UnitBounds::new(1_000, 500)?)?;
    assert_eq!(liability.reserve(), micros(5_450));
    Ok(())
}

#[test]
fn liability_rounds_up_to_the_ledger_micro_unit() -> TestResult {
    let one_atto = Rate::from_attodollars(1);
    for (prompt_tokens, expected) in [
        (0, 0),
        (1, 1),
        (999_999_999_999, 1),
        (1_000_000_000_000, 1),
        (1_000_000_000_001, 2),
        (2_000_000_000_000, 2),
    ] {
        let snapshot = PriceSnapshot::new(one_atto, Rate::ZERO);
        let liability = Liability::worst_case(&snapshot, UnitBounds::new(prompt_tokens, 1)?)?;
        assert_eq!(liability.attodollars(), u128::from(prompt_tokens));
        assert_eq!(liability.reserve(), micros(expected), "{prompt_tokens}");
        assert!(
            u128::from(liability.reserve().micros().unsigned_abs()) * ATTO_PER_MICRO
                >= liability.attodollars()
        );
    }
    Ok(())
}

#[test]
fn reasoning_and_cache_prices_use_the_worst_billing_class() -> TestResult {
    let reasoning = PriceSnapshot::new(Rate::ZERO, rate("0.0000001")?)
        .with(Dimension::InternalReasoning, rate("0.0000003")?);
    let liability = Liability::worst_case(&reasoning, UnitBounds::new(0, 10)?)?;
    assert_eq!(liability.attodollars(), 3_000_000_000_000);
    assert_eq!(liability.reserve(), micros(3));

    let cache_write = PriceSnapshot::new(rate("0.000001")?, Rate::ZERO)
        .with(Dimension::InputCacheWrite, rate("0.00000125")?)
        .with(Dimension::InputCacheRead, rate("0.0000001")?);
    let liability = Liability::worst_case(&cache_write, UnitBounds::new(4, 1)?)?;
    assert_eq!(liability.attodollars(), 5_000_000_000_000);

    let cache_read = PriceSnapshot::new(rate("0.000001")?, Rate::ZERO)
        .with(Dimension::InputCacheRead, rate("0.000002")?);
    let liability = Liability::worst_case(&cache_read, UnitBounds::new(4, 1)?)?;
    assert_eq!(liability.reserve(), micros(8));
    Ok(())
}

#[test]
fn free_routes_have_zero_liability_which_the_budget_refuses() -> TestResult {
    let snapshot = PriceSnapshot::new(Rate::ZERO, Rate::ZERO);
    let liability = Liability::worst_case(&snapshot, UnitBounds::new(100, 100)?)?;
    assert_eq!(liability.reserve(), Usd::ZERO);
    assert_eq!(
        Balance::new(micros(1)).reserve(liability.reserve()),
        Err(BudgetError::ZeroReservation)
    );
    Ok(())
}

#[test]
fn unbounded_dimensions_make_a_route_ineligible() -> TestResult {
    let base = PriceSnapshot::new(rate("0.000001")?, rate("0.000002")?);
    let bounds = UnitBounds::new(10, 10)?;
    for dimension in [Dimension::Image, Dimension::Audio, Dimension::WebSearch] {
        let listed = base.with(dimension, Rate::from_attodollars(1));
        assert_eq!(
            Liability::worst_case(&listed, bounds),
            Err(PricingError::UnboundedDimension(dimension))
        );
        let zero = base.with(dimension, Rate::ZERO);
        assert!(Liability::worst_case(&zero, bounds).is_ok());
    }
    let unknown = base
        .with_unrecognized_charge(Rate::from_attodollars(7))
        .with_unrecognized_charge(Rate::ZERO);
    assert_eq!(
        Liability::worst_case(&unknown, bounds),
        Err(PricingError::UnrecognizedCharge)
    );
    assert!(Liability::worst_case(&base.with_unrecognized_charge(Rate::ZERO), bounds).is_ok());
    assert_eq!(
        UnitBounds::new(10, 0),
        Err(PricingError::ZeroCompletionBound)
    );
    Ok(())
}

#[test]
fn liability_overflow_is_an_error() -> TestResult {
    let max = Rate::from_attodollars(u128::MAX);
    let prompt = PriceSnapshot::new(max, Rate::ZERO);
    assert_eq!(
        Liability::worst_case(&prompt, UnitBounds::new(2, 1)?),
        Err(PricingError::Overflow)
    );
    let sum =
        PriceSnapshot::new(Rate::from_attodollars(1), Rate::ZERO).with(Dimension::Request, max);
    assert_eq!(
        Liability::worst_case(&sum, UnitBounds::new(1, 1)?),
        Err(PricingError::Overflow)
    );
    let ledger = PriceSnapshot::new(rate("1")?, Rate::ZERO);
    assert_eq!(
        Liability::worst_case(&ledger, UnitBounds::new(10_000_000_000_000, 1)?),
        Err(PricingError::Overflow)
    );
    let largest = PriceSnapshot::new(Rate::from_attodollars(ATTO_PER_MICRO), Rate::ZERO);
    let bound = u64::try_from(i64::MAX).map_err(|_| PricingError::Overflow)?;
    assert_eq!(
        Liability::worst_case(&largest, UnitBounds::new(bound, 1)?)?.reserve(),
        micros(i64::MAX)
    );
    Ok(())
}

#[test]
fn reported_costs_parse_exactly_without_floating_point() -> TestResult {
    for (input, attodollars, sub, usd) in [
        ("0", 0, false, 0),
        ("0.0", 0, false, 0),
        ("0e10", 0, false, 0),
        ("0E-5", 0, false, 0),
        ("1", ATTO_PER_DOLLAR, false, 1_000_000),
        ("0.000123456789", 123_456_789_000_000, false, 124),
        ("1.5e-5", 15_000_000_000_000, false, 15),
        ("15E-6", 15_000_000_000_000, false, 15),
        ("0.000001", ATTO_PER_MICRO, false, 1),
        ("1e-18", 1, false, 1),
        ("1e-19", 1, true, 1),
        ("1.5e-18", 2, true, 1),
        ("1e-999", 1, true, 1),
        ("1E+2", 100 * ATTO_PER_DOLLAR, false, 100_000_000),
        ("1e05", 100_000 * ATTO_PER_DOLLAR, false, 100_000_000_000),
        ("12.5000", 12_500_000_000_000_000_000, false, 12_500_000),
        (
            "9223372036854.775807",
            9_223_372_036_854_775_807_000_000_000_000,
            false,
            i64::MAX,
        ),
        ("0.00000000000000000000000000000000000000000001", 1, true, 1),
    ] {
        let cost = ReportedCost::parse(input)?;
        assert_eq!(cost.attodollars(), attodollars, "{input}");
        assert_eq!(cost.sub_attodollar(), sub, "{input}");
        assert_eq!(cost.usd(), micros(usd), "{input}");
    }
    Ok(())
}

#[test]
fn hostile_reported_costs_are_rejected() {
    let precise = format!("1.{}1", "0".repeat(35));
    let long = format!("0.{}1", "0".repeat(MAX_TEXT_BYTES));
    for (input, expected) in [
        ("", PricingError::Empty),
        ("-0", PricingError::Negative),
        ("-1", PricingError::Negative),
        ("-1.5e-5", PricingError::Negative),
        ("--1", PricingError::Negative),
        ("+1", PricingError::InvalidNumber),
        (" 1", PricingError::InvalidNumber),
        ("1 ", PricingError::InvalidNumber),
        ("1,0", PricingError::InvalidNumber),
        ("NaN", PricingError::InvalidNumber),
        ("Infinity", PricingError::InvalidNumber),
        ("inf", PricingError::InvalidNumber),
        ("01", PricingError::InvalidNumber),
        ("1.", PricingError::InvalidNumber),
        (".5", PricingError::InvalidNumber),
        ("1.e5", PricingError::InvalidNumber),
        ("1e", PricingError::InvalidNumber),
        ("1e+", PricingError::InvalidNumber),
        ("1e-", PricingError::InvalidNumber),
        ("1ee5", PricingError::InvalidNumber),
        ("1e5.0", PricingError::InvalidNumber),
        ("1e 5", PricingError::InvalidNumber),
        ("0x1", PricingError::InvalidNumber),
        ("\"0.1\"", PricingError::InvalidNumber),
        ("1e999", PricingError::Overflow),
        ("1e1000000", PricingError::Overflow),
        ("1e-1000000", PricingError::Overflow),
        ("9223372036854.775808", PricingError::Overflow),
        ("9223372036854.7758070000001", PricingError::Overflow),
        ("1e20", PricingError::Overflow),
        (precise.as_str(), PricingError::TooPrecise),
        (long.as_str(), PricingError::TooLong),
    ] {
        assert_eq!(ReportedCost::parse(input), Err(expected), "{input:?}");
    }
}

#[test]
fn reported_cost_rounding_never_undercharges() -> TestResult {
    for input in [
        "0.000000999999",
        "0.0000010000001",
        "3.3e-7",
        "1.000000000001",
    ] {
        let cost = ReportedCost::parse(input)?;
        let reserved = u128::from(cost.usd().micros().unsigned_abs()) * ATTO_PER_MICRO;
        assert!(reserved >= cost.attodollars(), "{input}");
        assert!(reserved - cost.attodollars() < ATTO_PER_MICRO, "{input}");
    }
    Ok(())
}
