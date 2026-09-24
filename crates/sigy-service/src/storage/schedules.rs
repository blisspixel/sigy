//! One station rule and its next occurrence. Admitted plans stay immutable.

use std::time::Duration;

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::{
    Store,
    captures::CaptureJob,
    dvr::{self, ScheduledRecording},
    now_ms, validate_key,
};
use crate::{
    Error, Result,
    schedule::{
        self, Cadence, Clock, MAX_OCCURRENCES, MAX_SCHEDULES, Slot, Timing, add_days, civil_today,
        is_weekday, parse_date, resolve,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleRule {
    pub id: String,
    pub source_revision: String,
    pub zone: String,
    pub recurrence: String,
    pub civil_date: Option<String>,
    pub weekday: Option<i64>,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub duration_seconds: i64,
    pub maximum_bytes: i64,
    pub revision: i64,
    pub created_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleOccurrence {
    pub id: String,
    pub rule_id: String,
    pub rule_revision: i64,
    pub civil_date: String,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub offset_seconds: Option<i64>,
    pub transition_ms: Option<i64>,
    pub duration_seconds: i64,
    pub maximum_bytes: i64,
    pub state: String,
    pub miss_reason: Option<String>,
    pub recording_id: Option<String>,
    pub created_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleSaved {
    pub rule: ScheduleRule,
    pub occurrences: Vec<ScheduleOccurrence>,
    pub newly_created: bool,
}

#[derive(Debug, Clone)]
pub struct ScheduleDraft {
    pub id: String,
    pub source_revision: String,
    pub zone: String,
    pub recurrence: String,
    pub civil_date: Option<String>,
    pub weekday: Option<i64>,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub duration_seconds: i64,
    pub maximum_bytes: i64,
}

#[derive(Debug)]
pub(crate) struct ScheduleLaunch {
    pub job: CaptureJob,
    pub source_revision: String,
    pub remaining: Duration,
    pub maximum_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct ScheduleReconcile {
    pub launches: Vec<ScheduleLaunch>,
    pub quota_exhausted: bool,
}

impl Store {
    /// Store one rule and its next occurrence. An exact replay does not add another.
    /// # Errors
    /// Rejects a bad zone, an unknown source, or a reused id with different parameters.
    pub fn create_schedule_at(
        &mut self,
        draft: &ScheduleDraft,
        now_ms: i64,
    ) -> Result<ScheduleSaved> {
        let prepared = prepare(draft)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = read_rule(&tx, &prepared.id)? {
            if !same_rule(&existing, &prepared) {
                return Err(Error::IdempotencyConflict);
            }
            let saved = saved(&tx, &prepared.id, false)?;
            tx.commit()?;
            return Ok(saved);
        }
        let count: i64 =
            tx.query_row("SELECT count(*) FROM schedule_rules", [], |row| row.get(0))?;
        if count >= MAX_SCHEDULES {
            return Err(Error::CaptureCapacity);
        }
        let source_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_revisions WHERE id = ?1)",
            [&prepared.source_revision],
            |row| row.get(0),
        )?;
        if !source_exists {
            return Err(Error::NotFound);
        }
        tx.execute(
            "INSERT INTO schedule_rules(id, source_revision, zone, recurrence, civil_date, weekday, hour, minute, second, duration_seconds, maximum_bytes, revision, created_ms, updated_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?12)",
            params![
                prepared.id,
                prepared.source_revision,
                prepared.zone,
                prepared.recurrence,
                prepared.civil_date,
                prepared.weekday,
                prepared.hour,
                prepared.minute,
                prepared.second,
                prepared.duration_seconds,
                prepared.maximum_bytes,
                now_ms,
            ],
        )?;
        let rule = read_rule(&tx, &prepared.id)?.ok_or(Error::StorageIntegrity)?;
        materialize(&tx, &rule, now_ms)?;
        let saved = saved(&tx, &prepared.id, true)?;
        tx.commit()?;
        Ok(saved)
    }

    /// Change future slots. An admitted occurrence keeps the plan it already has.
    /// # Errors
    /// Rejects an unknown rule, a source change, or an invalid civil clock.
    pub fn revise_schedule_at(
        &mut self,
        draft: &ScheduleDraft,
        now_ms: i64,
    ) -> Result<ScheduleSaved> {
        let prepared = prepare(draft)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = read_rule(&tx, &prepared.id)?.ok_or(Error::NotFound)?;
        if existing.source_revision != prepared.source_revision {
            return Err(Error::InvalidInput("schedule source revision"));
        }
        if same_rule(&existing, &prepared) {
            let saved = saved(&tx, &prepared.id, false)?;
            tx.commit()?;
            return Ok(saved);
        }
        let revision = existing
            .revision
            .checked_add(1)
            .ok_or(Error::StorageIntegrity)?;
        tx.execute(
            "UPDATE schedule_rules SET zone = ?2, recurrence = ?3, civil_date = ?4, weekday = ?5, hour = ?6, minute = ?7, second = ?8, duration_seconds = ?9, maximum_bytes = ?10, revision = ?11, updated_ms = ?12 WHERE id = ?1",
            params![
                prepared.id,
                prepared.zone,
                prepared.recurrence,
                prepared.civil_date,
                prepared.weekday,
                prepared.hour,
                prepared.minute,
                prepared.second,
                prepared.duration_seconds,
                prepared.maximum_bytes,
                revision,
                now_ms,
            ],
        )?;
        tx.execute(
            "DELETE FROM schedule_occurrences WHERE rule_id = ?1 AND state = 'waiting'",
            [&prepared.id],
        )?;
        let rule = read_rule(&tx, &prepared.id)?.ok_or(Error::StorageIntegrity)?;
        materialize(&tx, &rule, now_ms)?;
        let saved = saved(&tx, &prepared.id, false)?;
        tx.commit()?;
        Ok(saved)
    }

    /// # Errors
    /// Rejects a malformed id or a corrupt row.
    pub fn schedule(&self, id: &str) -> Result<Option<ScheduleRule>> {
        validate_rule_id(id)?;
        read_rule(&self.connection, id)
    }

    /// # Errors
    /// Rejects a page outside 1..=64 or a corrupt row.
    pub fn schedules(&self, after: Option<&str>, limit: u32) -> Result<Vec<ScheduleRule>> {
        validate_page(limit)?;
        if let Some(id) = after {
            validate_rule_id(id)?;
        }
        let mut statement = self
            .connection
            .prepare("SELECT id FROM schedule_rules WHERE id > ?1 ORDER BY id LIMIT ?2")?;
        let ids = statement
            .query_map(params![after.unwrap_or(""), limit], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| read_rule(&self.connection, &id)?.ok_or(Error::StorageIntegrity))
            .collect()
    }

    /// # Errors
    /// Rejects a malformed id or a corrupt occurrence.
    pub fn schedule_occurrences(&self, id: &str) -> Result<Vec<ScheduleOccurrence>> {
        validate_rule_id(id)?;
        if read_rule(&self.connection, id)?.is_none() {
            return Err(Error::NotFound);
        }
        read_occurrences(&self.connection, id)
    }

    /// Resolve due rules. `admit` starts at most the open occurrence. A second call does not.
    /// # Errors
    /// Returns catalog errors. Storage quota is reported on the batch instead of failing earlier rules.
    pub(crate) fn reconcile_schedules_at(
        &mut self,
        now_ms: i64,
        admit: bool,
    ) -> Result<ScheduleReconcile> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM schedule_rules ORDER BY id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let mut launches = Vec::new();
        let mut quota_exhausted = false;
        for id in ids {
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            match reconcile_rule(&tx, &id, now_ms, admit) {
                Ok(launch) => {
                    tx.commit()?;
                    if let Some(launch) = launch {
                        launches.push(launch);
                    }
                }
                Err(Error::StorageQuota) => {
                    quota_exhausted = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(ScheduleReconcile {
            launches,
            quota_exhausted,
        })
    }

    /// # Errors
    /// Returns a storage error when a rule or occurrence breaks its contract.
    pub(crate) fn audit_schedules(&self) -> Result<()> {
        let analysis: i64 = self.connection.query_row(
            "SELECT count(*) FROM pragma_table_info('schedule_rules') WHERE name LIKE '%analysis%' OR name LIKE '%profile%'",
            [],
            |row| row.get(0),
        )?;
        let occurrence_analysis: i64 = self.connection.query_row(
            "SELECT count(*) FROM pragma_table_info('schedule_occurrences') WHERE name LIKE '%analysis%' OR name LIKE '%profile%'",
            [],
            |row| row.get(0),
        )?;
        if analysis != 0 || occurrence_analysis != 0 {
            return Err(Error::StorageIntegrity);
        }
        let broken: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schedule_occurrences o LEFT JOIN schedule_rules r ON r.id = o.rule_id WHERE r.id IS NULL OR (o.state = 'admitted' AND NOT EXISTS(SELECT 1 FROM recordings m WHERE m.id = o.recording_id)) OR (o.state != 'admitted' AND o.recording_id IS NOT NULL)) OR EXISTS(SELECT 1 FROM schedule_occurrences o JOIN capture_jobs c ON c.id = o.recording_id JOIN schedule_rules r ON r.id = o.rule_id WHERE o.state = 'admitted' AND (c.starts_ms != o.start_ms OR c.ends_ms != o.end_ms OR c.maximum_bytes != o.maximum_bytes OR c.source_revision != r.source_revision OR o.end_ms - o.start_ms != o.duration_seconds * 1000))",
            [],
            |row| row.get(0),
        )?;
        if broken {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }
}

/// Uses the wall clock. Tests pass an explicit instant to the `_at` methods.
/// # Errors
/// Returns the same errors as [`Store::create_schedule_at`].
pub fn create_schedule(store: &mut Store, draft: &ScheduleDraft) -> Result<ScheduleSaved> {
    store.create_schedule_at(draft, now_ms()?)
}

/// # Errors
/// Returns the same errors as [`Store::revise_schedule_at`].
pub fn revise_schedule(store: &mut Store, draft: &ScheduleDraft) -> Result<ScheduleSaved> {
    store.revise_schedule_at(draft, now_ms()?)
}

fn prepare(draft: &ScheduleDraft) -> Result<ScheduleDraft> {
    validate_rule_id(&draft.id)?;
    validate_key(&draft.source_revision, "source revision")?;
    validate_limits(draft.duration_seconds, draft.maximum_bytes)?;
    if !(0..24).contains(&draft.hour)
        || !(0..60).contains(&draft.minute)
        || !(0..60).contains(&draft.second)
    {
        return Err(Error::InvalidInput("civil time"));
    }
    let (_, zone) = schedule::canonical_zone(&draft.zone)?;
    let (recurrence, civil_date, weekday) = match draft.recurrence.as_str() {
        "once" => {
            let date = draft
                .civil_date
                .as_deref()
                .ok_or(Error::InvalidInput("civil date"))?;
            if draft.weekday.is_some() {
                return Err(Error::InvalidInput("weekday"));
            }
            let parsed = parse_date(date)?;
            ("once".to_owned(), Some(parsed.to_string()), None)
        }
        "daily" => {
            if draft.civil_date.is_some() || draft.weekday.is_some() {
                return Err(Error::InvalidInput("civil date"));
            }
            ("daily".to_owned(), None, None)
        }
        "weekly" => {
            let weekday = draft.weekday.ok_or(Error::InvalidInput("weekday"))?;
            if !(1..=7).contains(&weekday) || draft.civil_date.is_some() {
                return Err(Error::InvalidInput("weekday"));
            }
            ("weekly".to_owned(), None, Some(weekday))
        }
        _ => return Err(Error::InvalidInput("schedule recurrence")),
    };
    Ok(ScheduleDraft {
        id: draft.id.clone(),
        source_revision: draft.source_revision.clone(),
        zone,
        recurrence,
        civil_date,
        weekday,
        hour: draft.hour,
        minute: draft.minute,
        second: draft.second,
        duration_seconds: draft.duration_seconds,
        maximum_bytes: draft.maximum_bytes,
    })
}

fn same_rule(existing: &ScheduleRule, draft: &ScheduleDraft) -> bool {
    existing.source_revision == draft.source_revision
        && existing.zone == draft.zone
        && existing.recurrence == draft.recurrence
        && existing.civil_date == draft.civil_date
        && existing.weekday == draft.weekday
        && existing.hour == draft.hour
        && existing.minute == draft.minute
        && existing.second == draft.second
        && existing.duration_seconds == draft.duration_seconds
        && existing.maximum_bytes == draft.maximum_bytes
}

fn materialize(tx: &rusqlite::Connection, rule: &ScheduleRule, now_ms: i64) -> Result<()> {
    if waiting(tx, &rule.id)? || open_admission(tx, &rule.id, now_ms)? {
        return Ok(());
    }
    let zone = schedule::zone(&rule.zone)?;
    let Some(slot) = find_next(tx, rule, &zone, now_ms)? else {
        return Ok(());
    };
    insert_slot(tx, rule, &slot, now_ms)
}

fn reconcile_rule(
    tx: &rusqlite::Connection,
    id: &str,
    now_ms: i64,
    admit: bool,
) -> Result<Option<ScheduleLaunch>> {
    let rule = read_rule(tx, id)?.ok_or(Error::StorageIntegrity)?;
    if let Some(occurrence) = waiting_occurrence(tx, id)? {
        let slot = slot_from_occurrence(&rule, &occurrence)?;
        match slot.timing(now_ms) {
            Timing::Future => return Ok(None),
            Timing::Ended => {
                mark_missed(tx, &occurrence, &slot)?;
                materialize(
                    tx,
                    &read_rule(tx, id)?.ok_or(Error::StorageIntegrity)?,
                    now_ms,
                )?;
                return Ok(None);
            }
            Timing::Open if !admit => return Ok(None),
            Timing::Open => return admit_occurrence(tx, &rule, &occurrence, now_ms),
        }
    }
    materialize(tx, &rule, now_ms)?;
    let Some(occurrence) = waiting_occurrence(tx, id)? else {
        return Ok(None);
    };
    let slot = slot_from_occurrence(&rule, &occurrence)?;
    if admit && slot.timing(now_ms) == Timing::Open {
        return admit_occurrence(tx, &rule, &occurrence, now_ms);
    }
    Ok(None)
}

fn admit_occurrence(
    tx: &rusqlite::Connection,
    rule: &ScheduleRule,
    occurrence: &ScheduleOccurrence,
    now_ms: i64,
) -> Result<Option<ScheduleLaunch>> {
    let start_ms = occurrence.start_ms.ok_or(Error::StorageIntegrity)?;
    let end_ms = occurrence.end_ms.ok_or(Error::StorageIntegrity)?;
    let seconds =
        u64::try_from(occurrence.duration_seconds).map_err(|_| Error::StorageIntegrity)?;
    let maximum = u64::try_from(occurrence.maximum_bytes).map_err(|_| Error::StorageIntegrity)?;
    let job = match dvr::admit_scheduled_recording(
        tx,
        &ScheduledRecording {
            id: &occurrence.id,
            source: &rule.source_revision,
            planned_start_ms: start_ms,
            planned_end_ms: end_ms,
            seconds,
            maximum,
            now_ms,
        },
    ) {
        Err(Error::CaptureCapacity) => return Ok(None),
        other => other?,
    };
    let changed = tx.execute(
        "UPDATE schedule_occurrences SET state = 'admitted', recording_id = ?2 WHERE id = ?1 AND state = 'waiting'",
        params![occurrence.id, occurrence.id],
    )?;
    if changed != 1 {
        return Err(Error::StorageIntegrity);
    }
    let remaining_ms = u64::try_from(end_ms - now_ms).map_err(|_| Error::StorageIntegrity)?;
    Ok(Some(ScheduleLaunch {
        job,
        source_revision: rule.source_revision.clone(),
        remaining: Duration::from_millis(remaining_ms),
        maximum_bytes: maximum,
    }))
}

fn mark_missed(
    tx: &rusqlite::Connection,
    occurrence: &ScheduleOccurrence,
    slot: &Slot,
) -> Result<()> {
    let reason = match slot {
        Slot::Spring { .. } => "spring_forward",
        Slot::Window { .. } => "elapsed",
    };
    let changed = tx.execute(
        "UPDATE schedule_occurrences SET state = 'missed', miss_reason = ?2 WHERE id = ?1 AND state = 'waiting'",
        params![occurrence.id, reason],
    )?;
    if changed != 1 {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

fn find_next(
    tx: &rusqlite::Connection,
    rule: &ScheduleRule,
    zone: &jiff::tz::TimeZone,
    now_ms: i64,
) -> Result<Option<Slot>> {
    let clock = clock_of(rule)?;
    match cadence(rule)? {
        Cadence::Once(date) => {
            if taken(tx, rule, &date.to_string())? {
                Ok(None)
            } else {
                Ok(Some(resolve(zone, date, &clock)?))
            }
        }
        Cadence::Daily => scan(tx, rule, zone, &clock, now_ms, None),
        Cadence::Weekly(weekday) => scan(tx, rule, zone, &clock, now_ms, Some(weekday)),
    }
}

fn scan(
    tx: &rusqlite::Connection,
    rule: &ScheduleRule,
    zone: &jiff::tz::TimeZone,
    clock: &Clock,
    now_ms: i64,
    weekday: Option<i8>,
) -> Result<Option<Slot>> {
    let today = civil_today(zone, now_ms)?;
    for offset in 0..400 {
        let date = add_days(today, offset)?;
        if weekday.is_some_and(|day| !is_weekday(date, day)) {
            continue;
        }
        if taken(tx, rule, &date.to_string())? {
            continue;
        }
        let slot = resolve(zone, date, clock)?;
        if slot.timing(now_ms) != Timing::Ended {
            return Ok(Some(slot));
        }
    }
    Ok(None)
}

fn insert_slot(
    tx: &rusqlite::Connection,
    rule: &ScheduleRule,
    slot: &Slot,
    now_ms: i64,
) -> Result<()> {
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM schedule_occurrences WHERE rule_id = ?1",
        [&rule.id],
        |row| row.get(0),
    )?;
    if count >= MAX_OCCURRENCES {
        return Ok(());
    }
    let id = format!("{}:{}", rule.id, slot.civil_date());
    validate_key(&id, "schedule occurrence")?;
    let (start_ms, end_ms, offset_seconds, transition_ms, state, reason) = match slot {
        Slot::Window {
            start_ms,
            end_ms,
            offset_seconds,
            ..
        } => {
            let missed = slot.timing(now_ms) == Timing::Ended;
            (
                Some(*start_ms),
                Some(*end_ms),
                Some(i64::from(*offset_seconds)),
                None,
                if missed { "missed" } else { "waiting" },
                if missed { Some("elapsed") } else { None },
            )
        }
        Slot::Spring { transition_ms, .. } => {
            let missed = slot.timing(now_ms) == Timing::Ended;
            (
                None,
                None,
                None,
                Some(*transition_ms),
                if missed { "missed" } else { "waiting" },
                if missed { Some("spring_forward") } else { None },
            )
        }
    };
    tx.execute(
        "INSERT INTO schedule_occurrences(id, rule_id, rule_revision, civil_date, hour, minute, second, start_ms, end_ms, offset_seconds, transition_ms, duration_seconds, maximum_bytes, state, miss_reason, recording_id, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL, ?16)",
        params![
            id,
            rule.id,
            rule.revision,
            slot.civil_date(),
            rule.hour,
            rule.minute,
            rule.second,
            start_ms,
            end_ms,
            offset_seconds,
            transition_ms,
            rule.duration_seconds,
            rule.maximum_bytes,
            state,
            reason,
            now_ms,
        ],
    )?;
    Ok(())
}

fn slot_from_occurrence(rule: &ScheduleRule, occurrence: &ScheduleOccurrence) -> Result<Slot> {
    let zone = schedule::zone(&rule.zone)?;
    let saved = Clock {
        hour: i8::try_from(occurrence.hour).map_err(|_| Error::StorageIntegrity)?,
        minute: i8::try_from(occurrence.minute).map_err(|_| Error::StorageIntegrity)?,
        second: i8::try_from(occurrence.second).map_err(|_| Error::StorageIntegrity)?,
        duration_seconds: occurrence.duration_seconds,
    };
    resolve(&zone, parse_date(&occurrence.civil_date)?, &saved)
}

fn clock_of(rule: &ScheduleRule) -> Result<Clock> {
    Ok(Clock {
        hour: i8::try_from(rule.hour).map_err(|_| Error::StorageIntegrity)?,
        minute: i8::try_from(rule.minute).map_err(|_| Error::StorageIntegrity)?,
        second: i8::try_from(rule.second).map_err(|_| Error::StorageIntegrity)?,
        duration_seconds: rule.duration_seconds,
    })
}

fn cadence(rule: &ScheduleRule) -> Result<Cadence> {
    match rule.recurrence.as_str() {
        "once" => Ok(Cadence::Once(parse_date(
            rule.civil_date.as_deref().ok_or(Error::StorageIntegrity)?,
        )?)),
        "daily" => Ok(Cadence::Daily),
        "weekly" => {
            let weekday = i8::try_from(rule.weekday.ok_or(Error::StorageIntegrity)?)
                .map_err(|_| Error::StorageIntegrity)?;
            Ok(Cadence::Weekly(weekday))
        }
        _ => Err(Error::StorageIntegrity),
    }
}

fn taken(tx: &rusqlite::Connection, rule: &ScheduleRule, civil_date: &str) -> Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM schedule_occurrences WHERE rule_id = ?1 AND civil_date = ?2 AND hour = ?3 AND minute = ?4 AND second = ?5)",
        params![rule.id, civil_date, rule.hour, rule.minute, rule.second],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn waiting(tx: &rusqlite::Connection, id: &str) -> Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM schedule_occurrences WHERE rule_id = ?1 AND state = 'waiting')",
        [id],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn waiting_occurrence(tx: &rusqlite::Connection, id: &str) -> Result<Option<ScheduleOccurrence>> {
    let occurrence_id: Option<String> = tx
        .query_row(
            "SELECT id FROM schedule_occurrences WHERE rule_id = ?1 AND state = 'waiting'",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    occurrence_id
        .map(|occurrence| read_occurrence(tx, &occurrence))
        .transpose()
}

fn open_admission(tx: &rusqlite::Connection, id: &str, now_ms: i64) -> Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM schedule_occurrences WHERE rule_id = ?1 AND state = 'admitted' AND end_ms > ?2)",
        params![id, now_ms],
        |row| row.get(0),
    )
    .map_err(Error::from)
}

fn saved(tx: &rusqlite::Connection, id: &str, newly_created: bool) -> Result<ScheduleSaved> {
    Ok(ScheduleSaved {
        rule: read_rule(tx, id)?.ok_or(Error::StorageIntegrity)?,
        occurrences: read_occurrences(tx, id)?,
        newly_created,
    })
}

fn read_rule(connection: &rusqlite::Connection, id: &str) -> Result<Option<ScheduleRule>> {
    connection
        .query_row(
            "SELECT source_revision, zone, recurrence, civil_date, weekday, hour, minute, second, duration_seconds, maximum_bytes, revision, created_ms, updated_ms FROM schedule_rules WHERE id = ?1",
            [id],
            |row| {
                Ok(ScheduleRule {
                    id: id.to_owned(),
                    source_revision: row.get(0)?,
                    zone: row.get(1)?,
                    recurrence: row.get(2)?,
                    civil_date: row.get(3)?,
                    weekday: row.get(4)?,
                    hour: row.get(5)?,
                    minute: row.get(6)?,
                    second: row.get(7)?,
                    duration_seconds: row.get(8)?,
                    maximum_bytes: row.get(9)?,
                    revision: row.get(10)?,
                    created_ms: row.get(11)?,
                    updated_ms: row.get(12)?,
                })
            },
        )
        .optional()
        .map_err(Error::from)
}

fn read_occurrences(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Vec<ScheduleOccurrence>> {
    let mut statement = connection.prepare(
        "SELECT id FROM schedule_occurrences WHERE rule_id = ?1 ORDER BY civil_date, hour, minute, second",
    )?;
    let ids = statement
        .query_map([id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|occurrence| read_occurrence(connection, &occurrence))
        .collect()
}

fn read_occurrence(connection: &rusqlite::Connection, id: &str) -> Result<ScheduleOccurrence> {
    connection
        .query_row(
            "SELECT rule_id, rule_revision, civil_date, hour, minute, second, start_ms, end_ms, offset_seconds, transition_ms, duration_seconds, maximum_bytes, state, miss_reason, recording_id, created_ms FROM schedule_occurrences WHERE id = ?1",
            [id],
            |row| {
                Ok(ScheduleOccurrence {
                    id: id.to_owned(),
                    rule_id: row.get(0)?,
                    rule_revision: row.get(1)?,
                    civil_date: row.get(2)?,
                    hour: row.get(3)?,
                    minute: row.get(4)?,
                    second: row.get(5)?,
                    start_ms: row.get(6)?,
                    end_ms: row.get(7)?,
                    offset_seconds: row.get(8)?,
                    transition_ms: row.get(9)?,
                    duration_seconds: row.get(10)?,
                    maximum_bytes: row.get(11)?,
                    state: row.get(12)?,
                    miss_reason: row.get(13)?,
                    recording_id: row.get(14)?,
                    created_ms: row.get(15)?,
                })
            },
        )
        .map_err(Error::from)
}

fn validate_rule_id(id: &str) -> Result<()> {
    validate_key(id, "schedule ID")?;
    if id.len() > 80 {
        return Err(Error::InvalidInput("schedule ID"));
    }
    Ok(())
}

fn validate_page(limit: u32) -> Result<()> {
    if !(1..=64).contains(&limit) {
        return Err(Error::InvalidInput("schedule page size"));
    }
    Ok(())
}

fn validate_limits(seconds: i64, bytes: i64) -> Result<()> {
    let seconds = u64::try_from(seconds)
        .map_err(|_| Error::InvalidInput("recording duration or byte ceiling"))?;
    let bytes = u64::try_from(bytes)
        .map_err(|_| Error::InvalidInput("recording duration or byte ceiling"))?;
    if !(1..=crate::sources::http::MAXIMUM_DURATION.as_secs()).contains(&seconds)
        || !(1..=crate::sources::http::MAXIMUM_BODY_BYTES).contains(&bytes)
    {
        return Err(Error::InvalidInput("recording duration or byte ceiling"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ScheduleDraft, Store};
    use crate::{
        Error,
        sources::{HttpSource, NetworkScope},
        storage::dvr::{GapCause, Publication},
    };

    type TestResult = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

    fn setup(path: &std::path::Path) -> Result<Store, Error> {
        let mut store = Store::open(path)?;
        let executable =
            std::env::current_exe().map_err(|_| Error::InvalidInput("test executable"))?;
        store.configure_dvr(
            512 * 1024 * 1024,
            64 * 1024 * 1024,
            14,
            executable
                .to_str()
                .ok_or(Error::InvalidInput("test executable"))?,
        )?;
        store.register_source(
            "radio:v1",
            &HttpSource::new(
                "Test radio",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
        Ok(store)
    }

    fn draft(id: &str, zone: &str, recurrence: &str, hour: i64, minute: i64) -> ScheduleDraft {
        ScheduleDraft {
            id: id.to_owned(),
            source_revision: "radio:v1".to_owned(),
            zone: zone.to_owned(),
            recurrence: recurrence.to_owned(),
            civil_date: None,
            weekday: None,
            hour,
            minute,
            second: 0,
            duration_seconds: 120,
            maximum_bytes: 1024,
        }
    }

    fn once(id: &str, zone: &str, civil: &str, seconds: i64) -> ScheduleDraft {
        let mut parts = civil.split('T');
        let date = parts.next().unwrap_or(civil);
        let time = parts.next().unwrap_or("00:00:00");
        let mut clock = time.split(':');
        let hour = clock.next().unwrap_or("0").parse().unwrap_or(0);
        let minute = clock.next().unwrap_or("0").parse().unwrap_or(0);
        let second = clock.next().unwrap_or("0").parse().unwrap_or(0);
        let mut draft = draft(id, zone, "once", hour, minute);
        draft.civil_date = Some(date.to_owned());
        draft.second = second;
        draft.duration_seconds = seconds;
        draft
    }

    fn jobs(store: &Store) -> Result<i64, Error> {
        store
            .connection
            .query_row("SELECT count(*) FROM capture_jobs", [], |row| row.get(0))
            .map_err(Error::from)
    }

    #[test]
    fn late_start_records_one_prefix_gap_and_no_second_job() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let mut spec = once("late", "Etc/UTC", "2026-09-22T12:00:00", 120);
        spec.maximum_bytes = 4096;
        let start_ms = 1_790_078_400_000_i64;
        let now_ms = start_ms + 30_000;
        let created = store.create_schedule_at(&spec, now_ms)?;
        assert!(created.newly_created);
        assert_eq!(created.occurrences.len(), 1);
        assert_eq!(created.occurrences[0].state, "waiting");
        let first = store.reconcile_schedules_at(now_ms, true)?;
        assert_eq!(first.launches.len(), 1);
        assert!(!first.quota_exhausted);
        let again = store.reconcile_schedules_at(now_ms, true)?;
        assert!(again.launches.is_empty());
        assert_eq!(jobs(&store)?, 1);
        let recording = store.recording("late:2026-09-22")?;
        assert_eq!(recording.gaps.len(), 1);
        assert_eq!(recording.gaps[0].cause, GapCause::LateStart);
        assert_eq!(recording.gaps[0].start_us, 0);
        assert_eq!(recording.gaps[0].end_us, 30_000_000);
        let capture = store.capture("late:2026-09-22")?.ok_or("missing capture")?;
        assert_eq!(capture.plan.starts_ms(), start_ms);
        assert_eq!(capture.plan.ends_ms(), start_ms + 120_000);
        assert_eq!(capture.plan.maximum_bytes(), 4096);
        let mut publication = Publication {
            bytes: 64,
            sha256: "ab".repeat(32),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![crate::sources::HttpHop {
                origin: "https://example.com".into(),
                peer: std::net::SocketAddr::from(([8, 8, 8, 8], 443)),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
            gap: None,
        };
        let _ = &mut publication;
        store.publish_recording(&capture.version, &publication)?;
        let published = store.recording("late:2026-09-22")?;
        assert_eq!(published.intervals.len(), 1);
        assert_eq!(published.intervals[0].decoded_start_us, 30_000_000);
        assert_eq!(published.intervals[0].decoded_end_us, 31_000_000);
        drop(store);
        Store::open(&directory.path().join("catalog.sqlite3"))?;
        Ok(())
    }

    #[test]
    fn missed_window_is_not_backfilled() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let spec = draft("daily", "Etc/UTC", "daily", 12, 0);
        let now_ms = 1_790_078_400_000_i64 + 2 * 60_000;
        let created = store.create_schedule_at(&spec, now_ms)?;
        assert!(
            created
                .occurrences
                .iter()
                .all(|row| row.civil_date != "2026-09-22")
        );
        assert_eq!(created.occurrences.len(), 1);
        assert_eq!(created.occurrences[0].state, "waiting");
        assert_eq!(created.occurrences[0].civil_date, "2026-09-23");
        let batch = store.reconcile_schedules_at(now_ms, true)?;
        assert!(batch.launches.is_empty());
        assert_eq!(jobs(&store)?, 0);
        let replay = store.create_schedule_at(&spec, now_ms)?;
        assert!(!replay.newly_created);
        assert_eq!(replay.occurrences.len(), 1);
        Ok(())
    }

    #[test]
    fn spring_forward_is_missed_without_a_job() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let spec = once("spring", "America/New_York", "2026-03-08T02:30:00", 60);
        let before = 1_772_953_200_000_i64 - 60_000;
        let created = store.create_schedule_at(&spec, before)?;
        assert_eq!(created.occurrences[0].state, "waiting");
        assert!(created.occurrences[0].start_ms.is_none());
        assert_eq!(jobs(&store)?, 0);
        let after = 1_772_953_200_000_i64 + 30 * 60_000;
        let batch = store.reconcile_schedules_at(after, true)?;
        assert!(batch.launches.is_empty());
        let rows = store.schedule_occurrences("spring")?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, "missed");
        assert_eq!(rows[0].miss_reason.as_deref(), Some("spring_forward"));
        assert!(rows[0].recording_id.is_none());
        assert_eq!(jobs(&store)?, 0);
        Ok(())
    }

    #[test]
    fn fall_back_admits_the_earlier_offset_once() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let spec = once("fall", "America/New_York", "2026-11-01T01:30:00", 900);
        let earlier = 1_793_511_000_000_i64;
        let late = earlier + 5 * 60_000;
        store.create_schedule_at(&spec, late)?;
        let batch = store.reconcile_schedules_at(late, true)?;
        assert_eq!(batch.launches.len(), 1);
        let row = &store.schedule_occurrences("fall")?[0];
        assert_eq!(row.state, "admitted");
        assert_eq!(row.start_ms, Some(earlier));
        assert_eq!(row.offset_seconds, Some(-4 * 3600));
        let recording = store.recording(&row.id)?;
        assert_eq!(recording.gaps[0].end_us, 5 * 60 * 1_000_000);
        let later = 1_793_514_600_000_i64 + 5 * 60_000;
        let second = store.reconcile_schedules_at(later, true)?;
        assert!(second.launches.is_empty());
        assert_eq!(jobs(&store)?, 1);
        Ok(())
    }

    #[test]
    fn fall_back_seen_on_the_later_offset_is_missed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let spec = once("later", "America/New_York", "2026-11-01T01:30:00", 900);
        let during_later = 1_793_514_600_000_i64 + 60_000;
        let created = store.create_schedule_at(&spec, during_later)?;
        assert_eq!(created.occurrences[0].state, "missed");
        assert_eq!(
            created.occurrences[0].miss_reason.as_deref(),
            Some("elapsed")
        );
        assert_eq!(created.occurrences[0].start_ms, Some(1_793_511_000_000));
        let batch = store.reconcile_schedules_at(during_later, true)?;
        assert!(batch.launches.is_empty());
        assert_eq!(jobs(&store)?, 0);
        Ok(())
    }

    #[test]
    fn revising_a_rule_does_not_rewrite_an_admitted_plan() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let spec = once("plan", "Etc/UTC", "2026-09-22T12:00:00", 120);
        let start_ms = 1_790_078_400_000_i64;
        store.create_schedule_at(&spec, start_ms)?;
        store.reconcile_schedules_at(start_ms, true)?;
        let mut changed = spec.clone();
        changed.duration_seconds = 180;
        changed.maximum_bytes = 2048;
        changed.minute = 30;
        store.revise_schedule_at(&changed, start_ms)?;
        let capture = store.capture("plan:2026-09-22")?.ok_or("missing capture")?;
        assert_eq!(capture.plan.starts_ms(), start_ms);
        assert_eq!(capture.plan.ends_ms(), start_ms + 120_000);
        assert_eq!(capture.plan.maximum_bytes(), 1024);
        let rows = store.schedule_occurrences("plan")?;
        let admitted = rows
            .iter()
            .find(|row| row.state == "admitted")
            .ok_or("missing plan")?;
        assert_eq!(admitted.duration_seconds, 120);
        assert_eq!(admitted.maximum_bytes, 1024);
        assert_eq!(jobs(&store)?, 1);
        let rule = store.schedule("plan")?.ok_or("missing rule")?;
        assert_eq!(rule.duration_seconds, 180);
        assert_eq!(rule.maximum_bytes, 2048);
        assert_eq!(rule.revision, 1);
        Ok(())
    }

    #[test]
    fn weekly_rule_materializes_only_the_next_weekday() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let mut spec = draft("week", "Etc/UTC", "weekly", 0, 0);
        spec.weekday = Some(1);
        spec.duration_seconds = 60;
        let wednesday = 1_790_164_800_000_i64;
        let created = store.create_schedule_at(&spec, wednesday)?;
        assert_eq!(created.occurrences.len(), 1);
        assert_eq!(created.occurrences[0].civil_date, "2026-09-28");
        assert_eq!(created.occurrences[0].state, "waiting");
        let batch = store.reconcile_schedules_at(wednesday, true)?;
        assert!(batch.launches.is_empty());
        assert_eq!(jobs(&store)?, 0);
        Ok(())
    }

    #[test]
    fn analysis_columns_are_absent() -> TestResult {
        let directory = tempfile::tempdir()?;
        let store = setup(&directory.path().join("catalog.sqlite3"))?;
        store.audit_schedules()?;
        Ok(())
    }
}
