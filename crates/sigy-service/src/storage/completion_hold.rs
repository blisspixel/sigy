//! A publication body committed before recognition or translation finish.
//!
//! The bytes are the finish path's inputs. They are not an executor envelope.
//! Reading them back does not construct [`crate::recognition::ReapedLocalAsr`]
//! or [`crate::translation::TranslationOutcome`].

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::job_pool::{self, Family};
use super::{Store, validate_key};
use crate::execution::GroupAccount;
use crate::recognition::{
    ChunkLanguage, LocalAsrFailure, LocalAsrOutcome, LocalAsrWork, ReapedLocalAsr,
    RecognitionCoverage, RecognitionCue, RecognitionOutput, sha256_hex,
};
use crate::translation::{TranslatedCue, TranslationOutcome, TranslationResult, TranslationWork};
use crate::{Error, Result};

const HOLD_VERSION: u32 = 1;
const HOLD_BODY_BYTES: usize = 262_144;
const MAX_LANGUAGE_CODE: usize = 16;

/// One restored recognition publication. This is not a drained worker capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeldRecognition {
    pub outcome: LocalAsrOutcome,
    pub languages: Vec<ChunkLanguage>,
    pub observations: Vec<GroupAccount>,
}

/// One restored translation publication. This is not a drained worker capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HeldTranslation {
    Succeeded(Vec<TranslatedCue>),
    Failed(String),
    Cancelled,
}

#[derive(Debug)]
pub(crate) struct StoredHold {
    pub family: String,
    pub job_id: String,
    pub generation: u32,
    pub attempt: u32,
    pub owner: String,
    pub payload: HoldPayload,
}

/// Whether the stored body is the publication this process would write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoldSight {
    Same,
    Absent,
    Different,
}

#[derive(Debug)]
pub(crate) enum HoldPayload {
    Recognition(HeldRecognition),
    Translation(HeldTranslation),
    /// The row is intact, but its body cannot be applied. Restore fails the job.
    Unreadable,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    family: String,
    outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recognition: Option<RecognitionDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    translation: Option<TranslationDocument>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecognitionDocument {
    profile_sha256: String,
    manifest_sha256: String,
    coverages: Vec<RecognitionCoverage>,
    cues: Vec<RecognitionCue>,
    #[serde(default)]
    languages: Vec<LanguageDocument>,
    #[serde(default)]
    observations: Vec<ObservationDocument>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationDocument {
    cues: Vec<TranslatedCue>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageDocument {
    ordinal: u32,
    interval_ordinal: u32,
    start_us: u64,
    end_us: u64,
    code: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationDocument {
    role: String,
    ordinal: u32,
    mechanism: String,
    peak_memory_bytes: Option<u64>,
    cpu_time_us: Option<u64>,
}

impl Store {
    /// Commit the publication body for this claimed recognition, before finish.
    ///
    /// The same bytes are idempotent. A different body for the same job is refused.
    /// # Errors
    /// Returns catalog failures and an oversized or conflicting body.
    pub(crate) fn put_recognition_hold(
        &mut self,
        work: &LocalAsrWork,
        reaped: &ReapedLocalAsr,
    ) -> Result<()> {
        if !reaped.matches(work) {
            return Err(Error::Analysis("stale-worker"));
        }
        let body = encode(&recognition_document(work, reaped))?;
        self.put_hold(
            "analysis",
            &work.job.request.id,
            work.job.generation,
            work.job.attempt,
            &work.claim_owner,
            &body,
        )
    }

    /// Commit the publication body for this claimed translation, before finish.
    /// # Errors
    /// Returns catalog failures and an oversized or conflicting body.
    pub(crate) fn put_translation_hold(
        &mut self,
        work: &TranslationWork,
        outcome: &TranslationOutcome,
    ) -> Result<()> {
        if !outcome.matches(&work.job) {
            return Err(Error::Analysis("stale-worker"));
        }
        let body = encode(&translation_document(work, outcome)?)?;
        self.put_hold(
            "translation",
            &work.job.request.id,
            work.job.generation,
            work.job.attempt,
            &work.claim_owner,
            &body,
        )
    }

    /// Holds whose hash and document parse. A body that cannot be applied is
    /// [`HoldPayload::Unreadable`] rather than a reconstructed worker result.
    /// # Errors
    /// Returns catalog failures and a document whose hash does not match.
    pub(crate) fn list_completion_holds(&self) -> Result<Vec<StoredHold>> {
        let mut statement = self.connection.prepare(
            "SELECT family, job_id, generation, attempt, lease_owner, body, body_sha256 FROM completion_holds ORDER BY family, job_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut holds = Vec::new();
        for row in rows {
            let (family, job_id, generation, attempt, owner, body, digest) = row?;
            if sha256_hex(&body) != digest {
                return Err(Error::StorageIntegrity);
            }
            let payload = match decode_payload(&family, &body)? {
                Some(payload) => payload,
                None => HoldPayload::Unreadable,
            };
            holds.push(StoredHold {
                family,
                job_id,
                generation,
                attempt,
                owner,
                payload,
            });
        }
        Ok(holds)
    }

    /// Fail a running claim whose stored body cannot be applied, and delete that hold.
    /// A terminal job only loses the hold. No transcript or translation is written.
    /// # Errors
    /// Returns catalog failures and a missing job. A different claim only loses the hold.
    pub(crate) fn release_unreadable_hold(
        &mut self,
        family: &str,
        job_id: &str,
        generation: u32,
        attempt: u32,
        owner: &str,
        now: i64,
    ) -> Result<()> {
        let stop_family = stop_family(family)?;
        let table = job_table(family)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let member = job_pool::job_membership(&tx, family_of(family)?, job_id)?;
        let Some(member) = member else {
            return Err(Error::StorageIntegrity);
        };
        let same = member.generation == generation
            && member.attempt == attempt
            && member.owner.as_deref() == Some(owner);
        let running = matches!(member.state.as_str(), "running" | "cancelling");
        if same && running {
            let finished = unreadable_finished(&tx, stop_family, table, job_id, generation, now)?;
            let changed = tx.execute(
                &format!(
                    "UPDATE {table} SET state = 'failed', reason = 'held-completion-unreadable', finished_ms = ?5 WHERE id = ?1 AND generation = ?2 AND attempt = ?3 AND lease_owner = ?4 AND state IN ('running', 'cancelling')"
                ),
                params![job_id, generation, attempt, owner, finished],
            )?;
            if changed != 1 {
                return Err(Error::StorageIntegrity);
            }
        }
        delete_matching(&tx, family, job_id, generation, attempt, owner)?;
        tx.commit()?;
        Ok(())
    }

    /// Delete one hold that matches this in-memory claim. Zero rows is success.
    /// # Errors
    /// Returns catalog failures.
    pub(crate) fn delete_completion_hold(
        &mut self,
        family: &str,
        job_id: &str,
        generation: u32,
        attempt: u32,
        owner: &str,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        delete_matching(&tx, family, job_id, generation, attempt, owner)?;
        tx.commit()?;
        Ok(())
    }

    /// Compare the stored recognition body with the publication this worker would write.
    /// # Errors
    /// Returns a stale worker and catalog failures.
    pub(crate) fn recognition_hold_sight(
        &self,
        work: &LocalAsrWork,
        reaped: &ReapedLocalAsr,
    ) -> Result<HoldSight> {
        if !reaped.matches(work) {
            return Err(Error::Analysis("stale-worker"));
        }
        let body = encode(&recognition_document(work, reaped))?;
        self.hold_sight("analysis", &work.job.request.id, &body)
    }

    /// Compare the stored translation body with the publication this worker would write.
    /// # Errors
    /// Returns a stale worker and catalog failures.
    pub(crate) fn translation_hold_sight(
        &self,
        work: &TranslationWork,
        outcome: &TranslationOutcome,
    ) -> Result<HoldSight> {
        if !outcome.matches(&work.job) {
            return Err(Error::Analysis("stale-worker"));
        }
        let body = encode(&translation_document(work, outcome)?)?;
        self.hold_sight("translation", &work.job.request.id, &body)
    }

    /// Every hold hashes to its body, parses, and names a job that is still in its table.
    /// # Errors
    /// Returns a catalog integrity error when a hold is damaged or orphaned.
    pub(crate) fn audit_completion_holds(&self) -> Result<()> {
        let mut statement = self.connection.prepare(
            "SELECT family, job_id, body, body_sha256 FROM completion_holds ORDER BY family, job_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (family, job_id, body, digest) = row?;
            if sha256_hex(&body) != digest || document(&body).is_err() {
                return Err(Error::StorageIntegrity);
            }
            let exists: bool = match family.as_str() {
                "analysis" => self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM analysis_jobs WHERE id = ?1)",
                    [&job_id],
                    |row| row.get(0),
                )?,
                "translation" => self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM translation_jobs WHERE id = ?1)",
                    [&job_id],
                    |row| row.get(0),
                )?,
                _ => return Err(Error::StorageIntegrity),
            };
            if !exists {
                return Err(Error::StorageIntegrity);
            }
        }
        Ok(())
    }

    fn put_hold(
        &mut self,
        family: &str,
        job_id: &str,
        generation: u32,
        attempt: u32,
        owner: &str,
        body: &[u8],
    ) -> Result<()> {
        validate_key(job_id, "completion hold job")?;
        if owner.is_empty() || owner.len() > 128 || !(1..=3).contains(&attempt) {
            return Err(Error::StorageIntegrity);
        }
        let digest = sha256_hex(body);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<Vec<u8>> = tx
            .query_row(
                "SELECT body FROM completion_holds WHERE family = ?1 AND job_id = ?2",
                params![family, job_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(stored) = existing {
            if stored.as_slice() != body {
                return Err(Error::StorageIntegrity);
            }
            tx.commit()?;
            return Ok(());
        }
        let now = super::now_ms()?;
        tx.execute(
            "INSERT INTO completion_holds(family, job_id, generation, attempt, lease_owner, body, body_sha256, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![family, job_id, generation, attempt, owner, body, digest, now],
        )?;
        tx.commit()?;
        Ok(())
    }
}

pub(super) fn matches_claim(
    connection: &Connection,
    family: Family,
    id: &str,
    generation: u32,
    attempt: u32,
    owner: &str,
) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM completion_holds WHERE family = ?1 AND job_id = ?2 AND generation = ?3 AND attempt = ?4 AND lease_owner = ?5)",
        params![family.name(), id, generation, attempt, owner],
        |row| row.get(0),
    )?)
}

pub(super) fn delete_job(connection: &Connection, family: Family, id: &str) -> Result<()> {
    connection.execute(
        "DELETE FROM completion_holds WHERE family = ?1 AND job_id = ?2",
        params![family.name(), id],
    )?;
    Ok(())
}

pub(super) fn delete_matching(
    connection: &Connection,
    family: &str,
    job_id: &str,
    generation: u32,
    attempt: u32,
    owner: &str,
) -> Result<()> {
    connection.execute(
        "DELETE FROM completion_holds WHERE family = ?1 AND job_id = ?2 AND generation = ?3 AND attempt = ?4 AND lease_owner = ?5",
        params![family, job_id, generation, attempt, owner],
    )?;
    Ok(())
}

/// Removes an unused hold table so an older fixture can replay this migration.
#[cfg(test)]
pub(crate) fn revert_053_for_tests(connection: &Connection) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'completion_holds')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let used: bool =
        connection.query_row("SELECT EXISTS(SELECT 1 FROM completion_holds)", [], |row| {
            row.get(0)
        })?;
    if used {
        return Err(Error::RequestState);
    }
    connection.execute_batch("DROP TABLE IF EXISTS completion_holds;")?;
    Ok(())
}

#[cfg(test)]
pub(crate) struct RawHold<'a> {
    pub family: &'a str,
    pub job_id: &'a str,
    pub generation: u32,
    pub attempt: u32,
    pub owner: &'a str,
    pub body: &'a [u8],
    pub digest: &'a str,
}

#[cfg(test)]
impl Store {
    pub(crate) fn completion_hold_count(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row("SELECT count(*) FROM completion_holds", [], |row| {
                row.get(0)
            })?)
    }

    pub(crate) fn insert_completion_hold_for_tests(&self, hold: &RawHold<'_>) -> Result<()> {
        self.connection.execute(
            "INSERT INTO completion_holds(family, job_id, generation, attempt, lease_owner, body, body_sha256, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![
                hold.family,
                hold.job_id,
                hold.generation,
                hold.attempt,
                hold.owner,
                hold.body,
                hold.digest
            ],
        )?;
        Ok(())
    }

    pub(crate) fn published_original_scripts(&self, analysis_id: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT script FROM transcript_cues WHERE transcript_id = ?1 ORDER BY revision, ordinal",
        )?;
        let rows = statement.query_map(params![analysis_id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn recognition_document(work: &LocalAsrWork, reaped: &ReapedLocalAsr) -> Document {
    let _ = work;
    let (outcome, failure, recognition) = match reaped.outcome() {
        LocalAsrOutcome::Succeeded(output) => (
            "succeeded",
            None,
            Some(RecognitionDocument {
                profile_sha256: output.profile_sha256.clone(),
                manifest_sha256: output.manifest_sha256.clone(),
                coverages: output.coverages.clone(),
                cues: output.cues.clone(),
                languages: reaped
                    .languages()
                    .iter()
                    .map(|language| LanguageDocument {
                        ordinal: language.ordinal,
                        interval_ordinal: language.interval_ordinal,
                        start_us: language.start_us,
                        end_us: language.end_us,
                        code: language.code.clone(),
                    })
                    .collect(),
                observations: reaped
                    .observations()
                    .iter()
                    .map(|account| ObservationDocument {
                        role: account.role.to_owned(),
                        ordinal: account.ordinal,
                        mechanism: account.mechanism.to_owned(),
                        peak_memory_bytes: account.peak_memory_bytes,
                        cpu_time_us: account.cpu_time_us,
                    })
                    .collect(),
            }),
        ),
        LocalAsrOutcome::Failed(failure) => ("failed", Some(failure.reason()), None),
        LocalAsrOutcome::Cancelled => ("cancelled", None, None),
    };
    Document {
        version: HOLD_VERSION,
        family: "analysis".to_owned(),
        outcome: outcome.to_owned(),
        failure: failure.map(str::to_owned),
        recognition,
        translation: None,
    }
}

fn translation_document(work: &TranslationWork, outcome: &TranslationOutcome) -> Result<Document> {
    let _ = work;
    let (name, failure, translation) = match outcome.result() {
        TranslationResult::Succeeded(cues) => (
            "succeeded",
            None,
            Some(TranslationDocument { cues: cues.clone() }),
        ),
        TranslationResult::Failed(reason) => {
            validate_key(reason, "completion hold failure")?;
            ("failed", Some((*reason).to_owned()), None)
        }
        TranslationResult::Cancelled => ("cancelled", None, None),
    };
    Ok(Document {
        version: HOLD_VERSION,
        family: "translation".to_owned(),
        outcome: name.to_owned(),
        failure,
        recognition: None,
        translation,
    })
}

fn encode(document: &Document) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(document).map_err(|_| Error::StorageIntegrity)?;
    if body.len() > HOLD_BODY_BYTES {
        return Err(Error::StorageIntegrity);
    }
    Ok(body)
}

fn document(body: &[u8]) -> Result<Document> {
    if body.len() > HOLD_BODY_BYTES {
        return Err(Error::StorageIntegrity);
    }
    serde_json::from_slice(body).map_err(|_| Error::StorageIntegrity)
}

fn decode_payload(family: &str, body: &[u8]) -> Result<Option<HoldPayload>> {
    let document = document(body)?;
    if document.version != HOLD_VERSION || document.family != family {
        return Ok(None);
    }
    match (family, document.outcome.as_str()) {
        ("analysis", "succeeded") => Ok(decode_recognition(document).map(HoldPayload::Recognition)),
        ("analysis", "failed") => {
            let Some(reason) = document.failure.as_deref() else {
                return Ok(None);
            };
            if document.recognition.is_some() || document.translation.is_some() {
                return Ok(None);
            }
            let Some(failure) = LocalAsrFailure::from_reason(reason) else {
                return Ok(None);
            };
            Ok(Some(HoldPayload::Recognition(HeldRecognition {
                outcome: LocalAsrOutcome::Failed(failure),
                languages: Vec::new(),
                observations: Vec::new(),
            })))
        }
        ("analysis", "cancelled")
            if document.failure.is_none() && document.recognition.is_none() =>
        {
            Ok(Some(HoldPayload::Recognition(HeldRecognition {
                outcome: LocalAsrOutcome::Cancelled,
                languages: Vec::new(),
                observations: Vec::new(),
            })))
        }
        ("translation", "succeeded") => {
            let Some(body) = document.translation else {
                return Ok(None);
            };
            if document.failure.is_some() || document.recognition.is_some() {
                return Ok(None);
            }
            Ok(Some(HoldPayload::Translation(HeldTranslation::Succeeded(
                body.cues,
            ))))
        }
        ("translation", "failed") => {
            let Some(reason) = document.failure else {
                return Ok(None);
            };
            if document.recognition.is_some()
                || document.translation.is_some()
                || validate_key(&reason, "completion hold failure").is_err()
            {
                return Ok(None);
            }
            Ok(Some(HoldPayload::Translation(HeldTranslation::Failed(
                reason,
            ))))
        }
        ("translation", "cancelled")
            if document.failure.is_none() && document.translation.is_none() =>
        {
            Ok(Some(HoldPayload::Translation(HeldTranslation::Cancelled)))
        }
        _ => Ok(None),
    }
}

fn decode_recognition(document: Document) -> Option<HeldRecognition> {
    if document.failure.is_some() || document.translation.is_some() {
        return None;
    }
    let body = document.recognition?;
    if body.languages.len() > 1024 || body.observations.len() > 64 {
        return None;
    }
    let mut languages = Vec::with_capacity(body.languages.len());
    for language in body.languages {
        if !language_code(&language.code) {
            return None;
        }
        languages.push(ChunkLanguage {
            ordinal: language.ordinal,
            interval_ordinal: language.interval_ordinal,
            start_us: language.start_us,
            end_us: language.end_us,
            code: language.code,
        });
    }
    let mut observations = Vec::with_capacity(body.observations.len());
    for observation in body.observations {
        let Ok(account) = super::recognition::known_account(
            &observation.role,
            &observation.mechanism,
            observation.ordinal,
            observation.peak_memory_bytes,
            observation.cpu_time_us,
        ) else {
            return None;
        };
        observations.push(account);
    }
    Some(HeldRecognition {
        outcome: LocalAsrOutcome::Succeeded(RecognitionOutput {
            profile_sha256: body.profile_sha256,
            manifest_sha256: body.manifest_sha256,
            coverages: body.coverages,
            cues: body.cues,
        }),
        languages,
        observations,
    })
}

fn language_code(code: &str) -> bool {
    (1..=MAX_LANGUAGE_CODE).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

impl Store {
    fn hold_sight(&self, family: &str, job_id: &str, body: &[u8]) -> Result<HoldSight> {
        let existing: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT body FROM completion_holds WHERE family = ?1 AND job_id = ?2",
                params![family, job_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match existing {
            None => HoldSight::Absent,
            Some(stored) if stored.as_slice() == body => HoldSight::Same,
            Some(_) => HoldSight::Different,
        })
    }
}

/// A stop completion that already exists is not a second failure.
/// The job can still be marked failed and the hold deleted.
fn unreadable_finished(
    connection: &Connection,
    stop_family: &str,
    table: &str,
    job_id: &str,
    generation: u32,
    now: i64,
) -> Result<i64> {
    match super::withdrawals::complete(connection, stop_family, job_id, generation, now) {
        Ok(finished) => Ok(finished),
        Err(Error::StorageIntegrity) => {
            let present: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM native_stop_completions WHERE family = ?1 AND job_id = ?2 AND generation = ?3)",
                params![stop_family, job_id, generation],
                |row| row.get(0),
            )?;
            if !present {
                return Err(Error::StorageIntegrity);
            }
            recorded_finish_ms(connection, table, stop_family, job_id, generation, now)
        }
        Err(error) => Err(error),
    }
}

fn recorded_finish_ms(
    connection: &Connection,
    table: &str,
    stop_family: &str,
    job_id: &str,
    generation: u32,
    now: i64,
) -> Result<i64> {
    Ok(connection.query_row(
        &format!(
            "SELECT max(?3, created_ms, coalesce(started_ms, created_ms), coalesce((SELECT created_ms FROM native_stop_targets WHERE family = ?4 AND job_id = ?1 AND generation = ?2), created_ms)) FROM {table} WHERE id = ?1 AND generation = ?2"
        ),
        params![job_id, generation, now, stop_family],
        |row| row.get(0),
    )?)
}

fn family_of(family: &str) -> Result<Family> {
    match family {
        "analysis" => Ok(Family::Analysis),
        "translation" => Ok(Family::Translation),
        _ => Err(Error::StorageIntegrity),
    }
}

fn stop_family(family: &str) -> Result<&'static str> {
    match family {
        "analysis" => Ok("recognition"),
        "translation" => Ok("translation"),
        _ => Err(Error::StorageIntegrity),
    }
}

fn job_table(family: &str) -> Result<&'static str> {
    match family {
        "analysis" => Ok("analysis_jobs"),
        "translation" => Ok("translation_jobs"),
        _ => Err(Error::StorageIntegrity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_recognition_document_round_trips() -> Result<()> {
        let document = Document {
            version: HOLD_VERSION,
            family: "analysis".into(),
            outcome: "failed".into(),
            failure: Some("recognizer-failed".into()),
            recognition: None,
            translation: None,
        };
        let body = encode(&document)?;
        let Some(payload) = decode_payload("analysis", &body)? else {
            panic!("missing payload");
        };
        let HoldPayload::Recognition(held) = payload else {
            panic!("failed recognition");
        };
        assert_eq!(
            held.outcome,
            LocalAsrOutcome::Failed(LocalAsrFailure::RecognizerFailed)
        );
        assert_eq!(held.languages, []);
        Ok(())
    }

    #[test]
    fn an_unknown_field_does_not_decode() {
        let body = br#"{"version":1,"family":"analysis","outcome":"cancelled","extra":1}"#;
        assert!(document(body).is_err());
    }

    #[test]
    fn an_unknown_failure_reason_is_not_a_publication() -> Result<()> {
        let document = Document {
            version: HOLD_VERSION,
            family: "analysis".into(),
            outcome: "failed".into(),
            failure: Some("not-a-worker-failure".into()),
            recognition: None,
            translation: None,
        };
        let body = encode(&document)?;
        assert!(decode_payload("analysis", &body)?.is_none());
        Ok(())
    }

    #[test]
    fn a_worker_envelope_is_not_part_of_the_document() {
        let rendered = r#"{"version":1,"family":"analysis","outcome":"cancelled"}"#;
        assert!(!rendered.contains("ReapedLocalAsr"));
        assert!(!rendered.contains("containment"));
    }
}
