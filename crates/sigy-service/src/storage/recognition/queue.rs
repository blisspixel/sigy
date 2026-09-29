//! Wall time of queued recognition audio at the observed pace.
//!
//! The figure is retained audio on queued local recognition jobs, converted
//! with the same per-profile pace used for claim classification. One profile's
//! audio is rounded up once. A running job is counted and its remaining audio
//! is not estimated. The result is processing time of audio already queued.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::super::deadline::lag_ms;
use super::Store;
use crate::{Error, Result};

/// One queued recognition job and the retained audio on its pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedAudio {
    pub profile: String,
    pub audio_us: u64,
}

/// Queued and active recognition, separated by whether a pace exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecognitionQueue {
    pub running_jobs: u32,
    pub stopping_jobs: u32,
    pub queued_jobs: u32,
    pub queued_audio_us: u64,
    pub measured_audio_us: u64,
    pub measured_wall_ms: u64,
    pub instant_audio_us: u64,
    pub unmeasured_audio_us: u64,
}

/// Combine queued jobs with the per-profile pace sample.
/// # Errors
/// Returns a storage error when a total or a rounded product overflows.
pub(crate) fn summarize(
    running_jobs: u32,
    stopping_jobs: u32,
    queued: &[QueuedAudio],
    paces: &BTreeMap<String, u64>,
) -> Result<RecognitionQueue> {
    let mut queued_audio_us = 0_u64;
    let mut instant_audio_us = 0_u64;
    let mut unmeasured_audio_us = 0_u64;
    let mut by_profile: BTreeMap<String, u64> = BTreeMap::new();
    for job in queued {
        queued_audio_us = queued_audio_us
            .checked_add(job.audio_us)
            .ok_or(Error::StorageIntegrity)?;
        if job.audio_us == 0 {
            continue;
        }
        match paces.get(&job.profile).copied() {
            None => {
                unmeasured_audio_us = unmeasured_audio_us
                    .checked_add(job.audio_us)
                    .ok_or(Error::StorageIntegrity)?;
            }
            Some(0) => {
                instant_audio_us = instant_audio_us
                    .checked_add(job.audio_us)
                    .ok_or(Error::StorageIntegrity)?;
            }
            Some(_) => {
                let total = by_profile.entry(job.profile.clone()).or_default();
                *total = total
                    .checked_add(job.audio_us)
                    .ok_or(Error::StorageIntegrity)?;
            }
        }
    }
    let mut measured_audio_us = 0_u64;
    let mut measured_wall_ms = 0_u64;
    for (profile, audio_us) in &by_profile {
        let pace = paces.get(profile).copied().ok_or(Error::StorageIntegrity)?;
        measured_audio_us = measured_audio_us
            .checked_add(*audio_us)
            .ok_or(Error::StorageIntegrity)?;
        let lag = lag_ms(*audio_us, pace).ok_or(Error::StorageIntegrity)?;
        let lag = u64::try_from(lag).map_err(|_| Error::StorageIntegrity)?;
        measured_wall_ms = measured_wall_ms
            .checked_add(lag)
            .ok_or(Error::StorageIntegrity)?;
    }
    Ok(RecognitionQueue {
        running_jobs,
        stopping_jobs,
        queued_jobs: u32::try_from(queued.len()).map_err(|_| Error::StorageIntegrity)?,
        queued_audio_us,
        measured_audio_us,
        measured_wall_ms,
        instant_audio_us,
        unmeasured_audio_us,
    })
}

/// Pace sentence, then the queue sentence. An unmeasured pace has no final period.
#[must_use]
pub(crate) fn describe_recognition(pace: &str, queue: &str) -> String {
    if pace.ends_with('.') {
        format!("{pace} {queue}")
    } else {
        format!("{pace}. {queue}")
    }
}

/// One doctor sentence. The check state stays ok.
#[must_use]
pub(crate) fn describe_queue(queue: &RecognitionQueue) -> String {
    let mut detail = String::new();
    write_activity(&mut detail, queue.running_jobs, "running");
    write_activity(&mut detail, queue.stopping_jobs, "stopping");
    if queue.queued_jobs == 0 {
        detail.push_str("No recognition job is queued.");
        return detail;
    }
    let noun = if queue.queued_jobs == 1 {
        "job"
    } else {
        "jobs"
    };
    if queue.queued_audio_us == 0 {
        let _ = write!(
            detail,
            "Queued recognition is {} {noun} with no retained audio on this library.",
            queue.queued_jobs
        );
        return detail;
    }
    let complete = queue.unmeasured_audio_us == 0
        && queue.instant_audio_us == 0
        && queue.measured_audio_us == queue.queued_audio_us;
    if complete {
        let _ = write!(
            detail,
            "Queued recognition is {} {noun}, {} us of audio, {} ms of wall time at the observed pace on this library.",
            queue.queued_jobs, queue.queued_audio_us, queue.measured_wall_ms
        );
        return detail;
    }
    let _ = write!(
        detail,
        "Queued recognition is {} {noun}, {} us of audio. ",
        queue.queued_jobs, queue.queued_audio_us
    );
    write_split(&mut detail, queue);
    detail
}

fn write_activity(detail: &mut String, count: u32, word: &str) {
    if count == 0 {
        return;
    }
    let noun = if count == 1 { "job" } else { "jobs" };
    let verb = if count == 1 { "is" } else { "are" };
    let _ = write!(detail, "{count} recognition {noun} {verb} {word}. ");
}

fn write_split(detail: &mut String, queue: &RecognitionQueue) {
    let mut wrote = false;
    if queue.measured_audio_us > 0 {
        let _ = write!(
            detail,
            "{} us is {} ms of wall time at the observed pace",
            queue.measured_audio_us, queue.measured_wall_ms
        );
        wrote = true;
    }
    if queue.instant_audio_us > 0 {
        if wrote {
            detail.push_str(". ");
        }
        let _ = write!(
            detail,
            "{} us is under 1 ms of wall time per audio second",
            queue.instant_audio_us
        );
        wrote = true;
    }
    if queue.unmeasured_audio_us > 0 {
        if wrote {
            detail.push_str(". ");
        }
        let _ = write!(
            detail,
            "{} us has no measured pace",
            queue.unmeasured_audio_us
        );
    }
    detail.push_str(". Observed on this library.");
}

impl Store {
    /// Queued recognition audio at the observed pace. Verification is omitted.
    /// # Errors
    /// Returns catalog, timeline, and overflow errors.
    pub(crate) fn recognition_queue(&self) -> Result<RecognitionQueue> {
        let paces = self.recognition_profile_paces()?;
        let mut statement = self.connection.prepare(
            "SELECT j.state, j.profile, a.timeline_json FROM analysis_jobs j JOIN analysis_inputs a ON a.id = j.analysis_id AND a.revision = j.analysis_revision WHERE j.kind = 'local_asr' AND j.state IN ('queued', 'running', 'cancelling')",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut running_jobs = 0_u32;
        let mut stopping_jobs = 0_u32;
        let mut queued = Vec::new();
        for row in rows {
            let (state, profile, timeline) = row?;
            if state == "running" {
                running_jobs = running_jobs.checked_add(1).ok_or(Error::StorageIntegrity)?;
                continue;
            }
            if state == "cancelling" {
                stopping_jobs = stopping_jobs
                    .checked_add(1)
                    .ok_or(Error::StorageIntegrity)?;
                continue;
            }
            if state != "queued" {
                return Err(Error::StorageIntegrity);
            }
            queued.push(QueuedAudio {
                profile,
                audio_us: super::super::analysis::retained_audio_us(&timeline)?,
            });
        }
        summarize(running_jobs, stopping_jobs, &queued, &paces)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{QueuedAudio, describe_queue, summarize};

    fn audio(profile: &str, audio_us: u64) -> QueuedAudio {
        QueuedAudio {
            profile: profile.to_owned(),
            audio_us,
        }
    }

    fn paces(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        pairs
            .iter()
            .map(|(profile, pace)| ((*profile).to_owned(), *pace))
            .collect()
    }

    fn queue(
        running_jobs: u32,
        stopping_jobs: u32,
        queued: &[QueuedAudio],
        sample: &[(&str, u64)],
    ) -> super::RecognitionQueue {
        summarize(running_jobs, stopping_jobs, queued, &paces(sample))
            .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn an_empty_queue_says_nothing_is_waiting() {
        let report = queue(0, 0, &[], &[]);
        assert_eq!(describe_queue(&report), "No recognition job is queued.");
        assert_eq!(
            super::describe_recognition("pace is unmeasured", "No recognition job is queued."),
            "pace is unmeasured. No recognition job is queued."
        );
        assert_eq!(
            super::describe_recognition(
                "alpha: 1 job, 1 ms wall per audio second. Observed on this library.",
                "No recognition job is queued."
            ),
            "alpha: 1 job, 1 ms wall per audio second. Observed on this library. No recognition job is queued."
        );
    }

    #[test]
    fn a_running_job_is_counted_apart_from_the_queue() {
        assert_eq!(
            describe_queue(&queue(1, 0, &[], &[])),
            "1 recognition job is running. No recognition job is queued."
        );
        assert_eq!(
            describe_queue(&queue(2, 0, &[], &[])),
            "2 recognition jobs are running. No recognition job is queued."
        );
        assert_eq!(
            describe_queue(&queue(0, 1, &[], &[])),
            "1 recognition job is stopping. No recognition job is queued."
        );
        let waiting = queue(1, 0, &[audio("alpha", 1_000_000)], &[("alpha", 1_000)]);
        assert_eq!(
            describe_queue(&waiting),
            "1 recognition job is running. Queued recognition is 1 job, 1000000 us of audio, 1000 ms of wall time at the observed pace on this library."
        );
    }

    #[test]
    fn queued_audio_rounds_up_once_per_profile() {
        let one = queue(0, 0, &[audio("alpha", 1_500_000)], &[("alpha", 1)]);
        assert_eq!(one.measured_wall_ms, 2);
        assert_eq!(
            describe_queue(&one),
            "Queued recognition is 1 job, 1500000 us of audio, 2 ms of wall time at the observed pace on this library."
        );
        let shared = queue(
            0,
            0,
            &[audio("alpha", 1), audio("alpha", 1)],
            &[("alpha", 1)],
        );
        assert_eq!(shared.measured_wall_ms, 1);
        let separate = queue(
            0,
            0,
            &[audio("alpha", 1), audio("beta", 1)],
            &[("alpha", 1), ("beta", 1)],
        );
        assert_eq!(separate.measured_wall_ms, 2);
    }

    #[test]
    fn a_missing_pace_and_a_zero_pace_stay_separate() {
        let report = queue(
            0,
            0,
            &[
                audio("alpha", 1_000_000),
                audio("beta", 1_000_000),
                audio("gamma", 500_000),
            ],
            &[("alpha", 6_000), ("beta", 0)],
        );
        assert_eq!(report.measured_wall_ms, 6_000);
        assert_eq!(report.instant_audio_us, 1_000_000);
        assert_eq!(report.unmeasured_audio_us, 500_000);
        assert_eq!(
            describe_queue(&report),
            "Queued recognition is 3 jobs, 2500000 us of audio. 1000000 us is 6000 ms of wall time at the observed pace. 1000000 us is under 1 ms of wall time per audio second. 500000 us has no measured pace. Observed on this library."
        );
    }

    #[test]
    fn no_retained_audio_has_no_wall_time() {
        let report = queue(0, 0, &[audio("alpha", 0)], &[("alpha", 1_000)]);
        assert_eq!(report.queued_audio_us, 0);
        assert_eq!(report.measured_wall_ms, 0);
        assert_eq!(
            describe_queue(&report),
            "Queued recognition is 1 job with no retained audio on this library."
        );
    }

    #[test]
    fn an_overflowing_product_is_an_error() {
        let queued = [audio("alpha", u64::MAX)];
        let sample = paces(&[("alpha", u64::MAX)]);
        assert!(summarize(0, 0, &queued, &sample).is_err());
    }
}
