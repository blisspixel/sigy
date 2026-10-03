//! Atomic owner withdrawal and durable native stop liability.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Store, job_pool, validate_key};
use crate::task::withdrawal::{TaskWithdrawal, TaskWithdrawalView, WithdrawnInterest};
use crate::{Error, Result, recognition::sha256_hex};

const SEMANTICS: &str = "task-interest-withdrawal-v1";
const MAX_OWNER_ROWS: usize = 64;

fn digest(json: &str) -> String {
    sha256_hex(format!("sigy-task-interest-withdrawal-v1:{json}").as_bytes())
}

pub(super) fn fenced(connection: &Connection, task: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_interest_withdrawals WHERE task_id = ?1)",
        [task],
        |row| row.get(0),
    )?)
}

/// Index-prefix probes stop at the first permanent authority. Task scanning is bounded;
/// an incomplete remaining-owner decision refuses the whole mutation.
fn other_authority(
    connection: &Connection,
    family: &str,
    job: &str,
    task: &str,
) -> Result<&'static str> {
    let permanent: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_interest_legacy_guards WHERE family = ?1 AND job_id = ?2)",
        params![family, job],
        |row| row.get(0),
    )?;
    if permanent {
        return Ok("legacy-preserved");
    }
    for authority in ["direct", "monitor"] {
        let present: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM job_interests WHERE family = ?1 AND job_id = ?2 AND authority = ?3)", params![family, job, authority], |row| row.get(0))?;
        if present {
            return Ok("shared-preserved");
        }
    }
    let mut statement = connection.prepare("SELECT owner_id FROM job_interests WHERE family = ?1 AND job_id = ?2 AND authority = 'task' ORDER BY owner_id LIMIT 65")?;
    let mut rows = statement.query(params![family, job])?;
    let mut examined = 0;
    while let Some(row) = rows.next()? {
        examined += 1;
        if examined > MAX_OWNER_ROWS {
            return Err(Error::Analysis("remaining-authority-unproven"));
        }
        let owner: String = row.get(0)?;
        if owner != task {
            let withdrawn: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM job_interest_withdrawals WHERE family = ?1 AND job_id = ?2 AND authority = 'task' AND owner_id = ?3)", params![family, job, owner], |row| row.get(0))?;
            if !withdrawn {
                return Ok("shared-preserved");
            }
        }
    }
    Ok("unshared")
}

fn table(family: &str) -> Result<&'static str> {
    match family {
        "recognition" => Ok("analysis_jobs"),
        "translation" => Ok("translation_jobs"),
        _ => Err(Error::StorageIntegrity),
    }
}

pub(in crate::storage) fn attachable(
    connection: &Connection,
    family: &str,
    job: &str,
) -> Result<()> {
    let table = table(family)?;
    let state: Option<String> = connection
        .query_row(
            &format!("SELECT state FROM {table} WHERE id = ?1"),
            [job],
            |row| row.get(0),
        )
        .optional()?;
    if state
        .as_deref()
        .is_some_and(|state| !matches!(state, "queued" | "running" | "succeeded"))
    {
        return Err(Error::Analysis("job-not-attachable"));
    }
    Ok(())
}

fn withdraw_job(
    connection: &Connection,
    receipt: &TaskWithdrawal,
    family: &str,
    job: &str,
    interest_created_ms: i64,
) -> Result<WithdrawnInterest> {
    let table = table(family)?;
    let (state, generation, attempt, owner): (String, u32, u32, Option<String>) = connection
        .query_row(
            &format!("SELECT state, generation, attempt, lease_owner FROM {table} WHERE id = ?1"),
            [job],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    let remaining = other_authority(connection, family, job, &receipt.task_id)?;
    let decision = if remaining != "unshared" {
        remaining
    } else if state == "queued" {
        let family = if family == "recognition" {
            job_pool::Family::Analysis
        } else {
            job_pool::Family::Translation
        };
        if !job_pool::end_queued(
            connection,
            family,
            job,
            "cancelled",
            "task-interest-withdrawn",
            receipt.created_ms,
        )? {
            return Err(Error::StorageIntegrity);
        }
        "queued-cancelled"
    } else if matches!(state.as_str(), "running" | "cancelling") {
        let owner = owner.ok_or(Error::StorageIntegrity)?;
        connection.execute("INSERT INTO native_stop_targets(family, job_id, generation, attempt, lease_owner, task_id, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![family, job, generation, attempt, owner, receipt.task_id, receipt.created_ms])?;
        let changed = connection.execute(&format!("UPDATE {table} SET state = 'cancelling' WHERE id = ?1 AND generation = ?2 AND state IN ('running', 'cancelling')"), params![job, generation])?;
        if changed != 1 {
            return Err(Error::StorageIntegrity);
        }
        "running-cancelling"
    } else {
        "terminal-preserved"
    };
    connection.execute("INSERT INTO job_interest_withdrawals(family, job_id, authority, owner_id, interest_created_ms) VALUES (?1, ?2, 'task', ?3, ?4)", params![family, job, receipt.task_id, interest_created_ms])?;
    Ok(WithdrawnInterest {
        family: family.into(),
        job_id: job.into(),
        interest_created_ms,
        job_generation: generation,
        decision: decision.into(),
    })
}

impl Store {
    /// Withdraw the task's own admitted interests; old admission cancellation is unchanged.
    pub(crate) fn withdraw_task_processing(
        &mut self,
        task: &str,
        request: &str,
        expected_processing_generation: u32,
        expected_withdrawal_generation: u32,
        now: i64,
    ) -> Result<TaskWithdrawalView> {
        validate_key(task, "task ID")?;
        validate_key(request, "withdrawal request")?;
        if let Some(view) = self.task_withdrawal(task)? {
            return if view.receipt.request_id == request
                && view.receipt.expected_processing_generation == expected_processing_generation
                && expected_withdrawal_generation == 0
            {
                Ok(view)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        if expected_withdrawal_generation != 0 {
            return Err(Error::IdempotencyConflict);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let view = super::tasks::processing::withdrawal_context(&tx, task)?;
        if view.generation != expected_processing_generation {
            return Err(Error::IdempotencyConflict);
        }
        if now < super::tasks::collection::latest_task_ms(&tx, task)? {
            return Err(Error::InvalidInput("task withdrawal clock"));
        }
        let mut receipt = TaskWithdrawal {
            task_id: task.into(),
            request_id: request.into(),
            semantics: SEMANTICS.into(),
            grant_sha256: view.grant_sha256,
            expected_processing_generation,
            withdrawal_generation: 1,
            step_mask: 0,
            created_ms: now,
            interests: Vec::new(),
        };
        for step in &view.steps {
            let job = &step.job_id;
            receipt.step_mask |= 1 << (step.ordinal * 2 + u32::from(step.stage == "translation"));
            receipt.interests.push(withdraw_job(
                &tx,
                &receipt,
                &step.stage,
                job,
                step.created_ms,
            )?);
        }
        let json = serde_json::to_string(&receipt)?;
        if json.len() > 16_384 {
            return Err(Error::StorageIntegrity);
        }
        tx.execute("INSERT INTO task_interest_withdrawals(task_id, request_id, expected_processing_generation, payload_json, receipt_sha256, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![task, request, expected_processing_generation, json, digest(&json), now])?;
        tx.commit()?;
        self.task_withdrawal(task)?.ok_or(Error::StorageIntegrity)
    }

    /// Inspect immutable withdrawal decisions and unresolved native completion.
    /// # Errors
    /// Refuses malformed identity, corrupt receipts or inconsistent interest bindings.
    pub fn task_withdrawal(&self, task: &str) -> Result<Option<TaskWithdrawalView>> {
        validate_key(task, "task ID")?;
        let row: Option<(String, String, String, u32, i64)> = self.connection.query_row("SELECT payload_json, receipt_sha256, request_id, expected_processing_generation, created_ms FROM task_interest_withdrawals WHERE task_id = ?1", [task], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).optional()?;
        let Some((json, hash, request, expected, created)) = row else {
            return Ok(None);
        };
        let receipt: TaskWithdrawal =
            serde_json::from_str(&json).map_err(|_| Error::StorageIntegrity)?;
        if receipt.task_id != task
            || receipt.semantics != SEMANTICS
            || receipt.withdrawal_generation != 1
            || receipt.interests.len() > 4
            || receipt.request_id != request
            || receipt.expected_processing_generation != expected
            || receipt.created_ms != created
            || digest(&json) != hash
        {
            return Err(Error::StorageIntegrity);
        }
        validate_receipt(&self.connection, &receipt)?;
        let completion_unproven: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM native_stop_targets t WHERE t.task_id = ?1 AND NOT EXISTS(SELECT 1 FROM native_stop_completions c WHERE c.family = t.family AND c.job_id = t.job_id AND c.generation = t.generation))", [task], |row| row.get(0))?;
        Ok(Some(TaskWithdrawalView {
            receipt,
            completion_unproven,
        }))
    }

    pub(crate) fn native_completion_unproven(&self) -> Result<bool> {
        // Queue indexes restrict these probes to at most 1,024 open jobs per table.
        // Completed immutable stop history must not grow scheduling work.
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM analysis_jobs j INDEXED BY analysis_job_queue WHERE j.kind = 'local_asr' AND j.state = 'cancelling' AND EXISTS(SELECT 1 FROM native_stop_targets t WHERE t.family = 'recognition' AND t.job_id = j.id AND t.generation = j.generation AND NOT EXISTS(SELECT 1 FROM native_stop_completions c WHERE c.family = t.family AND c.job_id = t.job_id AND c.generation = t.generation))) OR EXISTS(SELECT 1 FROM translation_jobs j INDEXED BY translation_job_queue WHERE j.state = 'cancelling' AND EXISTS(SELECT 1 FROM native_stop_targets t WHERE t.family = 'translation' AND t.job_id = j.id AND t.generation = j.generation AND NOT EXISTS(SELECT 1 FROM native_stop_completions c WHERE c.family = t.family AND c.job_id = t.job_id AND c.generation = t.generation)))", [], |row| row.get(0))?)
    }

    pub(crate) fn native_stop_requested(
        &self,
        family: &str,
        job: &str,
        generation: u32,
    ) -> Result<bool> {
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM native_stop_targets t WHERE t.family = ?1 AND t.job_id = ?2 AND t.generation = ?3 AND NOT EXISTS(SELECT 1 FROM native_stop_completions c WHERE c.family = t.family AND c.job_id = t.job_id AND c.generation = t.generation))", params![family, job, generation], |row| row.get(0))?)
    }

    pub(crate) fn audit_withdrawals(&self) -> Result<()> {
        let mut statement = self
            .connection
            .prepare("SELECT task_id FROM task_interest_withdrawals ORDER BY task_id")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let task: String = row.get(0)?;
            self.task_withdrawal(&task)?
                .ok_or(Error::StorageIntegrity)?;
        }
        for sql in AUDITS {
            let broken: bool = self.connection.query_row(sql, [], |row| row.get(0))?;
            if broken {
                return Err(Error::StorageIntegrity);
            }
        }
        Ok(())
    }
}

/// Called only by native finalization after the existing drained capability is validated.
/// Keep the observed clock separate from the effective lifecycle timestamp. A regression
/// cannot discard proven cleanup merely because it precedes the durable stop intent.
pub(super) fn complete(
    connection: &Connection,
    family: &str,
    job: &str,
    generation: u32,
    now: i64,
) -> Result<i64> {
    let table = table(family)?;
    if now < 0 {
        return Err(Error::InvalidInput("native completion clock"));
    }
    let finished: i64 = connection.query_row(&format!("SELECT max(?3, created_ms, coalesce(started_ms, created_ms), coalesce((SELECT created_ms FROM native_stop_targets WHERE family = ?4 AND job_id = ?1 AND generation = ?2), created_ms)) FROM {table} WHERE id = ?1 AND generation = ?2"), params![job, generation, now, family], |row| row.get(0))?;
    let target: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM native_stop_targets WHERE family = ?1 AND job_id = ?2 AND generation = ?3)", params![family, job, generation], |row| row.get(0))?;
    if !target {
        return Ok(finished);
    }
    let changed = connection.execute(&format!("INSERT INTO native_stop_completions(family, job_id, generation, attempt, lease_owner, observed_clock_ms, completed_ms) SELECT t.family, t.job_id, t.generation, t.attempt, t.lease_owner, ?4, ?5 FROM native_stop_targets t JOIN {table} j ON j.id = t.job_id AND j.generation = t.generation AND j.attempt = t.attempt AND j.lease_owner = t.lease_owner WHERE t.family = ?1 AND t.job_id = ?2 AND t.generation = ?3 AND NOT EXISTS(SELECT 1 FROM native_stop_completions c WHERE c.family = t.family AND c.job_id = t.job_id AND c.generation = t.generation)"), params![family, job, generation, now, finished])?;
    if changed != 1 {
        return Err(Error::StorageIntegrity);
    }
    Ok(finished)
}

fn validate_receipt(connection: &Connection, receipt: &TaskWithdrawal) -> Result<()> {
    validate_key(&receipt.request_id, "withdrawal request").map_err(|_| Error::StorageIntegrity)?;
    let grant: String = connection.query_row(
        "SELECT grant_sha256 FROM task_processing WHERE task_id = ?1",
        [&receipt.task_id],
        |row| row.get(0),
    )?;
    if grant != receipt.grant_sha256 {
        return Err(Error::StorageIntegrity);
    }
    let mut mask = 0;
    for interest in &receipt.interests {
        if !matches!(
            interest.decision.as_str(),
            "legacy-preserved"
                | "shared-preserved"
                | "queued-cancelled"
                | "running-cancelling"
                | "terminal-preserved"
        ) {
            return Err(Error::StorageIntegrity);
        }
        let (ordinal, created): (u32, i64) = connection.query_row("SELECT s.ordinal, i.created_ms FROM task_processing_steps s JOIN job_interests i ON i.family = s.stage AND i.job_id = s.job_id AND i.authority = 'task' AND i.owner_id = s.task_id JOIN job_interest_withdrawals w ON w.family = i.family AND w.job_id = i.job_id AND w.authority = i.authority AND w.owner_id = i.owner_id WHERE s.task_id = ?1 AND s.stage = ?2 AND s.job_id = ?3 AND s.decision = 'queued' AND w.interest_created_ms = i.created_ms", params![receipt.task_id, interest.family, interest.job_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let bit = 1 << (ordinal * 2 + u32::from(interest.family == "translation"));
        if mask & bit != 0
            || created != interest.interest_created_ms
            || receipt.created_ms < created
            || interest.job_generation == 0
        {
            return Err(Error::StorageIntegrity);
        }
        mask |= bit;
        let target: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM native_stop_targets WHERE family = ?1 AND job_id = ?2 AND generation = ?3 AND task_id = ?4 AND created_ms = ?5)", params![interest.family, interest.job_id, interest.job_generation, receipt.task_id, receipt.created_ms], |row| row.get(0))?;
        if target != (interest.decision == "running-cancelling") {
            return Err(Error::StorageIntegrity);
        }
    }
    let (queued, children): (u32, u32) = connection.query_row("SELECT (SELECT coalesce(sum(1 << (ordinal * 2 + (stage = 'translation'))), 0) FROM task_processing_steps WHERE task_id = ?1 AND decision = 'queued'), (SELECT count(*) FROM job_interest_withdrawals WHERE owner_id = ?1)", [&receipt.task_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if receipt.step_mask != mask
        || queued != mask
        || usize::try_from(children).ok() != Some(receipt.interests.len())
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

const AUDITS: [&str; 5] = [
    "SELECT EXISTS(SELECT 1 FROM job_interests i WHERE i.origin = 'migrated' AND NOT EXISTS(SELECT 1 FROM job_interest_legacy_guards g WHERE g.family = i.family AND g.job_id = i.job_id)) OR EXISTS(SELECT 1 FROM job_interest_legacy_guards g WHERE NOT EXISTS(SELECT 1 FROM job_interests i WHERE i.family = g.family AND i.job_id = g.job_id AND i.origin = 'migrated'))",
    "SELECT EXISTS(SELECT 1 FROM native_stop_targets t WHERE NOT EXISTS(SELECT 1 FROM job_interest_withdrawals w WHERE w.family = t.family AND w.job_id = t.job_id AND w.owner_id = t.task_id) OR NOT EXISTS(SELECT 1 FROM native_stop_completions c WHERE c.family = t.family AND c.job_id = t.job_id AND c.generation = t.generation) AND NOT (CASE t.family WHEN 'recognition' THEN EXISTS(SELECT 1 FROM analysis_jobs j WHERE j.id = t.job_id AND j.generation = t.generation AND j.attempt = t.attempt AND j.lease_owner = t.lease_owner AND j.state = 'cancelling') ELSE EXISTS(SELECT 1 FROM translation_jobs j WHERE j.id = t.job_id AND j.generation = t.generation AND j.attempt = t.attempt AND j.lease_owner = t.lease_owner AND j.state = 'cancelling') END))",
    "SELECT EXISTS(SELECT 1 FROM native_stop_completions c JOIN native_stop_targets t ON t.family = c.family AND t.job_id = c.job_id AND t.generation = c.generation WHERE c.attempt != t.attempt OR c.lease_owner != t.lease_owner OR c.completed_ms != max(c.observed_clock_ms, t.created_ms, CASE t.family WHEN 'recognition' THEN (SELECT max(created_ms, coalesce(started_ms, created_ms)) FROM analysis_jobs WHERE id = t.job_id AND generation = t.generation) ELSE (SELECT max(created_ms, coalesce(started_ms, created_ms)) FROM translation_jobs WHERE id = t.job_id AND generation = t.generation) END))",
    "SELECT EXISTS(SELECT 1 FROM native_stop_targets t JOIN task_interest_withdrawals w ON w.task_id = t.task_id WHERE t.created_ms != w.created_ms OR NOT EXISTS(SELECT 1 FROM json_each(w.payload_json, '$.interests') i WHERE json_extract(i.value, '$.family') = t.family AND json_extract(i.value, '$.job_id') = t.job_id AND json_extract(i.value, '$.job_generation') = t.generation AND json_extract(i.value, '$.decision') = 'running-cancelling'))",
    "SELECT EXISTS(SELECT 1 FROM native_stop_completions c WHERE NOT (CASE c.family WHEN 'recognition' THEN EXISTS(SELECT 1 FROM analysis_jobs j WHERE j.id = c.job_id AND j.generation = c.generation AND j.attempt = c.attempt AND j.lease_owner = c.lease_owner AND j.state = 'cancelled' AND j.finished_ms = c.completed_ms) ELSE EXISTS(SELECT 1 FROM translation_jobs j WHERE j.id = c.job_id AND j.generation = c.generation AND j.attempt = c.attempt AND j.lease_owner = c.lease_owner AND j.state = 'cancelled' AND j.finished_ms = c.completed_ms) END))",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Standalone future-sharing query model, deliberately not a valid Sigy catalog.
    /// Current task-owned collection cannot produce these many task owners on one job.
    #[test]
    fn a_late_surviving_owner_is_never_mistaken_for_no_authority() -> Result<()> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch("CREATE TABLE job_interest_legacy_guards(family TEXT, job_id TEXT, PRIMARY KEY(family, job_id)); CREATE TABLE job_interests(family TEXT, job_id TEXT, authority TEXT, owner_id TEXT, PRIMARY KEY(family, job_id, authority, owner_id)); CREATE TABLE job_interest_withdrawals(family TEXT, job_id TEXT, authority TEXT, owner_id TEXT, PRIMARY KEY(family, job_id, authority, owner_id));")?;
        for ordinal in 0..65 {
            let owner = format!("owner-{ordinal:03}");
            connection.execute(
                "INSERT INTO job_interests VALUES ('recognition', 'job', 'task', ?1)",
                [&owner],
            )?;
            if ordinal < 64 {
                connection.execute("INSERT INTO job_interest_withdrawals VALUES ('recognition', 'job', 'task', ?1)", [&owner])?;
            }
        }
        assert!(matches!(
            other_authority(&connection, "recognition", "job", "cancelling-owner"),
            Err(Error::Analysis("remaining-authority-unproven"))
        ));
        connection.execute(
            "DELETE FROM job_interest_withdrawals WHERE owner_id = 'owner-063'",
            [],
        )?;
        assert_eq!(
            other_authority(&connection, "recognition", "job", "cancelling-owner")?,
            "shared-preserved"
        );
        connection.execute(
            "INSERT INTO job_interests VALUES ('recognition', 'job', 'direct', '')",
            [],
        )?;
        assert_eq!(
            other_authority(&connection, "recognition", "job", "cancelling-owner")?,
            "shared-preserved"
        );
        Ok(())
    }
}
