//! In-memory playheads. A session is not a capture and not a direct-listen receipt.

use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    recordings::{earliest_retained, live_edge, position_expired},
    storage::dvr::Recording,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaybackOperation {
    Attach { id: String, recording_id: String },
    Pause { id: String },
    Live { id: String },
    Show { id: String },
    Detach { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackView {
    pub id: String,
    pub recording_id: String,
    pub state: String,
    pub playhead_us: u64,
    pub live_us: Option<u64>,
    pub earliest_us: Option<u64>,
    pub open_tail: bool,
}

pub(crate) fn describe(session: &PlaySession, recording: &Recording) -> PlaybackView {
    let live_us = live_edge(&recording.intervals);
    let earliest_us = earliest_retained(&recording.intervals);
    let state = if session.expired {
        "expired"
    } else if session.paused {
        "paused"
    } else if session.parked {
        "parked"
    } else {
        "attached"
    };
    PlaybackView {
        id: session.id.clone(),
        recording_id: session.recording.clone(),
        state: state.into(),
        playhead_us: session.playhead_us,
        live_us,
        earliest_us,
        open_tail: recording.state == "running"
            && recording.open_ceiling > 0
            && recording.open_object_key.is_some(),
    }
}

pub(crate) const MAX_PLAYBACK_SESSIONS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaySession {
    pub id: String,
    pub recording: String,
    pub paused: bool,
    pub parked: bool,
    pub expired: bool,
    pub playhead_us: u64,
}

#[derive(Debug, Default)]
pub(crate) struct PlaySessions {
    sessions: Vec<PlaySession>,
}

impl PlaySessions {
    pub(crate) fn open(&mut self, id: &str, recording: &str, playhead_us: u64) -> Result<()> {
        if self.sessions.iter().any(|session| session.id == id) {
            return Err(Error::IdempotencyConflict);
        }
        if self.sessions.len() >= MAX_PLAYBACK_SESSIONS {
            return Err(Error::InvalidInput("playback session capacity reached"));
        }
        self.sessions.push(PlaySession {
            id: id.to_owned(),
            recording: recording.to_owned(),
            paused: false,
            parked: false,
            expired: false,
            playhead_us,
        });
        Ok(())
    }

    pub(crate) fn pause(&mut self, id: &str) -> Result<()> {
        let session = self.session_mut(id)?;
        session.paused = true;
        session.parked = false;
        Ok(())
    }

    pub(crate) fn park(&mut self, id: &str, live_us: u64) -> Result<()> {
        let session = self.session_mut(id)?;
        session.playhead_us = live_us;
        session.parked = true;
        session.paused = false;
        session.expired = false;
        Ok(())
    }

    pub(crate) fn recording_id(&self, id: &str) -> Option<&str> {
        self.sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| session.recording.as_str())
    }

    pub(crate) fn close(&mut self, id: &str) -> Result<PlaySession> {
        let index = self
            .sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or(Error::NotFound)?;
        Ok(self.sessions.remove(index))
    }

    pub(crate) fn refresh(&mut self, id: &str, recording: &Recording) -> Result<PlaySession> {
        let session = self.session_mut(id)?;
        if session.recording != recording.id {
            return Err(Error::RequestState);
        }
        session.expired = session.paused
            && position_expired(session.playhead_us, earliest_retained(&recording.intervals));
        Ok(session.clone())
    }

    fn session_mut(&mut self, id: &str) -> Result<&mut PlaySession> {
        self.sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or(Error::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::PlaySessions;
    use crate::storage::dvr::{Recording, RecordingInterval, Retention};

    fn recording(start: u64) -> Recording {
        Recording {
            id: "job".into(),
            source_revision: "radio:v1".into(),
            state: "running".into(),
            object_key: "a".repeat(32),
            duration_seconds: 60,
            maximum_bytes: 1024,
            retention: Retention::Temporary,
            storage_state: "reserved".into(),
            charged_bytes: 1024,
            media_bytes: None,
            sha256: None,
            format: None,
            decoded_microseconds: None,
            end_reason: None,
            processing_receipt: None,
            failure_detail: None,
            profile: crate::storage::dvr::RecordingProfile::Radio,
            escrow_bytes: 0,
            open_ceiling: 32 * 1024 * 1024,
            open_object_key: Some("b".repeat(32)),
            lease_renewals: 1,
            intervals: vec![RecordingInterval {
                ordinal: 0,
                decoded_start_us: start,
                decoded_end_us: start + 1_000_000,
                byte_start: 0,
                byte_end: 1000,
                object_key: "c".repeat(32),
                sha256: "ab".repeat(32),
                format: "wav".into(),
                ceiling_bytes: 32 * 1024 * 1024,
            }],
            gaps: Vec::new(),
        }
    }

    #[test]
    fn two_sessions_pause_and_detach_without_changing_the_capture() -> Result<(), String> {
        let mut sessions = PlaySessions::default();
        sessions
            .open("one", "job", 200_000)
            .map_err(|error| error.to_string())?;
        sessions
            .open("two", "job", 800_000)
            .map_err(|error| error.to_string())?;
        sessions.pause("one").map_err(|error| error.to_string())?;
        let capture = recording(0);
        let paused = sessions
            .refresh("one", &capture)
            .map_err(|error| error.to_string())?;
        if !paused.paused
            || paused.expired
            || capture.state != "running"
            || !capture.gaps.is_empty()
        {
            return Err("pause changed the capture".into());
        }
        sessions.close("one").map_err(|error| error.to_string())?;
        let remaining = sessions
            .refresh("two", &capture)
            .map_err(|error| error.to_string())?;
        if remaining.playhead_us != 800_000 || capture.state != "running" {
            return Err("detach changed the capture".into());
        }
        sessions
            .park("two", 1_000_000)
            .map_err(|error| error.to_string())?;
        let parked = sessions
            .refresh("two", &capture)
            .map_err(|error| error.to_string())?;
        if !parked.parked || parked.playhead_us != 1_000_000 {
            return Err("live park moved into the tail".into());
        }
        sessions
            .open("old", "job", 200_000)
            .map_err(|error| error.to_string())?;
        sessions.pause("old").map_err(|error| error.to_string())?;
        let later = recording(1_000_000);
        let expired = sessions
            .refresh("old", &later)
            .map_err(|error| error.to_string())?;
        if !expired.expired || later.state != "running" || !later.gaps.is_empty() {
            return Err("expired pause changed the capture".into());
        }
        Ok(())
    }
}
