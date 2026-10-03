//! Validate compatible historical attempts without substituting today's state.

use crate::{Error, Result, task::snapshot::TaskJobObservation};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn validate(
    connection: &Connection,
    job: &TaskJobObservation,
    observed: i64,
) -> Result<()> {
    let (table, family) = match job.stage.as_str() {
        "recognition" => ("analysis_jobs", "analysis"),
        "translation" => ("translation_jobs", "translation"),
        _ => return Err(Error::StorageIntegrity),
    };
    let (generation, state, created, started, finished): (
        u32,
        String,
        i64,
        Option<i64>,
        Option<i64>,
    ) = connection.query_row(
        &format!(
            "SELECT generation, state, created_ms, started_ms, finished_ms FROM {table} WHERE id=?1"
        ),
        [&job.job_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    if created > observed {
        return Err(Error::StorageIntegrity);
    }
    if job.observed_generation > 1 {
        let began: Option<i64> = connection
            .query_row(
                "SELECT ended_ms FROM job_attempts WHERE family=?1 AND job_id=?2 AND generation=?3",
                params![family, job.job_id, job.observed_generation - 1],
                |r| r.get(0),
            )
            .optional()?;
        if began.is_none_or(|time| time > observed) {
            return Err(Error::StorageIntegrity);
        }
    }
    let prior: Option<(i64,i64)> = connection.query_row(
        "SELECT started_ms, ended_ms FROM job_attempts WHERE family=?1 AND job_id=?2 AND generation=?3",
        params![family,job.job_id,job.observed_generation], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    let valid = if job.observed_generation == generation {
        match job.observed_state.as_str() {
            "queued" => {
                started.is_none_or(|time| observed <= time)
                    && finished.is_none_or(|time| observed <= time)
            }
            "running" | "cancelling" => {
                started.is_some_and(|time| time <= observed)
                    && finished.is_none_or(|time| observed <= time)
            }
            "succeeded" | "failed" | "cancelled" | "interrupted" => {
                state == job.observed_state && finished.is_some_and(|time| time <= observed)
            }
            _ => false,
        }
    } else if let Some((start, end)) = prior {
        match job.observed_state.as_str() {
            "queued" => observed <= start,
            "running" | "cancelling" => start <= observed && observed <= end,
            _ => false,
        }
    } else {
        false
    };
    if !valid {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}
