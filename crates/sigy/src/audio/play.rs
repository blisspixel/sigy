//! Parent-owned PCM delivery, bounded child protocol and observed native closure.

use serde::Serialize;
use sigy_service::{
    control::RetainedReadSpec,
    recordings::{
        self, RetainedPlaybackReport,
        audio::{AudioClosure, NativeAudioGroup, PcmFormat, PcmReaderRequest},
    },
};
use std::{fmt, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::mpsc,
};

use super::{
    Failure,
    protocol::{self, Event, HelperLimits, Ready, Report, Status},
};

#[derive(Debug, Serialize)]
pub(crate) struct OutputReport {
    pub ready: Ready,
    pub sink: Report,
    pub native_closure: AudioClosure,
}

#[derive(Debug)]
pub(crate) struct Playback {
    pub decoder: RetainedPlaybackReport,
    pub output: OutputReport,
}

#[derive(Debug, Serialize)]
pub(crate) struct OutputError {
    pub reason: String,
    pub native_closure: Option<Box<AudioClosure>>,
    pub decoder: Option<RetainedPlaybackReport>,
    pub decoder_failure: Option<String>,
}

impl fmt::Display for OutputError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{}; native audio closure {}",
            self.reason,
            if self.native_closure.is_some() {
                "observed"
            } else {
                "unproven or not started"
            }
        )
    }
}

impl std::error::Error for OutputError {}

pub(crate) async fn retained(
    executable: &str,
    directory: &Path,
    nonce: &str,
    spec: &RetainedReadSpec,
) -> Result<Playback, OutputError> {
    if spec.file_duration_us == 0
        || spec.file_duration_us > 1_800_000_000
        || spec.file_seek_us >= spec.file_duration_us
    {
        return Err(before_start(&sigy_service::Error::Acquisition(
            "invalid output media bounds",
        )));
    }
    let group = NativeAudioGroup::new().map_err(|error| before_start(&error))?;
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(spec.file_duration_us / 1_000_000 + 30);
    let input = recordings::audio::open_protected_pcm(directory, nonce)
        .await
        .map_err(|error| before_start(&error))?;
    let mut completion = None;
    let mut decoder_failure = None;
    let result = tokio::select! {
        result = Box::pin(run(executable, input, spec, &group, &mut completion, &mut decoder_failure)) => result,
        () = tokio::time::sleep_until(deadline) => Err("audio execution deadline exceeded".into()),
        interrupted = tokio::signal::ctrl_c() => {
            interrupted.map_err(|error| -> Failure { error.into() })
                .and_then(|()| Err("playback interrupted".into()))
        }
    };
    if result.is_err() {
        let _ = group.kill();
    }
    let operation_reason = result
        .as_ref()
        .err()
        .map_or_else(|| "audio completion".into(), ToString::to_string);
    let closure = group.finish().await.map_err(|error| OutputError {
        reason: format!("{operation_reason}; {error}"),
        native_closure: None,
        decoder: completion,
        decoder_failure: decoder_failure.clone(),
    });
    let closure = closure?;
    match result {
        Ok((decoder, ready, sink)) => Ok(Playback {
            decoder,
            output: OutputReport {
                ready,
                sink,
                native_closure: closure,
            },
        }),
        Err(error) => Err(OutputError {
            reason: error.to_string(),
            native_closure: Some(Box::new(closure)),
            decoder: completion,
            decoder_failure,
        }),
    }
}

fn before_start(error: &sigy_service::Error) -> OutputError {
    OutputError {
        reason: error.to_string(),
        native_closure: None,
        decoder: None,
        decoder_failure: None,
    }
}

async fn run(
    executable: &str,
    input: impl AsyncRead + Unpin,
    spec: &RetainedReadSpec,
    group: &NativeAudioGroup,
    observed_decoder: &mut Option<RetainedPlaybackReport>,
    decoder_failure: &mut Option<String>,
) -> Result<(RetainedPlaybackReport, Ready, Report), Failure> {
    let duration_us = spec.playback_duration_us()?;
    let limits = HelperLimits { duration_us }.validate()?;
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .arg("--internal-audio-output-v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut helper = group.spawn(command)?;
    let mut stdin = helper.stdin.take().ok_or("audio helper input missing")?;
    let stdout = helper.stdout.take().ok_or("audio helper output missing")?;
    let mut stdout = BufReader::with_capacity(protocol::MESSAGE_BYTES, stdout);
    let config = serde_json::to_vec(&limits)?;
    if config.len() > protocol::CONFIG_BYTES {
        return Err("audio config bound".into());
    }
    stdin
        .write_all(&u32::try_from(config.len())?.to_le_bytes())
        .await?;
    stdin.write_all(&config).await?;
    let ready = match tokio::time::timeout(
        Duration::from_millis(protocol::STARTUP_MS),
        message(&mut stdout),
    )
    .await??
    {
        Event::Ready(ready) => {
            ready.validate(limits)?;
            ready
        }
        Event::Report(report) => return Err(startup_failure(&report)?.into()),
    };
    let format = PcmFormat {
        rate_hz: ready.rate_hz,
        channels: ready.channels,
    };
    if format.maximum_bytes(duration_us)? != ready.maximum_pcm_bytes {
        return Err("audio helper PCM bound mismatch".into());
    }
    let (send, receive) = mpsc::channel(2);
    let decode = recordings::audio::decode_pcm_reader(
        PcmReaderRequest {
            executable,
            spec,
            format,
            group,
        },
        input,
        send,
    );
    let feed = forward(receive, stdin, &ready);
    let decode = async {
        match decode.await {
            Ok(decoded) => {
                *observed_decoder = Some(decoded.decoder);
                Ok(decoded)
            }
            Err(error) => {
                *decoder_failure = Some(error.to_string());
                Err::<_, Failure>(error.into())
            }
        }
    };
    let (decoded, bytes, report) = tokio::try_join!(decode, feed, final_report(&mut stdout),)?;
    let status = helper.wait().await?;
    if report.status == Status::Failed {
        return Err(output_failure(&report, &ready, spec.file_duration_us + 30_000_000)?.into());
    }
    if !status.success() {
        return Err("audio helper did not complete".into());
    }
    if bytes != decoded.pcm_bytes {
        return Err("audio PCM accounting mismatch".into());
    }
    validate_report(&report, &ready, bytes, spec.file_duration_us + 30_000_000)?;
    Ok((decoded.decoder, ready, report))
}

fn startup_failure(report: &Report) -> Result<String, Failure> {
    let code = report
        .error
        .as_deref()
        .filter(|code| super::helper::is_failure_code(code))
        .ok_or("audio helper startup report invalid")?;
    if report.protocol != 1
        || report.status != Status::Failed
        || report.audibility_proven
        || !report.presentation_is_estimated
        || report.decoded_frames != 0
        || report.content_frames != 0
        || report.callbacks != 0
        || report.underrun_frames != 0
        || report.drain_zero_frames != 0
        || report.queue_high_water_frames != 0
        || report.clipped_samples != 0
        || report.predicted_presentation_us.is_some()
    {
        return Err("audio helper startup report invalid".into());
    }
    Ok(format!(
        "Windows output refused ({code}). Check the system's default output; this profile requires floating-point mono or stereo at 8 to 192 kHz"
    ))
}

fn output_failure(report: &Report, ready: &Ready, work_us: u64) -> Result<String, Failure> {
    let code = report
        .error
        .as_deref()
        .filter(|code| super::helper::is_failure_code(code))
        .ok_or("audio helper failure report invalid")?;
    let frame_bytes = u64::from(ready.channels)
        .checked_mul(4)
        .filter(|bytes| *bytes > 0)
        .ok_or("audio helper failure profile invalid")?;
    let maximum_frames = work_us
        .checked_mul(u64::from(ready.rate_hz))
        .map(|frames| frames / 1_000_000)
        .ok_or("audio helper failure counter overflow")?;
    let maximum_samples = report
        .decoded_frames
        .checked_mul(u64::from(ready.channels))
        .ok_or("audio helper failure counter overflow")?;
    let submitted = report
        .content_frames
        .checked_add(report.underrun_frames)
        .and_then(|frames| frames.checked_add(report.drain_zero_frames))
        .ok_or("audio helper failure counter overflow")?;
    if report.protocol != 1
        || report.status != Status::Failed
        || !report.presentation_is_estimated
        || report.audibility_proven
        || !(1..=2).contains(&ready.channels)
        || !(8_000..=192_000).contains(&ready.rate_hz)
        || work_us == 0
        || work_us > 1_830_000_000
        || report.decoded_frames > ready.maximum_pcm_bytes / frame_bytes
        || report.content_frames > report.decoded_frames
        || report.clipped_samples > maximum_samples
        || report.queue_high_water_frames > u64::from(ready.ring_frames)
        || report.callbacks > maximum_frames
        || submitted > maximum_frames
        || report
            .predicted_presentation_us
            .is_some_and(|time| time == 0 || time > work_us)
    {
        return Err("audio helper failure report invalid".into());
    }
    Ok(format!("Windows output failed ({code})"))
}

async fn final_report(input: &mut (impl AsyncRead + Unpin)) -> Result<Report, Failure> {
    let Event::Report(report) = message(input).await? else {
        return Err("audio helper repeated handshake".into());
    };
    match input.read_u8().await {
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(report),
        Err(error) => Err(error.into()),
        Ok(_) => Err("audio helper emitted trailing output".into()),
    }
}

async fn message(input: &mut (impl AsyncRead + Unpin)) -> Result<Event, Failure> {
    let mut line = Vec::with_capacity(protocol::MESSAGE_BYTES);
    loop {
        let byte = input
            .read_u8()
            .await
            .map_err(|_| "audio helper message incomplete")?;
        if byte == b'\n' {
            break;
        }
        if line.len() == protocol::MESSAGE_BYTES {
            return Err("audio helper message bound".into());
        }
        line.push(byte);
    }
    Ok(serde_json::from_slice(&line)?)
}

async fn forward(
    mut input: mpsc::Receiver<Vec<u8>>,
    mut output: impl AsyncWrite + Unpin,
    ready: &Ready,
) -> Result<u64, Failure> {
    let frame_bytes = usize::from(ready.channels) * 4;
    let mut buffer = [0_u8; 8200];
    let mut carry = 0;
    let mut total = 0_u64;
    while let Some(chunk) = input.recv().await {
        if chunk.is_empty() || chunk.len() > 8192 {
            return Err("audio PCM chunk bound".into());
        }
        total = total
            .checked_add(u64::try_from(chunk.len())?)
            .filter(|bytes| *bytes <= ready.maximum_pcm_bytes)
            .ok_or("audio PCM lifetime bound")?;
        let used = carry + chunk.len();
        buffer[carry..used].copy_from_slice(&chunk);
        let aligned = used - used % frame_bytes;
        if aligned > 0 {
            output
                .write_all(&u32::try_from(aligned)?.to_le_bytes())
                .await?;
            output.write_all(&buffer[..aligned]).await?;
        }
        carry = used - aligned;
        buffer.copy_within(aligned..used, 0);
    }
    if carry != 0 || total == 0 {
        return Err("audio PCM input incomplete".into());
    }
    output.write_all(&0_u32.to_le_bytes()).await?;
    output.shutdown().await?;
    Ok(total)
}

fn validate_report(
    report: &Report,
    ready: &Ready,
    bytes: u64,
    work_us: u64,
) -> Result<(), Failure> {
    if !(1..=2).contains(&ready.channels)
        || !(8_000..=192_000).contains(&ready.rate_hz)
        || work_us == 0
        || work_us > 1_830_000_000
    {
        return Err("audio report profile bound".into());
    }
    let frame_bytes = u64::from(ready.channels) * 4;
    let maximum_frames = work_us
        .checked_mul(u64::from(ready.rate_hz))
        .map(|frames| frames / 1_000_000)
        .ok_or("audio output frame bound")?;
    let submitted = report
        .content_frames
        .checked_add(report.underrun_frames)
        .and_then(|frames| frames.checked_add(report.drain_zero_frames))
        .ok_or("audio output counter overflow")?;
    let maximum_clipped = report
        .decoded_frames
        .checked_mul(u64::from(ready.channels))
        .ok_or("audio sample counter overflow")?;
    if report.protocol != 1
        || report.status != Status::Drained
        || report.error.is_some()
        || !report.presentation_is_estimated
        || report.audibility_proven
        || bytes == 0
        || bytes > ready.maximum_pcm_bytes
        || !bytes.is_multiple_of(frame_bytes)
        || report.decoded_frames != bytes / frame_bytes
        || report.decoded_frames == 0
        || report.content_frames != report.decoded_frames
        || report.clipped_samples > maximum_clipped
        || report.callbacks == 0
        || report.callbacks > maximum_frames
        || submitted > maximum_frames
        || report.queue_high_water_frames == 0
        || report.queue_high_water_frames > u64::from(ready.ring_frames)
        || report
            .predicted_presentation_us
            .is_none_or(|time| time == 0 || time > work_us)
    {
        return Err("audio helper report contradicted output scope".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
