//! Durable lifetime budgets. Every admission includes the global budget.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sigy_core::{budget::Balance, money::Usd};

use super::{Store, now_ms, validate_key};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    Reserved,
    Submitted,
    Uncertain,
    Settled,
    Released,
}

impl RequestState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Submitted => "submitted",
            Self::Uncertain => "uncertain",
            Self::Settled => "settled",
            Self::Released => "released",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "submitted" => Ok(Self::Submitted),
            "uncertain" => Ok(Self::Uncertain),
            "settled" => Ok(Self::Settled),
            "released" => Ok(Self::Released),
            _ => Err(Error::LedgerIntegrity),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub id: String,
    pub context: String,
    pub maximum: Usd,
    pub state: RequestState,
    pub actual: Option<Usd>,
    pub evidence: Option<String>,
    pub budgets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub reservation: Reservation,
    pub newly_reserved: bool,
}

/// Only an applied submission transition permits crossing the network boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Applied,
    Unchanged,
}

impl Store {
    /// Sets an explicit lifetime limit without discarding liabilities or thawing breaches.
    /// # Errors
    /// Rejects invalid IDs, an excessive scope count, or a limit below commitments.
    pub fn set_budget_limit(&mut self, id: &str, limit: Usd) -> Result<()> {
        validate_key(id, "budget ID")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = tx
            .query_row("SELECT 1 FROM budgets WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some();
        if exists {
            let balance = read_balance(&tx, id)?.with_limit(limit)?;
            write_balance(&tx, id, balance)?;
        } else {
            let count: i64 = tx.query_row("SELECT count(*) FROM budgets", [], |row| row.get(0))?;
            if count >= 256 {
                return Err(Error::InvalidInput("budget scope count"));
            }
            tx.execute(
                "INSERT INTO budgets(id, limit_micros) VALUES (?1, ?2)",
                params![id, limit.micros()],
            )?;
        }
        tx.execute("INSERT INTO ledger_events(budget_id, kind, amount_micros, recorded_ms) VALUES (?1, 'limit', ?2, ?3)", params![id, limit.micros(), now_ms()?])?;
        tx.commit()?;
        Ok(())
    }

    /// # Errors
    /// Returns missing-record, catalog, or balance-integrity errors.
    pub fn budget(&self, id: &str) -> Result<Balance> {
        read_balance(&self.connection, id)
    }

    /// # Errors
    /// Fails on unreadable or inconsistent budget records.
    pub fn budgets(&self) -> Result<Vec<(String, Balance)>> {
        let mut query = self
            .connection
            .prepare("SELECT id FROM budgets ORDER BY id LIMIT 257")?;
        let ids = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if ids.len() > 256 {
            return Err(Error::LedgerIntegrity);
        }
        ids.into_iter()
            .map(|id| {
                let balance = self.budget(&id)?;
                Ok((id, balance))
            })
            .collect()
    }

    /// Atomically reserves the maximum against global and every requested scope.
    /// `context` identifies the immutable operation/model/route contract, not prompt text.
    /// # Errors
    /// Rejects missing scopes, budget exhaustion, invalid keys, or conflicting retries.
    pub fn reserve(
        &mut self,
        id: &str,
        context: &str,
        maximum: Usd,
        additional_budgets: &[&str],
    ) -> Result<Admission> {
        validate_key(id, "request ID")?;
        validate_key(context, "request context")?;
        if additional_budgets.len() > 15 {
            return Err(Error::InvalidInput("request scope count"));
        }
        let mut budgets = BTreeSet::from(["global"]);
        for id in additional_budgets {
            validate_key(id, "budget ID")?;
            budgets.insert(*id);
        }
        let budgets: Vec<String> = budgets.into_iter().map(str::to_owned).collect();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = read_reservation(&tx, id)? {
            if existing.context != context
                || existing.maximum != maximum
                || existing.budgets != budgets
            {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(Admission {
                reservation: existing,
                newly_reserved: false,
            });
        }
        for scope in &budgets {
            let balance = read_balance(&tx, scope)?.reserve(maximum)?;
            write_balance(&tx, scope, balance)?;
        }
        tx.execute("INSERT INTO requests(id, context, maximum_micros, state, created_ms) VALUES (?1, ?2, ?3, 'reserved', ?4)", params![id, context, maximum.micros(), now_ms()?])?;
        for scope in &budgets {
            tx.execute(
                "INSERT INTO request_budgets(request_id, budget_id) VALUES (?1, ?2)",
                params![id, scope],
            )?;
        }
        event(&tx, id, "reserved", Some(maximum))?;
        tx.commit()?;
        Ok(Admission {
            reservation: Reservation {
                id: id.to_owned(),
                context: context.to_owned(),
                maximum,
                state: RequestState::Reserved,
                actual: None,
                evidence: None,
                budgets,
            },
            newly_reserved: true,
        })
    }

    /// Persist this transition before sending. An unchanged result is not permission to resend.
    /// # Errors
    /// Fails for missing requests, terminal/uncertain states, or catalog errors.
    pub fn mark_submitted(&mut self, id: &str) -> Result<Change> {
        self.transition_request(id, RequestState::Reserved, RequestState::Submitted)
    }

    /// Retains all reservations after a potentially billable failure.
    /// # Errors
    /// Fails unless the request is submitted or already uncertain.
    pub fn mark_uncertain(&mut self, id: &str) -> Result<Change> {
        self.transition_request(id, RequestState::Submitted, RequestState::Uncertain)
    }

    fn transition_request(
        &mut self,
        id: &str,
        from: RequestState,
        to: RequestState,
    ) -> Result<Change> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request = read_reservation(&tx, id)?.ok_or(Error::NotFound)?;
        if request.state == to {
            return Ok(Change::Unchanged);
        }
        if request.state != from {
            return Err(Error::RequestState);
        }
        tx.execute(
            "UPDATE requests SET state = ?1 WHERE id = ?2",
            params![to.as_str(), id],
        )?;
        event(&tx, id, to.as_str(), None)?;
        tx.commit()?;
        Ok(Change::Applied)
    }

    /// Releases only a request proven never submitted by its durable state.
    /// # Errors
    /// Submitted or uncertain requests cannot be made free by cancellation.
    pub fn release_unsubmitted(&mut self, id: &str) -> Result<Change> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request = read_reservation(&tx, id)?.ok_or(Error::NotFound)?;
        if request.state == RequestState::Released {
            return Ok(Change::Unchanged);
        }
        if request.state != RequestState::Reserved {
            return Err(Error::RequestState);
        }
        for scope in &request.budgets {
            let balance = read_balance(&tx, scope)?.release(request.maximum)?;
            write_balance(&tx, scope, balance)?;
        }
        tx.execute("UPDATE requests SET state = 'released' WHERE id = ?1", [id])?;
        event(&tx, id, "released", Some(request.maximum))?;
        tx.commit()?;
        Ok(Change::Applied)
    }

    /// Reconciles authoritative usage exactly once and freezes breached scopes.
    /// # Errors
    /// Rejects conflicting reconciliation, non-submitted requests, or invalid evidence IDs.
    pub fn settle(&mut self, id: &str, actual: Usd, evidence: &str) -> Result<Change> {
        validate_key(evidence, "billing evidence ID")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request = read_reservation(&tx, id)?.ok_or(Error::NotFound)?;
        if request.state == RequestState::Settled {
            return if request.actual == Some(actual)
                && request.evidence.as_deref() == Some(evidence)
            {
                Ok(Change::Unchanged)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        if !matches!(
            request.state,
            RequestState::Submitted | RequestState::Uncertain
        ) {
            return Err(Error::RequestState);
        }
        for scope in &request.budgets {
            let balance = read_balance(&tx, scope)?.settle(request.maximum, actual)?;
            write_balance(&tx, scope, balance)?;
        }
        tx.execute("UPDATE requests SET state = 'settled', actual_micros = ?1, evidence = ?2 WHERE id = ?3", params![actual.micros(), evidence, id])?;
        event(&tx, id, "settled", Some(actual))?;
        tx.commit()?;
        Ok(Change::Applied)
    }

    /// Recovers at most 256 potentially dispatched requests without freeing liability.
    /// Call until zero before enabling provider dispatch after exclusive service startup.
    /// # Errors
    /// Fails on catalog errors; never retries remote work.
    pub fn recover_submitted(&mut self) -> Result<usize> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = {
            let mut query = tx.prepare(
                "SELECT id FROM requests WHERE state = 'submitted' ORDER BY id LIMIT 256",
            )?;
            query
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for id in &ids {
            tx.execute(
                "UPDATE requests SET state = 'uncertain' WHERE id = ?1",
                [id],
            )?;
            event(&tx, id, "uncertain", None)?;
        }
        tx.commit()?;
        Ok(ids.len())
    }

    /// # Errors
    /// Returns catalog or integrity errors; an absent request returns `None`.
    pub fn reservation(&self, id: &str) -> Result<Option<Reservation>> {
        read_reservation(&self.connection, id)
    }

    /// Verifies projections against durable request commitments in one read snapshot.
    /// # Errors
    /// Fails closed when a request lacks global accounting or totals disagree.
    pub fn audit_ledger(&self) -> Result<()> {
        let inconsistent: i64 = self.connection.query_row(
            "SELECT (SELECT count(*) FROM requests r WHERE NOT EXISTS
                (SELECT 1 FROM request_budgets rb WHERE rb.request_id = r.id AND rb.budget_id = 'global'))
             + (SELECT count(*) FROM budgets b WHERE
                b.reserved_micros != COALESCE((SELECT sum(r.maximum_micros) FROM requests r JOIN request_budgets rb ON r.id = rb.request_id WHERE rb.budget_id = b.id AND r.state IN ('reserved', 'submitted', 'uncertain')), 0)
                OR b.settled_micros != COALESCE((SELECT sum(r.actual_micros) FROM requests r JOIN request_budgets rb ON r.id = rb.request_id WHERE rb.budget_id = b.id AND r.state = 'settled'), 0))
             + (SELECT CASE WHEN count(*) = 1 THEN 0 ELSE 1 END FROM budgets WHERE id = 'global')",
            [], |row| row.get(0))?;
        if inconsistent != 0 {
            return Err(Error::LedgerIntegrity);
        }
        Ok(())
    }
}

fn read_balance(connection: &Connection, id: &str) -> Result<Balance> {
    let values = connection.query_row("SELECT limit_micros, settled_micros, reserved_micros, frozen FROM budgets WHERE id = ?1", [id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, bool>(3)?))
    }).optional()?.ok_or(Error::NotFound)?;
    Ok(Balance::restore(
        Usd::from_micros(values.0)?,
        Usd::from_micros(values.1)?,
        Usd::from_micros(values.2)?,
        values.3,
    )?)
}

fn write_balance(connection: &Connection, id: &str, balance: Balance) -> Result<()> {
    let changed = connection.execute("UPDATE budgets SET limit_micros = ?1, settled_micros = ?2, reserved_micros = ?3, frozen = ?4 WHERE id = ?5", params![balance.limit().micros(), balance.settled().micros(), balance.reserved().micros(), balance.frozen(), id])?;
    if changed != 1 {
        return Err(Error::LedgerIntegrity);
    }
    Ok(())
}

fn read_reservation(connection: &Connection, id: &str) -> Result<Option<Reservation>> {
    let values = connection.query_row("SELECT context, maximum_micros, state, actual_micros, evidence FROM requests WHERE id = ?1", [id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<i64>>(3)?, row.get::<_, Option<String>>(4)?))
    }).optional()?;
    let Some((context, maximum, state, actual, evidence)) = values else {
        return Ok(None);
    };
    let mut query = connection.prepare(
        "SELECT budget_id FROM request_budgets WHERE request_id = ?1 ORDER BY budget_id LIMIT 17",
    )?;
    let budgets = query
        .query_map([id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if budgets.is_empty() || budgets.len() > 16 || !budgets.iter().any(|id| id == "global") {
        return Err(Error::LedgerIntegrity);
    }
    Ok(Some(Reservation {
        id: id.to_owned(),
        context,
        maximum: Usd::from_micros(maximum)?,
        state: RequestState::parse(&state)?,
        actual: actual.map(Usd::from_micros).transpose()?,
        evidence,
        budgets,
    }))
}

fn event(connection: &Connection, id: &str, kind: &str, amount: Option<Usd>) -> Result<()> {
    connection.execute("INSERT INTO ledger_events(request_id, kind, amount_micros, recorded_ms) VALUES (?1, ?2, ?3, ?4)", params![id, kind, amount.map(Usd::micros), now_ms()?])?;
    Ok(())
}
