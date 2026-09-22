//! One saved directory page. The service admits a due slot. A client read does not.

use super::{Store, discovery::RefreshStatus, validate_key};
use crate::{Error, Result, discovery::RefreshRequest};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

pub(crate) const MIN_DIRECTORY_POLICY_MS: i64 = 3_600_000;
pub(crate) const MAX_DIRECTORY_POLICY_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const MAX_DIRECTORY_POLICIES: i64 = 8;

#[derive(Clone)]
pub(crate) struct PolicyDraft {
    pub id: String,
    pub request: RefreshRequest,
    pub interval_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveResult {
    Created,
    Unchanged,
    Revised,
}

#[derive(Debug, Clone)]
pub(crate) struct DirectoryPolicyRecord {
    pub id: String,
    pub request: RefreshRequest,
    pub interval_ms: i64,
    pub revision: i64,
    pub updated_ms: i64,
    pub next_due_ms: i64,
    pub last_refresh: Option<RefreshStatus>,
}

#[derive(Debug, Clone)]
pub(crate) struct DuePolicy {
    pub id: String,
    pub request: RefreshRequest,
}

struct PolicyRow {
    id: String,
    request_json: String,
    interval_ms: i64,
    revision: i64,
    updated_ms: i64,
}

impl Store {
    pub(crate) fn save_directory_policy_at(
        &mut self,
        draft: &PolicyDraft,
        now: i64,
    ) -> Result<SaveResult> {
        validate_policy_id(&draft.id)?;
        draft.request.validate()?;
        if !(MIN_DIRECTORY_POLICY_MS..=MAX_DIRECTORY_POLICY_MS).contains(&draft.interval_ms) {
            return Err(Error::InvalidInput(
                "directory policy interval is 1 to 168 hours",
            ));
        }
        let json = serde_json::to_string(&draft.request)?;
        if json.len() > 8192 {
            return Err(Error::InvalidInput("directory policy request size"));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, i64)> = transaction
            .query_row(
                "SELECT request_json, interval_ms FROM directory_refresh_policies WHERE id = ?1",
                [&draft.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let disposition = if let Some((stored, interval_ms)) = existing {
            if stored == json && interval_ms == draft.interval_ms {
                SaveResult::Unchanged
            } else {
                transaction.execute(
                    "UPDATE directory_refresh_policies SET request_json = ?2, interval_ms = ?3, revision = revision + 1, updated_ms = ?4 WHERE id = ?1",
                    params![draft.id, json, draft.interval_ms, now],
                )?;
                SaveResult::Revised
            }
        } else {
            let count: i64 = transaction.query_row(
                "SELECT count(*) FROM directory_refresh_policies",
                [],
                |row| row.get(0),
            )?;
            if count >= MAX_DIRECTORY_POLICIES {
                return Err(Error::InvalidInput("directory policy capacity reached"));
            }
            transaction.execute(
                "INSERT INTO directory_refresh_policies(id, request_json, interval_ms, revision, created_ms, updated_ms) VALUES (?1, ?2, ?3, 1, ?4, ?4)",
                params![draft.id, json, draft.interval_ms, now],
            )?;
            SaveResult::Created
        };
        if disposition != SaveResult::Unchanged {
            transaction.commit()?;
        }
        Ok(disposition)
    }

    pub(crate) fn clear_directory_policy(&mut self, id: &str) -> Result<()> {
        validate_policy_id(id)?;
        if self
            .connection
            .execute("DELETE FROM directory_refresh_policies WHERE id = ?1", [id])?
            != 1
        {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    pub(crate) fn directory_policies(&self, now: i64) -> Result<Vec<DirectoryPolicyRecord>> {
        let rows = self.policy_rows()?;
        let mut policies = Vec::with_capacity(rows.len());
        for row in rows {
            policies.push(self.policy_record(row, now)?);
        }
        Ok(policies)
    }

    /// The current slot of the oldest due policy, when no refresh is already running.
    pub(crate) fn due_directory_policy(&self, now: i64) -> Result<Option<DuePolicy>> {
        let running: i64 = self.connection.query_row(
            "SELECT count(*) FROM directory_refreshes WHERE state = 'running'",
            [],
            |row| row.get(0),
        )?;
        if running != 0 {
            return Ok(None);
        }
        for row in self.policy_rows()? {
            let Some(slot) = elapsed_slots(row.updated_ms, row.interval_ms, now) else {
                continue;
            };
            let id = refresh_id(&row.id, row.revision, slot)?;
            let taken: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM directory_refreshes WHERE id = ?1)",
                [&id],
                |exists| exists.get(0),
            )?;
            if taken {
                continue;
            }
            return Ok(Some(DuePolicy {
                id,
                request: serde_json::from_str(&row.request_json)?,
            }));
        }
        Ok(None)
    }

    fn policy_rows(&self) -> Result<Vec<PolicyRow>> {
        let mut statement = self.connection.prepare(
            "SELECT id, request_json, interval_ms, revision, updated_ms FROM directory_refresh_policies ORDER BY updated_ms, id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(PolicyRow {
                    id: row.get(0)?,
                    request_json: row.get(1)?,
                    interval_ms: row.get(2)?,
                    revision: row.get(3)?,
                    updated_ms: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn policy_record(&self, row: PolicyRow, now: i64) -> Result<DirectoryPolicyRecord> {
        let request: RefreshRequest = serde_json::from_str(&row.request_json)?;
        request.validate()?;
        let slot = elapsed_slots(row.updated_ms, row.interval_ms, now);
        let taken = match slot {
            Some(slot) => {
                let id = refresh_id(&row.id, row.revision, slot)?;
                self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM directory_refreshes WHERE id = ?1)",
                    [&id],
                    |exists| exists.get(0),
                )?
            }
            None => false,
        };
        let latest: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM directory_refreshes WHERE id GLOB ?1 ORDER BY started_ms DESC, id DESC LIMIT 1",
                [format!("{}:*", row.id)],
                |found| found.get(0),
            )
            .optional()?;
        Ok(DirectoryPolicyRecord {
            id: row.id,
            request,
            interval_ms: row.interval_ms,
            revision: row.revision,
            updated_ms: row.updated_ms,
            next_due_ms: next_due_ms(row.updated_ms, row.interval_ms, slot, taken),
            last_refresh: latest.map(|id| self.directory_refresh(&id)).transpose()?,
        })
    }
}

fn validate_policy_id(id: &str) -> Result<()> {
    validate_key(id, "directory policy ID")?;
    if id.len() > 64 {
        return Err(Error::InvalidInput("directory policy ID"));
    }
    Ok(())
}

fn elapsed_slots(updated_ms: i64, interval_ms: i64, now: i64) -> Option<i64> {
    let elapsed = now.checked_sub(updated_ms)?;
    if interval_ms <= 0 || elapsed < interval_ms {
        return None;
    }
    Some(elapsed / interval_ms)
}

fn next_due_ms(updated_ms: i64, interval_ms: i64, slot: Option<i64>, slot_taken: bool) -> i64 {
    let current = slot.unwrap_or(0);
    let next_slot = if current == 0 || slot_taken {
        current.saturating_add(1)
    } else {
        current
    };
    updated_ms.saturating_add(interval_ms.saturating_mul(next_slot))
}

fn refresh_id(id: &str, revision: i64, slot: i64) -> Result<String> {
    let refresh_id = format!("{id}:{revision}:{slot}");
    validate_key(&refresh_id, "refresh ID")?;
    Ok(refresh_id)
}

#[cfg(test)]
mod tests {
    use super::{MIN_DIRECTORY_POLICY_MS, PolicyDraft, SaveResult};
    use crate::{
        Error,
        control::{DirectoryOperation, Operation, apply},
        discovery::{RefreshRequest, StationFilter, radio_browser},
        sources::NetworkScope,
        storage::Store,
    };

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn draft(id: &str, limit: u32) -> PolicyDraft {
        PolicyDraft {
            id: id.into(),
            request: RefreshRequest {
                filter: StationFilter::default(),
                limit,
                offset: 0,
                mirror: None,
                network: NetworkScope::PublicInternet {},
            },
            interval_ms: MIN_DIRECTORY_POLICY_MS,
        }
    }

    fn sample_batch() -> crate::Result<crate::discovery::RefreshBatch> {
        let body = serde_json::json!([{
            "stationuuid": "12345678-1234-1234-1234-123456789abc",
            "name": "Radio Québec",
            "url": "https://radio.example/audio",
            "countrycode": "CA",
            "language": "french",
            "tags": "news",
            "lastcheckok": 1
        }]);
        radio_browser::parse(
            &serde_json::to_vec(&body)?,
            10,
            "https://directory.example".into(),
        )
    }

    #[test]
    fn client_reads_do_not_admit_a_due_policy() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        let saved = draft("french", 10);
        let started = 1_700_000_000_000;
        assert_eq!(
            store.save_directory_policy_at(&saved, started)?,
            SaveResult::Created
        );
        let due_at = started + MIN_DIRECTORY_POLICY_MS;
        assert!(store.due_directory_policy(due_at)?.is_some());
        let status = apply(
            &mut store,
            Operation::Radio {
                command: DirectoryOperation::Status {},
            },
        )?;
        assert!(status.directory_refresh.is_none());
        apply(
            &mut store,
            Operation::Radio {
                command: DirectoryOperation::Search {
                    filter: StationFilter::default(),
                    favorites_only: false,
                    after: None,
                    limit: 16,
                },
            },
        )?;
        apply(&mut store, Operation::Doctor {})?;
        apply(&mut store, Operation::Status {})?;
        let refresh = apply(
            &mut store,
            Operation::Radio {
                command: DirectoryOperation::Refresh {
                    id: "manual".into(),
                    request: saved.request.clone(),
                },
            },
        );
        assert!(matches!(refresh, Err(Error::ServiceRequired)));
        let click = apply(
            &mut store,
            Operation::Radio {
                command: DirectoryOperation::Click {
                    id: "heard".into(),
                    request: crate::discovery::ClickRequest {
                        station_id: "12345678-1234-1234-1234-123456789abc".into(),
                        mirror: None,
                        network: NetworkScope::PublicInternet {},
                    },
                },
            },
        );
        assert!(matches!(click, Err(Error::ServiceRequired)));
        assert!(store.due_directory_policy(due_at)?.is_some());
        assert!(store.directory_refresh("french:1:1").is_err());
        Ok(())
    }

    #[test]
    fn due_slot_admits_once_and_a_missed_slot_is_not_backfilled() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        let saved = draft("french", 10);
        let started = 1_700_000_000_000;
        store.save_directory_policy_at(&saved, started)?;
        assert!(
            store
                .due_directory_policy(started + MIN_DIRECTORY_POLICY_MS - 1)?
                .is_none()
        );
        store.begin_refresh_at("manual", &saved.request, started + 2_000)?;
        let due_at = started + MIN_DIRECTORY_POLICY_MS;
        assert!(store.due_directory_policy(due_at)?.is_none());
        store.fail_refresh("manual", &Error::Acquisition("fixture failure"))?;
        let due = store
            .due_directory_policy(due_at)?
            .ok_or("policy was not due")?;
        assert_eq!(due.id, "french:1:1");
        assert!(store.begin_refresh_at(&due.id, &due.request, due_at)?);
        assert!(store.due_directory_policy(due_at)?.is_none());
        assert!(!store.begin_refresh_at(&due.id, &due.request, due_at + 2_000)?);
        store.fail_refresh(&due.id, &Error::Acquisition("fixture failure"))?;
        let later = started + (3 * MIN_DIRECTORY_POLICY_MS);
        let next = store
            .due_directory_policy(later)?
            .ok_or("current slot missing")?;
        assert_eq!(next.id, "french:1:3");
        assert!(store.directory_refresh("french:1:2").is_err());
        Ok(())
    }

    #[test]
    fn failed_or_interrupted_policy_refresh_keeps_the_cache() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        let saved = draft("french", 10);
        store.begin_refresh_at("seed", &saved.request, 1)?;
        store.finish_refresh("seed", sample_batch()?)?;
        let station = "12345678-1234-1234-1234-123456789abc";
        store.set_station_favorite(station, true)?;
        let started = 10_000;
        store.save_directory_policy_at(&saved, started)?;
        let due_at = started + MIN_DIRECTORY_POLICY_MS;
        let due = store.due_directory_policy(due_at)?.ok_or("due")?;
        assert!(store.begin_refresh_at(&due.id, &due.request, due_at)?);
        store.recover_directory_refreshes()?;
        assert_eq!(store.directory_refresh(&due.id)?.state, "interrupted");
        assert_eq!(store.directory_status()?.cached_stations, 1);
        assert!(store.is_station_favorite(station)?);
        assert!(store.due_directory_policy(due_at)?.is_none());
        Ok(())
    }

    #[test]
    fn replay_revise_clear_and_capacity_follow_the_saved_row() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        let mut saved = draft("french", 10);
        let started = 5_000;
        assert_eq!(
            store.save_directory_policy_at(&saved, started)?,
            SaveResult::Created
        );
        assert_eq!(
            store.save_directory_policy_at(&saved, started + 4_000)?,
            SaveResult::Unchanged
        );
        assert_eq!(store.directory_policies(started)?[0].revision, 1);
        assert_eq!(store.directory_policies(started)?[0].updated_ms, started);
        saved.request.limit = 11;
        assert_eq!(
            store.save_directory_policy_at(&saved, started + 4_000)?,
            SaveResult::Revised
        );
        let revised = &store.directory_policies(started + 4_000)?[0];
        assert_eq!(revised.revision, 2);
        assert_eq!(revised.updated_ms, started + 4_000);
        assert!(
            store
                .due_directory_policy(started + MIN_DIRECTORY_POLICY_MS)?
                .is_none()
        );
        let due = store
            .due_directory_policy(started + 4_000 + MIN_DIRECTORY_POLICY_MS)?
            .ok_or("revised policy was not due")?;
        assert_eq!(due.id, "french:2:1");
        store.clear_directory_policy("french")?;
        assert!(store.directory_policies(started)?.is_empty());
        assert!(store.clear_directory_policy("french").is_err());
        for index in 0..8 {
            let policy = draft(&format!("policy-{index}"), 10);
            store.save_directory_policy_at(&policy, started)?;
        }
        let extra = draft("policy-extra", 10);
        assert!(matches!(
            store.save_directory_policy_at(&extra, started),
            Err(Error::InvalidInput("directory policy capacity reached"))
        ));
        Ok(())
    }

    #[test]
    fn one_running_refresh_blocks_the_other_due_policy() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        let started = 5_000;
        store.save_directory_policy_at(&draft("alpha", 10), started)?;
        store.save_directory_policy_at(&draft("beta", 10), started)?;
        let due_at = started + MIN_DIRECTORY_POLICY_MS;
        let first = store.due_directory_policy(due_at)?.ok_or("first")?;
        assert_eq!(first.id, "alpha:1:1");
        assert!(store.begin_refresh_at(&first.id, &first.request, due_at)?);
        assert!(store.due_directory_policy(due_at)?.is_none());
        store.fail_refresh(&first.id, &Error::Acquisition("fixture failure"))?;
        assert_eq!(store.directory_status()?.cached_stations, 0);
        let second = store
            .due_directory_policy(due_at)?
            .ok_or("second policy stayed blocked")?;
        assert_eq!(second.id, "beta:1:1");
        Ok(())
    }
}
