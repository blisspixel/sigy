//! Direct-listen receipts. A receipt is not a recording and stores no stream URL.

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::{Store, now_ms, validate_key};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenStatus {
    pub id: String,
    pub source_revision: String,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub format: Option<String>,
    pub failure: Option<String>,
}

impl Store {
    /// Returns whether a new session should open the source. An existing id does not.
    /// # Errors
    /// Rejects an unknown revision, a conflicting replay, a held revision, or admission limits.
    pub(crate) fn begin_listen(&mut self, id: &str, revision: &str) -> Result<bool> {
        validate_key(id, "listen ID")?;
        validate_key(revision, "source revision")?;
        let now = now_ms()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT source_revision FROM listen_sessions WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != revision {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(false);
        }
        let found: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_revisions WHERE id = ?1)",
            [revision],
            |row| row.get(0),
        )?;
        if !found {
            return Err(Error::NotFound);
        }
        if revision_held(&tx, revision)? {
            return Err(Error::InvalidInput("source revision is in use"));
        }
        let count: u32 =
            tx.query_row("SELECT count(*) FROM listen_sessions", [], |row| row.get(0))?;
        if count >= 4096 {
            return Err(Error::InvalidInput("listen receipt capacity reached"));
        }
        tx.execute(
            "INSERT INTO listen_sessions(id, source_revision, state, started_ms) VALUES (?1, ?2, 'running', ?3)",
            params![id, revision, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// # Errors
    /// Rejects an unknown session or a format that is not one of the audio labels.
    pub(crate) fn finish_listen(&mut self, id: &str, format: &str) -> Result<()> {
        validate_key(id, "listen ID")?;
        let format = audio_format(format)?;
        let now = now_ms()?;
        if self.connection.execute(
            "UPDATE listen_sessions SET state = 'completed', completed_ms = ?2, format = ?3 WHERE id = ?1 AND state = 'running'",
            params![id, now, format],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    /// # Errors
    /// Rejects an unknown session or a failure detail that cannot be stored.
    pub(crate) fn fail_listen(&mut self, id: &str, error: &Error) -> Result<()> {
        let detail = listen_failure(error);
        if self.connection.execute(
            "UPDATE listen_sessions SET state = 'failed', failure = ?2 WHERE id = ?1 AND state = 'running'",
            params![id, detail],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    /// # Errors
    /// Rejects an unknown session. A terminal receipt is left unchanged.
    pub(crate) fn interrupt_listen(&mut self, id: &str, reason: &'static str) -> Result<()> {
        validate_key(id, "listen ID")?;
        validate_detail(reason)?;
        let now = now_ms()?;
        if self.connection.execute(
            "UPDATE listen_sessions SET state = 'interrupted', completed_ms = ?2, failure = ?3 WHERE id = ?1 AND state = 'running'",
            params![id, now, reason],
        )? != 1
        {
            return Err(Error::RequestState);
        }
        Ok(())
    }

    pub(crate) fn recover_listens(&mut self) -> Result<()> {
        self.connection.execute(
            "UPDATE listen_sessions SET state = 'interrupted', failure = 'service stopped before the listen completed' WHERE state = 'running'",
            [],
        )?;
        Ok(())
    }

    /// # Errors
    /// Rejects an unknown listen or a row whose text fields are not valid.
    pub fn listen(&self, id: &str) -> Result<ListenStatus> {
        validate_key(id, "listen ID")?;
        let row = self
            .connection
            .query_row(
                "SELECT source_revision, state, started_ms, completed_ms, format, failure FROM listen_sessions WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        validate_key(&row.0, "source revision")?;
        if let Some(format) = &row.4 {
            audio_format(format)?;
        }
        if let Some(failure) = &row.5 {
            validate_detail(failure)?;
        }
        Ok(ListenStatus {
            id: id.into(),
            source_revision: row.0,
            state: row.1,
            started_ms: row.2,
            completed_ms: row.3,
            format: row.4,
            failure: row.5,
        })
    }

    pub(crate) fn audit_listens(&self) -> Result<()> {
        let invalid: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM listen_sessions WHERE (state = 'completed' AND (format IS NULL OR failure IS NOT NULL OR completed_ms IS NULL)) OR (state = 'failed' AND (failure IS NULL OR format IS NOT NULL)) OR (state = 'running' AND (completed_ms IS NOT NULL OR format IS NOT NULL OR failure IS NOT NULL)) OR (state = 'interrupted' AND failure IS NULL))",
            [],
            |row| row.get(0),
        )?;
        if invalid {
            return Err(Error::StorageIntegrity);
        }
        Ok(())
    }
}

fn revision_held(tx: &rusqlite::Transaction<'_>, revision: &str) -> Result<bool> {
    let capture: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM capture_jobs WHERE source_revision = ?1 AND state IN ('scheduled', 'starting', 'running', 'retrying', 'stopping'))",
        [revision],
        |row| row.get(0),
    )?;
    let listen: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM listen_sessions WHERE source_revision = ?1 AND state = 'running')",
        [revision],
        |row| row.get(0),
    )?;
    Ok(capture || listen)
}

fn audio_format(value: &str) -> Result<&'static str> {
    match value {
        "mp3" => Ok("mp3"),
        "aac" => Ok("aac"),
        "flac" => Ok("flac"),
        "ogg" => Ok("ogg"),
        "wav" => Ok("wav"),
        _ => Err(Error::StorageIntegrity),
    }
}

fn listen_failure(error: &Error) -> &'static str {
    match error {
        Error::Acquisition(detail) | Error::InvalidInput(detail) => detail,
        Error::DestinationDenied => "listen destination is not authorized",
        Error::NotFound => "listen source was not found",
        _ => "listen failed",
    }
}

fn validate_detail(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        return Err(Error::StorageIntegrity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{HttpSource, NetworkScope};

    type TestResult = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

    fn source(url: &str) -> Result<HttpSource> {
        HttpSource::new("Radio", url, NetworkScope::PublicInternet {})
    }

    fn open_ready(path: &std::path::Path) -> Result<Store> {
        let mut store = Store::open(path)?;
        store.register_source("radio:v1", &source("https://example.com/audio")?)?;
        store.register_source("radio:v2", &source("https://example.com/other")?)?;
        let executable =
            std::env::current_exe().map_err(|_| Error::InvalidInput("test executable"))?;
        store.configure_dvr(
            1_000_000_000,
            64 * 1024 * 1024,
            14,
            executable
                .to_str()
                .ok_or(Error::InvalidInput("test executable"))?,
        )?;
        Ok(store)
    }

    #[test]
    fn listen_replay_does_not_admit_twice_and_a_held_revision_is_refused() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = open_ready(directory.path().join("catalog.sqlite3").as_path())?;
        assert!(store.begin_listen("hear", "radio:v1")?);
        assert!(!store.begin_listen("hear", "radio:v1")?);
        assert!(matches!(
            store.begin_listen("hear", "radio:v2"),
            Err(Error::IdempotencyConflict)
        ));
        assert!(matches!(
            store.begin_listen("other", "radio:v1"),
            Err(Error::InvalidInput("source revision is in use"))
        ));
        assert!(matches!(
            store.admit_recording(
                "take",
                "radio:v1",
                30,
                1_048_576,
                crate::storage::dvr::Retention::Temporary,
                false,
            ),
            Err(Error::InvalidInput("source revision is in use"))
        ));
        let charged = store.dvr_status()?.charged_bytes;
        store.finish_listen("hear", "wav")?;
        assert_eq!(store.listen("hear")?.state, "completed");
        assert_eq!(store.dvr_status()?.charged_bytes, charged);
        assert_eq!(store.dvr_status()?.reserved_bytes, 0);
        assert!(store.recordings(None, 1)?.is_empty());
        assert!(store.begin_listen("again", "radio:v1")?);
        store.interrupt_listen("again", "listen stopped")?;
        let admitted = store.admit_recording(
            "take",
            "radio:v1",
            30,
            1_048_576,
            crate::storage::dvr::Retention::Temporary,
            false,
        )?;
        assert!(admitted.is_some());
        assert!(matches!(
            store.begin_listen("blocked", "radio:v1"),
            Err(Error::InvalidInput("source revision is in use"))
        ));
        Ok(())
    }

    #[test]
    fn restart_marks_a_running_listen_interrupted() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog.sqlite3");
        {
            let mut store = open_ready(&path)?;
            assert!(store.begin_listen("live", "radio:v1")?);
        }
        let mut store = Store::open(&path)?;
        store.recover_listens()?;
        assert_eq!(store.listen("live")?.state, "interrupted");
        assert!(!store.begin_listen("live", "radio:v1")?);
        Ok(())
    }

    #[test]
    fn listen_migration_preserves_v9_and_rolls_back_conflicts() -> TestResult {
        let directory = tempfile::tempdir()?;
        for fail in [false, true] {
            let path = directory
                .path()
                .join(if fail { "bad.sqlite3" } else { "good.sqlite3" });
            let connection = rusqlite::Connection::open(&path)?;
            for migration in [
                include_str!("001-foundation.sql"),
                include_str!("002-captures.sql"),
                include_str!("003-sources.sql"),
                include_str!("004-dvr.sql"),
                include_str!("005-discovery.sql"),
                include_str!("006-redirects.sql"),
                include_str!("007-favorites.sql"),
                include_str!("008-playlists.sql"),
                include_str!("009-clicks.sql"),
            ] {
                connection.execute_batch(migration)?;
            }
            connection.execute("UPDATE budgets SET limit_micros = 4242", [])?;
            connection.execute(
                "INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms, redirect_policy) VALUES ('radio:v1', 'http_audio', 'Radio', 'https://example.com/audio', 'public_internet', 1, 'deny')",
                [],
            )?;
            connection.execute(
                "INSERT INTO directory_clicks(id, station_id, request_json, state, started_ms, acknowledged) VALUES ('heard', '12345678-1234-1234-1234-123456789abc', '{}', 'completed', 1, 1)",
                [],
            )?;
            if fail {
                connection.execute("CREATE TABLE listen_sessions(existing TEXT)", [])?;
            }
            let opened = Store::open(&path);
            let version: u32 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if fail {
                assert!(opened.is_err());
                assert_eq!(version, 9);
            } else {
                let mut store = opened?;
                assert_eq!(version, super::super::SCHEMA_VERSION);
                assert_eq!(store.budget("global")?.limit().micros(), 4242);
                assert_eq!(store.directory_click("heard")?.state, "completed");
                assert!(store.begin_listen("next", "radio:v1")?);
            }
        }
        Ok(())
    }
}
