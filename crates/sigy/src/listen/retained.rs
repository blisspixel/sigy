//! Protected retained transfer and client decoding have separate outcomes.

use clap::Subcommand;
use sigy_service::{
    control::{self, Operation, RetainedOperation, RetainedPage, RetainedReadView},
    recordings::{self, PlaybackDestination, RetainedPlaybackReport},
};
use std::{io::Write, path::Path, time::Duration};

type Failure = Box<dyn std::error::Error>;

#[derive(Debug, Subcommand)]
pub enum ReaderCommand {
    /// Show a protected-reader receipt without reopening its stream.
    Show { id: String },
    /// Request stop for this exact reader generation; closure may still be pending.
    Stop {
        id: String,
        #[arg(long)]
        generation: u64,
    },
    /// List bounded stored reader receipts without starting playback.
    List,
}

pub(super) fn reader_id(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
    {
        return Err("Reader IDs use 1..128 ASCII letters, digits, -, _, : or .".into());
    }
    Ok(value.into())
}

pub(super) struct PlayResult {
    receipt: RetainedReadView,
    destination: PlaybackDestination,
    decoder: Option<RetainedPlaybackReport>,
    decoder_failure: Option<String>,
    output_failure: Option<String>,
    native_failure: Option<String>,
    operation_failure: Option<String>,
    audio_output: Option<serde_json::Value>,
    replayed: bool,
}

pub(super) async fn inspect(
    directory: &Path,
    command: &ReaderCommand,
    json: bool,
) -> Result<(), Failure> {
    let operation = match command {
        ReaderCommand::Show { id } => RetainedOperation::Show { id: id.clone() },
        ReaderCommand::Stop { id, generation } => RetainedOperation::Stop {
            id: id.clone(),
            generation: *generation,
        },
        ReaderCommand::List => RetainedOperation::List {},
    };
    let snapshot = super::view(directory, Operation::Retained { command: operation }).await?;
    let page = snapshot.retained.ok_or("reader receipt missing")?;
    render_page(&mut std::io::stdout().lock(), &page, json)
}

pub(super) async fn play(
    directory: &Path,
    recording: &str,
    destination: PlaybackDestination,
    seek_us: u64,
    request: Option<&str>,
) -> Result<PlayResult, Failure> {
    play_scoped(
        directory,
        Scope::File {
            recording,
            seek_us,
            end_us: None,
        },
        destination,
        request,
    )
    .await
}

#[derive(Clone, Copy)]
pub(super) enum Scope<'a> {
    File {
        recording: &'a str,
        seek_us: u64,
        end_us: Option<u64>,
    },
    Finding {
        monitor: &'a str,
        finding: &'a str,
    },
}

impl Scope<'_> {
    fn operation(self, id: String) -> Result<RetainedOperation, Failure> {
        Ok(match self {
            Self::File {
                recording,
                seek_us,
                end_us: Some(end_us),
            } => {
                if seek_us >= end_us {
                    return Err("excerpt requires --seek-us less than --end-us".into());
                }
                RetainedOperation::StartRange {
                    id,
                    recording_id: recording.into(),
                    seek_us,
                    end_us,
                }
            }
            Self::File {
                recording,
                seek_us,
                end_us: None,
            } => RetainedOperation::Start {
                id,
                recording_id: recording.into(),
                seek_us,
            },
            Self::Finding { monitor, finding } => RetainedOperation::StartFinding {
                id,
                monitor_id: monitor.into(),
                finding_id: finding.into(),
            },
        })
    }

    fn validate(self, receipt: &RetainedReadView) -> Result<(), Failure> {
        let spec = &receipt.spec;
        let expected = match self {
            Self::File {
                recording,
                seek_us,
                end_us,
            } => {
                if spec.recording_id != recording || receipt.seek_us != seek_us {
                    return Err("reader receipt scope mismatch".into());
                }
                match (end_us, spec.excerpt.as_ref()) {
                    (None, None) => {}
                    (Some(end), Some(excerpt))
                        if excerpt.version == 2
                            && excerpt.timeline_end_us == end
                            && excerpt.citation.is_none() => {}
                    _ => return Err("reader receipt excerpt mode/end mismatch".into()),
                }
                seek_us
            }
            Self::Finding { monitor, finding } => {
                let excerpt = spec
                    .excerpt
                    .as_ref()
                    .ok_or("reader receipt finding scope missing")?;
                let citation = excerpt
                    .citation
                    .as_ref()
                    .ok_or("reader receipt finding citation missing")?;
                if excerpt.version != 2
                    || citation.monitor_id != monitor
                    || citation.finding_id != finding
                    || citation.transcript_id.is_empty()
                    || citation.transcript_revision < 0
                    || citation.translation_revision < 0
                {
                    return Err("reader receipt finding identity mismatch".into());
                }
                receipt.seek_us
            }
        };
        validate_media(receipt, expected)?;
        spec.playback_end_us()?;
        spec.playback_duration_us()?;
        Ok(())
    }
}

pub(super) async fn play_scoped(
    directory: &Path,
    scope: Scope<'_>,
    destination: PlaybackDestination,
    request: Option<&str>,
) -> Result<PlayResult, Failure> {
    // Validate caller syntax before decoder configuration, including replay conflicts.
    scope.operation("scope-check".into())?;
    if let Some(replay) = existing_replay(directory, scope, destination, request).await? {
        return Ok(replay);
    }
    let decoder = super::decoder_path(directory).await?;
    let id = request
        .map(str::to_owned)
        .map_or_else(recordings::new_retained_request_id, Ok)?;
    let snapshot = control::request(directory, Operation::Retained {
        command: scope.operation(id.clone())?,
    }).await.map_err(|error| {
        format!("{}; admission outcome may be unresolved. Inspect `sigy listen reader show {id}` before retrying.", super::clean(&error.to_string()))
    })?;
    let admission = snapshot
        .retained
        .as_ref()
        .ok_or("reader admission omitted receipt".into())
        .and_then(|page| one(page, &id));
    let receipt = match admission {
        Ok(receipt) => receipt,
        Err(error) => {
            return Err(refuse_reply(directory, &id, scope, &error.to_string()).await);
        }
    };
    let Some(page) = snapshot.retained else {
        return Err(unresolved(&id, "admission receipt missing").into());
    };
    if page.newly_started == Some(false) {
        scope.validate(&receipt)?;
        return Ok(receipt_replay(receipt, destination));
    }
    let played = match scope.validate(&receipt) {
        Err(error) => Err(error),
        Ok(()) if page.newly_started != Some(true) || receipt.spec.generation != 1 => {
            Err("reader admission omitted or contradicted fresh generation".into())
        }
        Ok(()) => Box::pin(decode(directory, &decoder, destination, *page, &receipt)).await,
    };
    let closed = close_reader(directory, &receipt).await?;
    let observed = match played {
        Ok((report, output)) => DecodeObservation {
            decoder: Some(report),
            decoder_failure: None,
            output_failure: None,
            native_failure: None,
            operation_failure: None,
            audio_output: output,
        },
        Err(error) => failed_decode(error.as_ref()),
    };
    Ok(PlayResult {
        receipt: closed,
        destination,
        decoder: observed.decoder,
        decoder_failure: observed.decoder_failure,
        output_failure: observed.output_failure,
        native_failure: observed.native_failure,
        operation_failure: observed.operation_failure,
        audio_output: observed.audio_output,
        replayed: false,
    })
}

async fn existing_replay(
    directory: &Path,
    scope: Scope<'_>,
    destination: PlaybackDestination,
    request: Option<&str>,
) -> Result<Option<PlayResult>, Failure> {
    if let Some(id) = request {
        match read_page(directory, id).await {
            Ok(page) => {
                let receipt = one(&page, id)?;
                scope.validate(&receipt)?;
                return Ok(Some(receipt_replay(receipt, destination)));
            }
            Err(error) if is_not_found(error.as_ref()) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn receipt_replay(receipt: RetainedReadView, destination: PlaybackDestination) -> PlayResult {
    PlayResult {
        receipt,
        destination,
        decoder: None,
        decoder_failure: None,
        output_failure: None,
        native_failure: None,
        operation_failure: None,
        audio_output: None,
        replayed: true,
    }
}

async fn close_reader(
    directory: &Path,
    receipt: &RetainedReadView,
) -> Result<RetainedReadView, Failure> {
    let id = &receipt.spec.request_id;
    // A decoder ending never proves the service's original-file reader closed.
    let stopped = tokio::time::timeout(
        Duration::from_secs(15),
        control::request(
            directory,
            Operation::Retained {
                command: RetainedOperation::Stop {
                    id: id.clone(),
                    generation: 1,
                },
            },
        ),
    )
    .await
    .map_err(|error| error.to_string())
    .and_then(|result| result.map_err(|error| error.to_string()));
    wait_closed(directory, id, &receipt.spec)
        .await
        .map_err(|error| {
            let stop_note = stopped.err().map_or_else(String::new, |failure| {
                format!("; stop request: {}", super::clean(&failure))
            });
            format!("{error}{stop_note}")
        })
        .map_err(Into::into)
}

async fn refuse_reply(directory: &Path, id: &str, scope: Scope<'_>, detail: &str) -> Failure {
    // A successful but malformed Start reply cannot become a decoder capability.
    // All new request generations are 1; never use an untrusted foreign entry ID.
    let _ = tokio::time::timeout(
        Duration::from_secs(15),
        control::request(
            directory,
            Operation::Retained {
                command: RetainedOperation::Stop {
                    id: id.into(),
                    generation: 1,
                },
            },
        ),
    )
    .await;
    let cleanup = async {
        let page = read_page(directory, id).await?;
        let receipt = one(&page, id)?;
        scope.validate(&receipt)?;
        if receipt.spec.generation != 1 {
            return Err("unexpected reader generation".into());
        }
        wait_closed(directory, id, &receipt.spec).await
    };
    let closure = match tokio::time::timeout(Duration::from_secs(15), cleanup).await {
        Ok(Ok(receipt)) => format!("original reader closed ({})", super::clean(&receipt.state)),
        _ => unresolved(id, "malformed admission reply"),
    };
    format!(
        "{}; {closure}. Inspect `sigy listen reader show {id}`.",
        super::clean(detail)
    )
    .into()
}

fn is_not_found(error: &(dyn std::error::Error + 'static)) -> bool {
    match error.downcast_ref::<sigy_service::Error>() {
        Some(sigy_service::Error::NotFound) => true,
        Some(sigy_service::Error::Remote(message)) => {
            message == &sigy_service::Error::NotFound.to_string()
        }
        _ => false,
    }
}

fn one(page: &RetainedPage, id: &str) -> Result<RetainedReadView, Failure> {
    if page.entries.len() != 1 || page.entries[0].spec.request_id != id {
        return Err("reader receipt identity mismatch".into());
    }
    Ok(page.entries[0].clone())
}

#[cfg(test)]
fn validate_scope(
    receipt: &RetainedReadView,
    recording: &str,
    seek_us: u64,
) -> Result<(), Failure> {
    Scope::File {
        recording,
        seek_us,
        end_us: None,
    }
    .validate(receipt)
}

fn validate_media(receipt: &RetainedReadView, seek_us: u64) -> Result<(), Failure> {
    let spec = &receipt.spec;
    if receipt.seek_us != seek_us
        || spec.generation == 0
        || spec.timeline_start_us.checked_add(spec.file_seek_us) != Some(seek_us)
        || spec.timeline_end_us.checked_sub(spec.timeline_start_us) != Some(spec.file_duration_us)
        || spec.file_seek_us >= spec.file_duration_us
        || spec.file_duration_us > 30 * 60 * 1_000_000
        || spec.bytes == 0
        || spec.bytes > 512 * 1024 * 1024
        || !sigy_service::storage::dvr::retained_format(&spec.format)
    {
        return Err("reader receipt scope mismatch".into());
    }
    Ok(())
}

async fn decode(
    directory: &Path,
    executable: &str,
    destination: PlaybackDestination,
    page: RetainedPage,
    receipt: &RetainedReadView,
) -> Result<(RetainedPlaybackReport, Option<serde_json::Value>), Failure> {
    let nonce = ready_nonce(directory, page, receipt).await?;
    if destination == PlaybackDestination::Null && receipt.spec.excerpt.is_some() {
        let (decoder, report) = Box::pin(recordings::audio::decode_retained_excerpt_null(
            executable,
            directory,
            &nonce,
            &receipt.spec,
        ))
        .await
        .map_err(|error| -> Failure { error })?;
        return Ok((decoder, Some(serde_json::to_value(report)?)));
    }
    #[cfg(not(windows))]
    if destination == PlaybackDestination::System && receipt.spec.excerpt.is_some() {
        return Err("bounded excerpt system-output adapter is unavailable on this platform; use --destination null where native containment is available".into());
    }
    #[cfg(windows)]
    if destination == PlaybackDestination::System {
        let playback = Box::pin(crate::audio::play::retained(
            executable,
            directory,
            &nonce,
            &receipt.spec,
        ))
        .await?;
        return Ok((
            playback.decoder,
            Some(serde_json::to_value(playback.output)?),
        ));
    }
    let deadline = tokio::time::Instant::now()
        .checked_add(Duration::from_secs(
            receipt.spec.file_duration_us / 1_000_000 + 35,
        ))
        .ok_or("invalid retained decoder deadline")?;
    tokio::select! {
        result = Box::pin(recordings::play_retained_stream(executable, directory, &nonce, &receipt.spec, destination)) => Ok((result?, None)),
        () = tokio::time::sleep_until(deadline) => Err("retained decoder connection/execution deadline exceeded".into()),
        interrupted = tokio::signal::ctrl_c() => {
            interrupted?;
            Err("playback interrupted; stopping protected reader".into())
        }
    }
}

struct DecodeObservation {
    decoder: Option<RetainedPlaybackReport>,
    decoder_failure: Option<String>,
    output_failure: Option<String>,
    native_failure: Option<String>,
    operation_failure: Option<String>,
    audio_output: Option<serde_json::Value>,
}

fn failed_decode(error: &(dyn std::error::Error + 'static)) -> DecodeObservation {
    if let Some(null) = error.downcast_ref::<recordings::audio::ExcerptNullError>() {
        return DecodeObservation {
            decoder: null.completed_decoder,
            decoder_failure: null
                .decoder_failure
                .as_ref()
                .map(|reason| super::clean(reason)),
            native_failure: null
                .native_failure
                .as_ref()
                .map(|reason| super::clean(reason)),
            operation_failure: null
                .operation_failure
                .as_ref()
                .map(|reason| super::clean(reason)),
            output_failure: None,
            audio_output: serde_json::to_value(null).ok(),
        };
    }
    #[cfg(windows)]
    if let Some(output) = error.downcast_ref::<crate::audio::play::OutputError>() {
        return DecodeObservation {
            decoder: output.decoder,
            decoder_failure: output
                .decoder_failure
                .as_ref()
                .map(|reason| super::clean(reason)),
            output_failure: Some(super::clean(&output.to_string())),
            native_failure: None,
            operation_failure: None,
            audio_output: serde_json::to_value(output).ok(),
        };
    }
    DecodeObservation {
        decoder: None,
        decoder_failure: Some(super::clean(&error.to_string())),
        output_failure: None,
        native_failure: None,
        operation_failure: None,
        audio_output: None,
    }
}

async fn ready_nonce(
    directory: &Path,
    mut page: RetainedPage,
    receipt: &RetainedReadView,
) -> Result<String, Failure> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let current = one(&page, &receipt.spec.request_id)?;
        if current.spec != receipt.spec || current.state != "running" {
            return Err("protected reader is no longer ready for this generation".into());
        }
        if let Some(nonce) = page.pipe_nonce {
            return Ok(nonce);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("protected reader pipe timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        page = tokio::time::timeout_at(deadline, read_page(directory, &receipt.spec.request_id))
            .await
            .map_err(|_| "protected reader pipe timed out")??;
    }
}

async fn read_page(directory: &Path, id: &str) -> Result<RetainedPage, Failure> {
    let snapshot = super::view(
        directory,
        Operation::Retained {
            command: RetainedOperation::Show { id: id.into() },
        },
    )
    .await?;
    snapshot
        .retained
        .map(|page| *page)
        .ok_or_else(|| "reader receipt missing".into())
}

async fn wait_closed(
    directory: &Path,
    id: &str,
    expected: &control::RetainedReadSpec,
) -> Result<RetainedReadView, Failure> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let page = tokio::time::timeout_at(deadline, read_page(directory, id))
            .await
            .map_err(|_| unresolved(id, "inspection deadline"))?
            .map_err(|error| unresolved(id, &super::clean(&error.to_string())))?;
        let receipt = one(&page, id)?;
        if &receipt.spec != expected {
            return Err(unresolved(id, "reader identity/spec changed").into());
        }
        match receipt.state.as_str() {
            "completed" | "failed" => return Ok(receipt),
            "running" | "cancelling" if tokio::time::Instant::now() < deadline => {},
            _ => return Err(format!("Original-media protection remains unresolved ({}). Inspect with `sigy listen reader show {}`.", super::clean(&receipt.state), super::clean(id)).into()),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn unresolved(id: &str, detail: &str) -> String {
    format!(
        "Reader closure is unproven; original-media protection may remain held ({detail}). Inspect `sigy listen reader show {}`.",
        super::clean(id)
    )
}

pub(super) fn write_play(
    json: bool,
    result: &PlayResult,
    session: Option<&str>,
    detached: bool,
) -> Result<(), Failure> {
    render_play(
        &mut std::io::stdout().lock(),
        result,
        session,
        detached,
        json,
    )?;
    if let Some(failure) = &result.decoder_failure {
        return Err(failure.clone().into());
    }
    if let Some(failure) = &result.output_failure {
        return Err(failure.clone().into());
    }
    if let Some(failure) = &result.native_failure {
        return Err(failure.clone().into());
    }
    if let Some(failure) = &result.operation_failure {
        return Err(failure.clone().into());
    }
    check_outcome(result)?;
    Ok(())
}

fn check_outcome(result: &PlayResult) -> Result<(), Failure> {
    if !result.replayed
        && result.receipt.state == "failed"
        && (result.decoder.is_none()
            || !matches!(
                result.receipt.completion_reason.as_deref(),
                Some("cancelled" | "retained-pipe-failed")
            ))
    {
        return Err(format!(
            "Reader failed independently of decoder progress: {}.",
            super::clean(
                result
                    .receipt
                    .completion_reason
                    .as_deref()
                    .unwrap_or("unknown outcome")
            )
        )
        .into());
    }
    Ok(())
}

fn render_play(
    output: &mut impl Write,
    result: &PlayResult,
    session: Option<&str>,
    detached: bool,
    json: bool,
) -> Result<(), Failure> {
    let spec = &result.receipt.spec;
    let requested_end_us = spec
        .timeline_start_us
        .checked_add(spec.playback_end_us()?)
        .ok_or("requested timeline end overflow")?;
    let playhead_us = result
        .decoder
        .as_ref()
        .map(|report| {
            spec.timeline_start_us
                .checked_add(report.file_playhead_us)
                .ok_or("reported timeline progress overflow")
        })
        .transpose()?;
    if json {
        serde_json::to_writer(
            &mut *output,
            &serde_json::json!({
                "id": session.unwrap_or(&spec.recording_id), "recording_id": spec.recording_id,
                "request_id": spec.request_id, "generation": spec.generation,
                "destination": result.destination.as_str(), "seek_us": result.receipt.seek_us,
                "requested_end_us": requested_end_us, "playhead_us": playhead_us,
                "segment_ordinal": spec.ordinal, "replayed": result.replayed,
                "detached": session.map(|_| detached), "reader": result.receipt,
                "decoder_completed": result.decoder.is_some(), "decoder_failure": result.decoder_failure,
                "audio_output": result.audio_output, "output_failure": result.output_failure,
                "native_failure": result.native_failure, "operation_failure": result.operation_failure,
                "reported_elapsed_us": result.decoder.as_ref().map(|report| report.reported_elapsed_us),
                "boundary_tolerance_us": result.decoder.as_ref().map(|report| report.boundary_tolerance_us),
                "progress_advanced": result.decoder.as_ref().map(|report| report.progress_advanced),
            }),
        )?;
        writeln!(output)?;
    } else {
        writeln!(
            output,
            "Reader {} generation {}: {}. {}",
            super::clean(&spec.request_id),
            spec.generation,
            super::clean(&result.receipt.state),
            if result.replayed {
                "Receipt replay; no audio started."
            } else {
                "Original-file reader closed."
            }
        )?;
        if let Some(excerpt) = &spec.excerpt {
            writeln!(
                output,
                "Requested half-open excerpt [{}..{}) us; original object ends at {} us.",
                result.receipt.seek_us, requested_end_us, spec.timeline_end_us
            )?;
            if let Some(citation) = &excerpt.citation {
                writeln!(
                    output,
                    "Exact finding: {} / {}.",
                    super::clean(&citation.monitor_id),
                    super::clean(&citation.finding_id)
                )?;
            }
        }
        if let Some(position) = playhead_us {
            writeln!(
                output,
                "Decoder-reported timeline progress {position} us; requested {}..{} us. This does not prove audible output.",
                result.receipt.seek_us, requested_end_us
            )?;
        }
        render_decode_outcomes(output, result)?;
        if let Some(reason) = &result.receipt.completion_reason {
            writeln!(output, "Reader outcome: {}", super::clean(reason))?;
        }
        if session.is_some() {
            writeln!(output, "Playhead detach confirmed: {detached}.")?;
        }
    }
    Ok(())
}

fn render_decode_outcomes(output: &mut impl Write, result: &PlayResult) -> Result<(), Failure> {
    if let Some(failure) = &result.decoder_failure {
        writeln!(output, "Decoder failed: {}", super::clean(failure))?;
    }
    if let Some(failure) = &result.native_failure {
        writeln!(output, "Native closure failed: {}", super::clean(failure))?;
    }
    if let Some(failure) = &result.operation_failure {
        writeln!(
            output,
            "Playback operation failed: {}",
            super::clean(failure)
        )?;
    }
    if result.destination == PlaybackDestination::Null
        && (result.operation_failure.is_some() || result.native_failure.is_some())
    {
        let observed = result
            .audio_output
            .as_ref()
            .and_then(|value| value.get("native_closure"))
            .is_some_and(|value| !value.is_null());
        writeln!(
            output,
            "Native decoder closure {}.",
            if observed {
                "observed"
            } else {
                "unproven or not started"
            }
        )?;
    }
    if let Some(failure) = &result.output_failure {
        writeln!(output, "Output failed: {}", super::clean(failure))?;
    } else if result.audio_output.is_some()
        && result.destination == PlaybackDestination::Null
        && result.native_failure.is_none()
        && result.operation_failure.is_none()
    {
        writeln!(
            output,
            "Silent PCM counted; no device output requested. Native group closure observed."
        )?;
    } else if result.audio_output.is_some() && result.destination == PlaybackDestination::System {
        writeln!(
            output,
            "Output frames submitted; final presentation timing is estimated. Native group closure observed."
        )?;
    }
    Ok(())
}

fn render_page(output: &mut impl Write, page: &RetainedPage, json: bool) -> Result<(), Failure> {
    if json {
        serde_json::to_writer(&mut *output, page)?;
        writeln!(output)?;
    } else {
        if page.entries.is_empty() {
            writeln!(output, "No retained-reader receipts.")?;
        }
        for receipt in &page.entries {
            writeln!(
                output,
                "Reader {} generation {}: {} | recording {} | {}..{} us",
                super::clean(&receipt.spec.request_id),
                receipt.spec.generation,
                super::clean(&receipt.state),
                super::clean(&receipt.spec.recording_id),
                receipt.seek_us,
                receipt
                    .spec
                    .timeline_start_us
                    .checked_add(receipt.spec.playback_end_us()?)
                    .ok_or("requested timeline end overflow")?
            )?;
            if let Some(reason) = &receipt.recovery_reason {
                writeln!(output, "Protection held: {}", super::clean(reason))?;
            }
            if let Some(reason) = &receipt.completion_reason {
                writeln!(output, "Reader outcome: {}", super::clean(reason))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
