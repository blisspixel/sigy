//! A private hint for one panel listen. The catalog listen row remains authority.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use super::admit::SESSION_UNREADABLE;

const FILE_NAME: &str = "panel-session.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanelSession {
    pub listen_id: String,
    pub station_id: String,
    pub revision_id: String,
    pub state: String,
}

#[derive(Debug)]
pub(crate) enum SessionHint {
    Absent,
    Unreadable,
    Present(PanelSession),
}

#[must_use]
pub(crate) fn read(directory: &Path) -> SessionHint {
    let path = directory.join(FILE_NAME);
    match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => SessionHint::Absent,
        Err(_) => SessionHint::Unreadable,
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_or(SessionHint::Unreadable, SessionHint::Present)
        }
    }
}

/// # Errors
/// Returns an error when the hint cannot be replaced privately.
pub(crate) fn write(directory: &Path, session: &PanelSession) -> io::Result<()> {
    let path = directory.join(FILE_NAME);
    let bytes = serde_json::to_vec(session).map_err(io::Error::other)?;
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// # Errors
/// Returns an error when a matching hint exists and cannot be updated.
pub(crate) fn settle(directory: &Path, listen_id: &str, state: &str) -> Result<(), String> {
    match read(directory) {
        SessionHint::Present(mut session) if session.listen_id == listen_id => {
            state.clone_into(&mut session.state);
            write(directory, &session).map_err(|error| error.to_string())
        }
        SessionHint::Unreadable => Err(SESSION_UNREADABLE.into()),
        SessionHint::Absent | SessionHint::Present(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> PanelSession {
        PanelSession {
            listen_id: "listen-1".into(),
            station_id: "station".into(),
            revision_id: "rev-1".into(),
            state: "running".into(),
        }
    }

    #[test]
    fn hint_round_trip_rejects_unknown_fields_and_other_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        assert!(matches!(read(directory.path()), SessionHint::Absent));
        write(directory.path(), &session())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(directory.path().join(FILE_NAME))?
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        match read(directory.path()) {
            SessionHint::Present(read) => assert_eq!(read, session()),
            SessionHint::Absent | SessionHint::Unreadable => {
                return Err("session hint was not readable".into());
            }
        }
        fs::write(
            directory.path().join(FILE_NAME),
            b"{\"listen_id\":\"x\",\"url\":\"http://example\"}",
        )?;
        assert!(matches!(read(directory.path()), SessionHint::Unreadable));
        write(directory.path(), &session())?;
        settle(directory.path(), "other", "completed")?;
        match read(directory.path()) {
            SessionHint::Present(read) => assert_eq!(read.state, "running"),
            SessionHint::Absent | SessionHint::Unreadable => {
                return Err("session hint changed unexpectedly".into());
            }
        }
        settle(directory.path(), "listen-1", "completed")?;
        match read(directory.path()) {
            SessionHint::Present(read) => assert_eq!(read.state, "completed"),
            SessionHint::Absent | SessionHint::Unreadable => {
                return Err("matching session hint was not updated".into());
            }
        }
        Ok(())
    }
}
