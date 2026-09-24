//! Paid attempt rows. Compiled only for tests until a real transport, a
//! supervisor, and the operation 27 gate exist.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sigy_core::money::Usd;

use crate::{
    Error, Result,
    storage::{
        Store,
        ledger::{Admission, reserve_in},
        validate_key,
    },
};

/// The immutable contract of one attempt. The prompt text is identified by its hash only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttemptRow {
    pub(crate) request_id: String,
    pub(crate) route_id: String,
    pub(crate) snapshot_id: String,
    pub(crate) source_language: String,
    pub(crate) target_language: String,
    pub(crate) input_sha256: String,
    pub(crate) prompt_tokens: u32,
    pub(crate) completion_tokens: u32,
    pub(crate) retry_of: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptOutcome {
    Completed,
    UsageMissing,
    Timeout,
    Refused,
    RetryAfter,
    Failed,
}

impl AttemptOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::UsageMissing => "usage_missing",
            Self::Timeout => "timeout",
            Self::Refused => "refused",
            Self::RetryAfter => "retry_after",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "completed" => Self::Completed,
            "usage_missing" => Self::UsageMissing,
            "timeout" => Self::Timeout,
            "refused" => Self::Refused,
            "retry_after" => Self::RetryAfter,
            "failed" => Self::Failed,
            _ => return Err(Error::LedgerIntegrity),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttemptRecord {
    pub(crate) row: AttemptRow,
    pub(crate) outcome: Option<AttemptOutcome>,
    pub(crate) generation_id: Option<String>,
    pub(crate) retry_after_ms: Option<i64>,
    pub(crate) outcome_ms: Option<i64>,
}

impl Store {
    /// Reserves the liability and records the attempt contract in one transaction.
    /// A replay with the same contract returns the existing admission.
    /// # Errors
    /// Refuses budget exhaustion, frozen or disabled scopes, and a changed contract.
    pub(crate) fn admit_provider_attempt(
        &mut self,
        row: &AttemptRow,
        context: &str,
        maximum: Usd,
        budgets: &[&str],
        now_ms: i64,
    ) -> Result<Admission> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let admission = reserve_in(&tx, &row.request_id, context, maximum, budgets)?;
        if admission.newly_reserved {
            tx.execute(
                "INSERT INTO provider_attempts(request_id, route_id, snapshot_id, source_language, target_language, input_sha256, prompt_tokens, completion_tokens, retry_of, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    row.request_id, row.route_id, row.snapshot_id, row.source_language,
                    row.target_language, row.input_sha256, row.prompt_tokens,
                    row.completion_tokens, row.retry_of, now_ms
                ],
            )?;
        } else if read_attempt(&tx, &row.request_id)?.map(|record| record.row) != Some(row.clone())
        {
            return Err(Error::IdempotencyConflict);
        }
        tx.commit()?;
        Ok(admission)
    }

    /// # Errors
    /// Returns catalog or integrity errors.
    pub(crate) fn provider_attempt(&self, request_id: &str) -> Result<Option<AttemptRecord>> {
        validate_key(request_id, "request ID")?;
        read_attempt(&self.connection, request_id)
    }

    /// Whether any attempt already retries `request_id`.
    /// # Errors
    /// Returns catalog errors.
    pub(crate) fn provider_retry_exists(&self, request_id: &str) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM provider_attempts WHERE retry_of = ?1",
                [request_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Writes the transport outcome once. It never changes the ledger.
    /// # Errors
    /// Refuses a second outcome or an unknown attempt.
    pub(crate) fn record_attempt_outcome(
        &mut self,
        request_id: &str,
        outcome: AttemptOutcome,
        generation_id: Option<&str>,
        retry_after_ms: Option<i64>,
        now_ms: i64,
    ) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE provider_attempts SET outcome = ?1, generation_id = ?2, retry_after_ms = ?3, outcome_ms = ?4 WHERE request_id = ?5 AND outcome IS NULL",
            params![outcome.as_str(), generation_id, retry_after_ms, now_ms, request_id],
        )?;
        if changed != 1 {
            return Err(Error::RequestState);
        }
        Ok(())
    }
}

fn read_attempt(connection: &Connection, request_id: &str) -> Result<Option<AttemptRecord>> {
    let record = connection
        .query_row(
            "SELECT route_id, snapshot_id, source_language, target_language, input_sha256, prompt_tokens, completion_tokens, retry_of, outcome, generation_id, retry_after_ms, outcome_ms FROM provider_attempts WHERE request_id = ?1",
            [request_id],
            |row| {
                Ok((
                    AttemptRow {
                        request_id: request_id.to_owned(),
                        route_id: row.get(0)?,
                        snapshot_id: row.get(1)?,
                        source_language: row.get(2)?,
                        target_language: row.get(3)?,
                        input_sha256: row.get(4)?,
                        prompt_tokens: row.get(5)?,
                        completion_tokens: row.get(6)?,
                        retry_of: row.get(7)?,
                    },
                    row.get::<_, Option<String>>(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                ))
            },
        )
        .optional()?;
    record
        .map(
            |(row, outcome, generation_id, retry_after_ms, outcome_ms)| {
                Ok(AttemptRecord {
                    row,
                    outcome: outcome.as_deref().map(AttemptOutcome::parse).transpose()?,
                    generation_id,
                    retry_after_ms,
                    outcome_ms,
                })
            },
        )
        .transpose()
}
