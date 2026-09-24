//! The task contract between the job plane and an executor.
//!
//! A [`TaskSpec`] names every input and asset by content hash. It carries no source URL,
//! library path, object key, catalog handle or secret name. A [`LocalStage`] maps those
//! hashes to files that already exist on this host. An [`Executor`] runs one spec and
//! returns one [`ResultEnvelope`], whose [`Containment`] is evidence that the work
//! stopped before its result is offered for publication.
//!
//! Only this module and its children can construct [`Drained`], so no other part of the
//! service can produce an envelope, and therefore a completion, without it. This module
//! does not open the catalog; validation, currency checks and publication stay with the
//! service storage.

use std::future::Future;

use tokio::sync::watch;

use crate::{
    Error, Result,
    recognition::{LocalAsrFailure, RecognitionCoverage, RecognitionCue},
    translation::TranslationResult,
};

mod asr;
mod files;
mod local;
mod spec;
mod stage;
mod translate;

#[cfg(test)]
mod tests;

pub(crate) use files::{hash_file, runtime_manifest};
pub(crate) use local::{LocalProcessExecutor, clear_scratch};
pub(crate) use spec::{
    AssetRef, AssetRole, BlobRef, DECODER_TEMPLATE, DecoderLimits, InlineText, RecognitionParams,
    SPEC_VERSION, TaskInput, TaskKind, TaskLimits, TaskParams, TaskSpec, TranslationParams,
};
pub(crate) use stage::LocalStage;

/// Proof that every native process started for one task has left its group.
/// Only the execution module can construct it.
#[derive(Debug)]
pub(crate) struct Drained(());

/// How an executor shows that a task's work has stopped.
#[derive(Debug)]
pub(crate) enum Containment {
    /// The local executor saw its process group report no active member, or started no
    /// process at all. A remote executor would need fenced evidence instead.
    Drained(Drained),
}

/// The executor class and device that produced a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutorEvidence {
    pub executor: &'static str,
    pub device: &'static str,
}

/// The local process executor, on the CPU. GPU use is disabled by its engine templates.
pub(crate) const LOCAL_EVIDENCE: ExecutorEvidence = ExecutorEvidence {
    executor: "local-process-v1",
    device: "cpu",
};

/// Typed output of one recognition task, before the service validates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecognitionResult {
    Succeeded {
        coverage: RecognitionCoverage,
        cues: Vec<RecognitionCue>,
        /// The recognizer's own block language code, bounded and unmapped.
        language: Option<String>,
    },
    Failed(LocalAsrFailure),
    Cancelled,
}

/// Typed output of one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskResult {
    Recognition(RecognitionResult),
    Translation(TranslationResult),
}

/// One executor's answer for one task generation. Fields are private so a holder can
/// neither move its containment to another task nor rebind it to another generation.
#[derive(Debug)]
pub(crate) struct ResultEnvelope {
    task_id: String,
    generation: u32,
    /// Absent when the task was refused before a spec could be executed.
    spec_sha256: Option<String>,
    result: TaskResult,
    output_sha256: Option<String>,
    evidence: ExecutorEvidence,
    containment: Containment,
}

impl ResultEnvelope {
    fn new(spec: &TaskSpec, result: TaskResult, proof: Drained) -> Self {
        let output_sha256 = output_digest(&result);
        Self {
            task_id: spec.task_id.clone(),
            generation: spec.generation,
            spec_sha256: Some(spec.spec_sha256.clone()),
            result,
            output_sha256,
            evidence: LOCAL_EVIDENCE,
            containment: Containment::Drained(proof),
        }
    }

    fn refused(task_id: &str, generation: u32, result: TaskResult) -> Self {
        Self {
            task_id: task_id.to_owned(),
            generation,
            spec_sha256: None,
            result,
            output_sha256: None,
            evidence: LOCAL_EVIDENCE,
            containment: Containment::Drained(Drained(())),
        }
    }

    /// Accept this envelope as the answer to exactly one task generation and spec.
    /// Checks the binding, the output hash, the executor class and the containment.
    /// # Errors
    /// A different task or generation is stale; any other mismatch is an integrity fault.
    pub(crate) fn accept(
        self,
        task_id: &str,
        generation: u32,
        spec_sha256: Option<&str>,
    ) -> Result<TaskResult> {
        if self.task_id != task_id || self.generation != generation {
            return Err(Error::Analysis("stale-worker"));
        }
        let Containment::Drained(Drained(())) = self.containment;
        if self.spec_sha256.as_deref() != spec_sha256
            || self.output_sha256 != output_digest(&self.result)
            || self.evidence != LOCAL_EVIDENCE
        {
            return Err(Error::StorageIntegrity);
        }
        Ok(self.result)
    }
}

fn output_digest(result: &TaskResult) -> Option<String> {
    let bytes = match result {
        TaskResult::Recognition(RecognitionResult::Succeeded { coverage, cues, .. }) => {
            serde_json::to_vec(&("sigy-recognition-output-v1", coverage, cues)).ok()?
        }
        TaskResult::Translation(TranslationResult::Succeeded(cues)) => {
            serde_json::to_vec(&("sigy-translation-output-v1", cues)).ok()?
        }
        _ => return None,
    };
    Some(crate::recognition::sha256_hex(&bytes))
}

/// A recognition task that no process was started for. It can only fail.
pub(crate) fn refuse_recognition(
    task_id: &str,
    generation: u32,
    failure: LocalAsrFailure,
) -> ResultEnvelope {
    ResultEnvelope::refused(
        task_id,
        generation,
        TaskResult::Recognition(RecognitionResult::Failed(failure)),
    )
}

/// A translation task that no process was started for. It can only fail.
pub(crate) fn refuse_translation(
    task_id: &str,
    generation: u32,
    reason: &'static str,
) -> ResultEnvelope {
    ResultEnvelope::refused(
        task_id,
        generation,
        TaskResult::Translation(TranslationResult::Failed(reason)),
    )
}

/// Runs one task and returns its envelope. An error means containment could not be
/// shown; the caller must keep every lease the task holds and stop admitting work.
pub(crate) trait Executor {
    /// Where this executor finds the content the spec names by hash.
    type Stage;

    fn execute(
        &self,
        spec: TaskSpec,
        stage: Self::Stage,
        signal: watch::Receiver<bool>,
    ) -> impl Future<Output = Result<ResultEnvelope>> + Send;
}
