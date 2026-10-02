//! Exact USD amounts. Catalog rates and reported costs are parsed from text
//! with the product's `sigy-core` pricing types; nothing passes through binary
//! floating point. Ledger amounts are integer micro-USD.

use sigy_core::pricing::{ATTO_PER_MICRO, Rate, ReportedCost};

use crate::Result;

/// The 2026-10-02 allocation ceiling recorded in the work ledger, USD 20.
pub const CEILING_MICRO: u64 = 20_000_000;
/// This stream's own software cap, kept below the ceiling, USD 18.
pub const SOFTWARE_CAP_MICRO: u64 = 18_000_000;

/// Parse one catalog rate in USD per unit. Digits beyond the 18 fractional
/// digits that `Rate` represents round the rate up by one attodollar, so the
/// result is never below the listed value.
pub fn catalog_rate(text: &str) -> Result<u128> {
    if let Some((whole, fraction)) = text.split_once('.')
        && fraction.len() > 18
    {
        if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("invalid catalog rate".into());
        }
        let (kept, rest) = fraction.split_at(18);
        let base = Rate::parse(&format!("{whole}.{kept}"))?.attodollars();
        let extra = u128::from(rest.bytes().any(|byte| byte != b'0'));
        return base
            .checked_add(extra)
            .ok_or_else(|| "catalog rate overflow".into());
    }
    Ok(Rate::parse(text)?.attodollars())
}

/// Round an attodollar amount up to the ledger micro-unit.
pub fn ceil_micro(attodollars: u128) -> Result<u64> {
    Ok(u64::try_from(attodollars.div_ceil(ATTO_PER_MICRO))?)
}

/// Raise a rate by `percent` (for example 5) and round up to one attodollar.
pub fn with_headroom(attodollars: u128, percent: u32) -> Result<u128> {
    let scaled = attodollars
        .checked_mul(u128::from(100 + percent))
        .ok_or("headroom overflow")?;
    Ok(scaled.div_ceil(100))
}

/// Express a per-token rate in USD per million tokens as exact decimal text.
pub fn per_million(attodollars: u128) -> Result<String> {
    Ok(Rate::from_attodollars(attodollars).per_million_decimal()?)
}

/// Parse a provider-reported cost from its raw JSON number text.
pub fn reported(raw: &str) -> Result<(u64, u128)> {
    let cost = ReportedCost::parse(raw)?;
    Ok((u64::try_from(cost.usd().micros())?, cost.attodollars()))
}

/// Display micro-USD as a fixed six-decimal USD amount.
#[must_use]
pub fn usd(micro: u64) -> String {
    format!("{}.{:06}", micro / 1_000_000, micro % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_rates_are_exact_or_rounded_up() -> Result<()> {
        assert_eq!(catalog_rate("0.000002")?, 2_000_000_000_000);
        assert_eq!(catalog_rate("0")?, 0);
        // Twenty-two fractional digits, as served for one cache-write rate.
        assert_eq!(catalog_rate("0.0000000416666666666667")?, 41_666_666_667);
        assert_eq!(catalog_rate("0.0000000000000000010000")?, 1);
        for hostile in [
            "-1",
            "-0",
            "1e-6",
            " 0.1",
            "0.1 ",
            "1,0",
            "",
            ".",
            "0.1x00000000000000000000",
            "0.00000000000000000٣00",
        ] {
            assert!(catalog_rate(hostile).is_err(), "{hostile}");
        }
        Ok(())
    }

    #[test]
    fn rounding_headroom_and_display() -> Result<()> {
        assert_eq!(ceil_micro(0)?, 0);
        assert_eq!(ceil_micro(1)?, 1);
        assert_eq!(ceil_micro(ATTO_PER_MICRO)?, 1);
        assert_eq!(ceil_micro(ATTO_PER_MICRO + 1)?, 2);
        assert!(ceil_micro(u128::MAX).is_err());
        assert_eq!(with_headroom(100, 5)?, 105);
        assert_eq!(with_headroom(1, 5)?, 2);
        assert!(with_headroom(u128::MAX, 5).is_err());
        assert_eq!(per_million(2_200_000_000_000)?, "2.2");
        assert_eq!(usd(1_234_567), "1.234567");
        assert_eq!(usd(5), "0.000005");
        Ok(())
    }

    #[test]
    fn reported_costs_round_up_without_floating_point() -> Result<()> {
        assert_eq!(reported("0.0012345")?.0, 1235);
        assert_eq!(reported("1.5e-5")?.0, 15);
        assert_eq!(reported("0")?.0, 0);
        for hostile in ["-0.1", "NaN", "\"0.1\"", "1e9999999"] {
            assert!(reported(hostile).is_err(), "{hostile}");
        }
        Ok(())
    }
}
