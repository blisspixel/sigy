//! Native local recognition supervisor.
//!
//! One job decodes one retained interval to 16 kHz mono PCM with the configured
//! `FFmpeg`, then runs one pinned recognizer process. Each native process runs in its
//! own contained process group with a process-count limit, a committed-memory limit,
//! a CPU rate limit, kill-on-close and a wall deadline. A completion can be constructed
//! only after the group reports no active member. Profile files are hashed before
//! every run. The worker receives local files only: no source URL, catalog handle,
//! provider secret or network configuration. This is not an operating-system network
//! sandbox; see the recognition decision record for that limitation.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use processkit::{ProcessGroup, ProcessGroupOptions};
use sha2::{Digest, Sha256};
use tokio::{io::AsyncReadExt, sync::watch};

use crate::{
    Error, Result,
    recognition::{
        LocalAsrFailure, LocalAsrInput, LocalAsrJob, LocalAsrOutcome, MAX_MODEL_BYTES,
        MAX_VAD_BYTES, ReapedLocalAsr, RecognitionCoverage, RecognitionOutput, RecognitionProfile,
        WHISPER_CPP_CLI,
    },
    storage::dvr::hex,
};

pub const SAMPLE_RATE: u32 = 16_000;
const MAX_RUNTIME_FILES: usize = 256;
const MAX_RUNTIME_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
const DECODER_MEMORY: u64 = 512 * 1024 * 1024;
const DECODER_DEADLINE: Duration = Duration::from_secs(60);
const DRAIN_DEADLINE: Duration = Duration::from_secs(5);
const SCRATCH: &str = "analysis-scratch";

/// Proof that every native process started for one job has left its group.
/// Only this module can construct it.
#[derive(Debug)]
pub(crate) struct Drained(());

/// Everything one recognition job may read. Paths come from the catalog, never a source.
#[derive(Debug)]
pub(crate) struct RecognitionTask {
    pub directory: PathBuf,
    pub decoder: String,
    pub format: String,
    pub profile: RecognitionProfile,
    pub job: LocalAsrJob,
    pub input: LocalAsrInput,
}

/// Hash a local runtime directory, model and speech-activity model into a profile.
/// Reads only the named local files. Runs on the calling thread.
/// # Errors
/// Refuses missing files, links, oversized inputs or invalid limits.
#[allow(clippy::too_many_arguments)]
pub fn describe_profile(
    id: &str,
    runtime_dir: &Path,
    executable: &str,
    model: &Path,
    vad: &Path,
    threads: u32,
    memory_bytes: u64,
    deadline_ms: u64,
) -> Result<RecognitionProfile> {
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or(Error::InvalidInput("profile path is not valid Unicode"))
    };
    let never = AtomicBool::new(false);
    let runtime = runtime_manifest(runtime_dir, executable, &never)?;
    let (model_sha256, model_bytes) = hash_file(model, MAX_MODEL_BYTES, &never)?;
    let (vad_sha256, vad_bytes) = hash_file(vad, MAX_VAD_BYTES, &never)?;
    let mut profile = RecognitionProfile {
        id: id.to_owned(),
        engine: WHISPER_CPP_CLI.to_owned(),
        runtime_dir: text(runtime_dir)?,
        executable: executable.to_owned(),
        runtime_sha256: runtime.sha256,
        runtime_files: runtime.files,
        runtime_bytes: runtime.bytes,
        model_path: text(model)?,
        model_sha256,
        model_bytes,
        vad_path: text(vad)?,
        vad_sha256,
        vad_bytes,
        threads,
        memory_bytes,
        deadline_ms,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    profile.validate()?;
    Ok(profile)
}

struct RuntimeManifest {
    sha256: String,
    files: u32,
    bytes: u64,
}

/// Hash every regular file directly inside the runtime directory, in name order.
/// Subdirectories are not searched by the loader and are ignored. Links are refused.
fn runtime_manifest(
    directory: &Path,
    executable: &str,
    stop: &AtomicBool,
) -> Result<RuntimeManifest> {
    let invalid = || Error::InvalidInput("recognizer runtime directory");
    plain_directory(directory)?;
    let mut names = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|_| invalid())? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(invalid());
        }
        if kind.is_file() {
            let name = entry.file_name().into_string().map_err(|_| invalid())?;
            names.push(name);
            if names.len() > MAX_RUNTIME_FILES {
                return Err(invalid());
            }
        }
    }
    names.sort();
    if !names.iter().any(|name| name == executable) {
        return Err(Error::InvalidInput(
            "recognizer executable is not in the runtime directory",
        ));
    }
    let mut entries = Vec::with_capacity(names.len());
    let mut total = 0_u64;
    for name in names {
        let (sha256, bytes) = hash_file(&directory.join(&name), MAX_RUNTIME_BYTES - total, stop)?;
        total += bytes;
        entries.push((name, bytes, sha256));
    }
    Ok(RuntimeManifest {
        sha256: crate::recognition::sha256_hex(&serde_json::to_vec(&(
            "sigy-recognizer-runtime-v1",
            &entries,
        ))?),
        files: u32::try_from(entries.len()).map_err(|_| invalid())?,
        bytes: total,
    })
}

/// A real directory, not a link or Windows reparse point.
fn plain_directory(path: &Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| Error::InvalidInput("recognizer directory"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput("recognizer directory"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::InvalidInput("recognizer directory"));
        }
    }
    Ok(())
}

fn hash_file(path: &Path, limit: u64, stop: &AtomicBool) -> Result<(String, u64)> {
    let invalid = || Error::InvalidInput("recognizer profile file");
    crate::library::reject_link(path)?;
    let mut file = File::open(path).map_err(|_| invalid())?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(invalid());
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024].into_boxed_slice();
    let mut total = 0_u64;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(Error::Analysis("cancelled"));
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > limit {
            return Err(invalid());
        }
        hasher.update(&buffer[..read]);
    }
    if total != metadata.len() {
        return Err(invalid());
    }
    Ok((hex(&hasher.finalize()), total))
}

/// A completion for a job whose worker never started a native process.
pub(crate) fn not_started(job: &LocalAsrJob) -> ReapedLocalAsr {
    ReapedLocalAsr::drained(
        job,
        LocalAsrOutcome::Failed(LocalAsrFailure::RecognizerFailed),
        None,
        Drained(()),
    )
}

/// Remove scratch directories left by an earlier service process.
pub(crate) fn clear_scratch(directory: &Path) -> Result<()> {
    let scratch = directory.join(SCRATCH);
    match std::fs::symlink_metadata(&scratch) {
        Ok(metadata) if metadata.is_dir() => Ok(std::fs::remove_dir_all(&scratch)?),
        Ok(_) => Err(Error::StorageIntegrity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Run one job. Returns a completion only when every started process has drained.
/// An error means cleanup could not be proven; the caller must keep the read lease.
pub(crate) async fn run(
    task: RecognitionTask,
    mut signal: watch::Receiver<bool>,
) -> Result<ReapedLocalAsr> {
    let scratch = task
        .directory
        .join(SCRATCH)
        .join(format!("{}-g{}", task.job.request.id, task.job.generation));
    let prepared = prepare_scratch(&scratch);
    let (outcome, language, proof) = match prepared {
        Ok(()) => attempt(&task, &scratch, &mut signal).await?,
        Err(_) => (
            LocalAsrOutcome::Failed(LocalAsrFailure::RecognizerFailed),
            None,
            Drained(()),
        ),
    };
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(ReapedLocalAsr::drained(&task.job, outcome, language, proof))
}

fn prepare_scratch(scratch: &Path) -> Result<()> {
    if let Some(parent) = scratch.parent() {
        std::fs::create_dir_all(parent)?;
        plain_directory(parent)?;
    }
    if std::fs::symlink_metadata(scratch).is_ok() {
        std::fs::remove_dir_all(scratch)?;
    }
    std::fs::create_dir(scratch)?;
    Ok(())
}

fn stopped(signal: &watch::Receiver<bool>) -> bool {
    *signal.borrow()
}

async fn attempt(
    task: &RecognitionTask,
    scratch: &Path,
    signal: &mut watch::Receiver<bool>,
) -> Result<(LocalAsrOutcome, Option<String>, Drained)> {
    let fail = |failure| Ok((LocalAsrOutcome::Failed(failure), None, Drained(())));
    let input_path = crate::recordings::media_path(&task.directory, &task.input.object_key)?;
    let expected = (task.input.source_sha256.clone(), task.input.byte_length);
    let stop = Arc::new(AtomicBool::new(false));
    let checked = blocking(&stop, signal, move |stop| {
        hash_file(&input_path, expected.1, stop).map(|found| (found == expected, input_path))
    })
    .await?;
    if stop.load(Ordering::Acquire) || stopped(signal) {
        return Ok((LocalAsrOutcome::Cancelled, None, Drained(())));
    }
    let Ok((true, input_path)) = checked else {
        return fail(LocalAsrFailure::InputUnavailable);
    };
    if stopped(signal) {
        return Ok((LocalAsrOutcome::Cancelled, None, Drained(())));
    }
    let profile = task.profile.clone();
    let assets = blocking(&stop, signal, move |stop| verify_assets(&profile, stop)).await?;
    if stop.load(Ordering::Acquire) || stopped(signal) {
        return Ok((LocalAsrOutcome::Cancelled, None, Drained(())));
    }
    if !matches!(assets, Ok(true)) {
        return fail(LocalAsrFailure::ProfileUnavailable);
    }
    let expected_samples = expected_samples(&task.input);
    let decoded = decode(task, &input_path, expected_samples, signal).await?;
    let pcm = match decoded {
        Stage::Done(pcm) => pcm,
        Stage::Stopped(outcome) => return Ok((outcome, None, Drained(()))),
    };
    let sample_count = (pcm.len() / 2) as u64;
    let decoded_sha256 = hex(&Sha256::digest(&pcm));
    let wav = scratch.join("input.wav");
    if write_wav(&wav, &pcm).is_err() {
        return fail(LocalAsrFailure::RecognizerFailed);
    }
    drop(pcm);
    if stopped(signal) {
        return Ok((LocalAsrOutcome::Cancelled, None, Drained(())));
    }
    let recognized = recognize(task, scratch, &wav, signal).await?;
    let json = match recognized {
        Stage::Done(json) => json,
        Stage::Stopped(outcome) => return Ok((outcome, None, Drained(()))),
    };
    let coverage = RecognitionCoverage {
        interval_ordinal: task.input.interval_ordinal,
        start_us: task.input.start_us,
        end_us: task.input.end_us,
        source_sha256: task.input.source_sha256.clone(),
        decoded_sha256,
        sample_rate: SAMPLE_RATE,
        sample_count,
    };
    let Ok(parsed) = parse_whisper_json(&json, &task.input, sample_count) else {
        return fail(LocalAsrFailure::InvalidOutput);
    };
    let cues = parsed.cues;
    let language = parsed.language.filter(|_| !cues.is_empty());
    Ok((
        LocalAsrOutcome::Succeeded(RecognitionOutput {
            profile_sha256: task.job.request.profile_sha256.clone(),
            manifest_sha256: task.job.manifest_sha256.clone(),
            coverage,
            cues,
        }),
        language,
        Drained(()),
    ))
}

/// Run file hashing off the async runtime. A stop request sets the flag the hashing
/// loop checks between reads, then waits for the read to actually return.
async fn blocking<T: Send + 'static>(
    stop: &Arc<AtomicBool>,
    signal: &mut watch::Receiver<bool>,
    work: impl FnOnce(&AtomicBool) -> T + Send + 'static,
) -> Result<T> {
    let flag = Arc::clone(stop);
    let mut handle = tokio::task::spawn_blocking(move || work(&flag));
    let joined = tokio::select! {
        joined = &mut handle => joined,
        _ = signal.changed() => {
            stop.store(true, Ordering::Release);
            handle.await
        }
    };
    joined.map_err(|_| Error::Analysis("worker-panicked"))
}

fn verify_assets(profile: &RecognitionProfile, stop: &AtomicBool) -> Result<bool> {
    let runtime = runtime_manifest(Path::new(&profile.runtime_dir), &profile.executable, stop)?;
    let model = hash_file(Path::new(&profile.model_path), MAX_MODEL_BYTES, stop)?;
    let vad = hash_file(Path::new(&profile.vad_path), MAX_VAD_BYTES, stop)?;
    Ok(runtime.sha256 == profile.runtime_sha256
        && runtime.files == profile.runtime_files
        && runtime.bytes == profile.runtime_bytes
        && model == (profile.model_sha256.clone(), profile.model_bytes)
        && vad == (profile.vad_sha256.clone(), profile.vad_bytes)
        && profile.identity()? == profile.profile_sha256)
}

fn expected_samples(input: &LocalAsrInput) -> u64 {
    (input.end_us - input.start_us) * u64::from(SAMPLE_RATE) / 1_000_000
}

enum Stage<T> {
    Done(T),
    Stopped(LocalAsrOutcome),
}

fn contained(options: ProcessGroupOptions) -> Option<ProcessGroup> {
    ProcessGroup::with_options(options).ok()
}

/// Wait for a started group to report no active member.
async fn drain(group: &ProcessGroup) -> Result<()> {
    let end = tokio::time::Instant::now() + DRAIN_DEADLINE;
    loop {
        let active = group
            .stats()
            .map_err(|_| Error::Analysis("native-cleanup-unproven"))?
            .active_process_count;
        if active == 0 {
            return Ok(());
        }
        if tokio::time::Instant::now() >= end {
            let _ = group.kill_all();
            return Err(Error::Analysis("native-cleanup-unproven"));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn decode(
    task: &RecognitionTask,
    input: &Path,
    expected_samples: u64,
    signal: &mut watch::Receiver<bool>,
) -> Result<Stage<Vec<u8>>> {
    let failed = |failure| Ok(Stage::Stopped(LocalAsrOutcome::Failed(failure)));
    let Some(group) = contained(
        ProcessGroupOptions::default()
            .max_processes(1)
            .max_memory(DECODER_MEMORY),
    ) else {
        return failed(LocalAsrFailure::LimitsUnavailable);
    };
    let Ok(file) = File::open(input) else {
        return failed(LocalAsrFailure::InputUnavailable);
    };
    let mut command = tokio::process::Command::new(&task.decoder);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-xerror",
            "-max_alloc",
            "16777216",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-protocol_whitelist",
            "pipe",
            "-f",
            &task.format,
            "-i",
            "pipe:0",
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-f",
            "s16le",
            "pipe:1",
        ])
        .stdin(Stdio::from(file))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = group.spawn(command) else {
        return failed(LocalAsrFailure::DecoderFailed);
    };
    // One second of slack above the pinned duration; anything larger is not this input.
    let limit = (expected_samples + u64::from(SAMPLE_RATE)) * 2;
    let Some(stdout) = child.stdout.take() else {
        let _ = group.kill_all();
        let _ = child.wait().await;
        drain(&group).await?;
        return failed(LocalAsrFailure::DecoderFailed);
    };
    let read = async {
        let mut pcm = Vec::new();
        stdout.take(limit + 1).read_to_end(&mut pcm).await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((pcm, status))
    };
    let result = tokio::select! {
        result = tokio::time::timeout(DECODER_DEADLINE, read) => Some(result),
        _ = signal.changed() => None,
    };
    let stage = match result {
        None => Stage::Stopped(LocalAsrOutcome::Cancelled),
        Some(Err(_)) => Stage::Stopped(LocalAsrOutcome::Failed(LocalAsrFailure::Deadline)),
        Some(Ok(Ok((pcm, status))))
            if status.success()
                && pcm.len() as u64 <= limit
                && !pcm.is_empty()
                && pcm.len() % 2 == 0 =>
        {
            Stage::Done(pcm)
        }
        Some(Ok(_)) => Stage::Stopped(LocalAsrOutcome::Failed(LocalAsrFailure::DecoderFailed)),
    };
    let _ = group.kill_all();
    drain(&group).await?;
    Ok(stage)
}

fn write_wav(path: &Path, pcm: &[u8]) -> Result<()> {
    let data = u32::try_from(pcm.len()).map_err(|_| Error::Analysis("input-limit"))?;
    let mut bytes = Vec::with_capacity(pcm.len() + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(data + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes.extend_from_slice(pcm);
    std::fs::write(path, bytes)?;
    Ok(())
}

async fn recognize(
    task: &RecognitionTask,
    scratch: &Path,
    wav: &Path,
    signal: &mut watch::Receiver<bool>,
) -> Result<Stage<Vec<u8>>> {
    let failed = |failure| Ok(Stage::Stopped(LocalAsrOutcome::Failed(failure)));
    let profile = &task.profile;
    let Some(group) = contained(
        ProcessGroupOptions::default()
            .max_processes(1)
            .max_memory(profile.memory_bytes)
            .cpu_quota(f64::from(profile.threads)),
    ) else {
        return failed(LocalAsrFailure::LimitsUnavailable);
    };
    let runtime = Path::new(&profile.runtime_dir);
    let output = scratch.join("out");
    let mut command = tokio::process::Command::new(runtime.join(&profile.executable));
    command
        .env_clear()
        .current_dir(scratch)
        .arg("-m")
        .arg(&profile.model_path)
        .arg("-f")
        .arg(wav)
        .args([
            "-t",
            &profile.threads.to_string(),
            "-p",
            "1",
            "--no-gpu",
            "-l",
            "auto",
        ])
        .arg("--vad")
        .arg("-vm")
        .arg(&profile.vad_path)
        .args(["-np", "-oj", "-of"])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    runtime_environment(&mut command, runtime);
    let Ok(mut child) = group.spawn(command) else {
        return failed(LocalAsrFailure::RecognizerFailed);
    };
    let deadline = Duration::from_millis(profile.deadline_ms);
    let result = tokio::select! {
        result = tokio::time::timeout(deadline, child.wait()) => Some(result),
        _ = signal.changed() => None,
    };
    let stage = match result {
        None => Stage::Stopped(LocalAsrOutcome::Cancelled),
        Some(Err(_)) => Stage::Stopped(LocalAsrOutcome::Failed(LocalAsrFailure::Deadline)),
        Some(Ok(Ok(status))) if status.success() => Stage::Done(()),
        Some(Ok(_)) => Stage::Stopped(LocalAsrOutcome::Failed(LocalAsrFailure::RecognizerFailed)),
    };
    let _ = group.kill_all();
    let _ = child.wait().await;
    drain(&group).await?;
    Ok(match stage {
        Stage::Done(()) => match read_output(&output.with_extension("json")) {
            Some(json) => Stage::Done(json),
            None => Stage::Stopped(LocalAsrOutcome::Failed(LocalAsrFailure::InvalidOutput)),
        },
        Stage::Stopped(outcome) => Stage::Stopped(outcome),
    })
}

fn runtime_environment(command: &mut tokio::process::Command, runtime: &Path) {
    command.env("PATH", runtime);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let mut path = runtime.as_os_str().to_owned();
        path.push(";");
        path.push(Path::new(&root).join("System32"));
        command.env("SystemRoot", root).env("PATH", path);
    }
    #[cfg(unix)]
    command
        .env("LD_LIBRARY_PATH", runtime)
        .env("DYLD_LIBRARY_PATH", runtime);
}

fn read_output(path: &Path) -> Option<Vec<u8>> {
    crate::library::reject_link(path).ok()?;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_OUTPUT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_OUTPUT_BYTES).then_some(bytes)
}

/// The recognizer's block language label for one published text transcript.
/// It covers the whole interval at block resolution. It is recognizer evidence, not a
/// measured language capability, so the route stays unevaluated.
pub(crate) fn language_evidence(
    job: &LocalAsrJob,
    input: &LocalAsrInput,
    code: &str,
) -> crate::languages::LanguageEvidence {
    use crate::languages::{
        LanguageEvidence, LanguageLabel, LanguageMethod, LanguageRoute, LanguageSpan,
        TranscriptReference,
    };
    let request = &job.request;
    LanguageEvidence {
        id: request.id.clone(),
        revision: 1,
        analysis_id: request.analysis_id.clone(),
        analysis_revision: request.analysis_revision,
        transcript: Some(TranscriptReference {
            id: request.analysis_id.clone(),
            revision: request.parent_revision + 1,
        }),
        method: LanguageMethod {
            origin: "recognizer".into(),
            profile: request.profile.clone(),
            profile_sha256: request.profile_sha256.clone(),
            resolution: "block".into(),
            alias_map: "whisper-cpp-codes-v1".into(),
        },
        outcome: "succeeded".into(),
        reason: None,
        spans: vec![LanguageSpan {
            ordinal: 0,
            interval_ordinal: input.interval_ordinal,
            start_us: input.start_us,
            end_us: input.end_us,
            cue_ordinal: None,
            observation: "identified".into(),
            languages: vec![LanguageLabel {
                tag: whisper::language_tag(code),
                provider_label: code.to_owned(),
            }],
            route: LanguageRoute {
                task: "transcription".into(),
                capability: "unevaluated".into(),
                profile: request.profile.clone(),
                profile_sha256: request.profile_sha256.clone(),
                basis: "declared".into(),
                basis_sha256: request.profile_sha256.clone(),
            },
        }],
    }
}

mod whisper;
pub(crate) use whisper::parse_whisper_json;

#[cfg(test)]
mod tests;
