//! Capture lifecycle. Analysis and client connection state are independent.

use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    Scheduled,
    Starting,
    Running,
    Retrying,
    Stopping,
    Completed,
    Interrupted,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureEvent {
    Start,
    Connected,
    Retry,
    Stop,
    Finalized,
    Lost,
    Cancel,
    Fail,
}

impl CaptureEvent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Connected => "connected",
            Self::Retry => "retry",
            Self::Stop => "stop",
            Self::Finalized => "finalized",
            Self::Lost => "lost",
            Self::Cancel => "cancel",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: CaptureState,
    pub event: CaptureEvent,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot apply {:?} to {} capture", self.event, self.from)
    }
}

impl std::error::Error for InvalidTransition {}

impl CaptureState {
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Retrying | Self::Stopping
        )
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Retrying => "retrying",
            Self::Stopping => "stopping",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    /// # Errors
    /// Rejects transitions outside the capture lifecycle contract.
    pub const fn transition(self, event: CaptureEvent) -> Result<Self, InvalidTransition> {
        use CaptureEvent as E;
        use CaptureState as S;
        match (self, event) {
            (S::Scheduled | S::Retrying | S::Interrupted, E::Start) => Ok(S::Starting),
            (S::Starting, E::Connected) => Ok(S::Running),
            (S::Starting | S::Running, E::Retry) => Ok(S::Retrying),
            (S::Starting | S::Running | S::Retrying, E::Stop) => Ok(S::Stopping),
            (S::Stopping, E::Finalized) => Ok(S::Completed),
            (S::Starting | S::Running | S::Retrying | S::Stopping, E::Lost) => Ok(S::Interrupted),
            (S::Scheduled | S::Interrupted, E::Cancel) => Ok(S::Cancelled),
            (
                S::Scheduled
                | S::Starting
                | S::Running
                | S::Retrying
                | S::Stopping
                | S::Interrupted,
                E::Fail,
            ) => Ok(S::Failed),
            _ => Err(InvalidTransition { from: self, event }),
        }
    }
}

impl fmt::Display for CaptureState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownCaptureState;

impl fmt::Display for UnknownCaptureState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown capture state")
    }
}
impl std::error::Error for UnknownCaptureState {}

impl FromStr for CaptureState {
    type Err = UnknownCaptureState;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "scheduled" => Ok(Self::Scheduled),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "retrying" => Ok(Self::Retrying),
            "stopping" => Ok(Self::Stopping),
            "completed" => Ok(Self::Completed),
            "interrupted" => Ok(Self::Interrupted),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            _ => Err(UnknownCaptureState),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_finalization() -> Result<(), InvalidTransition> {
        let running = CaptureState::Scheduled
            .transition(CaptureEvent::Start)?
            .transition(CaptureEvent::Connected)?;
        assert!(running.transition(CaptureEvent::Finalized).is_err());
        assert_eq!(
            running
                .transition(CaptureEvent::Stop)?
                .transition(CaptureEvent::Finalized)?,
            CaptureState::Completed
        );
        assert_eq!(
            running.transition(CaptureEvent::Lost)?,
            CaptureState::Interrupted
        );
        Ok(())
    }

    #[test]
    fn terminal_states_cannot_restart_or_mutate() {
        for state in [
            CaptureState::Completed,
            CaptureState::Cancelled,
            CaptureState::Failed,
        ] {
            for event in [
                CaptureEvent::Start,
                CaptureEvent::Connected,
                CaptureEvent::Retry,
                CaptureEvent::Stop,
                CaptureEvent::Finalized,
                CaptureEvent::Lost,
                CaptureEvent::Cancel,
                CaptureEvent::Fail,
            ] {
                assert!(state.transition(event).is_err(), "{state:?} {event:?}");
            }
        }
    }

    #[test]
    fn interrupted_capture_requires_a_new_start() -> Result<(), InvalidTransition> {
        assert!(
            CaptureState::Interrupted
                .transition(CaptureEvent::Finalized)
                .is_err()
        );
        assert!(
            CaptureState::Interrupted
                .transition(CaptureEvent::Connected)
                .is_err()
        );
        assert_eq!(
            CaptureState::Interrupted.transition(CaptureEvent::Start)?,
            CaptureState::Starting
        );
        Ok(())
    }
}
