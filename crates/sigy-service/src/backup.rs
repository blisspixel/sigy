//! Library backup and restore.
//!
//! A backup is taken while this process owns the library, so no service can write during
//! it. It holds a `VACUUM INTO` catalog snapshot, a copy of every retained media object the
//! catalog vouches for, and a manifest of sizes and SHA-256 hashes. Restore verifies every
//! file against the manifest and the snapshot's own intervals before anything becomes a
//! library, so a restored library never has missing or changed media. Hashes detect
//! corruption; they are not signatures.

use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result, library::Library, storage::dvr::hex};

pub const BACKUP_FORMAT: &str = "sigy-backup-v1";
const MANIFEST: &str = "manifest.json";
const CATALOG: &str = "catalog.sqlite3";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupFile {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupObject {
    /// The catalog's object key; the file is `media/<key>.media`.
    pub key: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub format: String,
    pub schema_version: u32,
    pub created_ms: i64,
    pub catalog: BackupFile,
    pub media: Vec<BackupObject>,
    pub media_bytes: u64,
}

fn private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

/// Refuse anything but an absent path, so a backup or restore never mixes with other files.
fn fresh(path: &Path) -> Result<()> {
    if path.try_exists()? || fs::symlink_metadata(path).is_ok() {
        return Err(Error::InvalidInput(
            "destination already exists; choose a new directory",
        ));
    }
    Ok(())
}

/// Stream a file into a new file, returning its size and SHA-256. Links are refused.
fn copy_hashed(source: &Path, destination: &Path) -> Result<BackupFile> {
    crate::library::reject_link(source)?;
    let mut input = File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024].into_boxed_slice();
    let mut bytes = 0_u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
        bytes += read as u64;
    }
    output.sync_all()?;
    Ok(BackupFile {
        bytes,
        sha256: hex(&hasher.finalize()),
    })
}

fn hash_file(path: &Path) -> Result<BackupFile> {
    crate::library::reject_link(path)?;
    let mut input = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024].into_boxed_slice();
    let mut bytes = 0_u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok(BackupFile {
        bytes,
        sha256: hex(&hasher.finalize()),
    })
}

fn media_file(root: &Path, key: &str) -> Result<PathBuf> {
    crate::storage::dvr::validate_object_key(key)?;
    Ok(root.join("media").join(format!("{key}.media")))
}

/// Back up an owned library into a new directory.
/// # Errors
/// Refuses an existing destination, a failed integrity check, or any retained object that is
/// missing or does not match the catalog. A failed backup leaves a partial directory that is
/// not a valid backup because it has no manifest.
pub fn backup(library: &Library, destination: &Path) -> Result<BackupManifest> {
    fresh(destination)?;
    let store = library.store();
    if !store.integrity_ok()? {
        return Err(Error::CatalogIntegrity);
    }
    private_directory(destination)?;
    let catalog_path = destination.join(CATALOG);
    store.snapshot_catalog(&catalog_path)?;
    let catalog = hash_file(&catalog_path)?;
    private_directory(&destination.join("media"))?;
    let mut media = Vec::new();
    let mut media_bytes = 0_u64;
    for object in store.retained_objects()? {
        let source = crate::recordings::media_path(library.directory(), &object.key)?;
        let copied = copy_hashed(&source, &media_file(destination, &object.key)?)
            .map_err(|_| Error::Analysis("backup-media-unavailable"))?;
        if copied.bytes != object.bytes || copied.sha256 != object.sha256 {
            return Err(Error::Analysis("backup-media-mismatch"));
        }
        media_bytes += copied.bytes;
        media.push(BackupObject {
            key: object.key,
            bytes: copied.bytes,
            sha256: copied.sha256,
        });
    }
    let manifest = BackupManifest {
        format: BACKUP_FORMAT.into(),
        schema_version: crate::storage::SCHEMA_VERSION,
        created_ms: crate::storage::now_ms()?,
        catalog,
        media,
        media_bytes,
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination.join(MANIFEST))?;
    file.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    file.sync_all()?;
    Ok(manifest)
}

/// Read a manifest and check every file in the backup against it.
/// # Errors
/// Refuses a malformed or oversized manifest, an unknown format, a newer schema, extra or
/// missing media, or any size or hash mismatch.
pub fn verify(source: &Path) -> Result<BackupManifest> {
    let manifest_path = source.join(MANIFEST);
    crate::library::reject_link(&manifest_path)?;
    let mut bytes = Vec::new();
    File::open(&manifest_path)
        .map_err(|_| Error::InvalidInput("backup has no manifest"))?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(Error::InvalidInput("backup manifest is too large"));
    }
    let manifest: BackupManifest = serde_json::from_slice(&bytes)
        .map_err(|_| Error::InvalidInput("backup manifest is malformed"))?;
    if manifest.format != BACKUP_FORMAT {
        return Err(Error::InvalidInput("unknown backup format"));
    }
    if manifest.schema_version > crate::storage::SCHEMA_VERSION {
        return Err(Error::FutureSchema {
            found: i64::from(manifest.schema_version),
            supported: i64::from(crate::storage::SCHEMA_VERSION),
        });
    }
    if hash_file(&source.join(CATALOG))? != manifest.catalog {
        return Err(Error::InvalidInput(
            "backup catalog does not match its manifest",
        ));
    }
    let listed: std::collections::BTreeSet<String> = manifest
        .media
        .iter()
        .map(|object| format!("{}.media", object.key))
        .collect();
    if listed.len() != manifest.media.len() {
        return Err(Error::InvalidInput(
            "backup manifest repeats a media object",
        ));
    }
    let mut present = std::collections::BTreeSet::new();
    for entry in fs::read_dir(source.join("media"))? {
        let entry = entry?;
        present.insert(entry.file_name().to_string_lossy().into_owned());
    }
    if present != listed {
        return Err(Error::InvalidInput(
            "backup media does not match its manifest",
        ));
    }
    let mut total = 0_u64;
    for object in &manifest.media {
        let found = hash_file(&media_file(source, &object.key)?)?;
        if found.bytes != object.bytes || found.sha256 != object.sha256 {
            return Err(Error::InvalidInput(
                "backup media does not match its manifest",
            ));
        }
        total += found.bytes;
    }
    if total != manifest.media_bytes {
        return Err(Error::InvalidInput("backup media total does not match"));
    }
    Ok(manifest)
}

/// Restore a verified backup into a new library directory.
///
/// The files are verified, copied into a staging directory beside the destination, verified
/// against the snapshot's own retained intervals after opening (which also migrates an
/// older schema), and only then renamed into place. Recovery on the next service start marks
/// any capture or job that was running at backup time as interrupted, a schedule does not
/// backfill missed windows, and uncertain paid liabilities stay reserved.
/// # Errors
/// Refuses an existing destination, any verification failure, or a snapshot whose retained
/// intervals are not all present in the backup.
pub fn restore(source: &Path, destination: &Path) -> Result<BackupManifest> {
    fresh(destination)?;
    let manifest = verify(source)?;
    let name = destination
        .file_name()
        .ok_or(Error::InvalidInput("restore destination has no name"))?
        .to_string_lossy()
        .into_owned();
    let staging = destination.with_file_name(format!(".{name}.restoring"));
    fresh(&staging)?;
    let staged = stage(source, &staging, &manifest);
    if let Err(error) = staged {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    fs::rename(&staging, destination)?;
    Ok(manifest)
}

fn stage(source: &Path, staging: &Path, manifest: &BackupManifest) -> Result<()> {
    private_directory(staging)?;
    let catalog = copy_hashed(&source.join(CATALOG), &staging.join(CATALOG))?;
    if catalog != manifest.catalog {
        return Err(Error::InvalidInput("backup catalog changed during restore"));
    }
    private_directory(&staging.join("media"))?;
    for object in &manifest.media {
        let copied = copy_hashed(
            &media_file(source, &object.key)?,
            &media_file(staging, &object.key)?,
        )?;
        if copied.bytes != object.bytes || copied.sha256 != object.sha256 {
            return Err(Error::InvalidInput("backup media changed during restore"));
        }
    }
    // Opening runs migrations and the catalog audits; the lock is released on drop.
    let library = Library::open(staging, false)?;
    if !library.store().integrity_ok()? {
        return Err(Error::CatalogIntegrity);
    }
    let listed: std::collections::BTreeMap<&str, &BackupObject> = manifest
        .media
        .iter()
        .map(|object| (object.key.as_str(), object))
        .collect();
    for object in library.store().retained_objects()? {
        match listed.get(object.key.as_str()) {
            Some(backed) if backed.sha256 == object.sha256 && backed.bytes == object.bytes => {}
            _ => {
                return Err(Error::InvalidInput(
                    "restored catalog references missing media",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
