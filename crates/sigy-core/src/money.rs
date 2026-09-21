//! Exact USD amounts for reservation accounting. Provider bounds must round up.

use std::{fmt, str::FromStr};

/// Nonnegative USD micro-units, bounded to a signed 64-bit storage integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Usd(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyError {
    Negative,
    Overflow,
    InvalidDecimal,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Negative => "USD amount cannot be negative",
            Self::Overflow => "USD amount exceeds the accounting range",
            Self::InvalidDecimal => "expected unsigned USD with at most six decimal places",
        })
    }
}

impl std::error::Error for MoneyError {}

impl Usd {
    pub const ZERO: Self = Self(0);
    pub const MICROS_PER_DOLLAR: i64 = 1_000_000;

    /// # Errors
    /// Rejects negative amounts.
    pub const fn from_micros(micros: i64) -> Result<Self, MoneyError> {
        if micros < 0 {
            Err(MoneyError::Negative)
        } else {
            Ok(Self(micros))
        }
    }

    #[must_use]
    pub const fn micros(self) -> i64 {
        self.0
    }

    /// # Errors
    /// Rejects arithmetic overflow instead of wrapping the balance.
    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(MoneyError::Overflow)
    }

    /// # Errors
    /// Rejects subtraction that would produce a negative balance.
    pub const fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        if self.0 < other.0 {
            Err(MoneyError::Negative)
        } else {
            Ok(Self(self.0 - other.0))
        }
    }
}

impl FromStr for Usd {
    type Err = MoneyError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (whole, fractional) = input.split_once('.').unwrap_or((input, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || fractional.len() > 6
            || !fractional.bytes().all(|b| b.is_ascii_digit())
            || (input.contains('.') && fractional.is_empty())
        {
            return Err(MoneyError::InvalidDecimal);
        }
        let dollars = whole.parse::<i64>().map_err(|_| MoneyError::Overflow)?;
        let mut fraction = 0_i64;
        for digit in fractional.bytes() {
            fraction = fraction * 10 + i64::from(digit - b'0');
        }
        for _ in fractional.len()..6 {
            fraction *= 10;
        }
        dollars
            .checked_mul(Self::MICROS_PER_DOLLAR)
            .and_then(|value| value.checked_add(fraction))
            .map(Self)
            .ok_or(MoneyError::Overflow)
    }
}

impl fmt::Display for Usd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{:06}",
            self.0 / Self::MICROS_PER_DOLLAR,
            self.0 % Self::MICROS_PER_DOLLAR
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_roundtrips_are_exact() -> Result<(), MoneyError> {
        for (input, micros) in [
            ("0", 0),
            ("10", 10_000_000),
            ("0.000001", 1),
            ("3.2", 3_200_000),
            ("9223372036854.775807", i64::MAX),
        ] {
            let value: Usd = input.parse()?;
            assert_eq!(value.micros(), micros);
            assert_eq!(value.to_string().parse::<Usd>()?, value);
        }
        Ok(())
    }

    #[test]
    fn malformed_or_inexact_amounts_are_rejected() {
        for input in [
            "",
            "-1",
            "+1",
            " 1",
            "1 ",
            "1.",
            ".1",
            "NaN",
            "1e2",
            "1.0000001",
            "1.2.3",
            "１",
            "9223372036854.775808",
        ] {
            assert!(input.parse::<Usd>().is_err(), "accepted {input}");
        }
    }

    #[test]
    fn arithmetic_never_wraps_or_goes_negative() -> Result<(), MoneyError> {
        let one = Usd::from_micros(1)?;
        assert_eq!(Usd::ZERO.checked_sub(one), Err(MoneyError::Negative));
        assert_eq!(
            Usd::from_micros(i64::MAX)?.checked_add(one),
            Err(MoneyError::Overflow)
        );
        assert!(Usd::from_micros(-1).is_err());
        Ok(())
    }
}
