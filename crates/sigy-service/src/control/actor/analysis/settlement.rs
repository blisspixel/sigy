//! In-memory completed slots for recognition and translation settlement.
//!
//! A slot keeps one drained outcome while its job row stays `running` or `cancelling`.
//! It is not a live worker. The attempt counter is local to the slot: it does not change
//! `job.attempt`, write `job_attempts`, start a recognizer or translator, or add a charge.

use std::time::{Duration, Instant};

use rusqlite::ffi;

use super::super::Actor;
use crate::{
    Error, Result,
    recognition::{LocalAsrJob, LocalAsrWork, ReapedLocalAsr},
    storage::job_pool::{JobKind, JobMembership},
    translation::{TranslationJob, TranslationOutcome, TranslationWork},
};

/// Finish calls for one parked outcome. This is not [`crate::storage::job_pool::MAX_ATTEMPTS`].
pub(super) const SETTLEMENT_ATTEMPTS: u32 = 3;
/// Monotonic bound from the moment the slot is taken.
/// It is three times the five-second catalog `busy_timeout` set by `Store::open`.
pub(super) const SETTLEMENT_DEADLINE: Duration = Duration::from_secs(15);
/// Longest pause while a slot is waiting out an uncertain commit.
const HOLD_PAUSE: Duration = Duration::from_secs(1);

/// How one settlement error may be followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettlementClass {
    /// The attempt did not commit. Another finish is allowed after a membership read.
    Retry,
    /// The commit outcome is unknown. Do not finish again unless a read shows it is already terminal.
    Uncertain,
    /// Drop the outcome and do not publish it again.
    Refuse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pace {
    Attempt,
    Wait,
    Exhaust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Membership {
    /// Same claim, and the row is no longer running or cancelling.
    Terminal,
    /// Same claim, still running or cancelling.
    Active,
    /// The stored attempt, owner or generation is not this token, or the row is gone.
    Changed,
    /// The membership read itself was busy or uncertain.
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// Publication committed, or a read proved this claim is already terminal.
    Committed,
    /// The outcome stays in the slot.
    Pending,
    /// The slot is dropped. The job row is left unchanged.
    Exhausted,
}

/// Test stand-in for a catalog error. Production settlement calls the real finish path.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettlementOverride {
    Busy,
    Locked,
    Io,
    DiskFull,
    Constraint,
    Corrupt,
    StaleWorker,
    StorageIntegrity,
}

#[cfg(test)]
impl SettlementOverride {
    fn error(self) -> Error {
        match self {
            Self::Busy => sqlite_failure(ffi::SQLITE_BUSY),
            Self::Locked => sqlite_failure(ffi::SQLITE_LOCKED),
            Self::Io => Error::Io(std::io::Error::other("injected settlement io")),
            Self::DiskFull => sqlite_failure(ffi::SQLITE_FULL),
            Self::Constraint => sqlite_failure(ffi::SQLITE_CONSTRAINT),
            Self::Corrupt => sqlite_failure(ffi::SQLITE_CORRUPT),
            Self::StaleWorker => Error::Analysis("stale-worker"),
            Self::StorageIntegrity => Error::StorageIntegrity,
        }
    }
}

/// Monotonic settlement clock. Production stays at a zero advance.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SettlementClock {
    pub(crate) advance: Duration,
}

impl SettlementClock {
    pub(crate) const fn system() -> Self {
        Self {
            advance: Duration::ZERO,
        }
    }

    pub(crate) fn now(self) -> Instant {
        match Instant::now().checked_add(self.advance) {
            Some(now) => now,
            None => Instant::now(),
        }
    }
}

pub(super) struct ParkedRecognition {
    work: LocalAsrWork,
    reaped: ReapedLocalAsr,
    taken: Instant,
    tries: u32,
    /// An uncertain commit was observed. Do not call finish again.
    hold: bool,
}

pub(super) struct ParkedTranslation {
    work: TranslationWork,
    outcome: TranslationOutcome,
    taken: Instant,
    tries: u32,
    hold: bool,
}

/// Classify one settlement error.
/// Busy and locked, including extended codes whose primary code is those values, can be retried.
/// I/O is uncertain. Every other error refuses another publication.
#[must_use]
pub(super) fn classify(error: &Error) -> SettlementClass {
    match error {
        Error::Io(_) => SettlementClass::Uncertain,
        Error::Database(rusqlite::Error::SqliteFailure(sqlite, _)) => match sqlite.code {
            ffi::ErrorCode::DatabaseBusy | ffi::ErrorCode::DatabaseLocked => SettlementClass::Retry,
            ffi::ErrorCode::SystemIoFailure => SettlementClass::Uncertain,
            _ => SettlementClass::Refuse,
        },
        _ => SettlementClass::Refuse,
    }
}

#[cfg(test)]
fn sqlite_failure(code: i32) -> Error {
    Error::Database(rusqlite::Error::SqliteFailure(ffi::Error::new(code), None))
}

fn pace(tries: u32, hold: bool, taken: Instant, now: Instant) -> Pace {
    let expired = now.saturating_duration_since(taken) >= SETTLEMENT_DEADLINE;
    if expired || tries >= SETTLEMENT_ATTEMPTS {
        Pace::Exhaust
    } else if hold {
        Pace::Wait
    } else {
        Pace::Attempt
    }
}

fn interpret(
    member: Option<&JobMembership>,
    generation: u32,
    attempt: u32,
    owner: &str,
) -> Membership {
    let Some(member) = member else {
        return Membership::Changed;
    };
    let same = member.generation == generation
        && member.attempt == attempt
        && member.owner.as_deref() == Some(owner);
    if !same {
        Membership::Changed
    } else if matches!(member.state.as_str(), "running" | "cancelling") {
        Membership::Active
    } else {
        Membership::Terminal
    }
}

fn read_failure(error: Error) -> Result<Membership> {
    match classify(&error) {
        SettlementClass::Retry | SettlementClass::Uncertain => Ok(Membership::Unreadable),
        SettlementClass::Refuse => Err(error),
    }
}

fn apply_failure(
    class: SettlementClass,
    membership: Membership,
    hold: &mut bool,
    tries: u32,
    taken: Instant,
    now: Instant,
) -> Result<Step> {
    match membership {
        Membership::Terminal => Ok(Step::Committed),
        Membership::Changed => Err(Error::Analysis("stale-worker")),
        Membership::Active | Membership::Unreadable => {
            if matches!(class, SettlementClass::Uncertain) {
                *hold = true;
            }
            let expired = now.saturating_duration_since(taken) >= SETTLEMENT_DEADLINE;
            if tries >= SETTLEMENT_ATTEMPTS || expired {
                Ok(Step::Exhausted)
            } else {
                Ok(Step::Pending)
            }
        }
    }
}

impl super::Pool {
    pub(crate) fn has_settlement(&self) -> bool {
        !self.recognition_slots.is_empty() || !self.translation_slots.is_empty()
    }

    pub(super) fn occupied(&self, kind: JobKind) -> usize {
        match kind {
            JobKind::Verification => self.verification.len(),
            JobKind::Recognition => self.recognition.len() + self.recognition_slots.len(),
            JobKind::Translation => self.translation.len() + self.translation_slots.len(),
        }
    }

    /// A slot needs a finish attempt, or its deadline has already passed.
    pub(crate) fn settlement_due(&self, now: Instant) -> bool {
        let due = |tries, hold, taken| {
            matches!(pace(tries, hold, taken, now), Pace::Attempt | Pace::Exhaust)
        };
        self.recognition_slots
            .values()
            .any(|slot| due(slot.tries, slot.hold, slot.taken))
            || self
                .translation_slots
                .values()
                .any(|slot| due(slot.tries, slot.hold, slot.taken))
    }

    /// `None` means there is nothing to settle, so the catalog thread may block.
    /// A zero wait means a slot should be driven immediately.
    #[must_use]
    pub(crate) fn settlement_wait(&self, now: Instant) -> Option<Duration> {
        if !self.has_settlement() {
            None
        } else if self.settlement_due(now) {
            Some(Duration::ZERO)
        } else {
            Some(self.hold_pause(now))
        }
    }

    #[must_use]
    pub(crate) fn hold_pause(&self, now: Instant) -> Duration {
        let mut remaining = HOLD_PAUSE;
        for taken in self
            .recognition_slots
            .values()
            .map(|slot| slot.taken)
            .chain(self.translation_slots.values().map(|slot| slot.taken))
        {
            let left = SETTLEMENT_DEADLINE.saturating_sub(now.saturating_duration_since(taken));
            if left < remaining {
                remaining = left;
            }
        }
        remaining
    }

    #[cfg(test)]
    pub(super) fn recognition_tries(&self, id: &str) -> Option<u32> {
        self.recognition_slots.get(id).map(|slot| slot.tries)
    }

    #[cfg(test)]
    pub(super) fn translation_tries(&self, id: &str) -> Option<u32> {
        self.translation_slots.get(id).map(|slot| slot.tries)
    }

    #[cfg(test)]
    pub(super) fn set_recognition_attempt(&mut self, id: &str, attempt: u32) -> bool {
        let Some(slot) = self.recognition_slots.get_mut(id) else {
            return false;
        };
        slot.work.job.attempt = attempt;
        true
    }
}

impl Actor {
    pub(super) fn park_recognition(
        &mut self,
        work: LocalAsrWork,
        reaped: ReapedLocalAsr,
    ) -> Result<Step> {
        let id = work.job.request.id.clone();
        let mut parked = ParkedRecognition {
            work,
            reaped,
            taken: self.settlement_clock.now(),
            tries: 0,
            hold: false,
        };
        match self.advance_recognition(&mut parked)? {
            Step::Pending => {
                self.pool.recognition_slots.insert(id, parked);
                Ok(Step::Pending)
            }
            step => Ok(step),
        }
    }

    pub(super) fn park_translation(
        &mut self,
        work: TranslationWork,
        outcome: TranslationOutcome,
    ) -> Result<Step> {
        let id = work.job.request.id.clone();
        let mut parked = ParkedTranslation {
            work,
            outcome,
            taken: self.settlement_clock.now(),
            tries: 0,
            hold: false,
        };
        match self.advance_translation(&mut parked)? {
            Step::Pending => {
                self.pool.translation_slots.insert(id, parked);
                Ok(Step::Pending)
            }
            step => Ok(step),
        }
    }

    /// Drive every parked outcome one step. A commit schedules once, after the slot is gone.
    ///
    /// # Errors
    /// Returns a refused settlement fault or a scheduling failure. A scheduling failure cannot
    /// replay a publication that already committed.
    pub(crate) fn drive_settlement(&mut self) -> Result<()> {
        let recognition = self.drive_recognition()?;
        let translation = self.drive_translation()?;
        if recognition || translation {
            self.schedule()?;
        }
        Ok(())
    }

    fn drive_recognition(&mut self) -> Result<bool> {
        let ids: Vec<String> = self.pool.recognition_slots.keys().cloned().collect();
        let mut committed = false;
        for id in ids {
            let Some(mut parked) = self.pool.recognition_slots.remove(&id) else {
                continue;
            };
            match self.advance_recognition(&mut parked)? {
                Step::Committed => committed = true,
                Step::Pending => {
                    self.pool.recognition_slots.insert(id, parked);
                }
                Step::Exhausted => {}
            }
        }
        Ok(committed)
    }

    fn drive_translation(&mut self) -> Result<bool> {
        let ids: Vec<String> = self.pool.translation_slots.keys().cloned().collect();
        let mut committed = false;
        for id in ids {
            let Some(mut parked) = self.pool.translation_slots.remove(&id) else {
                continue;
            };
            match self.advance_translation(&mut parked)? {
                Step::Committed => committed = true,
                Step::Pending => {
                    self.pool.translation_slots.insert(id, parked);
                }
                Step::Exhausted => {}
            }
        }
        Ok(committed)
    }

    fn advance_recognition(&mut self, parked: &mut ParkedRecognition) -> Result<Step> {
        let now = self.settlement_clock.now();
        match pace(parked.tries, parked.hold, parked.taken, now) {
            Pace::Wait => Ok(Step::Pending),
            Pace::Exhaust => Ok(Step::Exhausted),
            Pace::Attempt => {
                parked.tries = parked.tries.saturating_add(1);
                self.finish_parked_recognition(parked)
            }
        }
    }

    fn advance_translation(&mut self, parked: &mut ParkedTranslation) -> Result<Step> {
        let now = self.settlement_clock.now();
        match pace(parked.tries, parked.hold, parked.taken, now) {
            Pace::Wait => Ok(Step::Pending),
            Pace::Exhaust => Ok(Step::Exhausted),
            Pace::Attempt => {
                parked.tries = parked.tries.saturating_add(1);
                self.finish_parked_translation(parked)
            }
        }
    }

    fn finish_parked_recognition(&mut self, parked: &mut ParkedRecognition) -> Result<Step> {
        let now = self.settlement_clock.now();
        match self.observed_recognition(parked) {
            Ok(job) => {
                note_recognition_evidence(self, &job, &parked.reaped);
                Ok(Step::Committed)
            }
            Err(error) => match classify(&error) {
                SettlementClass::Refuse => Err(error),
                class => self.recover_recognition(parked, class, now),
            },
        }
    }

    fn finish_parked_translation(&mut self, parked: &mut ParkedTranslation) -> Result<Step> {
        let now = self.settlement_clock.now();
        match self.observed_translation(parked) {
            Ok(_) => Ok(Step::Committed),
            Err(error) => match classify(&error) {
                SettlementClass::Refuse => Err(error),
                class => self.recover_translation(parked, class, now),
            },
        }
    }

    fn observed_recognition(&mut self, parked: &ParkedRecognition) -> Result<LocalAsrJob> {
        #[cfg(test)]
        if let Some(error) = self.injected_error() {
            return Err(error);
        }
        let now = crate::storage::now_ms()?;
        self.library
            .store_mut()
            .finish_local_asr(&parked.work, &parked.reaped, now)
    }

    fn observed_translation(&mut self, parked: &ParkedTranslation) -> Result<TranslationJob> {
        #[cfg(test)]
        if let Some(error) = self.injected_error() {
            return Err(error);
        }
        let now = crate::storage::now_ms()?;
        self.library
            .store_mut()
            .finish_translation(&parked.work, &parked.outcome, now)
    }

    /// The override stays until the test clears it, so several tries can share one fault.
    #[cfg(test)]
    fn injected_error(&mut self) -> Option<Error> {
        let kind = self.settlement_override?;
        self.settlement_attempts = self.settlement_attempts.saturating_add(1);
        Some(kind.error())
    }

    fn recover_recognition(
        &mut self,
        parked: &mut ParkedRecognition,
        class: SettlementClass,
        now: Instant,
    ) -> Result<Step> {
        let membership = self.recognition_membership(parked)?;
        let step = apply_failure(
            class,
            membership,
            &mut parked.hold,
            parked.tries,
            parked.taken,
            now,
        )?;
        if matches!(step, Step::Committed) {
            let id = parked.work.job.request.id.clone();
            if let Ok(job) = self.library.store().local_asr_job(&id) {
                note_recognition_evidence(self, &job, &parked.reaped);
            }
        }
        Ok(step)
    }

    fn recover_translation(
        &mut self,
        parked: &mut ParkedTranslation,
        class: SettlementClass,
        now: Instant,
    ) -> Result<Step> {
        let membership = self.translation_membership(parked)?;
        apply_failure(
            class,
            membership,
            &mut parked.hold,
            parked.tries,
            parked.taken,
            now,
        )
    }

    fn recognition_membership(&mut self, parked: &ParkedRecognition) -> Result<Membership> {
        let id = parked.work.job.request.id.as_str();
        match self.library.store().analysis_job_membership(id) {
            Ok(member) => Ok(interpret(
                member.as_ref(),
                parked.work.job.generation,
                parked.work.job.attempt,
                parked.work.claim_owner.as_str(),
            )),
            Err(error) => read_failure(error),
        }
    }

    fn translation_membership(&mut self, parked: &ParkedTranslation) -> Result<Membership> {
        let id = parked.work.job.request.id.as_str();
        match self.library.store().translation_job_membership(id) {
            Ok(member) => Ok(interpret(
                member.as_ref(),
                parked.work.job.generation,
                parked.work.job.attempt,
                parked.work.claim_owner.as_str(),
            )),
            Err(error) => read_failure(error),
        }
    }
}

fn note_recognition_evidence(actor: &mut Actor, job: &LocalAsrJob, reaped: &ReapedLocalAsr) {
    if job.state == "succeeded"
        && !reaped.languages().is_empty()
        && let Some(now) = job.finished_ms
    {
        let evidence = crate::recognizer::language_evidence(job, reaped.languages());
        let _ = actor
            .library
            .store_mut()
            .publish_language_evidence(evidence, now);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SETTLEMENT_ATTEMPTS, SETTLEMENT_DEADLINE, SettlementClass, classify, ffi, pace,
        sqlite_failure,
    };
    use crate::Error;
    use std::time::{Duration, Instant};

    #[test]
    fn busy_and_locked_retry_and_io_is_uncertain() {
        assert_eq!(
            classify(&sqlite_failure(ffi::SQLITE_BUSY)),
            SettlementClass::Retry
        );
        assert_eq!(
            classify(&sqlite_failure(ffi::SQLITE_BUSY_RECOVERY)),
            SettlementClass::Retry
        );
        assert_eq!(
            classify(&sqlite_failure(ffi::SQLITE_LOCKED)),
            SettlementClass::Retry
        );
        assert_eq!(
            classify(&sqlite_failure(ffi::SQLITE_LOCKED_SHAREDCACHE)),
            SettlementClass::Retry
        );
        assert_eq!(
            classify(&sqlite_failure(ffi::SQLITE_IOERR)),
            SettlementClass::Uncertain
        );
        assert_eq!(
            classify(&sqlite_failure(ffi::SQLITE_IOERR_READ)),
            SettlementClass::Uncertain
        );
        assert_eq!(
            classify(&Error::Io(std::io::Error::other("read"))),
            SettlementClass::Uncertain
        );
        for code in [
            ffi::SQLITE_CONSTRAINT,
            ffi::SQLITE_FULL,
            ffi::SQLITE_CORRUPT,
        ] {
            assert_eq!(classify(&sqlite_failure(code)), SettlementClass::Refuse);
        }
        assert_eq!(
            classify(&Error::Analysis("stale-worker")),
            SettlementClass::Refuse
        );
        assert_eq!(classify(&Error::StorageIntegrity), SettlementClass::Refuse);
        assert_eq!(classify(&Error::NotFound), SettlementClass::Refuse);
    }

    #[test]
    fn the_third_attempt_and_the_deadline_exhaust_without_a_fourth_timer() {
        let taken = Instant::now();
        assert_eq!(pace(0, false, taken, taken), super::Pace::Attempt);
        assert_eq!(
            pace(SETTLEMENT_ATTEMPTS - 1, false, taken, taken),
            super::Pace::Attempt
        );
        assert_eq!(
            pace(SETTLEMENT_ATTEMPTS, false, taken, taken),
            super::Pace::Exhaust
        );
        assert_eq!(pace(1, true, taken, taken), super::Pace::Wait);
        let later = taken + SETTLEMENT_DEADLINE;
        assert_eq!(pace(1, false, taken, later), super::Pace::Exhaust);
        assert_eq!(pace(1, true, taken, later), super::Pace::Exhaust);
        assert_eq!(SETTLEMENT_DEADLINE, Duration::from_secs(15));
        assert_eq!(SETTLEMENT_ATTEMPTS, 3);
    }

    #[test]
    fn every_injected_fault_uses_the_same_classes() {
        use super::SettlementOverride;
        let cases = [
            (SettlementOverride::Busy, SettlementClass::Retry),
            (SettlementOverride::Locked, SettlementClass::Retry),
            (SettlementOverride::Io, SettlementClass::Uncertain),
            (SettlementOverride::DiskFull, SettlementClass::Refuse),
            (SettlementOverride::Constraint, SettlementClass::Refuse),
            (SettlementOverride::Corrupt, SettlementClass::Refuse),
            (SettlementOverride::StaleWorker, SettlementClass::Refuse),
            (
                SettlementOverride::StorageIntegrity,
                SettlementClass::Refuse,
            ),
        ];
        for (kind, class) in cases {
            assert_eq!(classify(&kind.error()), class);
        }
    }
}
