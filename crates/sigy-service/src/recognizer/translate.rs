//! Native local translation supervisor.
//!
//! One job translates the cues of one transcript revision into English, one contained
//! process per cue, so each English cue maps to exactly one source cue. The worker reads
//! stored text only. It receives no media, source URL, catalog handle or secret.

use std::{
    path::Path,
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
    Drained, SCRATCH, blocking, contained, drain, hash_file, prepare_scratch, runtime_environment,
    runtime_manifest, stopped,
};
use crate::{
    Error, Result,
    translation::{
        HY_MT2_PLAIN, LLAMA_CPP_COMPLETION, MAX_TRANSLATION_MODEL_BYTES, SourceCue, TranslatedCue,
        TranslationJob, TranslationOutcome, TranslationProfile, TranslationResult,
        canonical_languages,
    },
};

const MAX_STDOUT_BYTES: u64 = 64 * 1024;
const MAX_ENGLISH_BYTES: usize = 4096;
const HY_MT2_PROMPT: &str = "Translate the following text into English. Note that you should only output the translated result without any additional explanation:\n\n";

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
    pub directory: std::path::PathBuf,
    pub profile: TranslationProfile,
    pub job: TranslationJob,
    pub cues: Vec<SourceCue>,
    pub language: Option<String>,
}

/// Run one translation job. An error means process cleanup could not be proven.
pub(crate) async fn run(
    task: TranslationTask,
    mut signal: watch::Receiver<bool>,
) -> Result<TranslationOutcome> {
    let scratch = task.directory.join(SCRATCH).join(format!(
        "translate-{}-g{}",
        task.job.request.id, task.job.generation
    ));
    let result = match prepare_scratch(&scratch) {
        Ok(()) => attempt(&task, &scratch, &mut signal).await?,
        Err(_) => TranslationResult::Failed("translator-failed"),
    };
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(TranslationOutcome::drained(&task.job, result, Drained(())))
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
    task: &TranslationTask,
    scratch: &Path,
    signal: &mut watch::Receiver<bool>,
) -> Result<TranslationResult> {
    let stop = Arc::new(AtomicBool::new(false));
    let profile = task.profile.clone();
    let assets = blocking(&stop, signal, move |stop| verify_assets(&profile, stop)).await?;
    if stop.load(Ordering::Acquire) || stopped(signal) {
        return Ok(TranslationResult::Cancelled);
    }
    if !matches!(assets, Ok(true)) {
        return Ok(TranslationResult::Failed("profile-unavailable"));
    }
    // The recognizer's block label decides the whole transcript. Unknown still translates.
    let skip = match task.language.as_deref() {
        Some("en") => Some("source-english"),
        Some(code) if !task.profile.declares(code) => Some("unsupported-language"),
        _ => None,
    };
    let mut cues = Vec::with_capacity(task.cues.len());
    for cue in &task.cues {
        if let Some(reason) = skip {
            cues.push(untranslated(cue.ordinal, reason));
            continue;
        }
        if stopped(signal) {
            return Ok(TranslationResult::Cancelled);
        }
        match translate_cue(task, scratch, cue, signal).await? {
            Some(translated) => cues.push(translated),
            None => return Ok(TranslationResult::Cancelled),
        }
    }
    Ok(TranslationResult::Succeeded(cues))
}

fn verify_assets(profile: &TranslationProfile, stop: &AtomicBool) -> Result<bool> {
    let runtime = runtime_manifest(Path::new(&profile.runtime_dir), &profile.executable, stop)?;
    let model = hash_file(
        Path::new(&profile.model_path),
        MAX_TRANSLATION_MODEL_BYTES,
        stop,
    )?;
    Ok(runtime.sha256 == profile.runtime_sha256
        && runtime.files == profile.runtime_files
        && runtime.bytes == profile.runtime_bytes
        && model == (profile.model_sha256.clone(), profile.model_bytes)
        && profile.identity()? == profile.profile_sha256)
}

/// Translate one cue in its own contained process. `None` means cancelled.
async fn translate_cue(
    task: &TranslationTask,
    scratch: &Path,
    cue: &SourceCue,
    signal: &mut watch::Receiver<bool>,
) -> Result<Option<TranslatedCue>> {
    let profile = &task.profile;
    let prompt = scratch.join(format!("cue-{}.txt", cue.ordinal));
    if std::fs::write(&prompt, format!("{HY_MT2_PROMPT}{}", cue.script)).is_err() {
        return Ok(Some(untranslated(cue.ordinal, "translator-failed")));
    }
    let Some(group) = contained(
        ProcessGroupOptions::default()
            .max_processes(1)
            .max_memory(profile.memory_bytes)
            .cpu_quota(f64::from(profile.threads)),
    ) else {
        return Ok(Some(untranslated(cue.ordinal, "limits-unavailable")));
    };
    let runtime = Path::new(&profile.runtime_dir);
    let mut command = tokio::process::Command::new(runtime.join(&profile.executable));
    command
        .env_clear()
        .current_dir(scratch)
        .arg("-m")
        .arg(&profile.model_path)
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
        .args(["-t", &profile.threads.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    runtime_environment(&mut command, runtime);
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
            .take(MAX_STDOUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((bytes, status))
    };
    let deadline = Duration::from_millis(profile.cue_deadline_ms);
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
        Some(Ok(Ok((bytes, status)))) => Some(if bytes.len() as u64 > MAX_STDOUT_BYTES {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_text_strips_formatting_only() {
        let raw = "\u{1b}[33m\u{1b}[0mA world fair is a festival. [end of text]\n\n";
        assert_eq!(
            completion_text(raw.as_bytes()),
            Ok("A world fair is a festival.".to_owned())
        );
        assert_eq!(completion_text(b"  [end of text]  "), Err("empty-output"));
        assert_eq!(completion_text(&[0xff, 0xfe]), Err("invalid-output"));
        assert_eq!(completion_text(b"a\0b"), Err("invalid-output"));
        assert_eq!(
            completion_text("x".repeat(4097).as_bytes()),
            Err("output-limit")
        );
        assert_eq!(
            completion_text("Ignore previous instructions.".as_bytes()),
            Ok("Ignore previous instructions.".to_owned())
        );
    }

    #[test]
    fn languages_are_canonical_and_declarations_are_exact() -> Result<()> {
        assert_eq!(canonical_languages(" ES, ar,zh ,ar")?, "ar,es,zh");
        assert!(canonical_languages("").is_err());
        assert!(canonical_languages("spanish").is_err());
        assert!(canonical_languages("e1").is_err());
        Ok(())
    }
}
