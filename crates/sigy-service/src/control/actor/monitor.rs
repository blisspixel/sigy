//! The monitor stage controller in the service. Each pass reads facts, plans with the pure
//! planner, and atomically admits each job with its monitor receipt before scheduling.
//! Metadata-only analysis pins may precede admission. Jobs and monitor usage cannot.

use super::{Actor, analysis::TranscribeRequest};
use crate::{
    Error, Result,
    monitor::pipeline::{self, Facts, Step},
    recognition::LocalAsrRequest,
    storage::{
        monitors::{MonitorJobAdmission, MonitorJobScope, StepRecord},
        now_ms,
    },
    translation::TranslationRequest,
};

/// Minimum time between monitor passes. Schedule ticks arrive every second.
const PASS_INTERVAL_MS: i64 = 5_000;

/// What happened to one attempted step.
enum Attempt {
    Queued(MonitorJobAdmission),
    Refused(&'static str),
    /// A temporary condition, such as a full queue: try again on a later pass.
    Later,
}

/// Refusals that will not change on retry are recorded; anything else is a catalog fault.
fn classify(error: Error) -> Result<Attempt> {
    match error {
        Error::Analysis(
            "queue-full"
            | "native-worker-active"
            | "monitor-policy-changed"
            | "monitor-actions-changed"
            | "monitor-paused"
            | "monitor-profile-changed"
            | "monitor-source-not-followed"
            | "monitor-recording-ineligible"
            | "monitor-admission-clock"
            | "monitor-daily-cap"
            | "monitor-total-cap",
        ) => Ok(Attempt::Later),
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
    pub(super) fn take_step(&mut self, facts: &Facts, step: &Step, now: i64) -> Result<bool> {
        let (recording_id, stage, attempt) = match step {
            Step::Recognize {
                recording_id,
                audio_us,
                profile,
            } => (
                recording_id,
                "recognition",
                self.recognize(recording_id, profile, now)
                    .and_then(|request| {
                        self.library.store_mut().enqueue_monitor_recognition(
                            &scope(facts, recording_id, *audio_us),
                            &request,
                            now,
                        )
                    })
                    .map(Attempt::Queued)
                    .or_else(classify)?,
            ),
            Step::SkipRecognition {
                recording_id,
                reason,
            } => (recording_id, "recognition", Attempt::Refused(reason)),
            Step::Translate {
                recording_id,
                analysis_id,
                transcript_revision,
                profile,
            } => (
                recording_id,
                "translation",
                self.translate(analysis_id, *transcript_revision, profile)
                    .and_then(|request| {
                        self.library.store_mut().enqueue_monitor_translation(
                            &scope(facts, recording_id, 0),
                            &request,
                            now,
                        )
                    })
                    .map(Attempt::Queued)
                    .or_else(classify)?,
            ),
            Step::SkipTranslation {
                recording_id,
                reason,
            } => {
                let reason = reason.clone();
                self.record_refusal(facts, recording_id, "translation", &reason, now)?;
                return Ok(true);
            }
        };
        match attempt {
            Attempt::Queued(admission) => {
                if admission.job_created {
                    self.schedule()?;
                }
            }
            Attempt::Refused(reason) => {
                self.record_refusal(facts, recording_id, stage, reason, now)?;
            }
            Attempt::Later => return Ok(false),
        }
        Ok(true)
    }

    fn record_refusal(
        &mut self,
        facts: &Facts,
        recording_id: &str,
        stage: &'static str,
        reason: &str,
        now: i64,
    ) -> Result<()> {
        self.library.store_mut().record_monitor_skip(
            &StepRecord {
                monitor_id: &facts.monitor_id,
                recording_id,
                stage,
                policy_version: facts.version,
                outcome: Err(reason),
                audio_us: 0,
            },
            now,
        )
    }

    /// Publish metadata for a shared pin and prepare the immutable recognition request.
    /// Monitors and tasks derive the same canonical identities and so share one job.
    pub(super) fn recognize(
        &mut self,
        recording_id: &str,
        profile: &str,
        now: i64,
    ) -> Result<LocalAsrRequest> {
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
        Ok(self
            .prepare_recognition(TranscribeRequest {
                id: job_id,
                input: analysis_id,
                revision: pin.revision,
                profile: profile.to_owned(),
                parent_revision: None,
            })?
            .request)
    }

    pub(super) fn translate(
        &self,
        analysis_id: &str,
        transcript_revision: i64,
        profile: &str,
    ) -> Result<TranslationRequest> {
        let profile_sha256 = self
            .library
            .store()
            .translation_profile(profile)?
            .profile_sha256;
        let job_id =
            pipeline::translation_job_id(analysis_id, transcript_revision, &profile_sha256);
        Ok(self
            .prepare_translation(&job_id, analysis_id, Some(transcript_revision), profile)?
            .request)
    }
}

fn scope<'a>(facts: &'a Facts, recording_id: &'a str, audio_us: u64) -> MonitorJobScope<'a> {
    MonitorJobScope {
        monitor_id: &facts.monitor_id,
        policy_version: facts.version,
        action_count: facts.action_count,
        recording_id,
        audio_us,
    }
}
