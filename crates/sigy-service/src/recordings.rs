//! Owned media files. Only the library owner may publish or reclaim these paths.

mod decoder;
mod live;
pub mod metadata;
mod pipe;

pub(crate) use live::stream_revision;
pub(crate) use pipe::valid_listen_nonce as pipe_nonce_valid;

use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    Error, Result,
    library::{Library, control_directory, reject_link},
    sources::{
        HttpSource,
        http::{AcquisitionLimits, AudioContentType, HttpAcquirer, TransferEnd},
    },
    storage::dvr::{Publication, hex, validate_object_key},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackDestination {
    Null,
    System,
}

impl PlaybackDestination {
    /// # Errors
    /// Returns an error when the destination name is not `null` or `system`.
    pub fn parse(value: &str) -> std::result::Result<Self, &'static str> {
        match value {
            "null" => Ok(Self::Null),
            "system" => Ok(Self::System),
            _ => Err("destination: use null or system"),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackReport {
    pub playhead_us: u64,
    pub progress_advanced: bool,
}

/// Play one local published file in the calling process.
///
/// The decoder receives this path only. Playback does not reserve quota or open a source URL.
///
/// Decode one local pipe of bytes the service already fetched.
///
/// The decoder receives `pipe:0` only. This does not reserve quota or open a source URL.
/// # Errors
/// Rejects an unsupported format, a pipe that cannot be opened, or a decoder that cannot finish.
pub async fn play_direct_listen(
    executable: &str,
    directory: &Path,
    nonce: &str,
    format: &str,
    destination: PlaybackDestination,
) -> Result<PlaybackReport> {
    let input = pipe::connect_listen_pipe(directory, nonce).await?;
    let report = decoder::play_reader(
        executable,
        input,
        format,
        destination == PlaybackDestination::System,
    )
    .await?;
    Ok(PlaybackReport {
        playhead_us: report.playhead_us,
        progress_advanced: report.progress_advanced,
    })
}

/// # Errors
/// Rejects an unsupported format, a seek outside the published duration, a missing file,
/// or a decoder that cannot finish.
pub async fn play_retained_file(
    executable: &str,
    path: &Path,
    format: &str,
    destination: PlaybackDestination,
    seek_us: u64,
    decoded_us: u64,
) -> Result<PlaybackReport> {
    let report = decoder::play_file(
        executable,
        path,
        format,
        destination == PlaybackDestination::System,
        seek_us,
        decoded_us,
    )
    .await?;
    Ok(PlaybackReport {
        playhead_us: report.playhead_us,
        progress_advanced: report.progress_advanced,
    })
}

/// # Errors
/// Rejects invalid keys and links at the owned media boundary.
pub fn media_path(directory: &Path, key: &str) -> Result<PathBuf> {
    let media = checked_directory(directory, false)?;
    object_path(&media, key, "media")
}

pub(crate) fn checked_directory(directory: &Path, create: bool) -> Result<PathBuf> {
    let path = directory.join("media");
    if create && !path.try_exists()? {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
    }
    control_directory(&path)
}

fn object_path(media: &Path, key: &str, extension: &str) -> Result<PathBuf> {
    validate_object_key(key)?;
    let path = media.join(format!("{key}.{extension}"));
    reject_link(&path)?;
    Ok(path)
}

pub(crate) fn delete(library: &mut Library, id: &str, pruning: bool) -> Result<()> {
    let record = library.store_mut().begin_delete(id, pruning)?;
    if record.storage_state == "deleted" {
        return Ok(());
    }
    let media = checked_directory(library.directory(), true)?;
    for extension in ["part", "media"] {
        let path = object_path(&media, &record.object_key, extension)?;
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    #[cfg(unix)]
    sync_directory(&media)?;
    library.store_mut().finish_delete(id)
}

pub(crate) fn prune(library: &mut Library) -> Result<()> {
    for id in library.store().prune_candidates(false)? {
        delete(library, &id, true)?;
    }
    Ok(())
}

pub(crate) fn recover_deletions(library: &mut Library) -> Result<()> {
    loop {
        let pending = library.store().pending_deletions()?;
        if pending.is_empty() {
            return Ok(());
        }
        for id in pending {
            delete(library, &id, false)?;
        }
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

pub(crate) struct CaptureRequest {
    pub directory: PathBuf,
    pub key: String,
    pub source: HttpSource,
    pub limits: AcquisitionLimits,
    pub decoder: String,
    pub acquirer: HttpAcquirer,
    pub hls: bool,
    pub icy: bool,
}

pub(crate) async fn capture(
    request: CaptureRequest,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Result<Publication> {
    let CaptureRequest {
        directory,
        key,
        source,
        limits,
        decoder,
        acquirer,
        hls,
        icy,
    } = request;
    let media = checked_directory(&directory, true)?;
    let part = object_path(&media, &key, "part")?;
    let destination = object_path(&media, &key, "media")?;
    if destination.try_exists()? {
        return Err(Error::StorageIntegrity);
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(&part).await?;
    let result = if hls {
        crate::sources::hls::record(&acquirer, &source, limits, &mut file, &mut stop).await
    } else {
        acquirer
            .record_with(
                &source,
                limits,
                &mut file,
                &mut stop,
                if icy {
                    crate::sources::icy::MetadataPolicy::Requested
                } else {
                    crate::sources::icy::MetadataPolicy::Off
                },
            )
            .await
    };
    // The file and its quota stay owned even when transport fails or is cancelled.
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    let receipt = result?;
    let format = match receipt.declared_content_type {
        AudioContentType::Mpeg => "mp3",
        AudioContentType::Aac => "aac",
        AudioContentType::Flac => "flac",
        AudioContentType::Ogg => "ogg",
        AudioContentType::Wave => "wav",
    };
    let decoded_microseconds = decoder::verify(&decoder, &part, format).await?;
    let file = tokio::fs::File::open(&part).await?;
    if file.metadata().await?.len() != receipt.bytes {
        return Err(Error::StorageIntegrity);
    }
    let mut digest = Sha256::new();
    let mut file = file.take(receipt.bytes + 1);
    let mut hashed_bytes = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        hashed_bytes += u64::try_from(read).map_err(|_| Error::StorageIntegrity)?;
    }
    drop(file);
    if hashed_bytes != receipt.bytes {
        return Err(Error::StorageIntegrity);
    }
    // The exclusive library lock and generated key make this a single publisher.
    tokio::fs::rename(&part, &destination).await?;
    #[cfg(unix)]
    sync_directory(&media)?;
    Ok(Publication {
        bytes: receipt.bytes,
        sha256: hex(&digest.finalize()),
        format,
        decoded_microseconds,
        http_route: receipt.route,
        observations: receipt.observations,
        end_reason: match receipt.end {
            TransferEnd::EndOfBody => "end_of_body",
            TransferEnd::ByteLimit => "byte_limit",
            TransferEnd::DurationLimit => "duration_limit",
            TransferEnd::UserStop => "user_stop",
        },
    })
}

pub(crate) fn check_free_space(library: &Library) -> Result<()> {
    let policy = library.store().dvr_status()?;
    let required = policy
        .reserved_bytes
        .checked_add(policy.minimum_free_bytes)
        .ok_or(Error::StorageQuota)?;
    if fs4::available_space(library.directory())? < required {
        return Err(Error::StorageQuota);
    }
    Ok(())
}

pub(crate) fn limits(bytes: u64, seconds: u64) -> Result<AcquisitionLimits> {
    AcquisitionLimits::new(bytes, Duration::from_secs(seconds))
}
