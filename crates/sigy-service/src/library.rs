//! Explicit library ownership for maintenance and the service process.

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{Error, Result, storage::Store};

/// An OS-backed exclusive lock, released even if the owning process crashes.
#[derive(Debug)]
pub struct Library {
    directory: PathBuf,
    store: Store,
    lock: Arc<File>,
}

impl Library {
    /// Creates or opens an explicitly chosen library, holding its exclusive lock.
    /// # Errors
    /// Rejects a busy library, links at owned paths, or storage/catalog failures.
    pub fn open(directory: &Path, create: bool) -> Result<Self> {
        if create {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(directory)?;
        }
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::InvalidInput("library directory"));
        }
        let directory = directory.canonicalize()?;
        for name in [
            "service.lock",
            "catalog.sqlite3",
            "catalog.sqlite3-wal",
            "catalog.sqlite3-shm",
        ] {
            reject_link(&directory.join(name))?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("service.lock"))?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => Error::LibraryBusy,
            std::fs::TryLockError::Error(error) => Error::Io(error),
        })?;
        let catalog = directory.join("catalog.sqlite3");
        if !create && !catalog.is_file() {
            return Err(Error::InvalidInput("library has not been initialized"));
        }
        let store = Store::open(&catalog)?;
        Ok(Self {
            directory,
            lock: Arc::new(lock),
            store,
        })
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    #[must_use]
    pub const fn store(&self) -> &Store {
        &self.store
    }

    pub const fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    pub(crate) fn hold_ownership(&self) -> Arc<File> {
        Arc::clone(&self.lock)
    }
}

pub(crate) fn reject_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            Err(Error::InvalidInput("library-owned file path"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Checks the directory boundary before publishing or trusting a control endpoint.
pub(crate) fn control_directory(path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput("control directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != rustix::process::geteuid().as_raw() {
            return Err(Error::InvalidInput(
                "control directory must be owned by this user with mode 0700",
            ));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::InvalidInput("control directory reparse point"));
        }
    }
    Ok(path.canonicalize()?)
}
