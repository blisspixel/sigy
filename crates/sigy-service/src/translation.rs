//! Local English translation of one exact transcript revision.
//! Each source cue maps to at most one English cue with the same ordinal. No word
//! alignment is invented. A translation is model output, not support for its claims.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, recognition::is_sha256, storage::validate_key};

pub const LLAMA_CPP_COMPLETION: &str = "llama-cpp-completion-v1";
pub const HY_MT2_PLAIN: &str = "hy-mt2-plain-v1";
pub const MAX_TRANSLATION_MODEL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const TRANSLATION_PAGE_ITEMS: usize = 16;

/// One immutable local translator configuration. Paths locate files; hashes identify them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationProfile {
    pub id: String,
    pub engine: String,
    pub template: String,
    pub runtime_dir: String,
    pub executable: String,
    pub runtime_sha256: String,
    pub runtime_files: u32,
    pub runtime_bytes: u64,
    pub model_path: String,
    pub model_sha256: String,
    pub model_bytes: u64,
    /// Declared source languages as a sorted, comma-separated list of primary subtags.
    /// A declaration, not a measured capability.
    pub languages: String,
    pub threads: u32,
    pub memory_bytes: u64,
    pub cue_deadline_ms: u64,
    pub profile_sha256: String,
}

impl TranslationProfile {
    /// The identity recorded on every translation produced with this profile.
    /// # Errors
    /// Fails only if serialization fails.
    pub fn identity(&self) -> Result<String> {
        Ok(crate::recognition::sha256_hex(&serde_json::to_vec(&(
            "sigy-translation-profile-v1",
            &self.engine,
            &self.template,
            &self.executable,
            &self.runtime_sha256,
            self.runtime_files,
            self.runtime_bytes,
            &self.model_sha256,
            self.model_bytes,
            &self.languages,
            self.threads,
            self.memory_bytes,
            self.cue_deadline_ms,
        ))?))
    }

    /// Whether the profile declares this primary language subtag.
    #[must_use]
    pub fn declares(&self, language: &str) -> bool {
        self.languages.split(',').any(|code| code == language)
    }

    /// Validate bounds and the derived identity. Files are checked by the worker.
    /// # Errors
    /// Refuses invalid names, paths, hashes, languages, limits or a mismatched identity.
    pub fn validate(&self) -> Result<()> {
        validate_key(&self.id, "translation profile")?;
        let invalid = Error::InvalidInput("translation profile");
        let path = |value: &str| {
            !value.is_empty()
                && value.len() <= crate::recognition::MAX_PROFILE_PATH_BYTES
                && !value.chars().any(char::is_control)
                && std::path::Path::new(value).is_absolute()
        };
        let executable = !self.executable.is_empty()
            && self.executable.len() <= 128
            && !matches!(self.executable.as_str(), "." | "..")
            && !self
                .executable
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
        if self.engine != LLAMA_CPP_COMPLETION
            || self.template != HY_MT2_PLAIN
            || !path(&self.runtime_dir)
            || !path(&self.model_path)
            || !executable
            || !is_sha256(&self.runtime_sha256)
            || !is_sha256(&self.model_sha256)
            || !(1..=256).contains(&self.runtime_files)
            || !(1..=1024 * 1024 * 1024).contains(&self.runtime_bytes)
            || !(1..=MAX_TRANSLATION_MODEL_BYTES).contains(&self.model_bytes)
            || !languages_valid(&self.languages)
            || !(1..=crate::recognition::MAX_WORKER_THREADS).contains(&self.threads)
            || !(crate::recognition::MIN_WORKER_MEMORY..=crate::recognition::MAX_WORKER_MEMORY)
                .contains(&self.memory_bytes)
            || !(1_000..=600_000).contains(&self.cue_deadline_ms)
        {
            return Err(invalid);
        }
        if self.identity()? != self.profile_sha256 {
            return Err(invalid);
        }
        Ok(())
    }
}

/// Canonical form: sorted, unique, lowercase two or three letter subtags, comma separated.
#[must_use]
pub fn languages_valid(value: &str) -> bool {
    let codes: Vec<&str> = value.split(',').collect();
    !codes.is_empty()
        && codes.len() <= 64
        && codes.windows(2).all(|pair| pair[0] < pair[1])
        && codes.iter().all(|code| {
            (2..=3).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_lowercase())
        })
}

/// Normalize a user-supplied language list into the canonical form.
/// # Errors
/// Refuses an empty list or an invalid code.
pub fn canonical_languages(value: &str) -> Result<String> {
    let mut codes: Vec<String> = value
        .split(',')
        .map(|code| code.trim().to_ascii_lowercase())
        .filter(|code| !code.is_empty())
        .collect();
    codes.sort();
    codes.dedup();
    let joined = codes.join(",");
    if !languages_valid(&joined) {
        return Err(Error::InvalidInput("translation languages"));
    }
    Ok(joined)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationRequest {
    pub id: String,
    pub transcript_id: String,
    pub transcript_revision: i64,
    pub profile: String,
    pub profile_sha256: String,
}

impl TranslationRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_key(&self.id, "translation job ID")?;
        validate_key(&self.transcript_id, "transcript ID")?;
        validate_key(&self.profile, "translation profile")?;
        if !(1..=64).contains(&self.transcript_revision) || !is_sha256(&self.profile_sha256) {
            return Err(Error::InvalidInput("translation request"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationJob {
    pub request: TranslationRequest,
    pub generation: u32,
    pub state: String,
    pub reason: Option<String>,
    pub amount_usd: String,
    pub created_ms: i64,
    pub finished_ms: Option<i64>,
}

/// One source cue given to the worker. The worker receives text only, never media or a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCue {
    pub ordinal: u32,
    pub script: String,
}

/// Returned only by a fresh admission. Replays cannot obtain a work token.
#[derive(Debug)]
pub struct TranslationWork {
    pub(crate) job: TranslationJob,
    pub(crate) cues: Vec<SourceCue>,
    /// The recognizer's block language label for the source transcript, when stored.
    pub(crate) language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslatedCue {
    pub ordinal: u32,
    /// `translated` or `untranslated`.
    pub state: String,
    pub english: Option<String>,
    /// Why the cue has no English text.
    pub reason: Option<String>,
}

/// A worker result, constructible only from an executor envelope with containment evidence.
#[derive(Debug)]
pub(crate) struct TranslationOutcome {
    job_id: String,
    generation: u32,
    result: TranslationResult,
}

impl TranslationOutcome {
    /// Accept an executor's envelope for exactly this job generation and spec. Only the
    /// execution module can construct an envelope, and only with containment evidence.
    /// # Errors
    /// Refuses an envelope for another task, generation or spec, or another task kind.
    pub(crate) fn from_envelope(
        job: &TranslationJob,
        envelope: crate::execution::ResultEnvelope,
        spec_sha256: Option<&str>,
    ) -> Result<Self> {
        let result = envelope.accept(&job.request.id, job.generation, spec_sha256)?;
        let crate::execution::TaskResult::Translation(result) = result else {
            return Err(Error::StorageIntegrity);
        };
        Ok(Self {
            job_id: job.request.id.clone(),
            generation: job.generation,
            result,
        })
    }

    #[cfg(test)]
    pub(crate) fn synthetic_fixture(job: &TranslationJob, result: TranslationResult) -> Self {
        Self {
            job_id: job.request.id.clone(),
            generation: job.generation,
            result,
        }
    }

    pub(crate) fn matches(&self, job: &TranslationJob) -> bool {
        self.job_id == job.request.id && self.generation == job.generation
    }

    pub(crate) fn result(&self) -> &TranslationResult {
        &self.result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranslationResult {
    Succeeded(Vec<TranslatedCue>),
    Failed(&'static str),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationPairView {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    /// The original script, unchanged.
    pub original: String,
    pub state: String,
    pub english: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationPage {
    pub transcript_id: String,
    pub transcript_revision: i64,
    pub revision: i64,
    pub job_id: String,
    pub profile: String,
    pub profile_sha256: String,
    pub cue_count: u32,
    pub translated_count: u32,
    pub amount_usd: String,
    pub created_ms: i64,
    pub pairs: Vec<TranslationPairView>,
    pub next_after_ordinal: Option<u32>,
}
