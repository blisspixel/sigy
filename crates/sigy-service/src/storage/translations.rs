//! Translation profiles, jobs and immutable English revisions of one transcript revision.

use rusqlite::{OptionalExtension, Row, TransactionBehavior, params};

use super::{
    Store,
    job_pool::{self, Family},
    validate_key,
};
use crate::{
    Error, Result,
    translation::{
        SourceCue, TRANSLATION_PAGE_ITEMS, TranslatedCue, TranslationJob, TranslationOutcome,
        TranslationPage, TranslationPairView, TranslationProfile, TranslationRequest,
        TranslationResult, TranslationWork,
    },
};

const PROFILE_COLUMNS: &str = "id, engine, template, runtime_dir, executable, runtime_sha256, runtime_files, runtime_bytes, model_path, model_sha256, model_bytes, languages, threads, memory_bytes, cue_deadline_ms, profile_sha256";
const MAX_ENGLISH_BYTES: usize = 4096;

fn sql(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::InvalidInput("translation integer range"))
}

fn unsigned(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn profile_row(row: &Row<'_>) -> rusqlite::Result<TranslationProfile> {
    Ok(TranslationProfile {
        id: row.get(0)?,
        engine: row.get(1)?,
        template: row.get(2)?,
        runtime_dir: row.get(3)?,
        executable: row.get(4)?,
        runtime_sha256: row.get(5)?,
        runtime_files: row.get(6)?,
        runtime_bytes: unsigned(row, 7)?,
        model_path: row.get(8)?,
        model_sha256: row.get(9)?,
        model_bytes: unsigned(row, 10)?,
        languages: row.get(11)?,
        threads: row.get(12)?,
        memory_bytes: unsigned(row, 13)?,
        cue_deadline_ms: unsigned(row, 14)?,
        profile_sha256: row.get(15)?,
    })
}

fn job_row(row: &Row<'_>) -> rusqlite::Result<TranslationJob> {
    Ok(TranslationJob {
        request: TranslationRequest {
            id: row.get(0)?,
            transcript_id: row.get(2)?,
            transcript_revision: row.get(3)?,
            profile: row.get(4)?,
            profile_sha256: row.get(5)?,
        },
        generation: row.get(1)?,
        state: row.get(6)?,
        reason: row.get(7)?,
        amount_usd: "0.000000".into(),
        created_ms: row.get(8)?,
        finished_ms: row.get(9)?,
        attempt: row.get(10)?,
        started_ms: row.get(11)?,
    })
}

const JOB_COLUMNS: &str = "id, generation, transcript_id, transcript_revision, profile, profile_sha256, state, reason, created_ms, finished_ms, attempt, started_ms";

impl Store {
    /// Store one immutable translation profile. An identical replay is unchanged.
    /// # Errors
    /// Refuses invalid bounds, a mismatched identity, or a changed profile under an existing ID.
    pub(crate) fn add_translation_profile(
        &mut self,
        profile: &TranslationProfile,
        now: i64,
    ) -> Result<bool> {
        profile.validate()?;
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        if let Some(existing) = self.find_translation_profile(&profile.id)? {
            if existing != *profile {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(false);
        }
        self.connection.execute(
            &format!("INSERT INTO translation_profiles({PROFILE_COLUMNS}, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)"),
            params![
                profile.id, profile.engine, profile.template, profile.runtime_dir,
                profile.executable, profile.runtime_sha256, profile.runtime_files,
                sql(profile.runtime_bytes)?, profile.model_path, profile.model_sha256,
                sql(profile.model_bytes)?, profile.languages, profile.threads,
                sql(profile.memory_bytes)?, sql(profile.cue_deadline_ms)?,
                profile.profile_sha256, now
            ],
        )?;
        Ok(true)
    }

    /// Read one translation profile.
    /// # Errors
    /// Refuses a malformed or missing ID and invalid stored rows.
    pub fn translation_profile(&self, id: &str) -> Result<TranslationProfile> {
        validate_key(id, "translation profile")?;
        self.find_translation_profile(id)?.ok_or(Error::NotFound)
    }

    /// Read every translation profile, at most 64, ordered by ID.
    /// # Errors
    /// Refuses invalid stored rows.
    pub fn translation_profiles(&self) -> Result<Vec<TranslationProfile>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM translation_profiles ORDER BY id LIMIT 65"
        ))?;
        let profiles = statement
            .query_map([], profile_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if profiles.len() > 64 {
            return Err(Error::StorageIntegrity);
        }
        for profile in &profiles {
            profile.validate().map_err(|_| Error::StorageIntegrity)?;
        }
        Ok(profiles)
    }

    fn find_translation_profile(&self, id: &str) -> Result<Option<TranslationProfile>> {
        let profile = self
            .connection
            .query_row(
                &format!("SELECT {PROFILE_COLUMNS} FROM translation_profiles WHERE id = ?1"),
                [id],
                profile_row,
            )
            .optional()?;
        if let Some(profile) = &profile {
            profile.validate().map_err(|_| Error::StorageIntegrity)?;
        }
        Ok(profile)
    }

    /// Read one translation job.
    /// # Errors
    /// Refuses a malformed or missing ID.
    pub fn translation_job(&self, id: &str) -> Result<TranslationJob> {
        validate_key(id, "translation job ID")?;
        self.find_translation_job(id)?.ok_or(Error::NotFound)
    }

    fn find_translation_job(&self, id: &str) -> Result<Option<TranslationJob>> {
        Ok(self
            .connection
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM translation_jobs WHERE id = ?1"),
                [id],
                job_row,
            )
            .optional()?)
    }

    /// Queue one translation of an exact recognized transcript revision.
    /// A replay returns history and never dispatches.
    /// # Errors
    /// Refuses a changed replay, a transcript without recognized text, or a full queue.
    pub(crate) fn enqueue_translation(
        &mut self,
        request: &TranslationRequest,
        now: i64,
    ) -> Result<(TranslationJob, bool)> {
        request.validate()?;
        if let Some(job) = self.find_translation_job(&request.id)? {
            if job.request != *request {
                return Err(Error::IdempotencyConflict);
            }
            return Ok((job, false));
        }
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !has_text(&tx, request)? {
            return Err(Error::Analysis("transcript-has-no-recognized-text"));
        }
        job_pool::check_open_bound(&tx, Family::Translation)?;
        tx.execute(
            "INSERT INTO translation_jobs(id, generation, transcript_id, transcript_revision, profile, profile_sha256, state, created_ms, lineage) VALUES (?1, 1, ?2, ?3, ?4, ?5, 'queued', ?6, ?2)",
            params![request.id, request.transcript_id, request.transcript_revision, request.profile, request.profile_sha256, now],
        )?;
        tx.commit()?;
        Ok((self.translation_job(&request.id)?, true))
    }

    /// Start one queued translation under this owner, reading its cues and the stored
    /// block language label. Only a committed claim returns a work token.
    /// # Errors
    /// Returns catalog failures.
    pub(crate) fn claim_translation(
        &mut self,
        id: &str,
        owner: &str,
        now: i64,
    ) -> Result<Option<TranslationWork>> {
        let job = self.translation_job(id)?;
        if job.state != "queued" {
            return Ok(None);
        }
        let request = &job.request;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !has_text(&tx, request)? {
            job_pool::end_queued(
                &tx,
                Family::Translation,
                id,
                "failed",
                "transcript-has-no-recognized-text",
                now,
            )?;
            tx.commit()?;
            return Ok(None);
        }
        let cues = source_cues(&tx, request)?;
        if cues.is_empty() || cues.len() > 256 {
            return Err(Error::StorageIntegrity);
        }
        let language: Option<String> = tx
            .query_row(
                "SELECT json_extract(payload_json, '$.spans[0].languages[0].tag') FROM language_evidence WHERE transcript_id = ?1 AND transcript_revision = ?2 AND json_extract(payload_json, '$.method.origin') = 'recognizer' AND json_extract(payload_json, '$.outcome') = 'succeeded' AND json_extract(payload_json, '$.spans[0].observation') = 'identified' ORDER BY created_ms, id LIMIT 1",
                params![request.transcript_id, request.transcript_revision],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let cue_deadline: i64 = tx.query_row(
            "SELECT cue_deadline_ms FROM translation_profiles WHERE id = ?1",
            [&request.profile],
            |row| row.get(0),
        )?;
        let count = i64::try_from(cues.len()).map_err(|_| Error::StorageIntegrity)?;
        let lease = now + cue_deadline * count + job_pool::LEASE_MARGIN_MS;
        if !job_pool::claim_row(&tx, Family::Translation, id, owner, lease, now)? {
            return Ok(None);
        }
        tx.commit()?;
        let language = language.map(|tag| {
            tag.split('-')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        });
        Ok(Some(TranslationWork {
            job: self.translation_job(id)?,
            cues,
            language,
        }))
    }

    /// Queue and immediately start one translation, as tests did before the pool.
    #[cfg(test)]
    pub(crate) fn admit_translation(
        &mut self,
        request: &TranslationRequest,
        now: i64,
    ) -> Result<(TranslationJob, Option<TranslationWork>)> {
        let (job, created) = self.enqueue_translation(request, now)?;
        if !created {
            return Ok((job, None));
        }
        match self.claim_translation(&request.id, super::analysis_jobs::TEST_OWNER, now)? {
            Some(work) => Ok((work.job.clone(), Some(work))),
            None => Ok((self.translation_job(&request.id)?, None)),
        }
    }

    /// Request cancellation of the exact generation.
    /// # Errors
    /// Refuses a missing ID or a stale generation.
    pub(crate) fn cancel_translation(
        &mut self,
        id: &str,
        generation: u32,
    ) -> Result<TranslationJob> {
        let job = self.translation_job(id)?;
        if job.generation != generation {
            return Err(Error::Analysis("stale-worker"));
        }
        if job.state == "queued" {
            job_pool::end_queued(
                &self.connection,
                Family::Translation,
                id,
                "cancelled",
                "cancelled",
                super::now_ms()?,
            )?;
        } else if job.state == "running" {
            self.connection.execute(
                "UPDATE translation_jobs SET state = 'cancelling' WHERE id = ?1 AND generation = ?2 AND state = 'running'",
                params![id, generation],
            )?;
        }
        self.translation_job(id)
    }

    /// Finalize a drained worker's result in one transaction with a zero-USD decision.
    /// Cancellation wins over a queued result.
    /// # Errors
    /// Refuses a stale outcome or a failed transaction.
    pub(crate) fn finish_translation(
        &mut self,
        work: &TranslationWork,
        outcome: &TranslationOutcome,
        now: i64,
    ) -> Result<TranslationJob> {
        if !outcome.matches(&work.job) {
            return Err(Error::Analysis("stale-worker"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = tx
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM translation_jobs WHERE id = ?1"),
                [&work.job.request.id],
                job_row,
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if job.generation != work.job.generation || job.request != work.job.request {
            return Err(Error::Analysis("stale-worker"));
        }
        if !matches!(job.state.as_str(), "running" | "cancelling") {
            return Ok(job);
        }
        let finished = now.max(job.created_ms);
        let (state, reason) = match outcome.result() {
            _ if job.state == "cancelling" => ("cancelled", Some("cancelled")),
            TranslationResult::Cancelled => ("cancelled", Some("cancelled")),
            TranslationResult::Failed(reason) => ("failed", Some(*reason)),
            TranslationResult::Succeeded(cues) if !cues_valid(work, cues) => {
                ("failed", Some("invalid-worker-output"))
            }
            TranslationResult::Succeeded(cues) => {
                insert_translation(&tx, &job, cues, finished)?;
                ("succeeded", None)
            }
        };
        let changed = tx.execute(
            "UPDATE translation_jobs SET state = ?3, reason = ?4, finished_ms = ?5 WHERE id = ?1 AND generation = ?2 AND state IN ('running', 'cancelling')",
            params![job.request.id, job.generation, state, reason, finished],
        )?;
        if changed != 1 {
            return Err(Error::StorageIntegrity);
        }
        tx.commit()?;
        self.translation_job(&job.request.id)
    }

    /// End every translation lease a previous service process held: requeue running
    /// work under a new generation until its attempt limit, interrupt the rest.
    pub(crate) fn recover_translation_jobs(&mut self) -> Result<()> {
        let now = super::now_ms()?;
        job_pool::recover(&mut self.connection, Family::Translation, now)
    }

    /// The newest translation revision of a transcript revision, or zero.
    /// # Errors
    /// Refuses a malformed ID.
    pub fn latest_translation_revision(&self, id: &str, transcript_revision: i64) -> Result<i64> {
        validate_key(id, "transcript ID")?;
        Ok(self.connection.query_row(
            "SELECT coalesce(max(revision), 0) FROM translations WHERE transcript_id = ?1 AND transcript_revision = ?2",
            params![id, transcript_revision],
            |row| row.get(0),
        )?)
    }

    /// Read up to sixteen original and English cue pairs after an ordinal cursor.
    /// # Errors
    /// Refuses a missing translation or invalid stored rows.
    pub fn translation_page(
        &self,
        id: &str,
        transcript_revision: i64,
        revision: i64,
        after: Option<u32>,
    ) -> Result<TranslationPage> {
        validate_key(id, "transcript ID")?;
        let header = self
            .connection
            .query_row(
                "SELECT job_id, profile, profile_sha256, cue_count, translated_count, created_ms FROM translations WHERE transcript_id = ?1 AND transcript_revision = ?2 AND revision = ?3",
                params![id, transcript_revision, revision],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, u32>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let after = after.map_or(-1, i64::from);
        let mut statement = self.connection.prepare(
            "SELECT c.ordinal, s.start_us, s.end_us, s.script, c.state, c.english, c.reason FROM translation_cues c JOIN transcript_cues s ON s.transcript_id = c.transcript_id AND s.revision = c.transcript_revision AND s.ordinal = c.ordinal WHERE c.transcript_id = ?1 AND c.transcript_revision = ?2 AND c.revision = ?3 AND c.ordinal > ?4 ORDER BY c.ordinal LIMIT ?5",
        )?;
        let limit =
            i64::try_from(TRANSLATION_PAGE_ITEMS + 1).map_err(|_| Error::StorageIntegrity)?;
        let mut pairs = statement
            .query_map(
                params![id, transcript_revision, revision, after, limit],
                |row| {
                    Ok(TranslationPairView {
                        ordinal: row.get(0)?,
                        start_us: unsigned(row, 1)?,
                        end_us: unsigned(row, 2)?,
                        original: row.get(3)?,
                        state: row.get(4)?,
                        english: row.get(5)?,
                        reason: row.get(6)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_after_ordinal = if pairs.len() > TRANSLATION_PAGE_ITEMS {
            pairs.truncate(TRANSLATION_PAGE_ITEMS);
            pairs.last().map(|pair| pair.ordinal)
        } else {
            None
        };
        let (job_id, profile, profile_sha256, cue_count, translated_count, created_ms) = header;
        Ok(TranslationPage {
            transcript_id: id.to_owned(),
            transcript_revision,
            revision,
            job_id,
            profile,
            profile_sha256,
            cue_count,
            translated_count,
            amount_usd: "0.000000".into(),
            created_ms,
            pairs,
            next_after_ordinal,
        })
    }
}

fn has_text(connection: &rusqlite::Connection, request: &TranslationRequest) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM transcripts WHERE id = ?1 AND revision = ?2 AND kind = 'recognition' AND outcome = 'text')",
        params![request.transcript_id, request.transcript_revision],
        |row| row.get(0),
    )?)
}

fn source_cues(
    connection: &rusqlite::Connection,
    request: &TranslationRequest,
) -> Result<Vec<SourceCue>> {
    let mut statement = connection.prepare(
        "SELECT ordinal, script FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 ORDER BY ordinal LIMIT 257",
    )?;
    Ok(statement
        .query_map(
            params![request.transcript_id, request.transcript_revision],
            |row| {
                Ok(SourceCue {
                    ordinal: row.get(0)?,
                    script: row.get(1)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Exactly one result per source cue, in order, with bounded untrusted text.
fn cues_valid(work: &TranslationWork, cues: &[TranslatedCue]) -> bool {
    cues.len() == work.cues.len()
        && cues.iter().zip(&work.cues).all(|(cue, source)| {
            cue.ordinal == source.ordinal
                && match cue.state.as_str() {
                    "translated" => {
                        cue.reason.is_none()
                            && cue.english.as_ref().is_some_and(|text| {
                                !text.is_empty()
                                    && text.len() <= MAX_ENGLISH_BYTES
                                    && !text.contains('\0')
                            })
                    }
                    "untranslated" => {
                        cue.english.is_none()
                            && cue
                                .reason
                                .as_ref()
                                .is_some_and(|reason| validate_key(reason, "reason").is_ok())
                    }
                    _ => false,
                }
        })
}

fn insert_translation(
    connection: &rusqlite::Connection,
    job: &TranslationJob,
    cues: &[TranslatedCue],
    now: i64,
) -> Result<()> {
    let request = &job.request;
    let revision: i64 = connection.query_row(
        "SELECT coalesce(max(revision), 0) + 1 FROM translations WHERE transcript_id = ?1 AND transcript_revision = ?2",
        params![request.transcript_id, request.transcript_revision],
        |row| row.get(0),
    )?;
    let translated = cues.iter().filter(|cue| cue.state == "translated").count();
    connection.execute(
        "INSERT INTO translations(transcript_id, transcript_revision, revision, job_id, job_generation, profile, profile_sha256, target, cue_count, translated_count, amount_micros, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'en', ?8, ?9, 0, ?10)",
        params![
            request.transcript_id, request.transcript_revision, revision, request.id,
            job.generation, request.profile, request.profile_sha256,
            i64::try_from(cues.len()).map_err(|_| Error::StorageIntegrity)?,
            i64::try_from(translated).map_err(|_| Error::StorageIntegrity)?, now
        ],
    )?;
    for cue in cues {
        connection.execute(
            "INSERT INTO translation_cues(transcript_id, transcript_revision, revision, ordinal, state, english, reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![request.transcript_id, request.transcript_revision, revision, cue.ordinal, cue.state, cue.english, cue.reason],
        )?;
    }
    Ok(())
}
