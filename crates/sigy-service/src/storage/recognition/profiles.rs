use rusqlite::{OptionalExtension, Row};

use super::{Error, Result, Store, params, sql_integer, validate_key};
use crate::recognition::RecognitionProfile;

const COLUMNS: &str = "id, engine, runtime_dir, executable, runtime_sha256, runtime_files, runtime_bytes, model_path, model_sha256, model_bytes, vad_path, vad_sha256, vad_bytes, threads, memory_bytes, deadline_ms, profile_sha256";

impl Store {
    /// Store one immutable recognizer profile. An identical replay is unchanged.
    /// Returns whether a new row was created.
    /// # Errors
    /// Refuses invalid bounds, a mismatched identity, or a changed profile under an existing ID.
    pub(crate) fn add_recognition_profile(
        &mut self,
        profile: &RecognitionProfile,
        now: i64,
    ) -> Result<bool> {
        profile.validate()?;
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        if let Some(existing) = self.find_recognition_profile(&profile.id)? {
            if existing != *profile {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(false);
        }
        self.connection.execute(
            &format!("INSERT INTO recognition_profiles({COLUMNS}, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"),
            params![
                profile.id, profile.engine, profile.runtime_dir, profile.executable,
                profile.runtime_sha256, profile.runtime_files, sql_integer(profile.runtime_bytes)?,
                profile.model_path, profile.model_sha256, sql_integer(profile.model_bytes)?,
                profile.vad_path, profile.vad_sha256, sql_integer(profile.vad_bytes)?,
                profile.threads, sql_integer(profile.memory_bytes)?, sql_integer(profile.deadline_ms)?,
                profile.profile_sha256, now
            ],
        )?;
        Ok(true)
    }

    /// Read one recognizer profile.
    /// # Errors
    /// Refuses a malformed or missing ID and invalid stored rows.
    pub fn recognition_profile(&self, id: &str) -> Result<RecognitionProfile> {
        validate_key(id, "recognition profile")?;
        self.find_recognition_profile(id)?.ok_or(Error::NotFound)
    }

    /// Read every recognizer profile, at most 64, ordered by ID.
    /// # Errors
    /// Refuses invalid stored rows.
    pub fn recognition_profiles(&self) -> Result<Vec<RecognitionProfile>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {COLUMNS} FROM recognition_profiles ORDER BY id LIMIT 65"
        ))?;
        let profiles = statement
            .query_map([], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if profiles.len() > 64 {
            return Err(Error::StorageIntegrity);
        }
        for profile in &profiles {
            profile.validate().map_err(|_| Error::StorageIntegrity)?;
        }
        Ok(profiles)
    }

    fn find_recognition_profile(&self, id: &str) -> Result<Option<RecognitionProfile>> {
        let profile = self
            .connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM recognition_profiles WHERE id = ?1"),
                [id],
                row,
            )
            .optional()?;
        if let Some(profile) = &profile {
            profile.validate().map_err(|_| Error::StorageIntegrity)?;
        }
        Ok(profile)
    }
}

fn unsigned(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn row(row: &Row<'_>) -> rusqlite::Result<RecognitionProfile> {
    Ok(RecognitionProfile {
        id: row.get(0)?,
        engine: row.get(1)?,
        runtime_dir: row.get(2)?,
        executable: row.get(3)?,
        runtime_sha256: row.get(4)?,
        runtime_files: row.get(5)?,
        runtime_bytes: unsigned(row, 6)?,
        model_path: row.get(7)?,
        model_sha256: row.get(8)?,
        model_bytes: unsigned(row, 9)?,
        vad_path: row.get(10)?,
        vad_sha256: row.get(11)?,
        vad_bytes: unsigned(row, 12)?,
        threads: row.get(13)?,
        memory_bytes: unsigned(row, 14)?,
        deadline_ms: unsigned(row, 15)?,
        profile_sha256: row.get(16)?,
    })
}
