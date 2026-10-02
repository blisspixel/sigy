//! Bounded local file access for evaluator-only artifacts.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use scorer_probe::selection::{is_sha256, sha256};

use crate::Result;

/// Refuse links, reparse points and anything other than a plain file.
fn plain_file(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("input must be a plain file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("reparse point refused".into());
        }
    }
    Ok(())
}

pub fn read(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    plain_file(path)?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("input exceeds its byte ceiling".into());
    }
    Ok(bytes)
}

pub fn pinned(path: &Path, expected: &str, maximum: u64) -> Result<Vec<u8>> {
    let bytes = read(path, maximum)?;
    if !is_sha256(expected) || sha256(&bytes) != expected {
        return Err("input bytes differ from the supplied SHA-256".into());
    }
    Ok(bytes)
}

/// Create a new file, refusing to replace anything, and sync it.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Append one line and sync it.
pub fn append_line(path: &Path, line: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::tests::scratch;

    #[test]
    fn bounded_pinned_and_create_only_files() -> Result<()> {
        let directory = scratch("files")?;
        let path = directory.join("a.json");
        write_new(&path, b"abc")?;
        assert!(write_new(&path, b"def").is_err());
        assert_eq!(read(&path, 3)?, b"abc");
        assert!(read(&path, 2).is_err());
        assert!(pinned(&path, &sha256(b"abc"), 3).is_ok());
        assert!(pinned(&path, &sha256(b"abd"), 3).is_err());
        assert!(pinned(&path, "not-a-digest", 3).is_err());
        assert!(read(&directory, 3).is_err());
        append_line(&directory.join("log.jsonl"), b"{}")?;
        append_line(&directory.join("log.jsonl"), b"{}")?;
        assert_eq!(std::fs::read(directory.join("log.jsonl"))?, b"{}\n{}\n");
        Ok(())
    }
}
