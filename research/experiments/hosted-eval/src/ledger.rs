//! Append-only runtime ledger for paid requests.
//!
//! Each line is one JSON event with a sequence number and the SHA-256 of the
//! previous line, written and synced before the next effect. A reservation is
//! appended before a request is sent; a settlement, release or uncertainty is
//! appended after. Outstanding liability is every reservation without a
//! settlement or release, so a crash between reserve and outcome keeps the full
//! reservation. A lock file excludes a second writer.

use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use scorer_probe::selection::sha256;
use serde::{Deserialize, Serialize};

use crate::Result;

const MAX_LEDGER_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Cap {
        software_cap_micro: u64,
        ceiling_micro: u64,
        note: String,
    },
    Allocate {
        batch: String,
        micro: u64,
        note: String,
    },
    Reserve {
        request_id: String,
        batch: String,
        micro: u64,
        hard_bound_micro: u64,
        model: String,
        manifest_sha256: String,
        index: u32,
    },
    Settle {
        request_id: String,
        micro: u64,
        cost_text: String,
        generation_id: String,
        provider: String,
    },
    Uncertain {
        request_id: String,
        reason: String,
        generation_id: Option<String>,
    },
    Release {
        request_id: String,
        reason: String,
    },
    Breach {
        request_id: String,
        reason: String,
    },
    KeyCheck {
        limit: Option<String>,
        limit_remaining: Option<String>,
        limit_reset: Option<String>,
        usage: Option<String>,
        is_free_tier: Option<bool>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Line {
    seq: u64,
    unix_seconds: u64,
    prev_sha256: String,
    event: Event,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub batch: String,
    pub reserved: u64,
    pub settled: Option<u64>,
    pub generation_id: Option<String>,
    pub released: bool,
    pub uncertain: Option<String>,
    pub uncertain_generation: Option<String>,
}

impl Request {
    #[must_use]
    pub fn outstanding(&self) -> u64 {
        if self.settled.is_some() || self.released {
            0
        } else {
            self.reserved
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    pub allocated: u64,
    pub settled: u64,
    pub outstanding: u64,
    pub uncertain_requests: u32,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub software_cap: u64,
    pub ceiling: u64,
    pub allocations: BTreeMap<String, u64>,
    pub requests: BTreeMap<String, Request>,
    pub breaches: Vec<String>,
    pub key_checks: u32,
}

impl State {
    fn apply(&mut self, event: &Event) -> Result<()> {
        match event {
            Event::Cap {
                software_cap_micro,
                ceiling_micro,
                ..
            } => {
                if self.ceiling != 0 || *software_cap_micro > *ceiling_micro {
                    return Err("ledger cap must be first, once, and below its ceiling".into());
                }
                self.software_cap = *software_cap_micro;
                self.ceiling = *ceiling_micro;
            }
            Event::Allocate { batch, micro, .. } => {
                let allocated: u64 = self.allocations.values().sum();
                if self.ceiling == 0
                    || self.allocations.contains_key(batch)
                    || allocated
                        .checked_add(*micro)
                        .is_none_or(|sum| sum > self.software_cap)
                {
                    return Err(
                        "batch allocation duplicates a batch or exceeds the software cap".into(),
                    );
                }
                self.allocations.insert(batch.clone(), *micro);
            }
            Event::Reserve {
                request_id,
                batch,
                micro,
                ..
            } => {
                if self.requests.contains_key(request_id) || !self.allocations.contains_key(batch) {
                    return Err("reservation repeats a request or names no allocated batch".into());
                }
                self.requests.insert(
                    request_id.clone(),
                    Request {
                        batch: batch.clone(),
                        reserved: *micro,
                        ..Request::default()
                    },
                );
            }
            Event::Settle {
                request_id,
                micro,
                generation_id,
                ..
            } => {
                let request = self.request(request_id)?;
                if request.settled.is_some() || request.released {
                    return Err("request already has a terminal settlement".into());
                }
                request.settled = Some(*micro);
                request.generation_id = Some(generation_id.clone());
            }
            Event::Uncertain {
                request_id,
                reason,
                generation_id,
            } => {
                let request = self.request(request_id)?;
                if request.settled.is_some() || request.released || request.uncertain.is_some() {
                    return Err("uncertainty follows a terminal or repeated outcome".into());
                }
                request.uncertain = Some(reason.clone());
                request.uncertain_generation.clone_from(generation_id);
            }
            Event::Release { request_id, .. } => {
                let request = self.request(request_id)?;
                if request.settled.is_some() || request.released || request.uncertain.is_some() {
                    return Err("only an unsent request without another outcome is released".into());
                }
                request.released = true;
            }
            Event::Breach { request_id, .. } => {
                self.request(request_id)?;
                self.breaches.push(request_id.clone());
            }
            Event::KeyCheck { .. } => self.key_checks += 1,
        }
        Ok(())
    }

    fn request(&mut self, request_id: &str) -> Result<&mut Request> {
        self.requests
            .get_mut(request_id)
            .ok_or_else(|| "outcome names an unreserved request".into())
    }

    #[must_use]
    pub fn totals(&self, batch: Option<&str>) -> Totals {
        let mut totals = Totals {
            allocated: match batch {
                Some(batch) => self.allocations.get(batch).copied().unwrap_or(0),
                None => self.allocations.values().sum(),
            },
            ..Totals::default()
        };
        for request in self
            .requests
            .values()
            .filter(|request| batch.is_none_or(|batch| request.batch == batch))
        {
            totals.settled += request.settled.unwrap_or(0);
            totals.outstanding += request.outstanding();
            totals.uncertain_requests +=
                u32::from(request.uncertain.is_some() && request.settled.is_none());
        }
        totals
    }

    /// Refuse a reservation that would exceed the batch allocation or software
    /// cap, or whose hard bound would exceed the ceiling.
    pub fn admit(&self, batch: &str, reservation: u64, hard_bound: u64) -> Result<()> {
        if !self.breaches.is_empty() {
            return Err("a recorded billing breach freezes every batch".into());
        }
        let allocation = self
            .allocations
            .get(batch)
            .ok_or("batch has no allocation")?;
        let local = self.totals(Some(batch));
        let global = self.totals(None);
        let committed = |totals: Totals, extra: u64| {
            totals
                .settled
                .checked_add(totals.outstanding)
                .and_then(|sum| sum.checked_add(extra))
        };
        if reservation == 0
            || committed(local, reservation).is_none_or(|sum| sum > *allocation)
            || committed(global, reservation).is_none_or(|sum| sum > self.software_cap)
            || committed(global, hard_bound).is_none_or(|sum| sum > self.ceiling)
        {
            return Err(
                "reservation would exceed the batch allocation, software cap or ceiling".into(),
            );
        }
        Ok(())
    }
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

/// An exclusively opened ledger. Dropping it removes the lock file.
#[derive(Debug)]
pub struct Ledger {
    lock: PathBuf,
    file: File,
    seq: u64,
    last_sha256: String,
    pub state: State,
}

impl Ledger {
    /// Create a new ledger whose first event fixes the software cap and ceiling.
    pub fn create(path: &Path, software_cap: u64, ceiling: u64, note: &str) -> Result<Self> {
        OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(path)?;
        let mut ledger = Self::open(path)?;
        ledger.append(Event::Cap {
            software_cap_micro: software_cap,
            ceiling_micro: ceiling,
            note: note.into(),
        })?;
        Ok(ledger)
    }

    /// Open an existing ledger, verify its hash chain and replay its state.
    pub fn open(path: &Path) -> Result<Self> {
        let lock = path.with_extension("lock");
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&lock)
            .map_err(|_| "ledger lock exists: another writer is active or a previous run stopped; inspect before removing it")?;
        let opened = Self::load(path, lock.clone());
        if opened.is_err() {
            let _ = std::fs::remove_file(&lock);
        }
        opened
    }

    fn load(path: &Path, lock: PathBuf) -> Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_LEDGER_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_LEDGER_BYTES {
            return Err("ledger exceeds its byte ceiling".into());
        }
        let text = std::str::from_utf8(&bytes)?;
        if !text.is_empty() && !text.ends_with('\n') {
            return Err("ledger ends with a partial line; repair it explicitly".into());
        }
        let mut state = State::default();
        let mut last = sha256(b"");
        let mut seq = 0;
        for raw in text.lines() {
            let line: Line = serde_json::from_str(raw)?;
            if line.seq != seq + 1 || line.prev_sha256 != last {
                return Err("ledger hash chain or sequence is broken".into());
            }
            if seq == 0 && !matches!(line.event, Event::Cap { .. }) {
                return Err("ledger must begin with its cap".into());
            }
            state.apply(&line.event)?;
            seq = line.seq;
            last = sha256(raw.as_bytes());
        }
        let file = OpenOptions::new().append(true).open(path)?;
        Ok(Self {
            lock,
            file,
            seq,
            last_sha256: last,
            state,
        })
    }

    /// Validate, append and sync one event.
    pub fn append(&mut self, event: Event) -> Result<()> {
        let mut next = self.state.clone();
        next.apply(&event)?;
        let line = Line {
            seq: self.seq + 1,
            unix_seconds: now()?,
            prev_sha256: self.last_sha256.clone(),
            event,
        };
        let text = serde_json::to_string(&line)?;
        self.file.write_all(text.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.file.sync_all()?;
        self.seq += 1;
        self.last_sha256 = sha256(text.as_bytes());
        self.state = next;
        Ok(())
    }

    /// Settle once. Repeating the identical settlement is a no-op; a different
    /// one is a conflict.
    pub fn settle(&mut self, event: Event) -> Result<bool> {
        let Event::Settle {
            request_id,
            micro,
            generation_id,
            ..
        } = &event
        else {
            return Err("settle requires a settlement event".into());
        };
        if let Some(existing) = self.state.requests.get(request_id)
            && let Some(settled) = existing.settled
        {
            if settled == *micro && existing.generation_id.as_ref() == Some(generation_id) {
                return Ok(false);
            }
            return Err("conflicting settlement for an already settled request".into());
        }
        self.append(event)?;
        Ok(true)
    }
}

impl Drop for Ledger {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn scratch(name: &str) -> Result<PathBuf> {
        let directory = std::env::temp_dir().join(format!(
            "sigy-hosted-eval-{name}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        std::fs::create_dir_all(&directory)?;
        Ok(directory)
    }

    pub(crate) fn reserve(request_id: &str, batch: &str, micro: u64) -> Event {
        Event::Reserve {
            request_id: request_id.into(),
            batch: batch.into(),
            micro,
            hard_bound_micro: micro,
            model: "vendor/model".into(),
            manifest_sha256: sha256(b"manifest"),
            index: 0,
        }
    }

    fn settle(request_id: &str, micro: u64, generation: &str) -> Event {
        Event::Settle {
            request_id: request_id.into(),
            micro,
            cost_text: "0.001".into(),
            generation_id: generation.into(),
            provider: "Fixture".into(),
        }
    }

    #[test]
    fn allocation_and_caps_refuse_before_any_send() -> Result<()> {
        let path = scratch("caps")?.join("ledger.jsonl");
        let mut ledger = Ledger::create(&path, 100, 120, "fixture")?;
        ledger.append(Event::Allocate {
            batch: "a".into(),
            micro: 60,
            note: String::new(),
        })?;
        assert!(
            ledger
                .append(Event::Allocate {
                    batch: "b".into(),
                    micro: 41,
                    note: String::new(),
                })
                .is_err()
        );
        assert!(
            ledger
                .append(Event::Allocate {
                    batch: "a".into(),
                    micro: 1,
                    note: String::new(),
                })
                .is_err()
        );
        ledger.state.admit("a", 60, 60)?;
        assert!(ledger.state.admit("a", 61, 61).is_err());
        assert!(ledger.state.admit("a", 0, 0).is_err());
        assert!(ledger.state.admit("a", 10, 121).is_err());
        assert!(ledger.state.admit("missing", 1, 1).is_err());
        ledger.append(reserve("r1", "a", 50))?;
        assert!(ledger.state.admit("a", 11, 11).is_err());
        ledger.state.admit("a", 10, 10)?;
        assert!(ledger.append(reserve("r1", "a", 1)).is_err());
        assert!(ledger.append(reserve("r2", "missing", 1)).is_err());
        Ok(())
    }

    #[test]
    fn ambiguous_outcome_retains_liability_and_settlement_is_idempotent() -> Result<()> {
        let path = scratch("liability")?.join("ledger.jsonl");
        let mut ledger = Ledger::create(&path, 1000, 2000, "fixture")?;
        ledger.append(Event::Allocate {
            batch: "a".into(),
            micro: 1000,
            note: String::new(),
        })?;
        ledger.append(reserve("sent", "a", 100))?;
        ledger.append(Event::Uncertain {
            request_id: "sent".into(),
            reason: "timeout-after-send".into(),
            generation_id: None,
        })?;
        assert_eq!(ledger.state.totals(Some("a")).outstanding, 100);
        assert_eq!(ledger.state.totals(None).uncertain_requests, 1);
        assert!(
            ledger
                .append(Event::Release {
                    request_id: "sent".into(),
                    reason: "not allowed".into(),
                })
                .is_err()
        );
        assert!(
            ledger
                .append(Event::Uncertain {
                    request_id: "sent".into(),
                    reason: "again".into(),
                    generation_id: None,
                })
                .is_err()
        );
        // A later authoritative reconciliation may settle it exactly once.
        assert!(ledger.settle(settle("sent", 7, "gen-1"))?);
        assert!(!ledger.settle(settle("sent", 7, "gen-1"))?);
        assert!(ledger.settle(settle("sent", 8, "gen-1")).is_err());
        assert!(ledger.settle(settle("sent", 7, "gen-2")).is_err());
        assert!(ledger.settle(reserve("x", "a", 1)).is_err());
        ledger.append(reserve("unsent", "a", 50))?;
        ledger.append(Event::Release {
            request_id: "unsent".into(),
            reason: "connect-failed".into(),
        })?;
        assert!(ledger.settle(settle("unsent", 1, "gen-3")).is_err());
        let totals = ledger.state.totals(None);
        assert_eq!((totals.settled, totals.outstanding), (7, 0));
        assert!(
            ledger
                .append(Event::Breach {
                    request_id: "missing".into(),
                    reason: "x".into(),
                })
                .is_err()
        );
        ledger.append(Event::Breach {
            request_id: "sent".into(),
            reason: "cost above reservation".into(),
        })?;
        assert!(ledger.state.admit("a", 1, 1).is_err());
        assert!(ledger.append(settle("missing", 1, "gen")).is_err());
        Ok(())
    }

    #[test]
    fn reopen_replays_state_and_detects_tampering_and_concurrent_writers() -> Result<()> {
        let directory = scratch("reopen")?;
        let path = directory.join("ledger.jsonl");
        {
            let mut ledger = Ledger::create(&path, 1000, 2000, "fixture")?;
            assert!(Ledger::open(&path).is_err());
            ledger.append(Event::Allocate {
                batch: "a".into(),
                micro: 500,
                note: String::new(),
            })?;
            // A crash after reservation leaves no outcome.
            ledger.append(reserve("crashed", "a", 300))?;
            ledger.append(Event::KeyCheck {
                limit: Some("20".into()),
                limit_remaining: Some("20".into()),
                limit_reset: None,
                usage: Some("0".into()),
                is_free_tier: Some(false),
            })?;
        }
        let reopened = Ledger::open(&path)?;
        assert_eq!(reopened.state.totals(Some("a")).outstanding, 300);
        assert_eq!(reopened.state.key_checks, 1);
        assert!(reopened.state.admit("a", 201, 201).is_err());
        drop(reopened);
        assert!(Ledger::create(&path, 1, 2, "again").is_err());
        let original = std::fs::read_to_string(&path)?;
        std::fs::write(&path, original.replace("\"micro\":300", "\"micro\":3"))?;
        assert!(Ledger::open(&path).is_err());
        std::fs::write(&path, original.trim_end())?;
        assert!(Ledger::open(&path).is_err());
        let first_line_removed: String = original
            .lines()
            .skip(1)
            .flat_map(|line| [line, "\n"])
            .collect();
        std::fs::write(&path, first_line_removed)?;
        assert!(Ledger::open(&path).is_err());
        let other = directory.join("other.jsonl");
        assert!(Ledger::create(&other, 3, 2, "inverted").is_err());
        Ok(())
    }
}
