//! Observed recognition arrivals on this library.
//!
//! The rate is retained audio on admitted recognition jobs divided by the span
//! of their stored clocks. It is compared with the busy time of completed
//! recognition. A missing span or a missing pace stays uncompared. The figure
//! is not a host budget and it is not a clock time for an empty queue.

use std::cmp::Ordering;
use std::fmt::Write;

use rusqlite::params;

use super::Store;
use super::pace::{BusyRecognition, MAX_PACE_JOBS};
use crate::{Error, Result};

/// One admitted recognition job.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Admission {
    created_ms: i64,
    audio_us: u64,
}

/// Newest admissions with a positive audio total and a positive clock span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArrivalWindow {
    pub jobs: u32,
    pub audio_us: u64,
    pub span_ms: u64,
    pub newest_limited: bool,
}

/// How admissions compare with completed busy work. Equal is `Same`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrivalRelation {
    Faster,
    Same,
    Slower,
}

/// What doctor can say from stored clocks. Admission ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArrivalReport {
    Unmeasured,
    Uncompared {
        window: ArrivalWindow,
    },
    Compared {
        window: ArrivalWindow,
        busy: BusyRecognition,
        relation: ArrivalRelation,
    },
}

/// Build a report from an admission sample and the pace sample.
/// # Errors
/// Returns a storage error when the comparison overflows.
pub(crate) fn report(
    window: Option<ArrivalWindow>,
    busy: Option<BusyRecognition>,
) -> Result<ArrivalReport> {
    let Some(window) = window else {
        return Ok(ArrivalReport::Unmeasured);
    };
    let Some(busy) = busy else {
        return Ok(ArrivalReport::Uncompared { window });
    };
    Ok(ArrivalReport::Compared {
        relation: relation(&window, &busy)?,
        window,
        busy,
    })
}

/// One doctor sentence. Integers only. The check state stays ok.
#[must_use]
pub(crate) fn describe_arrival(report: &ArrivalReport) -> String {
    match report {
        ArrivalReport::Unmeasured => {
            "Recognition arrival is unmeasured on this library.".to_owned()
        }
        ArrivalReport::Uncompared { window } => uncompared(window),
        ArrivalReport::Compared {
            window,
            busy,
            relation,
        } => compared(window, busy, *relation),
    }
}

impl Store {
    /// Arrival of local recognition audio over the newest admitted jobs.
    /// # Errors
    /// Returns catalog, timeline, and overflow errors.
    pub(crate) fn recognition_arrival(&self) -> Result<ArrivalReport> {
        let eligible: i64 = self.connection.query_row(
            "SELECT count(*) FROM analysis_jobs WHERE kind = 'local_asr'",
            [],
            |row| row.get(0),
        )?;
        let admissions = {
            let mut statement = self.connection.prepare(
                "SELECT j.created_ms, a.timeline_json FROM analysis_jobs j JOIN analysis_inputs a ON a.id = j.analysis_id AND a.revision = j.analysis_revision WHERE j.kind = 'local_asr' ORDER BY j.created_ms DESC, j.id DESC LIMIT ?1",
            )?;
            let rows = statement.query_map(params![i64::from(MAX_PACE_JOBS)], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut admissions = Vec::new();
            for row in rows {
                let (created_ms, timeline) = row?;
                admissions.push(Admission {
                    created_ms,
                    audio_us: super::super::analysis::retained_audio_us(&timeline)?,
                });
            }
            admissions
        };
        let eligible = u32::try_from(eligible).map_err(|_| Error::StorageIntegrity)?;
        let window = window_from(&admissions, eligible > MAX_PACE_JOBS)?;
        report(window, self.recognition_busy_work()?)
    }
}

fn window_from(rows: &[Admission], newest_limited: bool) -> Result<Option<ArrivalWindow>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let mut earliest = i64::MAX;
    let mut latest = i64::MIN;
    let mut audio_us = 0_u64;
    for row in rows {
        if row.created_ms < 0 {
            return Err(Error::StorageIntegrity);
        }
        earliest = earliest.min(row.created_ms);
        latest = latest.max(row.created_ms);
        audio_us = audio_us
            .checked_add(row.audio_us)
            .ok_or(Error::StorageIntegrity)?;
    }
    let span_ms = u64::try_from(
        latest
            .checked_sub(earliest)
            .ok_or(Error::StorageIntegrity)?,
    )
    .map_err(|_| Error::StorageIntegrity)?;
    if span_ms == 0 || audio_us == 0 {
        return Ok(None);
    }
    Ok(Some(ArrivalWindow {
        jobs: u32::try_from(rows.len()).map_err(|_| Error::StorageIntegrity)?,
        audio_us,
        span_ms,
        newest_limited,
    }))
}

fn relation(window: &ArrivalWindow, busy: &BusyRecognition) -> Result<ArrivalRelation> {
    let admitted = u128::from(window.audio_us)
        .checked_mul(u128::from(busy.wall_ms))
        .ok_or(Error::StorageIntegrity)?;
    let processed = u128::from(window.span_ms)
        .checked_mul(u128::from(busy.audio_us))
        .ok_or(Error::StorageIntegrity)?;
    Ok(match admitted.cmp(&processed) {
        Ordering::Greater => ArrivalRelation::Faster,
        Ordering::Equal => ArrivalRelation::Same,
        Ordering::Less => ArrivalRelation::Slower,
    })
}

fn uncompared(window: &ArrivalWindow) -> String {
    let mut detail = String::new();
    push_window(&mut detail, window);
    detail.push_str(
        "Completed recognition has no measured duration, so arrivals are not compared. This is not a host budget. Observed on this library.",
    );
    detail
}

fn compared(window: &ArrivalWindow, busy: &BusyRecognition, relation: ArrivalRelation) -> String {
    let mut detail = String::new();
    push_window(&mut detail, window);
    let noun = if busy.jobs == 1 { "job" } else { "jobs" };
    let _ = write!(
        detail,
        "Completed recognition processed {} us in {} ms of busy wall time across {} {noun}. ",
        busy.audio_us, busy.wall_ms, busy.jobs
    );
    detail.push_str(relation_sentence(relation));
    detail.push_str(
        " This is not a clock time for an empty queue and it is not a host budget. Observed on this library.",
    );
    detail
}

fn push_window(detail: &mut String, window: &ArrivalWindow) {
    if window.newest_limited {
        let _ = write!(detail, "newest {MAX_PACE_JOBS} recognition jobs. ");
    }
    let noun = if window.jobs == 1 { "job" } else { "jobs" };
    let _ = write!(
        detail,
        "Observed recognition arrivals: {} {noun}, {} us of audio over {} ms. ",
        window.jobs, window.audio_us, window.span_ms
    );
}

const fn relation_sentence(relation: ArrivalRelation) -> &'static str {
    match relation {
        ArrivalRelation::Faster => {
            "Admissions brought more audio per wall millisecond than that completed work processed."
        }
        ArrivalRelation::Same => {
            "Admissions brought the same audio per wall millisecond as that completed work processed."
        }
        ArrivalRelation::Slower => {
            "Admissions brought less audio per wall millisecond than that completed work processed."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::pace::{BusyRecognition, RecognitionRun, busy_recognition};
    use super::{
        Admission, ArrivalRelation, ArrivalReport, ArrivalWindow, describe_arrival, report,
        window_from,
    };
    use crate::Error;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn admission(created_ms: i64, audio_us: u64) -> Admission {
        Admission {
            created_ms,
            audio_us,
        }
    }

    fn busy(jobs: u32, audio_us: u64, wall_ms: u64) -> BusyRecognition {
        BusyRecognition {
            jobs,
            audio_us,
            wall_ms,
        }
    }

    #[test]
    fn a_single_clock_and_empty_audio_stay_unmeasured() -> TestResult {
        assert!(window_from(&[], false)?.is_none());
        assert!(window_from(&[admission(10, 1_000)], false)?.is_none());
        assert!(window_from(&[admission(10, 1_000), admission(10, 1_000)], false)?.is_none());
        assert!(window_from(&[admission(10, 0), admission(20, 0)], false)?.is_none());
        assert!(matches!(
            window_from(&[admission(-1, 1_000)], false),
            Err(Error::StorageIntegrity)
        ));
        let report = report(None, None)?;
        assert_eq!(
            describe_arrival(&report),
            "Recognition arrival is unmeasured on this library."
        );
        Ok(())
    }

    #[test]
    fn arrivals_without_completed_work_are_not_compared() -> TestResult {
        let window = window_from(
            &[admission(0, 1_000_000), admission(1_000, 1_000_000)],
            true,
        )?
        .ok_or("window")?;
        let report = report(Some(window), None)?;
        assert_eq!(
            describe_arrival(&report),
            "newest 256 recognition jobs. Observed recognition arrivals: 2 jobs, 2000000 us of audio over 1000 ms. Completed recognition has no measured duration, so arrivals are not compared. This is not a host budget. Observed on this library."
        );
        Ok(())
    }

    #[test]
    fn the_comparison_uses_the_products_and_does_not_round() -> TestResult {
        let window = ArrivalWindow {
            jobs: 2,
            audio_us: 1_000,
            span_ms: 10,
            newest_limited: false,
        };
        let same = report(Some(window.clone()), Some(busy(1, 100, 1)))?;
        assert!(matches!(
            same,
            ArrivalReport::Compared {
                relation: ArrivalRelation::Same,
                ..
            }
        ));
        let faster = report(Some(window.clone()), Some(busy(1, 100, 2)))?;
        assert!(describe_arrival(&same).contains(
            "Admissions brought the same audio per wall millisecond as that completed work processed."
        ));
        assert!(describe_arrival(&faster).contains(
            "Admissions brought more audio per wall millisecond than that completed work processed."
        ));
        let slower = report(Some(window), Some(busy(4, 1_000, 1)))?;
        let ArrivalReport::Compared { relation, .. } = slower else {
            return Err("expected a comparison".into());
        };
        assert_eq!(relation, ArrivalRelation::Slower);
        assert_eq!(
            describe_arrival(&slower),
            "Observed recognition arrivals: 2 jobs, 1000 us of audio over 10 ms. Completed recognition processed 1000 us in 1 ms of busy wall time across 4 jobs. Admissions brought less audio per wall millisecond than that completed work processed. This is not a clock time for an empty queue and it is not a host budget. Observed on this library."
        );
        Ok(())
    }

    #[test]
    fn an_overflowing_audio_total_is_a_storage_error() -> TestResult {
        assert!(matches!(
            window_from(&[admission(0, u64::MAX), admission(1, 1)], false),
            Err(Error::StorageIntegrity)
        ));
        let runs = [
            RecognitionRun {
                profile: "a".into(),
                audio_us: u64::MAX,
                wall_ms: 1,
            },
            RecognitionRun {
                profile: "b".into(),
                audio_us: 1,
                wall_ms: 1,
            },
        ];
        assert!(matches!(
            busy_recognition(&runs),
            Err(Error::StorageIntegrity)
        ));
        let extreme = ArrivalWindow {
            jobs: 1,
            audio_us: u64::MAX,
            span_ms: u64::MAX,
            newest_limited: false,
        };
        let same = report(Some(extreme), Some(busy(1, u64::MAX, u64::MAX)))?;
        assert!(matches!(
            same,
            ArrivalReport::Compared {
                relation: ArrivalRelation::Same,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn busy_work_sums_the_pace_sample() -> TestResult {
        assert!(busy_recognition(&[])?.is_none());
        let runs = [
            RecognitionRun {
                profile: "a".into(),
                audio_us: 1_000,
                wall_ms: 4,
            },
            RecognitionRun {
                profile: "b".into(),
                audio_us: 3_000,
                wall_ms: 6,
            },
        ];
        assert_eq!(busy_recognition(&runs)?, Some(busy(2, 4_000, 10)));
        Ok(())
    }
}
