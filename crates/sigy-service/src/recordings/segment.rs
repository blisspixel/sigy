//! Seal one running capture into ordered files without a second upstream request.

use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::Instant,
};

#[cfg(unix)]
use super::sync_directory;
use super::{
    checked_directory, object_path,
    seal::{SealAction, SealReply},
};
use crate::{
    Error, Result,
    sources::http::{BodyEvent, TransferEnd},
    storage::{
        captures::CaptureVersion,
        dvr::{OPEN_SEGMENT_CEILING, Publication, SEGMENT_RECEIVE_WINDOW, hex},
    },
};

use super::CaptureRequest;

pub(super) fn enabled(
    transport: super::CaptureTransport,
    limits: &crate::sources::http::AcquisitionLimits,
) -> bool {
    transport == super::CaptureTransport::Direct
        && !limits.clean_end()
        && limits.bytes() >= OPEN_SEGMENT_CEILING
}

pub(super) async fn run<C, Fut>(
    request: CaptureRequest,
    stop: tokio::sync::watch::Receiver<bool>,
    catalog: C,
) -> Result<Publication>
where
    C: Fn(CaptureVersion, SealAction) -> Fut + Send,
    Fut: std::future::Future<Output = Result<SealReply>> + Send,
{
    let CaptureRequest {
        directory,
        source,
        limits,
        decoder,
        acquirer,
        icy,
        token,
        ..
    } = request;
    let media = checked_directory(&directory, true)?;
    let mut body = acquirer
        .open_recording(
            &source,
            limits,
            &stop,
            if icy {
                crate::sources::icy::MetadataPolicy::Requested
            } else {
                crate::sources::icy::MetadataPolicy::Off
            },
        )
        .await?;
    let mut token = ready(&catalog, token, SealAction::Connected).await?;
    let format = body.content().format_name();
    let mut aggregate = Sha256::new();
    let mut total = 0_u64;
    let mut decoded_total = 0_u64;
    let end_reason = loop {
        let opened = catalog(token.clone(), SealAction::Open).await?;
        let (key, ceiling) = match opened {
            SealReply::Opened {
                version,
                object_key,
                ceiling,
                ordinal,
            } => {
                if ordinal >= 1024 {
                    return Err(Error::StorageIntegrity);
                }
                token = version;
                (object_key, ceiling)
            }
            SealReply::BudgetHeld => break "byte_limit",
            SealReply::Ready(_) => return Err(Error::StorageIntegrity),
        };
        let cut = read_segment(&mut body, &media, &key, ceiling).await?;
        if cut.bytes == 0 {
            ready(&catalog, token, SealAction::ReleaseOpen).await?;
            break finish_name(&cut.reason)?;
        }
        let (sha, decoded) =
            publish_file(&media, &key, &decoder, format, cut.bytes, &mut aggregate).await?;
        total = total
            .checked_add(cut.bytes)
            .ok_or(Error::StorageIntegrity)?;
        decoded_total = decoded_total
            .checked_add(decoded)
            .ok_or(Error::StorageIntegrity)?;
        token = ready(
            &catalog,
            token,
            SealAction::Seal {
                bytes: cut.bytes,
                sha256: sha,
                format,
                decoded_microseconds: decoded,
            },
        )
        .await?;
        match cut.reason {
            CutReason::Window | CutReason::Ceiling => {
                token = match catalog(token.clone(), SealAction::Renew).await {
                    Ok(SealReply::Ready(version)) => version,
                    Ok(_) => return Err(Error::StorageIntegrity),
                    Err(Error::RequestState) => break "duration_limit",
                    Err(error) => return Err(error),
                };
            }
            CutReason::Ended(end) => break stored_end(end),
        }
    };
    if total == 0 {
        return Err(empty_capture(end_reason));
    }
    let route = body.route().to_vec();
    let observations = body.observations(end_reason == "end_of_body", total)?;
    Ok(Publication {
        bytes: total,
        sha256: hex(&aggregate.finalize()),
        format,
        decoded_microseconds: decoded_total,
        end_reason,
        http_route: route,
        observations,
        segments_sealed: true,
        gap: None,
    })
}

async fn ready<C, Fut>(
    catalog: &C,
    token: CaptureVersion,
    action: SealAction,
) -> Result<CaptureVersion>
where
    C: Fn(CaptureVersion, SealAction) -> Fut + Send,
    Fut: std::future::Future<Output = Result<SealReply>> + Send,
{
    match catalog(token, action).await? {
        SealReply::Ready(version) => Ok(version),
        SealReply::Opened { ordinal, .. } => {
            let _ = ordinal;
            Err(Error::StorageIntegrity)
        }
        SealReply::BudgetHeld => Err(Error::StorageIntegrity),
    }
}

fn finish_name(reason: &CutReason) -> Result<&'static str> {
    match reason {
        CutReason::Ended(end) => Ok(stored_end(*end)),
        CutReason::Window | CutReason::Ceiling => Err(Error::StorageIntegrity),
    }
}

const fn stored_end(end: TransferEnd) -> &'static str {
    match end {
        TransferEnd::EndOfBody => "end_of_body",
        TransferEnd::ByteLimit => "byte_limit",
        TransferEnd::DurationLimit => "duration_limit",
        TransferEnd::UserStop => "user_stop",
    }
}

fn empty_capture(end_reason: &str) -> Error {
    Error::Acquisition(match end_reason {
        "duration_limit" => "empty timed recording",
        "user_stop" => "stopped before receiving audio",
        _ => "empty body",
    })
}

struct Cut {
    bytes: u64,
    reason: CutReason,
}

enum CutReason {
    Window,
    Ceiling,
    Ended(TransferEnd),
}

async fn read_segment(
    body: &mut crate::sources::http::RecordingBody,
    media: &Path,
    key: &str,
    ceiling: u64,
) -> Result<Cut> {
    let mut written = 0_u64;
    let mut started: Option<Instant> = None;
    let mut file = None;
    let reason = loop {
        if written == ceiling {
            break CutReason::Ceiling;
        }
        let window = started.map(|start| start + SEGMENT_RECEIVE_WINDOW);
        match body.pull(ceiling - written, window).await? {
            BodyEvent::Audio(bytes) if bytes.is_empty() => {}
            BodyEvent::Audio(bytes) => {
                if started.is_none() {
                    started = Some(Instant::now());
                }
                if file.is_none() {
                    file = Some(create_part(media, key).await?);
                }
                let handle = file.as_mut().ok_or(Error::StorageIntegrity)?;
                handle.write_all(&bytes).await?;
                written = written
                    .checked_add(u64::try_from(bytes.len()).map_err(|_| Error::StorageIntegrity)?)
                    .ok_or(Error::StorageIntegrity)?;
            }
            BodyEvent::Window => break CutReason::Window,
            BodyEvent::Ended(end) => break CutReason::Ended(end),
        }
    };
    sync_part(file).await?;
    Ok(Cut {
        bytes: written,
        reason,
    })
}

async fn sync_part(file: Option<tokio::fs::File>) -> Result<()> {
    if let Some(mut handle) = file {
        handle.flush().await?;
        handle.sync_all().await?;
    }
    Ok(())
}

async fn create_part(media: &Path, key: &str) -> Result<tokio::fs::File> {
    let part = object_path(media, key, "part")?;
    if object_path(media, key, "media")?.try_exists()? || part.try_exists()? {
        return Err(Error::StorageIntegrity);
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    Ok(options.open(part).await?)
}

async fn publish_file(
    media: &Path,
    key: &str,
    decoder: &str,
    format: &str,
    bytes: u64,
    aggregate: &mut Sha256,
) -> Result<(String, u64)> {
    let part = object_path(media, key, "part")?;
    let destination = object_path(media, key, "media")?;
    let measured = super::decoder::verify(decoder, &part, format).await?;
    let sha = hash_file(&part, bytes, aggregate).await?;
    tokio::fs::rename(&part, &destination).await?;
    #[cfg(unix)]
    sync_directory(media)?;
    Ok((sha, measured))
}

async fn hash_file(path: &Path, expected: u64, aggregate: &mut Sha256) -> Result<String> {
    let file = tokio::fs::File::open(path).await?;
    if file.metadata().await?.len() != expected {
        return Err(Error::StorageIntegrity);
    }
    let mut file = file.take(expected + 1);
    let mut segment = Sha256::new();
    let mut hashed = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        segment.update(&buffer[..read]);
        aggregate.update(&buffer[..read]);
        hashed = hashed
            .checked_add(u64::try_from(read).map_err(|_| Error::StorageIntegrity)?)
            .ok_or(Error::StorageIntegrity)?;
    }
    if hashed != expected {
        return Err(Error::StorageIntegrity);
    }
    Ok(hex(&segment.finalize()))
}
