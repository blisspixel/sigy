//! Pure accounting transitions. Durable request identity belongs to the ledger.

use std::fmt;

use crate::money::{MoneyError, Usd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Balance {
    limit: Usd,
    settled: Usd,
    reserved: Usd,
    frozen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetError {
    Disabled,
    Frozen,
    InsufficientFunds,
    ZeroReservation,
    InvalidBalance,
    Arithmetic(MoneyError),
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => f.write_str("paid processing is disabled by a zero budget"),
            Self::Frozen => f.write_str("paid processing is frozen after a billing breach"),
            Self::InsufficientFunds => {
                f.write_str("maximum request liability exceeds the remaining budget")
            }
            Self::ZeroReservation => {
                f.write_str("a paid request needs a positive maximum liability")
            }
            Self::InvalidBalance => f.write_str("invalid accounting balance"),
            Self::Arithmetic(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BudgetError {}

impl From<MoneyError> for BudgetError {
    fn from(value: MoneyError) -> Self {
        Self::Arithmetic(value)
    }
}

impl Balance {
    #[must_use]
    pub const fn new(limit: Usd) -> Self {
        Self {
            limit,
            settled: Usd::ZERO,
            reserved: Usd::ZERO,
            frozen: false,
        }
    }

    /// # Errors
    /// Rejects overflow and overcommitted balances not marked frozen.
    pub fn restore(
        limit: Usd,
        settled: Usd,
        reserved: Usd,
        frozen: bool,
    ) -> Result<Self, BudgetError> {
        let committed = settled.checked_add(reserved)?;
        if committed > limit && !frozen {
            return Err(BudgetError::InvalidBalance);
        }
        Ok(Self {
            limit,
            settled,
            reserved,
            frozen,
        })
    }

    #[must_use]
    pub const fn limit(self) -> Usd {
        self.limit
    }
    #[must_use]
    pub const fn settled(self) -> Usd {
        self.settled
    }
    #[must_use]
    pub const fn reserved(self) -> Usd {
        self.reserved
    }
    #[must_use]
    pub const fn frozen(self) -> bool {
        self.frozen
    }

    #[must_use]
    pub fn available(self) -> Usd {
        self.limit
            .checked_sub(self.settled)
            .and_then(|rest| rest.checked_sub(self.reserved))
            .unwrap_or(Usd::ZERO)
    }

    /// # Errors
    /// Rejects frozen, disabled, zero-liability, or unaffordable requests.
    pub fn reserve(self, maximum: Usd) -> Result<Self, BudgetError> {
        if self.frozen {
            return Err(BudgetError::Frozen);
        }
        if self.limit == Usd::ZERO {
            return Err(BudgetError::Disabled);
        }
        if maximum == Usd::ZERO {
            return Err(BudgetError::ZeroReservation);
        }
        if maximum > self.available() {
            return Err(BudgetError::InsufficientFunds);
        }
        Ok(Self {
            reserved: self.reserved.checked_add(maximum)?,
            ..self
        })
    }

    /// # Errors
    /// Rejects release beyond the outstanding reservation total.
    pub fn release(self, maximum: Usd) -> Result<Self, BudgetError> {
        Ok(Self {
            reserved: self.reserved.checked_sub(maximum)?,
            ..self
        })
    }

    /// Accounts for actual charges, freezing admission when the bound was broken.
    /// # Errors
    /// Rejects invalid reservation totals or unrepresentable amounts.
    pub fn settle(self, maximum: Usd, actual: Usd) -> Result<Self, BudgetError> {
        let reserved = self.reserved.checked_sub(maximum)?;
        let settled = self.settled.checked_add(actual)?;
        let committed = settled.checked_add(reserved)?;
        Ok(Self {
            reserved,
            settled,
            frozen: self.frozen || actual > maximum || committed > self.limit,
            ..self
        })
    }

    /// # Errors
    /// A limit change cannot discard existing commitments or thaw a frozen ledger.
    pub fn with_limit(self, limit: Usd) -> Result<Self, BudgetError> {
        if self.settled.checked_add(self.reserved)? > limit {
            return Err(BudgetError::InsufficientFunds);
        }
        Ok(Self { limit, ..self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_maximum_before_charge() -> Result<(), Box<dyn std::error::Error>> {
        let balance = Balance::restore("5".parse()?, "3.2".parse()?, "1".parse()?, false)?;
        assert_eq!(balance.available(), "0.8".parse()?);
        let admitted = balance.reserve("0.6".parse()?)?;
        assert_eq!(
            admitted.reserve("0.6".parse()?),
            Err(BudgetError::InsufficientFunds)
        );
        let settled = admitted.settle("0.6".parse()?, "0.4".parse()?)?;
        assert_eq!(settled.available(), "0.4".parse()?);
        assert_eq!(settled.reserved(), "1".parse()?);
        Ok(())
    }

    #[test]
    fn breaches_preserve_actual_charge_and_freeze() -> Result<(), Box<dyn std::error::Error>> {
        let balance = Balance::new("1".parse()?).reserve("0.5".parse()?)?;
        let breached = balance.settle("0.5".parse()?, "1.5".parse()?)?;
        assert_eq!(breached.settled(), "1.5".parse()?);
        assert_eq!(breached.available(), Usd::ZERO);
        assert!(breached.frozen());
        assert!(breached.with_limit("10".parse()?)?.frozen());
        assert_eq!(breached.reserve("0.1".parse()?), Err(BudgetError::Frozen));
        Ok(())
    }

    #[test]
    fn disabled_and_invalid_balances_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            Balance::new(Usd::ZERO).reserve("0.1".parse()?),
            Err(BudgetError::Disabled)
        );
        assert_eq!(
            Balance::new("1".parse()?).reserve(Usd::ZERO),
            Err(BudgetError::ZeroReservation)
        );
        assert!(Balance::restore("1".parse()?, "0.5".parse()?, "0.6".parse()?, false).is_err());
        assert!(Balance::new("1".parse()?).release("0.1".parse()?).is_err());
        Ok(())
    }
}
