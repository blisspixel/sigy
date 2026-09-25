//! The monitor stage controller in the service. Each pass reads facts, plans with the pure
//! planner, and performs each step through the same paths a user command uses. A step is
//! recorded only after its work was queued or definitively refused, so a crash repeats a
//! pass instead of losing or duplicating work.

use super::{Actor, analysis::TranscribeRequest};
use crate::{
    Error, Result,
    monitor::pipeline::{self, Facts, Step},
    storage::{monitors::StepRecord, now_ms},
};

/// Minimum time between monitor passes. Schedule ticks arrive every second.
const PASS_INTERVAL_MS: i64 = 5_000;

/// What happened to one attempted step.
enum Attempt {
    Queued {
        analysis_id: String,
        job_id: String,
    },
    Refused(&'static str),
    /// A temporary condition, such as a full queue: try again on a later pass.
    Later,
}

/// Refusals that will not change on retry are recorded; anything else is a catalog fault.
fn classify(error: Error) -> Result<Attempt> {
    match error {
        Error::Analysis("queue-full" | "native-worker-active") => Ok(Attempt::Later),
        Error::Analysis(reason) => Ok(Attempt::Refused(reason)),
        Error::InvalidInput(_) => Ok(Attempt::Refused("invalid-input")),
        Error::NotFound => Ok(Attempt::Refused("not-found")),
        Error::IdempotencyConflict => Ok(Attempt::Refused("idempotency-conflict")),
        error => Err(error),
    }
}

impl Actor {
    pub(super) fn reconcile_monitors(&mut self) -> Result<()> {
        let now = now_ms()?;
        if now < self.next_monitor_pass_ms {
            return Ok(());
        }
        self.next_monitor_pass_ms = now + PASS_INTERVAL_MS;
        let can_recognize = self.decoder().is_ok();
        for id in self.library.store().monitor_ids()? {
            let facts = self.library.store().monitor_facts(&id, now)?;
            let (steps, _) = pipeline::plan(&facts);
            for step in steps {
                if matches!(step, Step::Recognize { .. }) && !can_recognize {
                    continue;
                }
                if !self.take_step(&facts, &step, now)? {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Returns false when the rest of this monitor's pass should wait.
    fn take_step(&mut self, facts: &Facts, step: &Step, now: i64) -> Result<bool> {
        let (recording_id, stage, audio_us, attempt) = match step {
            Step::Recognize {
                recording_id,
                audio_us,
                profile,
            } => (
                recording_id,
                "recognition",
                *audio_us,
                self.recognize(recording_id, profile, now)
                    .or_else(classify)?,
            ),
            Step::SkipRecognition {
                recording_id,
                reason,
            } => (recording_id, "recognition", 0, Attempt::Refused(reason)),
            Step::Translate {
                recording_id,
                analysis_id,
                transcript_revision,
                profile,
            } => (
                recording_id,
                "translation",
                0,
                self.translate(analysis_id, *transcript_revision, profile)
                    .or_else(classify)?,
            ),
            Step::SkipTranslation {
                recording_id,
                reason,
            } => {
                let reason = reason.clone();
                self.record(facts, recording_id, "translation", 0, Err(&reason), now)?;
                return Ok(true);
            }
        };
        match attempt {
            Attempt::Queued {
                analysis_id,
                job_id,
            } => self.record(
                facts,
                recording_id,
                stage,
                audio_us,
                Ok((&analysis_id, &job_id)),
                now,
            )?,
            Attempt::Refused(reason) => {
                self.record(facts, recording_id, stage, 0, Err(reason), now)?;
            }
            Attempt::Later => return Ok(false),
        }
        Ok(true)
    }

    fn record(
        &mut self,
        facts: &Facts,
        recording_id: &str,
        stage: &'static str,
        audio_us: u64,
        outcome: std::result::Result<(&str, &str), &str>,
        now: i64,
    ) -> Result<()> {
        self.library.store_mut().record_monitor_step(
            &StepRecord {
                monitor_id: &facts.monitor_id,
                recording_id,
                stage,
                policy_version: facts.version,
                outcome,
                audio_us,
            },
            now,
        )
    }

    /// Pin the recording (shared across monitors) and queue recognition on it.
    fn recognize(&mut self, recording_id: &str, profile: &str, now: i64) -> Result<Attempt> {
        let analysis_id = pipeline::pin_id(recording_id);
        let store = self.library.store_mut();
        let (_, pin) = store.admit_analysis(&analysis_id, recording_id, false, now)?;
        let pin = if pin.state == "published" {
            pin
        } else {
            store.publish_analysis(&analysis_id, pin.revision)?
        };
        let profile_sha256 = store.recognition_profile(profile)?.profile_sha256;
        let job_id = pipeline::recognition_job_id(recording_id, &profile_sha256);
        self.start_recognition(TranscribeRequest {
            id: job_id.clone(),
            input: analysis_id.clone(),
            revision: pin.revision,
            profile: profile.to_owned(),
            parent_revision: None,
        })?;
        Ok(Attempt::Queued {
            analysis_id,
            job_id,
        })
    }

    fn translate(
        &mut self,
        analysis_id: &str,
        transcript_revision: i64,
        profile: &str,
    ) -> Result<Attempt> {
        let profile_sha256 = self
            .library
            .store()
            .translation_profile(profile)?
            .profile_sha256;
        let job_id =
            pipeline::translation_job_id(analysis_id, transcript_revision, &profile_sha256);
        self.start_translation(&job_id, analysis_id, Some(transcript_revision), profile)?;
        Ok(Attempt::Queued {
            analysis_id: analysis_id.to_owned(),
            job_id,
        })
    }
}
