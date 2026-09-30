//! Append one original-script correction. The previous revision stays readable.

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::{Store, validate_key};
use crate::{Error, Result};

pub(crate) const PROFILE: &str = "user-correction-v1";
const MAX_CUE_BYTES: usize = 4096;
const MAX_TEXT_BYTES: usize = 65_536;

const KIND_CHECK: (&str, &str) = (
    "CHECK(kind IN ('legacy_placeholder', 'recognition'))",
    "CHECK(kind IN ('legacy_placeholder', 'recognition', 'correction'))",
);
const OUTCOME_CHECK: (&str, &str) = (
    "AND ((outcome = 'text' AND cue_count > 0 AND text_bytes > 0) OR (outcome = 'no_text' AND cue_count = 0 AND text_bytes = 0))))",
    "AND ((outcome = 'text' AND cue_count > 0 AND text_bytes > 0) OR (outcome = 'no_text' AND cue_count = 0 AND text_bytes = 0))) OR (kind = 'correction' AND outcome = 'text' AND profile = 'user-correction-v1' AND parent_revision IS NOT NULL AND revision = parent_revision + 1 AND job_id IS NULL AND job_generation IS NULL AND profile_sha256 IS NULL AND cue_count > 0 AND text_bytes > 0))",
);

struct Parent {
    analysis_revision: i64,
    recording_id: String,
    media_sha256: String,
    outcome: String,
    created_ms: i64,
    latest: i64,
    input_current: bool,
    retained: bool,
}

struct Cue {
    ordinal: u32,
    start_us: i64,
    end_us: i64,
    script: String,
}

/// Widen the transcript check and install the correction triggers.
/// # Errors
/// Fails closed when the stored transcript definition is not the v35 text.
pub(super) fn migrate_036(tx: &Transaction<'_>) -> Result<()> {
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    // A test catalog rewound to an older user_version can already carry this check.
    let definition: String = tx.query_row(
        "SELECT sql FROM main.sqlite_schema WHERE type = 'table' AND name = 'transcripts'",
        [],
        |row| row.get(0),
    )?;
    if !definition.contains(KIND_CHECK.1) {
        super::widen::rebuild(tx, "transcripts", &[KIND_CHECK, OUTCOME_CHECK], &[])?;
    }
    tx.execute_batch(include_str!("036-corrections.sql"))?;
    let violations: i64 =
        tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations != 0 {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}

impl Store {
    /// Append a revision that replaces one cue's original script and copies the rest.
    /// The expected revision must be the newest. Nothing is dispatched.
    /// # Errors
    /// Conflicts when that revision is no longer newest. An input that is not the current
    /// retained pin is refused, and the previous revision stays.
    pub(crate) fn correct_transcript(
        &mut self,
        id: &str,
        expected: i64,
        ordinal: u32,
        script: &str,
        now: i64,
    ) -> Result<i64> {
        validate_key(id, "transcript ID")?;
        accept_script(script)?;
        if now < 0 || !(1..=64).contains(&expected) {
            return Err(Error::InvalidInput("transcript revision"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let parent = parent(&tx, id, expected)?;
        if now < parent.created_ms {
            return Err(Error::InvalidInput("correction clock"));
        }
        let revision = expected + 1;
        let cues = corrected_cues(&tx, id, expected, ordinal, script)?;
        let count = i64::try_from(cues.len()).map_err(|_| Error::StorageIntegrity)?;
        let bytes = text_bytes(&cues)?;
        insert_revision(&tx, id, revision, &parent, count, bytes, now)?;
        for cue in &cues {
            tx.execute(
                "INSERT INTO transcript_cues(transcript_id, revision, ordinal, start_us, end_us, script, wording) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'uncertain')",
                params![id, revision, cue.ordinal, cue.start_us, cue.end_us, cue.script],
            )
            .map_err(correction_error)?;
        }
        tx.execute(
            "INSERT INTO analysis_decisions(transcript_id, transcript_revision, amount_micros, request_id, created_ms) VALUES (?1, ?2, 0, NULL, ?3)",
            params![id, revision, now],
        )
        .map_err(correction_error)?;
        tx.commit()?;
        Ok(revision)
    }
}

fn accept_script(script: &str) -> Result<()> {
    if script.is_empty() || script.len() > MAX_CUE_BYTES || script.chars().any(char::is_control) {
        return Err(Error::InvalidInput("correction text"));
    }
    Ok(())
}

fn parent(tx: &Transaction<'_>, id: &str, expected: i64) -> Result<Parent> {
    let Some(parent) = tx
        .query_row(
            "SELECT t.analysis_revision, t.recording_id, t.media_sha256, t.outcome, t.created_ms, (SELECT max(revision) FROM transcripts WHERE id = t.id), a.state = 'published' AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id), r.storage_state = 'retained' AND r.sha256 = t.media_sha256 FROM transcripts t JOIN analysis_inputs a ON a.id = t.analysis_id AND a.revision = t.analysis_revision JOIN recordings r ON r.id = t.recording_id WHERE t.id = ?1 AND t.revision = ?2",
            params![id, expected],
            |row| {
                Ok(Parent {
                    analysis_revision: row.get(0)?,
                    recording_id: row.get(1)?,
                    media_sha256: row.get(2)?,
                    outcome: row.get(3)?,
                    created_ms: row.get(4)?,
                    latest: row.get(5)?,
                    input_current: row.get(6)?,
                    retained: row.get(7)?,
                })
            },
        )
        .optional()?
    else {
        return Err(Error::NotFound);
    };
    if parent.latest == 64 && expected == parent.latest {
        return Err(Error::Analysis("revision-limit"));
    }
    if parent.latest != expected {
        return Err(Error::Analysis("revision-conflict"));
    }
    if parent.outcome != "text" {
        return Err(Error::Analysis("correction-unavailable"));
    }
    if !parent.input_current || !parent.retained {
        return Err(Error::Analysis("input-expired"));
    }
    Ok(parent)
}

fn corrected_cues(
    tx: &Transaction<'_>,
    id: &str,
    expected: i64,
    ordinal: u32,
    script: &str,
) -> Result<Vec<Cue>> {
    let mut statement = tx.prepare(
        "SELECT ordinal, start_us, end_us, script FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 ORDER BY ordinal",
    )?;
    let mut cues = statement
        .query_map(params![id, expected], |row| {
            Ok(Cue {
                ordinal: row.get(0)?,
                start_us: row.get(1)?,
                end_us: row.get(2)?,
                script: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let Some(cue) = cues.iter_mut().find(|cue| cue.ordinal == ordinal) else {
        return Err(Error::InvalidInput("transcript cue"));
    };
    if cue.script == script {
        return Err(Error::Analysis("correction-unchanged"));
    }
    script.clone_into(&mut cue.script);
    Ok(cues)
}

fn text_bytes(cues: &[Cue]) -> Result<i64> {
    let bytes = cues
        .iter()
        .map(|cue| cue.script.len())
        .try_fold(0_usize, usize::checked_add)
        .ok_or(Error::StorageIntegrity)?;
    if bytes == 0 || bytes > MAX_TEXT_BYTES {
        return Err(Error::InvalidInput("correction text"));
    }
    i64::try_from(bytes).map_err(|_| Error::StorageIntegrity)
}

fn insert_revision(
    tx: &Transaction<'_>,
    id: &str,
    revision: i64,
    parent: &Parent,
    count: i64,
    bytes: i64,
    now: i64,
) -> Result<()> {
    tx.execute(
        "INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, kind, outcome, parent_revision, job_id, job_generation, profile_sha256, cue_count, text_bytes, state, created_ms) VALUES (?1, ?2, ?1, ?3, ?4, ?5, 'original', ?6, 'correction', 'text', ?7, NULL, NULL, NULL, ?8, ?9, 'published', ?10)",
        params![id, revision, parent.analysis_revision, parent.recording_id, parent.media_sha256, PROFILE, revision - 1, count, bytes, now],
    )
    .map_err(correction_error)?;
    Ok(())
}

fn correction_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    if message.contains("transcript revision conflicts") {
        Error::Analysis("revision-conflict")
    } else if message.contains("invalid or sealed transcript cue")
        || message.contains("transcript result is incomplete")
    {
        Error::StorageIntegrity
    } else {
        Error::Database(error)
    }
}
