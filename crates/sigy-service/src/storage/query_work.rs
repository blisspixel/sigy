//! Scoped cooperative SQLite work bounds. No bound forces blocked external I/O to return.
//!
//! One guard owns the connection's progress hook. Do not nest guards on a connection.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rusqlite::Connection;

use crate::{Error, Result};

const CHECKPOINT_OPS: i32 = 1_000;

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub wall: Duration,
    pub vm_ops: u64,
    pub lock_wait: Duration,
}

impl Limits {
    /// Whole retained-reader history integrity, not an interactive projection.
    pub(crate) const RETAINED_HISTORY: Self = Self {
        wall: Duration::from_millis(1_000),
        vm_ops: 4_000_000,
        lock_wait: Duration::from_millis(10),
    };

    pub(crate) const TASK_EVIDENCE: Self = Self {
        wall: Duration::from_millis(100),
        vm_ops: 4_000_000,
        lock_wait: Duration::from_millis(10),
    };
}

pub(crate) struct QueryWork<'a> {
    connection: &'a Connection,
    previous_wait: Duration,
    deadline: Instant,
    ops: Arc<AtomicU64>,
    maximum_ops: u64,
    cleared: bool,
}

impl<'a> QueryWork<'a> {
    /// Installs one connection-local hook and short lock wait, also inside transactions.
    pub(crate) fn start(connection: &'a Connection, limits: Limits) -> Result<Self> {
        if limits.wall.is_zero() || limits.vm_ops == 0 {
            return Err(Error::InvalidInput("query work bounds"));
        }
        let deadline = Instant::now()
            .checked_add(limits.wall)
            .ok_or(Error::InvalidInput("query work deadline"))?;
        let previous_ms: u32 =
            connection.pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
        connection.busy_timeout(limits.lock_wait.min(limits.wall))?;
        let guard = Self {
            connection,
            previous_wait: Duration::from_millis(u64::from(previous_ms)),
            deadline,
            ops: Arc::new(AtomicU64::new(0)),
            maximum_ops: limits.vm_ops,
            cleared: false,
        };
        let ops = Arc::clone(&guard.ops);
        connection.progress_handler(
            CHECKPOINT_OPS,
            Some(move || {
                let previous = ops.fetch_add(1_000, Ordering::Relaxed);
                previous >= limits.vm_ops.saturating_sub(1_000) || Instant::now() >= deadline
            }),
        )?;
        Ok(guard)
    }

    pub(crate) fn exhausted(&self) -> bool {
        self.ops.load(Ordering::Relaxed) >= self.maximum_ops || Instant::now() >= self.deadline
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.exhausted() {
            Err(Error::Analysis("query-work-exhausted"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn check_connection(&self, connection: &Connection) -> Result<()> {
        if !std::ptr::eq(self.connection, connection) {
            return Err(Error::InvalidInput("query work connection"));
        }
        self.check()
    }

    /// Clears the hook and restores lock policy; callers surface cleanup errors.
    pub(crate) fn finish(mut self) -> Result<()> {
        let hook = self.connection.progress_handler(0, None::<fn() -> bool>);
        let wait = self.connection.busy_timeout(self.previous_wait);
        self.cleared = hook.is_ok() && wait.is_ok();
        hook?;
        wait?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn checkpoint_ops(&self) -> u64 {
        self.ops.load(Ordering::Relaxed)
    }
}

impl Drop for QueryWork<'_> {
    fn drop(&mut self) {
        if !self.cleared {
            let _ = self.connection.progress_handler(0, None::<fn() -> bool>);
            let _ = self.connection.busy_timeout(self.previous_wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_sql_clears_hook_and_restores_lock_wait() -> Result<()> {
        let connection = Connection::open_in_memory()?;
        connection.busy_timeout(Duration::from_millis(37))?;
        {
            let work = QueryWork::start(
                &connection,
                Limits {
                    wall: Duration::from_secs(1),
                    vm_ops: 2_000,
                    lock_wait: Duration::from_millis(1),
                },
            )?;
            let result = connection.query_row(
                "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) SELECT sum(x) FROM n",
                [], |row| row.get::<_, i64>(0),
            );
            assert!(
                matches!(result, Err(rusqlite::Error::SqliteFailure(ref e, _)) if e.code == rusqlite::ErrorCode::OperationInterrupted)
            );
            assert!(work.checkpoint_ops() >= 2_000);
        }
        let total: i64 = connection.query_row("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000) SELECT sum(x) FROM n", [], |row| row.get(0))?;
        assert_eq!(total, 500_500);
        assert_eq!(
            connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))?,
            37
        );
        Ok(())
    }

    #[test]
    fn deadline_and_invalid_limits_refuse_and_explicit_finish_restores() -> Result<()> {
        let connection = Connection::open_in_memory()?;
        connection.busy_timeout(Duration::from_millis(29))?;
        for (wall, vm_ops) in [
            (Duration::ZERO, 1),
            (Duration::from_secs(1), 0),
            (Duration::MAX, 1),
        ] {
            assert!(
                QueryWork::start(
                    &connection,
                    Limits {
                        wall,
                        vm_ops,
                        lock_wait: Duration::ZERO
                    }
                )
                .is_err()
            );
        }
        let work = QueryWork::start(
            &connection,
            Limits {
                wall: Duration::from_nanos(1),
                vm_ops: 1_000,
                lock_wait: Duration::ZERO,
            },
        )?;
        assert!(work.check().is_err());
        work.finish()?;
        assert_eq!(
            connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))?,
            29
        );
        Ok(())
    }

    #[test]
    fn busy_lock_has_finite_wait_and_the_next_query_succeeds() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("locked.sqlite");
        let connection = Connection::open(&path)?;
        connection.execute_batch("CREATE TABLE facts(n INTEGER); INSERT INTO facts VALUES(7)")?;
        let owner = Connection::open(&path)?;
        owner.execute_batch("BEGIN EXCLUSIVE")?;
        let work = QueryWork::start(
            &connection,
            Limits {
                wall: Duration::from_millis(100),
                vm_ops: 10_000,
                lock_wait: Duration::from_millis(1),
            },
        )?;
        let result = connection.query_row("SELECT n FROM facts", [], |row| row.get::<_, i64>(0));
        assert!(
            matches!(result, Err(rusqlite::Error::SqliteFailure(ref e, _)) if e.code == rusqlite::ErrorCode::DatabaseBusy)
        );
        work.finish()?;
        owner.execute_batch("COMMIT")?;
        assert_eq!(
            connection.query_row("SELECT n FROM facts", [], |row| row.get::<_, i64>(0))?,
            7
        );
        Ok(())
    }
}
