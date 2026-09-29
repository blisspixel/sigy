//! Observed recognition pace from completed jobs on this library.
//!
//! The figure is total wall time of those attempts divided by retained audio.
//! It is not a host capacity and it does not predict a backlog. Claim
//! classification reads the same per-profile figure, including profiles the
//! doctor report does not name.

use std::collections::BTreeMap;
use std::fmt::Write;

use rusqlite::params;

use super::Store;
use crate::{Error, Result};

/// Newest completed jobs included in one pace report.
pub(crate) const MAX_PACE_JOBS: u32 = 256;
/// Profiles named in one report. The rest are counted.
pub(crate) const MAX_PACE_PROFILES: usize = 8;

/// One successful recognition attempt whose clock and audio are both known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecognitionRun {
    pub profile: String,
    pub audio_us: u64,
    pub wall_ms: u64,
}

/// Pace of one profile, rounded down to a millisecond per second of audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfilePace {
    pub profile: String,
    pub jobs: u32,
    /// Zero means the jobs finished in under 1 ms of wall time per audio second.
    pub wall_ms_per_audio_second: u64,
}

/// What completed recognition on this library can support. Admission ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecognitionPace {
    Unmeasured,
    Observed {
        profiles: Vec<ProfilePace>,
        hidden_profiles: u32,
        newest_limited: bool,
    },
}

#[derive(Default)]
struct Accumulator {
    jobs: u32,
    audio_us: u64,
    wall_ms: u64,
}

fn accumulate(runs: &[RecognitionRun]) -> Result<BTreeMap<String, Accumulator>> {
    let mut totals: BTreeMap<String, Accumulator> = BTreeMap::new();
    for run in runs {
        if run.audio_us == 0 || run.wall_ms == 0 {
            continue;
        }
        let total = totals.entry(run.profile.clone()).or_default();
        total.jobs = total.jobs.checked_add(1).ok_or(Error::StorageIntegrity)?;
        total.audio_us = total
            .audio_us
            .checked_add(run.audio_us)
            .ok_or(Error::StorageIntegrity)?;
        total.wall_ms = total
            .wall_ms
            .checked_add(run.wall_ms)
            .ok_or(Error::StorageIntegrity)?;
    }
    Ok(totals)
}

/// Pace of every profile in the sample, including ones a doctor report would hide.
/// # Errors
/// Returns a storage error when a total overflows.
pub(crate) fn profile_paces(runs: &[RecognitionRun]) -> Result<BTreeMap<String, u64>> {
    let totals = accumulate(runs)?;
    let mut paces = BTreeMap::new();
    for (profile, total) in totals {
        let pace = pace_of(&profile, &total)?;
        paces.insert(profile, pace.wall_ms_per_audio_second);
    }
    Ok(paces)
}

/// Combine runs into one pace. `newest_limited` means the caller kept only the newest jobs.
/// # Errors
/// Returns a storage error when a total overflows.
pub(crate) fn observe(runs: &[RecognitionRun], newest_limited: bool) -> Result<RecognitionPace> {
    let totals = accumulate(runs)?;
    if totals.is_empty() {
        return Ok(RecognitionPace::Unmeasured);
    }
    let mut ranked = totals.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .audio_us
            .cmp(&left.1.audio_us)
            .then_with(|| left.0.cmp(&right.0))
    });
    let hidden_profiles = u32::try_from(ranked.len().saturating_sub(MAX_PACE_PROFILES))
        .map_err(|_| Error::StorageIntegrity)?;
    ranked.truncate(MAX_PACE_PROFILES);
    let profiles = ranked
        .into_iter()
        .map(|(profile, total)| pace_of(&profile, &total))
        .collect::<Result<Vec<_>>>()?;
    Ok(RecognitionPace::Observed {
        profiles,
        hidden_profiles,
        newest_limited,
    })
}

/// One line for the doctor report. The pace does not change the check's state.
#[must_use]
pub(crate) fn describe(pace: &RecognitionPace) -> String {
    let RecognitionPace::Observed {
        profiles,
        hidden_profiles,
        newest_limited,
    } = pace
    else {
        return unmeasured();
    };
    if profiles.is_empty() {
        return unmeasured();
    }
    let mut detail = String::new();
    if *newest_limited {
        let _ = write!(detail, "newest {MAX_PACE_JOBS} completed jobs. ");
    }
    for (index, profile) in profiles.iter().enumerate() {
        if index > 0 {
            detail.push_str("; ");
        }
        let jobs = if profile.jobs == 1 { "job" } else { "jobs" };
        let rate = if profile.wall_ms_per_audio_second == 0 {
            "under 1 ms wall per audio second"
        } else {
            "ms wall per audio second"
        };
        if profile.wall_ms_per_audio_second == 0 {
            let _ = write!(
                detail,
                "{}: {} {jobs}, {rate}",
                profile.profile, profile.jobs
            );
        } else {
            let _ = write!(
                detail,
                "{}: {} {jobs}, {} {rate}",
                profile.profile, profile.jobs, profile.wall_ms_per_audio_second
            );
        }
    }
    if *hidden_profiles == 1 {
        detail.push_str("; and 1 more profile");
    } else if *hidden_profiles > 1 {
        let _ = write!(detail, "; and {hidden_profiles} more profiles");
    }
    detail.push_str(". Observed on this library.");
    detail
}

fn unmeasured() -> String {
    "no completed recognition has a measured duration, so pace is unmeasured".to_owned()
}

fn pace_of(profile: &str, total: &Accumulator) -> Result<ProfilePace> {
    let numerator = u128::from(total.wall_ms)
        .checked_mul(1_000_000)
        .ok_or(Error::StorageIntegrity)?;
    let pace = numerator / u128::from(total.audio_us);
    Ok(ProfilePace {
        profile: profile.to_owned(),
        jobs: total.jobs,
        wall_ms_per_audio_second: u64::try_from(pace).map_err(|_| Error::StorageIntegrity)?,
    })
}

impl Store {
    fn recognition_runs(&self) -> Result<(Vec<RecognitionRun>, bool)> {
        let eligible: i64 = self.connection.query_row(
            "SELECT count(*) FROM analysis_jobs WHERE kind = 'local_asr' AND state = 'succeeded' AND started_ms IS NOT NULL AND finished_ms IS NOT NULL AND finished_ms > started_ms",
            [],
            |row| row.get(0),
        )?;
        let mut statement = self.connection.prepare(
            "SELECT j.profile, j.started_ms, j.finished_ms, a.timeline_json FROM analysis_jobs j JOIN analysis_inputs a ON a.id = j.analysis_id AND a.revision = j.analysis_revision WHERE j.kind = 'local_asr' AND j.state = 'succeeded' AND j.started_ms IS NOT NULL AND j.finished_ms IS NOT NULL AND j.finished_ms > j.started_ms ORDER BY j.finished_ms DESC, j.id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![i64::from(MAX_PACE_JOBS)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut runs = Vec::new();
        for row in rows {
            let (profile, started_ms, finished_ms, timeline) = row?;
            let audio_us = super::super::analysis::retained_audio_us(&timeline)?;
            let wall_ms = finished_ms
                .checked_sub(started_ms)
                .ok_or(Error::StorageIntegrity)?;
            if wall_ms <= 0 || audio_us == 0 {
                continue;
            }
            runs.push(RecognitionRun {
                profile,
                audio_us,
                wall_ms: u64::try_from(wall_ms).map_err(|_| Error::StorageIntegrity)?,
            });
        }
        let eligible = u32::try_from(eligible).map_err(|_| Error::StorageIntegrity)?;
        Ok((runs, eligible > MAX_PACE_JOBS))
    }

    /// Pace of the newest completed recognition jobs. Failed and queued work is omitted.
    /// # Errors
    /// Returns catalog and timeline errors.
    pub(crate) fn recognition_pace(&self) -> Result<RecognitionPace> {
        let (runs, newest_limited) = self.recognition_runs()?;
        observe(&runs, newest_limited)
    }

    /// Per-profile pace for claim classification. Profiles the doctor report hides are included.
    /// # Errors
    /// Returns catalog and timeline errors.
    pub(crate) fn recognition_profile_paces(&self) -> Result<BTreeMap<String, u64>> {
        let (runs, _) = self.recognition_runs()?;
        profile_paces(&runs)
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PACE_PROFILES, ProfilePace, RecognitionPace, RecognitionRun, observe};

    fn run(profile: &str, audio_us: u64, wall_ms: u64) -> RecognitionRun {
        RecognitionRun {
            profile: profile.to_owned(),
            audio_us,
            wall_ms,
        }
    }

    #[test]
    fn an_empty_library_is_unmeasured() {
        let pace = observe(&[], false).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(pace, RecognitionPace::Unmeasured);
    }

    #[test]
    fn zero_audio_and_zero_wall_do_not_invent_a_rate() {
        let runs = [run("quiet", 0, 1_000), run("instant", 1_000_000, 0)];
        let pace = observe(&runs, false).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(pace, RecognitionPace::Unmeasured);
    }

    #[test]
    fn two_jobs_share_one_pace_and_profiles_stay_separate() {
        let runs = [
            run("alpha", 30_000_000, 180_000),
            run("alpha", 30_000_000, 180_000),
            run("beta", 2_000_000, 1),
        ];
        let pace = observe(&runs, true).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            pace,
            RecognitionPace::Observed {
                profiles: vec![
                    ProfilePace {
                        profile: "alpha".into(),
                        jobs: 2,
                        wall_ms_per_audio_second: 6_000,
                    },
                    ProfilePace {
                        profile: "beta".into(),
                        jobs: 1,
                        wall_ms_per_audio_second: 0,
                    },
                ],
                hidden_profiles: 0,
                newest_limited: true,
            }
        );
    }

    #[test]
    fn only_the_largest_profiles_are_named() {
        let runs = (0..=MAX_PACE_PROFILES)
            .map(|index| {
                let audio_us = u64::try_from(index + 1).unwrap_or_else(|_| panic!("index"));
                run(&format!("p{index}"), audio_us * 1_000_000, 1_000)
            })
            .collect::<Vec<_>>();
        let pace = observe(&runs, false).unwrap_or_else(|error| panic!("{error}"));
        let RecognitionPace::Observed {
            profiles,
            hidden_profiles,
            newest_limited,
        } = pace
        else {
            panic!("expected a pace");
        };
        assert!(!newest_limited);
        assert_eq!(hidden_profiles, 1);
        assert_eq!(profiles.len(), MAX_PACE_PROFILES);
        assert_eq!(profiles[0].profile, format!("p{MAX_PACE_PROFILES}"));
        assert_eq!(profiles[0].wall_ms_per_audio_second, 111);
        let paces = super::profile_paces(&runs).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(paces.len(), MAX_PACE_PROFILES + 1);
        assert_eq!(paces.get("p0").copied(), Some(1_000));
        let text = super::describe(&RecognitionPace::Observed {
            profiles,
            hidden_profiles,
            newest_limited,
        });
        assert!(text.contains("and 1 more profile"));
        assert!(text.ends_with("Observed on this library."));
    }

    #[test]
    fn the_report_names_the_library_and_rounds_down() {
        let pace = observe(
            &[
                run("alpha", 30_000_000, 180_000),
                run("alpha", 30_000_000, 180_000),
                run("beta", 2_000_000, 1),
            ],
            false,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            super::describe(&pace),
            "alpha: 2 jobs, 6000 ms wall per audio second; beta: 1 job, under 1 ms wall per audio second. Observed on this library."
        );
    }
}
