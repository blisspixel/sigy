//! Native local translation supervisor.
//!
//! One job translates the cues of one transcript revision into English, one contained
//! process per cue, so each English cue maps to exactly one source cue. The job runs as a
//! content-addressed task spec through the local process executor. The spec carries the
//! stored cue text inline with its hash; it names no media, source URL, catalog handle,
//! library path or secret.

use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use tokio::sync::watch;

use crate::{
    Error, Result,
    execution::{
        self, AssetRef, AssetRole, Executor, InlineText, LocalProcessExecutor, LocalStage,
        TaskInput, TaskKind, TaskLimits, TaskParams, TaskSpec, TranslationParams, hash_file,
        runtime_manifest,
    },
    translation::{
        HY_MT2_PLAIN, LLAMA_CPP_COMPLETION, MAX_TRANSLATION_MODEL_BYTES, SourceCue, TranslationJob,
        TranslationOutcome, TranslationProfile, canonical_languages,
    },
};

const MAX_STDOUT_BYTES: u64 = 64 * 1024;

/// Hash a local llama.cpp runtime directory and model into a translation profile.
/// Reads only the named local files. Runs on the calling thread.
/// # Errors
/// Refuses missing files, links, oversized inputs, invalid languages or limits.
#[allow(clippy::too_many_arguments)]
pub fn describe_translation_profile(
    id: &str,
    runtime_dir: &Path,
    executable: &str,
    model: &Path,
    languages: &str,
    threads: u32,
    memory_bytes: u64,
    cue_deadline_ms: u64,
) -> Result<TranslationProfile> {
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or(Error::InvalidInput("profile path is not valid Unicode"))
    };
    let never = AtomicBool::new(false);
    let runtime = runtime_manifest(runtime_dir, executable, &never)?;
    let (model_sha256, model_bytes) = hash_file(model, MAX_TRANSLATION_MODEL_BYTES, &never)?;
    let mut profile = TranslationProfile {
        id: id.to_owned(),
        engine: LLAMA_CPP_COMPLETION.to_owned(),
        template: HY_MT2_PLAIN.to_owned(),
        runtime_dir: text(runtime_dir)?,
        executable: executable.to_owned(),
        runtime_sha256: runtime.sha256,
        runtime_files: runtime.files,
        runtime_bytes: runtime.bytes,
        model_path: text(model)?,
        model_sha256,
        model_bytes,
        languages: canonical_languages(languages)?,
        threads,
        memory_bytes,
        cue_deadline_ms,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    profile.validate()?;
    Ok(profile)
}

/// Everything one translation job may read.
#[derive(Debug)]
pub(crate) struct TranslationTask {
    pub directory: PathBuf,
    pub profile: TranslationProfile,
    pub job: TranslationJob,
    pub cues: Vec<SourceCue>,
    pub language: Option<String>,
}

/// The content-addressed description of one translation job.
/// # Errors
/// Refuses values that cannot form a valid spec.
pub(crate) fn translation_spec(
    job: &TranslationJob,
    profile: &TranslationProfile,
    cues: &[SourceCue],
    language: Option<&str>,
) -> Result<TaskSpec> {
    TaskSpec {
        version: execution::SPEC_VERSION.to_owned(),
        task_id: job.request.id.clone(),
        generation: job.generation,
        kind: TaskKind::Translation,
        engine: profile.engine.clone(),
        template: profile.template.clone(),
        profile_sha256: profile.profile_sha256.clone(),
        inputs: cues
            .iter()
            .map(|cue| TaskInput::Text(InlineText::new(cue.ordinal, &cue.script)))
            .collect(),
        assets: vec![
            AssetRef {
                role: AssetRole::Runtime,
                sha256: profile.runtime_sha256.clone(),
                bytes: profile.runtime_bytes,
                files: profile.runtime_files,
                entry: Some(profile.executable.clone()),
            },
            AssetRef {
                role: AssetRole::Model,
                sha256: profile.model_sha256.clone(),
                bytes: profile.model_bytes,
                files: 1,
                entry: None,
            },
        ],
        params: TaskParams::Translation(TranslationParams {
            source_language: language.map(str::to_owned),
            declared_languages: profile.languages.clone(),
            target: "en".to_owned(),
            threads: profile.threads,
        }),
        limits: TaskLimits {
            processes: 1,
            memory_bytes: profile.memory_bytes,
            cpu_rate: profile.threads,
            wall_ms: profile.cue_deadline_ms,
            output_bytes: MAX_STDOUT_BYTES,
            decoder: None,
        },
        spec_sha256: String::new(),
    }
    .seal()
}

pub(crate) fn translation_stage(task: &TranslationTask) -> LocalStage {
    LocalStage::new(&task.directory)
        .asset(
            AssetRole::Runtime,
            &task.profile.runtime_sha256,
            PathBuf::from(&task.profile.runtime_dir),
        )
        .asset(
            AssetRole::Model,
            &task.profile.model_sha256,
            PathBuf::from(&task.profile.model_path),
        )
}

/// Run one translation job. An error means process cleanup could not be proven.
pub(crate) async fn run(
    task: TranslationTask,
    signal: watch::Receiver<bool>,
) -> Result<TranslationOutcome> {
    let job = &task.job;
    let refuse = |reason| {
        let envelope = execution::refuse_translation(&job.request.id, job.generation, reason);
        TranslationOutcome::from_envelope(job, envelope, None)
    };
    if task.profile.identity().ok().as_deref() != Some(task.profile.profile_sha256.as_str()) {
        return refuse("profile-unavailable");
    }
    let Ok(spec) = translation_spec(job, &task.profile, &task.cues, task.language.as_deref())
    else {
        return refuse("translator-failed");
    };
    let spec_sha256 = spec.spec_sha256.clone();
    let stage = translation_stage(&task);
    let envelope = LocalProcessExecutor.execute(spec, stage, signal).await?;
    TranslationOutcome::from_envelope(job, envelope, Some(&spec_sha256))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_are_canonical_and_declarations_are_exact() -> Result<()> {
        assert_eq!(canonical_languages(" ES, ar,zh ,ar")?, "ar,es,zh");
        assert!(canonical_languages("").is_err());
        assert!(canonical_languages("spanish").is_err());
        assert!(canonical_languages("e1").is_err());
        Ok(())
    }
}
