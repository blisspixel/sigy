//! Which queued job the one local slot should claim next.
//!
//! Claims rotate across sources. Every fourth claim is reserved for the oldest
//! waiting batch job, so a source that keeps arriving cannot hold the slot.
//! Live work, when any is waiting, takes the other three claims and is ordered
//! by deadline inside each source. The caller decides which jobs are live at
//! claim time. That class is not stored. Restart begins again at the oldest job.

use std::cmp::Ordering;

/// How many claims pass before the oldest batch job is taken again.
pub(crate) const SHARE_PERIOD: u64 = 4;

/// Whether a job is trying to stay near a live edge or clearing a backlog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Live,
    Batch,
}

/// One queued job the chooser may select. The caller has already dropped jobs
/// whose lineage is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub id: String,
    pub source: String,
    pub class: Class,
    pub ready_ms: i64,
    pub deadline_ms: i64,
}

/// Rotation memory for one job kind. It lives with the service process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FairCursor {
    pub turn: u64,
    pub last_live: Option<String>,
    pub last_batch: Option<String>,
}

/// The job a claim should start, and the source that claim consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pick {
    pub id: String,
    pub source: String,
    pub class: Class,
}

impl FairCursor {
    /// Remember the claim that started.
    pub(crate) fn advance(&mut self, pick: &Pick) {
        self.turn = self.turn.saturating_add(1);
        let slot = match pick.class {
            Class::Live => &mut self.last_live,
            Class::Batch => &mut self.last_batch,
        };
        *slot = Some(pick.source.clone());
    }
}

/// Choose the next claim. `None` means nothing is waiting.
#[must_use]
pub(crate) fn choose(candidates: &[Candidate], cursor: &FairCursor) -> Option<Pick> {
    if candidates.is_empty() {
        return None;
    }
    let reserved = cursor.turn % SHARE_PERIOD == SHARE_PERIOD - 1;
    if reserved && let Some(pick) = oldest(candidates, Class::Batch) {
        return Some(pick);
    }
    let class = if candidates.iter().any(|item| item.class == Class::Live) {
        Class::Live
    } else {
        Class::Batch
    };
    rotate(candidates, class, last_source(cursor, class))
}

fn last_source(cursor: &FairCursor, class: Class) -> Option<&str> {
    match class {
        Class::Live => cursor.last_live.as_deref(),
        Class::Batch => cursor.last_batch.as_deref(),
    }
}

fn oldest(candidates: &[Candidate], class: Class) -> Option<Pick> {
    candidates
        .iter()
        .filter(|item| item.class == class)
        .min_by(|left, right| order(class, left, right))
        .map(pick_from)
}

fn rotate(candidates: &[Candidate], class: Class, last: Option<&str>) -> Option<Pick> {
    let mut winners: Vec<&Candidate> = Vec::new();
    for candidate in candidates.iter().filter(|item| item.class == class) {
        if let Some(slot) = winners
            .iter_mut()
            .find(|item| item.source == candidate.source)
        {
            if order(class, candidate, slot).is_lt() {
                *slot = candidate;
            }
        } else {
            winners.push(candidate);
        }
    }
    if winners.is_empty() {
        return None;
    }
    winners.sort_by(|left, right| left.source.cmp(&right.source));
    let index = match last {
        None => winners
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| order(class, left, right))
            .map(|(index, _)| index)?,
        Some(source) => winners
            .iter()
            .position(|item| item.source.as_str() > source)
            .unwrap_or(0),
    };
    winners.get(index).copied().map(pick_from)
}

fn order(class: Class, left: &Candidate, right: &Candidate) -> Ordering {
    match class {
        Class::Live => (left.deadline_ms, left.ready_ms, left.id.as_str()).cmp(&(
            right.deadline_ms,
            right.ready_ms,
            right.id.as_str(),
        )),
        Class::Batch => (left.ready_ms, left.deadline_ms, left.id.as_str()).cmp(&(
            right.ready_ms,
            right.deadline_ms,
            right.id.as_str(),
        )),
    }
}

fn pick_from(candidate: &Candidate) -> Pick {
    Pick {
        id: candidate.id.clone(),
        source: candidate.source.clone(),
        class: candidate.class,
    }
}

#[cfg(test)]
mod tests {
    use super::{Candidate, Class, FairCursor, SHARE_PERIOD, choose};

    fn item(id: &str, source: &str, class: Class, ready_ms: i64, deadline_ms: i64) -> Candidate {
        Candidate {
            id: id.to_owned(),
            source: source.to_owned(),
            class,
            ready_ms,
            deadline_ms,
        }
    }

    fn take(candidates: &mut Vec<Candidate>, cursor: &mut FairCursor) -> String {
        let pick = choose(candidates, cursor).unwrap_or_else(|| panic!("a job was waiting"));
        cursor.advance(&pick);
        let id = pick.id.clone();
        candidates.retain(|item| item.id != id);
        id
    }

    #[test]
    fn a_fast_live_source_cannot_take_the_older_share() {
        let mut cursor = FairCursor::default();
        let mut waiting = vec![item("old", "quiet", Class::Batch, 0, 0)];
        for index in 0..8 {
            waiting.push(item(
                &format!("fast-{index}"),
                "fast",
                Class::Live,
                10 + index,
                10 + index,
            ));
        }
        let mut claimed = Vec::new();
        for _ in 0..4 {
            claimed.push(take(&mut waiting, &mut cursor));
        }
        assert_eq!(claimed[3], "old");
        assert!(claimed[..3].iter().all(|id| id.starts_with("fast")));
    }

    #[test]
    fn one_source_stays_in_arrival_order() {
        let mut cursor = FairCursor::default();
        let mut waiting = (0..5)
            .map(|index| item(&format!("job-{index}"), "only", Class::Batch, index, index))
            .collect::<Vec<_>>();
        for index in 0..5 {
            assert_eq!(take(&mut waiting, &mut cursor), format!("job-{index}"));
        }
        assert!(choose(&waiting, &cursor).is_none());
    }

    #[test]
    fn sources_rotate_and_the_reserved_claim_is_the_oldest() {
        let mut cursor = FairCursor::default();
        let mut waiting = vec![
            item("a0", "alpha", Class::Batch, 0, 0),
            item("b0", "beta", Class::Batch, 1, 1),
            item("c0", "gamma", Class::Batch, 2, 2),
            item("a1", "alpha", Class::Batch, 3, 3),
            item("a2", "alpha", Class::Batch, 4, 4),
        ];
        assert_eq!(take(&mut waiting, &mut cursor), "a0");
        assert_eq!(take(&mut waiting, &mut cursor), "b0");
        assert_eq!(take(&mut waiting, &mut cursor), "c0");
        assert_eq!(take(&mut waiting, &mut cursor), "a1");
    }

    #[test]
    fn the_reserved_turn_keeps_an_old_job_when_rotation_would_pass_it() {
        let cursor = FairCursor {
            turn: SHARE_PERIOD - 1,
            last_live: None,
            last_batch: Some("quiet".to_owned()),
        };
        let waiting = vec![
            item("old", "quiet", Class::Batch, 0, 0),
            item("fast-0", "fast", Class::Batch, 5, 5),
            item("fast-1", "fast", Class::Batch, 6, 6),
        ];
        let pick = choose(&waiting, &cursor).unwrap_or_else(|| panic!("a job was waiting"));
        assert_eq!(pick.id, "old");
    }
}
