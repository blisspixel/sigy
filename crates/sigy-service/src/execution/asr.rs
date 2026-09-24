//! One recognition task: check the input and assets, decode the pinned interval to
//! 16 kHz mono PCM with the local decoder, run the pinned recognizer once, and map its
//! bounded output onto the media clock.

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

use processkit::ProcessGroupOptions;
use sha2::{Digest, Sha256};
use tokio::{io::AsyncReadExt, sync::watch};

use super::{
    AssetRole, Drained, RecognitionResult, ResultEnvelope, TaskParams, TaskResult, TaskSpec,
    files::{hash_file, runtime_manifest},
    local::{blocking, contained, drain, prepare_scratch, runtime_environment, stopped},
    spec::{DecoderLimits, RecognitionParams},
    stage::LocalStage,
};
use crate::{
    Result,
    recognition::{
        LocalAsrFailure::{
            self, Deadline, DecoderFailed, InputUnavailable, InvalidOutput, LimitsUnavailable,
            ProfileUnavailable, RecognizerFailed,
        },
        MAX_MODEL_BYTES, MAX_VAD_BYTES, RecognitionCoverage,
    },
    storage::dvr::hex,
};

/// Everything the pipeline resolved from the spec and stage before any process starts.
struct Plan<'a> {
    spec: &'a TaskSpec,
    params: &'a RecognitionParams,
    decoder: &'a Path,
    decoder_limits: DecoderLimits,
    format: &'a str,
    input: PathBuf,
    runtime: PathBuf,
    executable: &'a str,
    model: PathBuf,
    vad: PathBuf,
}

fn plan<'a>(spec: &'a TaskSpec, stage: &'a LocalStage) -> Option<Plan<'a>> {
    let TaskParams::Recognition(params) = &spec.params else {
        return None;
    };
    let blob = spec.blob()?;
    let runtime = spec.asset(AssetRole::Runtime)?;
    let model = spec.asset(AssetRole::Model)?;
    let vad = spec.asset(AssetRole::SpeechActivity)?;
    Some(Plan {
        spec,
        params,
        decoder: stage.decoder_path()?,
        decoder_limits: spec.limits.decoder?,
        format: blob.media_type.strip_prefix("audio-")?,
        input: stage.blob_path(blob)?.to_path_buf(),
        runtime: stage.asset_path(runtime)?.to_path_buf(),
        executable: runtime.entry.as_deref()?,
        model: stage.asset_path(model)?.to_path_buf(),
        vad: stage.asset_path(vad)?.to_path_buf(),
    })
}

/// Run one job. Returns an envelope only when every started process has drained.
/// An error means cleanup could not be proven; the caller must keep the read lease.
pub(super) async fn run(
    spec: TaskSpec,
    stage: LocalStage,
    mut signal: watch::Receiver<bool>,
) -> Result<ResultEnvelope> {
    let refuse = |failure| {
        Ok(ResultEnvelope::new(
            &spec,
            TaskResult::Recognition(RecognitionResult::Failed(failure)),
            Drained(()),
        ))
    };
    if spec.validate().is_err() {
        return refuse(RecognizerFailed);
    }
    let Some(plan) = plan(&spec, &stage) else {
        return refuse(RecognizerFailed);
    };
    let scratch = stage.scratch(&spec);
    let (result, proof) = match prepare_scratch(&scratch) {
        Ok(()) => attempt(&plan, &scratch, &mut signal).await?,
        Err(_) => (RecognitionResult::Failed(RecognizerFailed), Drained(())),
    };
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(ResultEnvelope::new(
        &spec,
        TaskResult::Recognition(result),
        proof,
    ))
}

type Attempt = Result<(RecognitionResult, Drained)>;

fn failed(failure: LocalAsrFailure) -> (RecognitionResult, Drained) {
    (RecognitionResult::Failed(failure), Drained(()))
}

fn cancelled() -> (RecognitionResult, Drained) {
    (RecognitionResult::Cancelled, Drained(()))
}

async fn attempt(plan: &Plan<'_>, scratch: &Path, signal: &mut watch::Receiver<bool>) -> Attempt {
    let Some(blob) = plan.spec.blob() else {
        return Ok(failed(InputUnavailable));
    };
    let input_path = plan.input.clone();
    let expected = (blob.sha256.clone(), blob.bytes);
    let stop = Arc::new(AtomicBool::new(false));
    let checked = blocking(&stop, signal, move |stop| {
        hash_file(&input_path, expected.1, stop).map(|found| (found == expected, input_path))
    })
    .await?;
    if stop.load(Ordering::Acquire) || stopped(signal) {
        return Ok(cancelled());
    }
    let Ok((true, input_path)) = checked else {
        return Ok(failed(InputUnavailable));
    };
    if stopped(signal) {
        return Ok(cancelled());
    }
    let assets = verify_assets(plan);
    let assets = blocking(&stop, signal, move |stop| assets.check(stop)).await?;
    if stop.load(Ordering::Acquire) || stopped(signal) {
        return Ok(cancelled());
    }
    if !matches!(assets, Ok(true)) {
        return Ok(failed(ProfileUnavailable));
    }
    decode_and_recognize(plan, scratch, &input_path, signal).await
}

async fn decode_and_recognize(
    plan: &Plan<'_>,
    scratch: &Path,
    input_path: &Path,
    signal: &mut watch::Receiver<bool>,
) -> Attempt {
    let params = plan.params;
    let expected_samples =
        (params.end_us - params.start_us) * u64::from(params.sample_rate) / 1_000_000;
    let pcm = match decode(plan, input_path, expected_samples, signal).await? {
        Stage::Done(pcm) => pcm,
        Stage::Stopped(result) => return Ok((result, Drained(()))),
    };
    let sample_count = (pcm.len() / 2) as u64;
    let decoded_sha256 = hex(&Sha256::digest(&pcm));
    let wav = scratch.join("input.wav");
    if write_wav(&wav, &pcm, params.sample_rate).is_err() {
        return Ok(failed(RecognizerFailed));
    }
    drop(pcm);
    if stopped(signal) {
        return Ok(cancelled());
    }
    let json = match recognize(plan, scratch, &wav, signal).await? {
        Stage::Done(json) => json,
        Stage::Stopped(result) => return Ok((result, Drained(()))),
    };
    let Some(blob) = plan.spec.blob() else {
        return Ok(failed(InputUnavailable));
    };
    let coverage = RecognitionCoverage {
        interval_ordinal: params.interval_ordinal,
        start_us: params.start_us,
        end_us: params.end_us,
        source_sha256: blob.sha256.clone(),
        decoded_sha256,
        sample_rate: params.sample_rate,
        sample_count,
    };
    let Ok(parsed) =
        crate::recognizer::parse_whisper_json(&json, params.start_us, params.end_us, sample_count)
    else {
        return Ok(failed(InvalidOutput));
    };
    let cues = parsed.cues;
    let language = parsed.language.filter(|_| !cues.is_empty());
    Ok((
        RecognitionResult::Succeeded {
            coverage,
            cues,
            language,
        },
        Drained(()),
    ))
}

/// The files a profile pins, re-hashed before every run.
struct AssetCheck {
    runtime: PathBuf,
    executable: String,
    expected_runtime: (String, u32, u64),
    model: (PathBuf, String, u64),
    vad: (PathBuf, String, u64),
}

fn verify_assets(plan: &Plan<'_>) -> AssetCheck {
    let asset = |role| {
        plan.spec
            .asset(role)
            .map(|asset| (asset.sha256.clone(), asset.files, asset.bytes))
            .unwrap_or_default()
    };
    let (runtime_sha256, runtime_files, runtime_bytes) = asset(AssetRole::Runtime);
    let (model_sha256, _, model_bytes) = asset(AssetRole::Model);
    let (vad_sha256, _, vad_bytes) = asset(AssetRole::SpeechActivity);
    AssetCheck {
        runtime: plan.runtime.clone(),
        executable: plan.executable.to_owned(),
        expected_runtime: (runtime_sha256, runtime_files, runtime_bytes),
        model: (plan.model.clone(), model_sha256, model_bytes),
        vad: (plan.vad.clone(), vad_sha256, vad_bytes),
    }
}

impl AssetCheck {
    fn check(self, stop: &AtomicBool) -> Result<bool> {
        let runtime = runtime_manifest(&self.runtime, &self.executable, stop)?;
        let model = hash_file(&self.model.0, MAX_MODEL_BYTES, stop)?;
        let vad = hash_file(&self.vad.0, MAX_VAD_BYTES, stop)?;
        Ok(
            (runtime.sha256, runtime.files, runtime.bytes) == self.expected_runtime
                && model == (self.model.1, self.model.2)
                && vad == (self.vad.1, self.vad.2),
        )
    }
}

enum Stage<T> {
    Done(T),
    Stopped(RecognitionResult),
}

fn stop_with<T>(failure: LocalAsrFailure) -> Stage<T> {
    Stage::Stopped(RecognitionResult::Failed(failure))
}

async fn decode(
    plan: &Plan<'_>,
    input: &Path,
    expected_samples: u64,
    signal: &mut watch::Receiver<bool>,
) -> Result<Stage<Vec<u8>>> {
    let Some(group) = contained(
        ProcessGroupOptions::default()
            .max_processes(1)
            .max_memory(plan.decoder_limits.memory_bytes),
    ) else {
        return Ok(stop_with(LimitsUnavailable));
    };
    let Ok(file) = File::open(input) else {
        return Ok(stop_with(InputUnavailable));
    };
    let sample_rate = plan.params.sample_rate.to_string();
    let mut command = tokio::process::Command::new(plan.decoder);
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
            plan.format,
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
            &sample_rate,
            "-f",
            "s16le",
            "pipe:1",
        ])
        .stdin(Stdio::from(file))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = group.spawn(command) else {
        return Ok(stop_with(DecoderFailed));
    };
    // One second of slack above the pinned duration; anything larger is not this input.
    let limit = (expected_samples + u64::from(plan.params.sample_rate)) * 2;
    let Some(stdout) = child.stdout.take() else {
        let _ = group.kill_all();
        let _ = child.wait().await;
        drain(&group).await?;
        return Ok(stop_with(DecoderFailed));
    };
    let read = async {
        let mut pcm = Vec::new();
        stdout.take(limit + 1).read_to_end(&mut pcm).await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((pcm, status))
    };
    let deadline = Duration::from_millis(plan.decoder_limits.wall_ms);
    let result = tokio::select! {
        result = tokio::time::timeout(deadline, read) => Some(result),
        _ = signal.changed() => None,
    };
    let stage = match result {
        None => Stage::Stopped(RecognitionResult::Cancelled),
        Some(Err(_)) => Stage::Stopped(RecognitionResult::Failed(Deadline)),
        Some(Ok(Ok((pcm, status))))
            if status.success()
                && pcm.len() as u64 <= limit
                && !pcm.is_empty()
                && pcm.len() % 2 == 0 =>
        {
            Stage::Done(pcm)
        }
        Some(Ok(_)) => Stage::Stopped(RecognitionResult::Failed(DecoderFailed)),
    };
    let _ = group.kill_all();
    drain(&group).await?;
    Ok(stage)
}

pub(super) fn write_wav(path: &Path, pcm: &[u8], sample_rate: u32) -> Result<()> {
    let data = u32::try_from(pcm.len()).map_err(|_| crate::Error::Analysis("input-limit"))?;
    let mut bytes = Vec::with_capacity(pcm.len() + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(data + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes.extend_from_slice(pcm);
    std::fs::write(path, bytes)?;
    Ok(())
}

async fn recognize(
    plan: &Plan<'_>,
    scratch: &Path,
    wav: &Path,
    signal: &mut watch::Receiver<bool>,
) -> Result<Stage<Vec<u8>>> {
    let limits = plan.spec.limits;
    let Some(group) = contained(
        ProcessGroupOptions::default()
            .max_processes(limits.processes)
            .max_memory(limits.memory_bytes)
            .cpu_quota(f64::from(limits.cpu_rate)),
    ) else {
        return Ok(stop_with(LimitsUnavailable));
    };
    let output = scratch.join("out");
    let mut command = tokio::process::Command::new(plan.runtime.join(plan.executable));
    command
        .env_clear()
        .current_dir(scratch)
        .arg("-m")
        .arg(&plan.model)
        .arg("-f")
        .arg(wav)
        .args([
            "-t",
            &plan.params.threads.to_string(),
            "-p",
            "1",
            "--no-gpu",
            "-l",
            "auto",
        ])
        .arg("--vad")
        .arg("-vm")
        .arg(&plan.vad)
        .args(["-np", "-oj", "-of"])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    runtime_environment(&mut command, &plan.runtime);
    let Ok(mut child) = group.spawn(command) else {
        return Ok(stop_with(RecognizerFailed));
    };
    let deadline = Duration::from_millis(limits.wall_ms);
    let result = tokio::select! {
        result = tokio::time::timeout(deadline, child.wait()) => Some(result),
        _ = signal.changed() => None,
    };
    let stage = match result {
        None => Stage::Stopped(RecognitionResult::Cancelled),
        Some(Err(_)) => Stage::Stopped(RecognitionResult::Failed(Deadline)),
        Some(Ok(Ok(status))) if status.success() => Stage::Done(()),
        Some(Ok(_)) => Stage::Stopped(RecognitionResult::Failed(RecognizerFailed)),
    };
    let _ = group.kill_all();
    let _ = child.wait().await;
    drain(&group).await?;
    Ok(match stage {
        Stage::Done(()) => match read_output(&output.with_extension("json"), limits.output_bytes) {
            Some(json) => Stage::Done(json),
            None => Stage::Stopped(RecognitionResult::Failed(InvalidOutput)),
        },
        Stage::Stopped(result) => Stage::Stopped(result),
    })
}

fn read_output(path: &Path, limit: u64) -> Option<Vec<u8>> {
    crate::library::reject_link(path).ok()?;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}
