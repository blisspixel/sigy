//! The serialized task specification and its canonical hash.

use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    recognition::{is_sha256, sha256_hex},
    storage::validate_key,
};

pub(crate) const SPEC_VERSION: &str = "sigy-task-spec-v1";
/// The decoder stage of a recognition task: one audio stream to 16 kHz mono PCM16.
pub(crate) const DECODER_TEMPLATE: &str = "ffmpeg-s16le-mono-16k-v1";
const MAX_TOKEN_BYTES: usize = 256;
const MAX_INLINE_BYTES: u64 = 4096;
const MAX_INPUTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskKind {
    Recognition,
    Translation,
}

/// Content named by hash. The media type is a Sigy token such as `audio-wav`, not a
/// MIME string, so it carries no separator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlobRef {
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
}

/// Short untrusted text carried inline, with its hash. The text is content: it is
/// never interpreted as a location or an instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InlineText {
    pub ordinal: u32,
    pub sha256: String,
    pub bytes: u64,
    pub text: String,
}

impl InlineText {
    pub(crate) fn new(ordinal: u32, text: &str) -> Self {
        Self {
            ordinal,
            sha256: sha256_hex(text.as_bytes()),
            bytes: text.len() as u64,
            text: text.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum TaskInput {
    Blob(BlobRef),
    Text(InlineText),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssetRole {
    Runtime,
    Model,
    SpeechActivity,
}

/// A pinned executable runtime or model. A runtime's hash is its directory manifest
/// hash and `entry` names the executable inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetRef {
    pub role: AssetRole,
    pub sha256: String,
    pub bytes: u64,
    pub files: u32,
    pub entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecognitionParams {
    /// The pinned interval on the media clock.
    pub interval_ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub sample_rate: u32,
    pub decoder: String,
    pub threads: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranslationParams {
    /// The recognizer's block label for the source transcript, as a primary subtag.
    pub source_language: Option<String>,
    /// The profile's declared source languages, canonical and comma separated.
    pub declared_languages: String,
    pub target: String,
    pub threads: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case")]
pub(crate) enum TaskParams {
    Recognition(RecognitionParams),
    Translation(TranslationParams),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecoderLimits {
    pub memory_bytes: u64,
    pub wall_ms: u64,
}

/// Bounds for every native process group the task starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskLimits {
    pub processes: u32,
    pub memory_bytes: u64,
    /// CPU rate in whole processors.
    pub cpu_rate: u32,
    /// Wall deadline of one recognizer run, or of one translated cue.
    pub wall_ms: u64,
    pub output_bytes: u64,
    pub decoder: Option<DecoderLimits>,
}

/// Everything an executor needs to run one task generation, by hash and value only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskSpec {
    pub version: String,
    pub task_id: String,
    pub generation: u32,
    pub kind: TaskKind,
    pub engine: String,
    pub template: String,
    pub profile_sha256: String,
    pub inputs: Vec<TaskInput>,
    pub assets: Vec<AssetRef>,
    pub params: TaskParams,
    pub limits: TaskLimits,
    pub spec_sha256: String,
}

/// The hashed form: every field except the hash, in declaration order, as compact JSON.
#[derive(Serialize)]
struct Canonical<'a> {
    version: &'a str,
    task_id: &'a str,
    generation: u32,
    kind: TaskKind,
    engine: &'a str,
    template: &'a str,
    profile_sha256: &'a str,
    inputs: &'a [TaskInput],
    assets: &'a [AssetRef],
    params: &'a TaskParams,
    limits: &'a TaskLimits,
}

impl TaskSpec {
    /// Compute the canonical hash and validate the result.
    /// # Errors
    /// Refuses a spec that names a location, has malformed hashes or inconsistent parts.
    pub(crate) fn seal(mut self) -> Result<Self> {
        self.spec_sha256 = self.digest()?;
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn digest(&self) -> Result<String> {
        let canonical = Canonical {
            version: &self.version,
            task_id: &self.task_id,
            generation: self.generation,
            kind: self.kind,
            engine: &self.engine,
            template: &self.template,
            profile_sha256: &self.profile_sha256,
            inputs: &self.inputs,
            assets: &self.assets,
            params: &self.params,
            limits: &self.limits,
        };
        Ok(sha256_hex(&serde_json::to_vec(&canonical)?))
    }

    /// Structural checks an executor repeats before it runs anything.
    /// # Errors
    /// Refuses a mismatched hash, a location-like token or inconsistent parts.
    pub(crate) fn validate(&self) -> Result<()> {
        let invalid = || Error::InvalidInput("task spec");
        validate_key(&self.task_id, "task ID").map_err(|_| invalid())?;
        let kind_matches = matches!(
            (self.kind, &self.params),
            (TaskKind::Recognition, TaskParams::Recognition(_))
                | (TaskKind::Translation, TaskParams::Translation(_))
        );
        if self.version != SPEC_VERSION
            || !(1..=64).contains(&self.generation)
            || !kind_matches
            || !token(&self.engine)
            || !token(&self.template)
            || !is_sha256(&self.profile_sha256)
            || self.inputs.is_empty()
            || self.inputs.len() > MAX_INPUTS
            || !self.inputs.iter().all(input_valid)
            || !self.assets.iter().all(asset_valid)
            || !params_valid(&self.params)
            || self.digest()? != self.spec_sha256
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn asset(&self, role: AssetRole) -> Option<&AssetRef> {
        self.assets.iter().find(|asset| asset.role == role)
    }

    pub(crate) fn blob(&self) -> Option<&BlobRef> {
        self.inputs.iter().find_map(|input| match input {
            TaskInput::Blob(blob) => Some(blob),
            TaskInput::Text(_) => None,
        })
    }

    pub(crate) fn texts(&self) -> impl Iterator<Item = &InlineText> {
        self.inputs.iter().filter_map(|input| match input {
            TaskInput::Text(text) => Some(text),
            TaskInput::Blob(_) => None,
        })
    }
}

/// A name or template value: bounded, printable, and never a location.
pub(crate) fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && !value.contains("://")
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

fn input_valid(input: &TaskInput) -> bool {
    match input {
        TaskInput::Blob(blob) => {
            is_sha256(&blob.sha256) && blob.bytes > 0 && token(&blob.media_type)
        }
        TaskInput::Text(text) => {
            text.bytes == text.text.len() as u64
                && text.bytes <= MAX_INLINE_BYTES
                && text.sha256 == sha256_hex(text.text.as_bytes())
        }
    }
}

fn asset_valid(asset: &AssetRef) -> bool {
    is_sha256(&asset.sha256)
        && asset.bytes > 0
        && asset.files > 0
        && asset.entry.as_deref().is_none_or(token)
        && (asset.role == AssetRole::Runtime) == asset.entry.is_some()
}

fn params_valid(params: &TaskParams) -> bool {
    match params {
        TaskParams::Recognition(params) => {
            params.start_us < params.end_us && params.sample_rate > 0 && token(&params.decoder)
        }
        TaskParams::Translation(params) => {
            params.source_language.as_deref().is_none_or(token)
                && token(&params.declared_languages)
                && token(&params.target)
        }
    }
}
