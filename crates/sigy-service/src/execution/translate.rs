//! One translation task: one contained process per inline cue, so each English cue
//! maps to exactly one source cue. The task reads the spec's inline text only.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use processkit::ProcessGroupOptions;
use tokio::{io::AsyncReadExt, sync::watch};

use super::{
    AssetRole, Drained, ResultEnvelope, TaskParams, TaskResult, TaskSpec,
    files::{hash_file, runtime_manifest},
    local::{blocking, contained, drain, prepare_scratch, runtime_environment, stopped},
    spec::{InlineText, TranslationParams},
    stage::LocalStage,
};
use crate::{
    Result,
    translation::{HY_MT2_PLAIN, MAX_TRANSLATION_MODEL_BYTES, TranslatedCue, TranslationResult},
};

const MAX_ENGLISH_BYTES: usize = 4096;
const HY_MT2_PROMPT: &str = "Translate the following text into English. Note that you should only output the translated result without any additional explanation:\n\n";

struct Plan<'a> {
    spec: &'a TaskSpec,
    params: &'a TranslationParams,
    prompt: &'static str,
    runtime: PathBuf,
    executable: &'a str,
    model: PathBuf,
}

fn plan<'a>(spec: &'a TaskSpec, stage: &'a LocalStage) -> Option<Plan<'a>> {
    let TaskParams::Translation(params) = &spec.params else {
        return None;
    };
    let runtime = spec.asset(AssetRole::Runtime)?;
    let model = spec.asset(AssetRole::Model)?;
    Some(Plan {
        spec,
        params,
        prompt: (spec.template == HY_MT2_PLAIN).then_some(HY_MT2_PROMPT)?,
        runtime: stage.asset_path(runtime)?.to_path_buf(),
        executable: runtime.entry.as_deref()?,
        model: stage.asset_path(model)?.to_path_buf(),
    })
}

/// Run one translation task. An error means process cleanup could not be proven.
pub(super) async fn run(
    spec: TaskSpec,
    stage: LocalStage,
    mut signal: watch::Receiver<bool>,
) -> Result<ResultEnvelope> {
    let failed = TranslationResult::Failed("translator-failed");
    if spec.validate().is_err() {
        return Ok(ResultEnvelope::new(
            &spec,
            TaskResult::Translation(failed),
            Drained(()),
        ));
    }
    let Some(plan) = plan(&spec, &stage) else {
        return Ok(ResultEnvelope::new(
            &spec,
            TaskResult::Translation(failed),
            Drained(()),
        ));
    };
    let scratch = stage.scratch(&spec);
    let result = match prepare_scratch(&scratch) {
        Ok(()) => attempt(&plan, &scratch, &mut signal).await?,
        Err(_) => failed,
    };
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(ResultEnvelope::new(
        &spec,
        TaskResult::Translation(result),
        Drained(()),
    ))
}

fn untranslated(ordinal: u32, reason: &str) -> TranslatedCue {
    TranslatedCue {
        ordinal,
        state: "untranslated".into(),
        english: None,
        reason: Some(reason.into()),
    }
}

async fn attempt(
    plan: &Plan<'_>,
    scratch: &Path,
    signal: &mut watch::Receiver<bool>,
) -> Result<TranslationResult> {
    let stop = Arc::new(AtomicBool::new(false));
    let check = AssetCheck::new(plan);
    let assets = blocking(&stop, signal, move |stop| check.check(stop)).await?;
    if stop.load(Ordering::Acquire) || stopped(signal) {
        return Ok(TranslationResult::Cancelled);
    }
    if !matches!(assets, Ok(true)) {
        return Ok(TranslationResult::Failed("profile-unavailable"));
    }
    // The recognizer's block label decides the whole transcript. Unknown still translates.
    let declares = |code: &str| {
        plan.params
            .declared_languages
            .split(',')
            .any(|declared| declared == code)
    };
    let skip = match plan.params.source_language.as_deref() {
        Some("en") => Some("source-english"),
        Some(code) if !declares(code) => Some("unsupported-language"),
        _ => None,
    };
    let mut cues = Vec::new();
    for cue in plan.spec.texts() {
        if let Some(reason) = skip {
            cues.push(untranslated(cue.ordinal, reason));
            continue;
        }
        if stopped(signal) {
            return Ok(TranslationResult::Cancelled);
        }
        match translate_cue(plan, scratch, cue, signal).await? {
            Some(translated) => cues.push(translated),
            None => return Ok(TranslationResult::Cancelled),
        }
    }
    Ok(TranslationResult::Succeeded(cues))
}

struct AssetCheck {
    runtime: PathBuf,
    executable: String,
    expected_runtime: (String, u32, u64),
    model: PathBuf,
    expected_model: (String, u64),
}

impl AssetCheck {
    fn new(plan: &Plan<'_>) -> Self {
        let asset = |role| {
            plan.spec
                .asset(role)
                .map(|asset| (asset.sha256.clone(), asset.files, asset.bytes))
                .unwrap_or_default()
        };
        let runtime = asset(AssetRole::Runtime);
        let (model_sha256, _, model_bytes) = asset(AssetRole::Model);
        Self {
            runtime: plan.runtime.clone(),
            executable: plan.executable.to_owned(),
            expected_runtime: runtime,
            model: plan.model.clone(),
            expected_model: (model_sha256, model_bytes),
        }
    }

    fn check(self, stop: &AtomicBool) -> Result<bool> {
        let runtime = runtime_manifest(&self.runtime, &self.executable, stop)?;
        let model = hash_file(&self.model, MAX_TRANSLATION_MODEL_BYTES, stop)?;
        Ok(
            (runtime.sha256, runtime.files, runtime.bytes) == self.expected_runtime
                && model == self.expected_model,
        )
    }
}

/// Translate one cue in its own contained process. `None` means cancelled.
async fn translate_cue(
    plan: &Plan<'_>,
    scratch: &Path,
    cue: &InlineText,
    signal: &mut watch::Receiver<bool>,
) -> Result<Option<TranslatedCue>> {
    let limits = plan.spec.limits;
    let prompt = scratch.join(format!("cue-{}.txt", cue.ordinal));
    if std::fs::write(&prompt, format!("{}{}", plan.prompt, cue.text)).is_err() {
        return Ok(Some(untranslated(cue.ordinal, "translator-failed")));
    }
    let Some(group) = contained(
        ProcessGroupOptions::default()
            .max_processes(limits.processes)
            .max_memory(limits.memory_bytes)
            .cpu_quota(f64::from(limits.cpu_rate)),
    ) else {
        return Ok(Some(untranslated(cue.ordinal, "limits-unavailable")));
    };
    let mut command = tokio::process::Command::new(plan.runtime.join(plan.executable));
    command
        .env_clear()
        .current_dir(scratch)
        .arg("-m")
        .arg(&plan.model)
        .args([
            "--offline",
            "--jinja",
            "-st",
            "--no-display-prompt",
            "--no-warmup",
        ])
        .arg("-f")
        .arg(&prompt)
        .args(["-n", "512", "-c", "4096", "--temp", "0", "-s", "0"])
        .args(["-t", &plan.params.threads.to_string()])
        // A CPU profile stays on the CPU even when its runtime folder carries a GPU backend,
        // which llama.cpp would otherwise use without saying so.
        .args(["-dev", "none", "-ngl", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    runtime_environment(&mut command, &plan.runtime);
    let Ok(mut child) = group.spawn(command) else {
        return Ok(Some(untranslated(cue.ordinal, "translator-failed")));
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = group.kill_all();
        let _ = child.wait().await;
        drain(&group).await?;
        return Ok(Some(untranslated(cue.ordinal, "translator-failed")));
    };
    let read = async {
        let mut bytes = Vec::new();
        stdout
            .take(limits.output_bytes + 1)
            .read_to_end(&mut bytes)
            .await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((bytes, status))
    };
    let deadline = Duration::from_millis(limits.wall_ms);
    let result = tokio::select! {
        result = tokio::time::timeout(deadline, read) => Some(result),
        _ = signal.changed() => None,
    };
    let _ = group.kill_all();
    drain(&group).await?;
    let _ = std::fs::remove_file(&prompt);
    Ok(match result {
        None => None,
        Some(Err(_)) => Some(untranslated(cue.ordinal, "deadline")),
        Some(Ok(Err(_))) => Some(untranslated(cue.ordinal, "translator-failed")),
        // An overrun is reported even when closing the pipe made the translator fail.
        Some(Ok(Ok((bytes, status)))) => Some(if bytes.len() as u64 > limits.output_bytes {
            untranslated(cue.ordinal, "output-limit")
        } else if !status.success() {
            untranslated(cue.ordinal, "translator-failed")
        } else {
            match completion_text(&bytes) {
                Ok(english) => TranslatedCue {
                    ordinal: cue.ordinal,
                    state: "translated".into(),
                    english: Some(english),
                    reason: None,
                },
                Err(reason) => untranslated(cue.ordinal, reason),
            }
        }),
    })
}

/// Extract the completion from llama.cpp standard output: terminal escape sequences and the
/// end-of-text marker are formatting, surrounding whitespace is trimmed, and nothing else
/// is changed. Empty, oversized, NUL-bearing or non-UTF-8 output is refused.
pub(crate) fn completion_text(bytes: &[u8]) -> std::result::Result<String, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid-output")?;
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        clean.push(c);
    }
    let trimmed = clean.trim();
    let trimmed = trimmed
        .strip_suffix("[end of text]")
        .unwrap_or(trimmed)
        .trim();
    if trimmed.is_empty() {
        return Err("empty-output");
    }
    if trimmed.contains('\0') {
        return Err("invalid-output");
    }
    if trimmed.len() > MAX_ENGLISH_BYTES {
        return Err("output-limit");
    }
    Ok(trimmed.to_owned())
}
