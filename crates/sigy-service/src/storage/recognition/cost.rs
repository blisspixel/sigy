//! Observed recognition cost from the snapshot that proved a process group empty.
//! The figure is not a host budget, a slot count, or a catch-up multiple.

use std::fmt::Write;

use rusqlite::{Connection, params};

use super::Store;
use crate::execution::GroupAccount;
use crate::{Error, Result};

const ROLES: [&str; 2] = ["decode", "recognize"];
const MECHANISMS: [&str; 4] = ["job_object", "cgroup_v2", "process_group", "process_reaper"];

/// Job-object groups with both measures, kept apart from every other row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerCost {
    pub complete_groups: u32,
    pub omitted_groups: u32,
    pub peak_memory_bytes: Option<u64>,
    pub cpu_time_us: Option<u64>,
}

/// One stored group, after the signed catalog columns have been checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationRow {
    pub mechanism: String,
    pub peak_memory_bytes: Option<u64>,
    pub cpu_time_us: Option<u64>,
}

impl Store {
    /// Highest job-object peak and the CPU sum on this library.
    /// # Errors
    /// Returns a storage error when a stored integer is negative or a total overflows.
    pub(crate) fn recognition_worker_cost(&self) -> Result<WorkerCost> {
        read(&self.connection)
    }
}

/// Insert the drained groups for one terminal transition.
/// # Errors
/// A role, mechanism, ordinal, duplicate or integer that cannot be stored rolls the caller back.
pub(super) fn insert(
    connection: &Connection,
    job_id: &str,
    generation: u32,
    accounts: &[GroupAccount],
) -> Result<()> {
    let generation = i64::from(generation);
    if !(1..=64).contains(&generation) {
        return Err(Error::StorageIntegrity);
    }
    let mut seen = std::collections::BTreeSet::new();
    for account in accounts {
        if !ROLES.contains(&account.role)
            || !MECHANISMS.contains(&account.mechanism)
            || account.ordinal > 1023
            || !seen.insert((account.role, account.ordinal))
        {
            return Err(Error::StorageIntegrity);
        }
        let peak = stored_measure(account.peak_memory_bytes)?;
        let cpu = stored_measure(account.cpu_time_us)?;
        connection.execute(
            "INSERT INTO worker_observations(job_id, generation, role, ordinal, mechanism, peak_memory_bytes, cpu_time_us) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![job_id, generation, account.role, account.ordinal, account.mechanism, peak, cpu],
        )?;
    }
    Ok(())
}

/// Every observation belongs to a terminal local recognition job of the same generation.
/// # Errors
/// Returns a catalog integrity error when a row has no such job.
pub(in crate::storage) fn audit_observations(connection: &Connection) -> Result<()> {
    let orphan: i64 = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM worker_observations o
            WHERE NOT EXISTS (
                SELECT 1 FROM analysis_jobs j
                WHERE j.id = o.job_id
                  AND j.generation = o.generation
                  AND j.kind = 'local_asr'
                  AND j.state IN ('succeeded', 'failed', 'cancelled')
            )
        )",
        [],
        |row| row.get(0),
    )?;
    if orphan != 0 {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}

/// Fold stored rows. A job-object row missing either measure is omitted, not blended.
/// # Errors
/// Returns a storage error when a count or the CPU sum overflows.
pub(crate) fn summarize(rows: &[ObservationRow]) -> Result<WorkerCost> {
    let mut complete_groups = 0_u32;
    let mut omitted_groups = 0_u32;
    let mut peak_memory_bytes: Option<u64> = None;
    let mut cpu_time_us = 0_u64;
    let mut saw_cpu = false;
    for row in rows {
        let complete = row.mechanism == "job_object"
            && row.peak_memory_bytes.is_some()
            && row.cpu_time_us.is_some();
        if !complete {
            omitted_groups = omitted_groups
                .checked_add(1)
                .ok_or(Error::StorageIntegrity)?;
            continue;
        }
        complete_groups = complete_groups
            .checked_add(1)
            .ok_or(Error::StorageIntegrity)?;
        let peak = row.peak_memory_bytes.ok_or(Error::StorageIntegrity)?;
        let cpu = row.cpu_time_us.ok_or(Error::StorageIntegrity)?;
        peak_memory_bytes = Some(peak_memory_bytes.map_or(peak, |current| current.max(peak)));
        cpu_time_us = cpu_time_us
            .checked_add(cpu)
            .ok_or(Error::StorageIntegrity)?;
        saw_cpu = true;
    }
    Ok(WorkerCost {
        complete_groups,
        omitted_groups,
        peak_memory_bytes,
        cpu_time_us: saw_cpu.then_some(cpu_time_us),
    })
}

/// One doctor sentence. Integers only. The check state stays ok.
#[must_use]
pub(crate) fn describe_cost(cost: &WorkerCost) -> String {
    match (
        cost.complete_groups,
        cost.peak_memory_bytes,
        cost.cpu_time_us,
    ) {
        (0, _, _) => unmeasured(cost.omitted_groups),
        (groups, Some(peak), Some(cpu)) => observed(groups, peak, cpu, cost.omitted_groups),
        _ => unmeasured(cost.omitted_groups.saturating_add(cost.complete_groups)),
    }
}

fn read(connection: &Connection) -> Result<WorkerCost> {
    let mut statement = connection
        .prepare("SELECT mechanism, peak_memory_bytes, cpu_time_us FROM worker_observations")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let mut parsed = Vec::new();
    for row in rows {
        let (mechanism, peak, cpu) = row?;
        parsed.push(ObservationRow {
            mechanism,
            peak_memory_bytes: measured(peak)?,
            cpu_time_us: measured(cpu)?,
        });
    }
    summarize(&parsed)
}

fn stored_measure(value: Option<u64>) -> Result<Option<i64>> {
    value
        .map(i64::try_from)
        .transpose()
        .map_err(|_| Error::StorageIntegrity)
}

fn measured(value: Option<i64>) -> Result<Option<u64>> {
    value
        .map(u64::try_from)
        .transpose()
        .map_err(|_| Error::StorageIntegrity)
}

fn observed(groups: u32, peak: u64, cpu: u64, omitted: u32) -> String {
    let mut detail = String::new();
    let noun = group_noun(groups);
    let _ = write!(
        detail,
        "Observed job-object recognition on this library: {groups} {noun}, highest peak committed memory {peak} bytes, {cpu} us of CPU time."
    );
    if omitted > 0 {
        let omitted_noun = group_noun(omitted);
        let verb = if omitted == 1 { "is" } else { "are" };
        let _ = write!(
            detail,
            " {omitted} {omitted_noun} {verb} not in that figure."
        );
    }
    detail.push_str(" This is not a host budget.");
    detail
}

fn unmeasured(omitted: u32) -> String {
    let mut detail = String::from("Recognition worker cost is unmeasured on this library.");
    if omitted > 0 {
        let noun = group_noun(omitted);
        let _ = write!(detail, " {omitted} {noun} had no job-object accounting.");
    }
    detail
}

fn group_noun(count: u32) -> &'static str {
    if count == 1 { "group" } else { "groups" }
}

#[cfg(test)]
mod tests {
    use super::{ObservationRow, describe_cost, summarize};
    use crate::Error;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn row(mechanism: &str, peak: Option<u64>, cpu: Option<u64>) -> ObservationRow {
        ObservationRow {
            mechanism: mechanism.to_owned(),
            peak_memory_bytes: peak,
            cpu_time_us: cpu,
        }
    }

    #[test]
    fn job_object_rows_keep_the_highest_peak_and_the_cpu_sum() -> TestResult {
        let cost = summarize(&[
            row("job_object", Some(1_000), Some(4)),
            row("job_object", Some(5_000), Some(6)),
            row("cgroup_v2", Some(9_000), Some(9_000)),
            row("job_object", Some(8_000), None),
        ])?;
        assert_eq!(cost.complete_groups, 2);
        assert_eq!(cost.omitted_groups, 2);
        assert_eq!(cost.peak_memory_bytes, Some(5_000));
        assert_eq!(cost.cpu_time_us, Some(10));
        assert_eq!(
            describe_cost(&cost),
            "Observed job-object recognition on this library: 2 groups, highest peak committed memory 5000 bytes, 10 us of CPU time. 2 groups are not in that figure. This is not a host budget."
        );
        Ok(())
    }

    #[test]
    fn one_complete_group_and_one_omitted_group_stay_distinct() -> TestResult {
        let cost = summarize(&[
            row("job_object", Some(80), Some(7)),
            row("process_group", None, None),
        ])?;
        assert_eq!(
            describe_cost(&cost),
            "Observed job-object recognition on this library: 1 group, highest peak committed memory 80 bytes, 7 us of CPU time. 1 group is not in that figure. This is not a host budget."
        );
        Ok(())
    }

    #[test]
    fn an_empty_library_and_an_unaccounted_group_are_unmeasured() -> TestResult {
        let empty = summarize(&[])?;
        assert_eq!(
            describe_cost(&empty),
            "Recognition worker cost is unmeasured on this library."
        );
        let omitted = summarize(&[row("cgroup_v2", None, None)])?;
        assert_eq!(omitted.complete_groups, 0);
        assert_eq!(omitted.peak_memory_bytes, None);
        assert_eq!(
            describe_cost(&omitted),
            "Recognition worker cost is unmeasured on this library. 1 group had no job-object accounting."
        );
        let groups = summarize(&[
            row("process_reaper", None, None),
            row("cgroup_v2", None, None),
        ])?;
        assert_eq!(
            describe_cost(&groups),
            "Recognition worker cost is unmeasured on this library. 2 groups had no job-object accounting."
        );
        Ok(())
    }

    #[test]
    fn a_cpu_sum_that_overflows_is_a_storage_error() {
        let overflow = summarize(&[
            row("job_object", Some(1), Some(u64::MAX)),
            row("job_object", Some(1), Some(1)),
        ]);
        assert!(matches!(overflow, Err(Error::StorageIntegrity)));
    }
}
