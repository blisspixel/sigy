//! One catalog effect per visited task on the existing service tick.
//! Intent and receipts live in the catalog; this cursor is only a fairness hint.

use super::Actor;
use crate::{Error, Result, storage::now_ms};

const TASKS_PER_PASS: u32 = 4;

impl Actor {
    pub(super) fn reconcile_tasks(&mut self) -> Result<()> {
        let store = self.library.store();
        let (mut ids, _) =
            store.pending_task_run_ids(self.task_cursor.as_deref(), TASKS_PER_PASS)?;
        if ids.is_empty() && self.task_cursor.is_some() {
            (ids, _) = store.pending_task_run_ids(None, TASKS_PER_PASS)?;
        }
        self.task_cursor = ids.last().cloned();
        for id in ids {
            let run = self
                .library
                .store()
                .task_run(&id)?
                .ok_or(Error::StorageIntegrity)?;
            let now = now_ms()?;
            if now < run.updated_ms {
                continue;
            }
            self.library
                .store_mut()
                .advance_task_run(&id, run.generation, now)?;
        }
        Ok(())
    }
}
