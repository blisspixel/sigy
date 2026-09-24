//! Cancellable hashing of local runtime, model and input files.

use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use sha2::{Digest, Sha256};

use crate::{Error, Result, storage::dvr::hex};

const MAX_RUNTIME_FILES: usize = 256;
const MAX_RUNTIME_BYTES: u64 = 1024 * 1024 * 1024;

pub(crate) struct RuntimeManifest {
    pub sha256: String,
    pub files: u32,
    pub bytes: u64,
}

/// Hash every regular file directly inside the runtime directory, in name order.
/// Subdirectories are not searched by the loader and are ignored. Links are refused.
pub(crate) fn runtime_manifest(
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
pub(crate) fn plain_directory(path: &Path) -> Result<()> {
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

/// Hash one regular file of at most `limit` bytes, checking `stop` between reads.
pub(crate) fn hash_file(path: &Path, limit: u64, stop: &AtomicBool) -> Result<(String, u64)> {
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
