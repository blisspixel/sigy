//! Silent excerpts use the same checked PCM selection without opening a device.

use super::{
    AudioClosure, NativeAudioGroup, PcmDecodeRequest, PcmDecoded, PcmFormat, PcmReaderRequest,
};
use crate::{Error, Result, control::RetainedReadSpec, recordings::RetainedPlaybackReport};
use serde::Serialize;
use std::{fmt, path::Path, time::Duration};

#[derive(Debug, Serialize)]
pub struct ExcerptNullError {
    pub reason: String,
    pub operation_failure: Option<String>,
    pub completed_decoder: Option<RetainedPlaybackReport>,
    pub decoder_failure: Option<String>,
    pub native_closure: Option<AudioClosure>,
    pub native_failure: Option<String>,
}

impl fmt::Display for ExcerptNullError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{}; native decoder closure {}",
            self.reason,
            if self.native_closure.is_some() {
                "observed"
            } else {
                "unproven or not started"
            }
        )
    }
}

impl std::error::Error for ExcerptNullError {}

fn before_start(error: &Error) -> Box<ExcerptNullError> {
    Box::new(ExcerptNullError {
        reason: error.to_string(),
        operation_failure: Some(error.to_string()),
        completed_decoder: None,
        decoder_failure: None,
        native_closure: None,
        native_failure: None,
    })
}

#[derive(Debug, Serialize)]
pub struct ExcerptNullReport {
    pub format: PcmFormat,
    pub pcm_bytes: u64,
    pub native_closure: AudioClosure,
}

/// Decode and count an explicit excerpt with a fixed silent 48 kHz stereo profile.
/// # Errors
/// Refuses unavailable containment, invalid ranges, incomplete samples or unproven closure.
pub async fn decode_retained_excerpt_null(
    executable: &str,
    directory: &Path,
    nonce: &str,
    spec: &RetainedReadSpec,
) -> std::result::Result<(RetainedPlaybackReport, ExcerptNullReport), Box<ExcerptNullError>> {
    if spec.excerpt.is_none() || spec.file_duration_us > 1_800_000_000 {
        return Err(before_start(&Error::Acquisition(
            "explicit PCM excerpt required",
        )));
    }
    let format = PcmFormat {
        rate_hz: 48_000,
        channels: 2,
    };
    format
        .excerpt_frames(
            spec.file_seek_us,
            spec.playback_end_us()
                .map_err(|error| before_start(&error))?,
        )
        .map_err(|error| before_start(&error))?;
    let group = NativeAudioGroup::for_decoder().map_err(|error| before_start(&error))?;
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(spec.file_duration_us / 1_000_000 + 35);
    let (send, mut receive) = tokio::sync::mpsc::channel(2);
    let decode = super::decode_retained_pcm(
        PcmDecodeRequest {
            reader: PcmReaderRequest {
                executable,
                spec,
                format,
                group: &group,
            },
            directory,
            nonce,
        },
        send,
    );
    let consume = async {
        let mut bytes = 0_u64;
        while let Some(chunk) = receive.recv().await {
            bytes = bytes
                .checked_add(
                    u64::try_from(chunk.len())
                        .map_err(|_| Error::Acquisition("silent PCM overflow"))?,
                )
                .ok_or(Error::Acquisition("silent PCM overflow"))?;
        }
        Ok::<_, Error>(bytes)
    };
    let mut completed_decoder = None;
    let mut decoder_failure = None;
    let work = async {
        let observe = async {
            match decode.await {
                Ok(decoded) => {
                    completed_decoder = Some(decoded.decoder);
                    Ok(decoded)
                }
                Err(error) => {
                    decoder_failure = Some(error.to_string());
                    Err(error)
                }
            }
        };
        tokio::try_join!(observe, consume)
    };
    let result = tokio::select! {
        result = work => result,
        () = tokio::time::sleep_until(deadline) => Err(Error::Acquisition("silent excerpt deadline exceeded")),
        interrupted = tokio::signal::ctrl_c() => {
            interrupted.map_err(Error::from)
                .and_then(|()| Err(Error::Acquisition("silent excerpt interrupted")))
        }
    };
    if result.is_err() {
        let _ = group.kill();
    }
    finish_outcome(
        format,
        result,
        completed_decoder,
        decoder_failure,
        group.finish().await,
    )
}

fn finish_outcome(
    format: PcmFormat,
    result: Result<(PcmDecoded, u64)>,
    completed_decoder: Option<RetainedPlaybackReport>,
    decoder_failure: Option<String>,
    closure: Result<AudioClosure>,
) -> std::result::Result<(RetainedPlaybackReport, ExcerptNullReport), Box<ExcerptNullError>> {
    let result = result.and_then(|(decoded, bytes)| {
        if bytes != decoded.pcm_bytes {
            return Err(Error::Acquisition("silent PCM accounting mismatch"));
        }
        Ok((decoded, bytes))
    });
    let reason = result
        .as_ref()
        .err()
        .map_or_else(|| "silent PCM completed".into(), ToString::to_string);
    let operation_failure = result.as_ref().err().map(ToString::to_string);
    let (native_closure, native_failure) = match closure {
        Ok(proof) => (Some(proof), None),
        Err(error) => (None, Some(error.to_string())),
    };
    match (result, native_closure) {
        (Ok((decoded, bytes)), Some(native_closure)) => Ok((
            decoded.decoder,
            ExcerptNullReport {
                format,
                pcm_bytes: bytes,
                native_closure,
            },
        )),
        (_, native_closure) => Err(Box::new(ExcerptNullError {
            reason,
            operation_failure,
            completed_decoder,
            decoder_failure,
            native_closure,
            native_failure,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_failure_keeps_decoder_and_native_proof_independent() -> Result<()> {
        let format = PcmFormat {
            rate_hz: 48_000,
            channels: 2,
        };
        let decoder = RetainedPlaybackReport {
            file_playhead_us: 375_000,
            reported_elapsed_us: 250_000,
            boundary_tolerance_us: 21,
            progress_advanced: true,
        };
        let completion = PcmDecoded {
            decoder,
            pcm_bytes: 96_000,
        };
        let error = finish_outcome(
            format,
            Ok((completion, 96_000)),
            Some(decoder),
            None,
            Err(Error::Acquisition("native closure unproven")),
        )
        .err()
        .ok_or(Error::StorageIntegrity)?;
        assert!(error.completed_decoder.is_some());
        assert!(error.decoder_failure.is_none() && error.native_closure.is_none());
        assert!(
            error
                .native_failure
                .as_deref()
                .is_some_and(|reason| reason.contains("native closure unproven"))
        );
        let closure = AudioClosure {
            mechanism: "fixture-empty-group".into(),
            peak_memory_bytes: None,
            cpu_time_us: None,
        };
        let error = finish_outcome(
            format,
            Err(Error::Acquisition("short input")),
            None,
            Some("short input".into()),
            Ok(closure),
        )
        .err()
        .ok_or(Error::StorageIntegrity)?;
        assert!(error.completed_decoder.is_none() && error.native_closure.is_some());
        assert_eq!(error.decoder_failure.as_deref(), Some("short input"));
        assert!(error.native_failure.is_none());
        let error = finish_outcome(
            format,
            Err(Error::Acquisition("cancelled after decode")),
            Some(decoder),
            None,
            Err(Error::Acquisition("native closure unproven")),
        )
        .err()
        .ok_or(Error::StorageIntegrity)?;
        assert!(error.completed_decoder.is_some() && error.decoder_failure.is_none());
        assert!(error.reason.contains("cancelled after decode"));
        Ok(())
    }
}
