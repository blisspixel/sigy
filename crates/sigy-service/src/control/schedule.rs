//! Station schedules. One rule, one next occurrence, and no analysis profile.

use serde::{Deserialize, Serialize};

use super::{Operation, Snapshot};
use crate::{
    Error, Result,
    schedule::{parse_clock, parse_weekday, weekday_name},
    storage::{
        Store,
        schedules::{ScheduleDraft, ScheduleOccurrence, ScheduleRule, ScheduleSaved},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleOperation {
    Create {
        id: String,
        source_revision: String,
        zone: String,
        #[serde(default)]
        once: Option<String>,
        #[serde(default)]
        daily: Option<String>,
        #[serde(default)]
        weekly: Option<String>,
        #[serde(default)]
        at: Option<String>,
        seconds: u64,
        maximum_bytes: u64,
        #[serde(default)]
        analysis_profile: Option<String>,
    },
    Revise {
        id: String,
        zone: String,
        #[serde(default)]
        once: Option<String>,
        #[serde(default)]
        daily: Option<String>,
        #[serde(default)]
        weekly: Option<String>,
        #[serde(default)]
        at: Option<String>,
        seconds: u64,
        maximum_bytes: u64,
        #[serde(default)]
        analysis_profile: Option<String>,
    },
    List {
        after: Option<String>,
        limit: u32,
    },
    Show {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulePage {
    pub rules: Vec<ScheduleRuleView>,
    pub occurrences: Vec<ScheduleOccurrenceView>,
    pub next_after: Option<String>,
    pub newly_created: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRuleView {
    pub id: String,
    pub source_revision: String,
    pub zone: String,
    pub recurrence: String,
    pub civil_date: Option<String>,
    pub weekday: Option<String>,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub duration_seconds: i64,
    pub maximum_bytes: i64,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleOccurrenceView {
    pub id: String,
    pub civil_date: String,
    pub state: String,
    pub miss_reason: Option<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub offset_seconds: Option<i64>,
    pub transition_ms: Option<i64>,
    pub duration_seconds: i64,
    pub maximum_bytes: i64,
    pub recording_id: Option<String>,
}

pub(super) fn apply(store: &mut Store, operation: ScheduleOperation) -> Result<Snapshot> {
    let page = match operation {
        ScheduleOperation::Create {
            id,
            source_revision,
            zone,
            once,
            daily,
            weekly,
            at,
            seconds,
            maximum_bytes,
            analysis_profile,
        } => {
            reject_analysis(analysis_profile.as_deref())?;
            let draft = draft(DraftInput {
                id: &id,
                source: &source_revision,
                zone: &zone,
                once: once.as_deref(),
                daily: daily.as_deref(),
                weekly: weekly.as_deref(),
                at: at.as_deref(),
                seconds,
                maximum_bytes,
            })?;
            let saved = crate::storage::schedules::create_schedule(store, &draft)?;
            page_from_saved(saved)?
        }
        ScheduleOperation::Revise {
            id,
            zone,
            once,
            daily,
            weekly,
            at,
            seconds,
            maximum_bytes,
            analysis_profile,
        } => {
            reject_analysis(analysis_profile.as_deref())?;
            let source_revision = store.schedule(&id)?.ok_or(Error::NotFound)?.source_revision;
            let draft = draft(DraftInput {
                id: &id,
                source: &source_revision,
                zone: &zone,
                once: once.as_deref(),
                daily: daily.as_deref(),
                weekly: weekly.as_deref(),
                at: at.as_deref(),
                seconds,
                maximum_bytes,
            })?;
            let saved = crate::storage::schedules::revise_schedule(store, &draft)?;
            page_from_saved(saved)?
        }
        ScheduleOperation::Show { id } => {
            let rule = store.schedule(&id)?.ok_or(Error::NotFound)?;
            let occurrences = store.schedule_occurrences(&id)?;
            SchedulePage {
                rules: vec![rule_view(rule)?],
                occurrences: occurrences.into_iter().map(occurrence_view).collect(),
                next_after: None,
                newly_created: None,
            }
        }
        ScheduleOperation::List { after, limit } => {
            let rules = store.schedules(after.as_deref(), limit)?;
            let next_after = if let Some(last) = rules.last()
                && !store.schedules(Some(&last.id), 1)?.is_empty()
            {
                Some(last.id.clone())
            } else {
                None
            };
            SchedulePage {
                rules: rules.into_iter().map(rule_view).collect::<Result<_>>()?,
                occurrences: Vec::new(),
                next_after,
                newly_created: None,
            }
        }
    };
    let mut view = super::apply(store, Operation::Status {})?;
    view.schedule = Some(page);
    Ok(view)
}

pub(crate) fn show(store: &Store, id: &str) -> Result<SchedulePage> {
    let rule = store.schedule(id)?.ok_or(Error::NotFound)?;
    let occurrences = store.schedule_occurrences(id)?;
    Ok(SchedulePage {
        rules: vec![rule_view(rule)?],
        occurrences: occurrences.into_iter().map(occurrence_view).collect(),
        next_after: None,
        newly_created: None,
    })
}

fn reject_analysis(profile: Option<&str>) -> Result<()> {
    if profile.is_some() {
        return Err(Error::InvalidInput("analysis profile cannot be attached"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct DraftInput<'a> {
    id: &'a str,
    source: &'a str,
    zone: &'a str,
    once: Option<&'a str>,
    daily: Option<&'a str>,
    weekly: Option<&'a str>,
    at: Option<&'a str>,
    seconds: u64,
    maximum_bytes: u64,
}

fn draft(input: DraftInput<'_>) -> Result<ScheduleDraft> {
    let DraftInput {
        id,
        source,
        zone,
        once,
        daily,
        weekly,
        at,
        seconds,
        maximum_bytes,
    } = input;
    let (recurrence, civil_date, weekday, hour, minute, second) = match (once, daily, weekly) {
        (Some(civil), None, None) => {
            if at.is_some() {
                return Err(Error::InvalidInput("civil time"));
            }
            let (date, time) = civil
                .split_once('T')
                .ok_or(Error::InvalidInput("civil date"))?;
            let (hour, minute, second) = parse_clock(time)?;
            ("once", Some(date.to_owned()), None, hour, minute, second)
        }
        (None, Some(time), None) => {
            if at.is_some() {
                return Err(Error::InvalidInput("civil time"));
            }
            let (hour, minute, second) = parse_clock(time)?;
            ("daily", None, None, hour, minute, second)
        }
        (None, None, Some(day)) => {
            let time = at.ok_or(Error::InvalidInput("civil time"))?;
            let (hour, minute, second) = parse_clock(time)?;
            (
                "weekly",
                None,
                Some(i64::from(parse_weekday(day)?)),
                hour,
                minute,
                second,
            )
        }
        _ => return Err(Error::InvalidInput("schedule recurrence")),
    };
    Ok(ScheduleDraft {
        id: id.to_owned(),
        source_revision: source.to_owned(),
        zone: zone.to_owned(),
        recurrence: recurrence.to_owned(),
        civil_date,
        weekday,
        hour: i64::from(hour),
        minute: i64::from(minute),
        second: i64::from(second),
        duration_seconds: i64::try_from(seconds)
            .map_err(|_| Error::InvalidInput("recording duration or byte ceiling"))?,
        maximum_bytes: i64::try_from(maximum_bytes)
            .map_err(|_| Error::InvalidInput("recording duration or byte ceiling"))?,
    })
}

fn page_from_saved(saved: ScheduleSaved) -> Result<SchedulePage> {
    Ok(SchedulePage {
        rules: vec![rule_view(saved.rule)?],
        occurrences: saved.occurrences.into_iter().map(occurrence_view).collect(),
        next_after: None,
        newly_created: Some(saved.newly_created),
    })
}

fn rule_view(rule: ScheduleRule) -> Result<ScheduleRuleView> {
    let weekday = rule
        .weekday
        .map(|day| {
            let day = i8::try_from(day).map_err(|_| Error::StorageIntegrity)?;
            weekday_name(day).map(str::to_owned)
        })
        .transpose()?;
    Ok(ScheduleRuleView {
        id: rule.id,
        source_revision: rule.source_revision,
        zone: rule.zone,
        recurrence: rule.recurrence,
        civil_date: rule.civil_date,
        weekday,
        hour: rule.hour,
        minute: rule.minute,
        second: rule.second,
        duration_seconds: rule.duration_seconds,
        maximum_bytes: rule.maximum_bytes,
        revision: rule.revision,
    })
}

fn occurrence_view(occurrence: ScheduleOccurrence) -> ScheduleOccurrenceView {
    ScheduleOccurrenceView {
        id: occurrence.id,
        civil_date: occurrence.civil_date,
        state: occurrence.state,
        miss_reason: occurrence.miss_reason,
        start_ms: occurrence.start_ms,
        end_ms: occurrence.end_ms,
        offset_seconds: occurrence.offset_seconds,
        transition_ms: occurrence.transition_ms,
        duration_seconds: occurrence.duration_seconds,
        maximum_bytes: occurrence.maximum_bytes,
        recording_id: occurrence.recording_id,
    }
}

#[cfg(test)]
mod tests {
    use super::ScheduleOperation;
    use crate::{
        Error,
        control::{Operation, apply},
        sources::{HttpSource, NetworkScope},
        storage::Store,
    };

    type TestResult = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[test]
    fn analysis_profile_is_rejected_before_a_rule_is_stored() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("catalog.sqlite3"))?;
        store.register_source(
            "radio:v1",
            &HttpSource::new(
                "Test radio",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
        let error = apply(
            &mut store,
            Operation::Schedule {
                command: ScheduleOperation::Create {
                    id: "blocked".into(),
                    source_revision: "radio:v1".into(),
                    zone: "Etc/UTC".into(),
                    once: Some("2026-09-23T06:00:00".into()),
                    daily: None,
                    weekly: None,
                    at: None,
                    seconds: 60,
                    maximum_bytes: 1024,
                    analysis_profile: Some("transcript".into()),
                },
            },
        );
        assert!(matches!(
            error,
            Err(Error::InvalidInput("analysis profile cannot be attached"))
        ));
        assert!(store.schedule("blocked")?.is_none());
        Ok(())
    }
}
