//! Confirms the retained SQLite sources match `vendor/checksums.json`.

use std::{fs, path::Path};

use sha2::{Digest, Sha256};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    sha256: String,
}

pub(crate) fn check_vendor(root: &Path) -> Result<(), String> {
    let vendor = root.join("vendor").join("libsqlite3-sys");
    let manifest = fs::read(root.join("vendor").join("checksums.json"))
        .map_err(|error| format!("cannot read vendor checksums: {error}"))?;
    let entries: Vec<Entry> =
        serde_json::from_slice(&manifest).map_err(|error| format!("vendor checksums: {error}"))?;
    let mut actual = walk_files(&vendor)?;
    actual.sort();
    let mut expected: Vec<String> = entries.iter().map(|entry| entry.path.clone()).collect();
    expected.sort();
    if actual != expected {
        return Err("native dependency file inventory changed".into());
    }
    for entry in entries {
        let path = contained_file(&vendor, &entry.path)?;
        let digest = hash_file(&path)?;
        if digest != entry.sha256.to_ascii_lowercase() {
            return Err(format!(
                "native dependency checksum mismatch: {}",
                entry.path
            ));
        }
    }
    Ok(())
}

fn walk_files(root: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    Ok(files)
}

fn walk(root: &Path, directory: &Path, files: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| format!("{error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| format!("{error}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "native dependency path is a symlink: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            walk(root, &path, files)?;
        } else if metadata.is_file() {
            files.push(relative_forward(root, &path)?);
        }
    }
    Ok(())
}

fn contained_file(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "..")
    {
        return Err(format!(
            "native dependency manifest contains an out-of-scope path: {relative}"
        ));
    }
    let path = root.join(relative);
    let relative_again = relative_forward(root, &path)?;
    if relative_again != relative.replace('\\', "/") {
        return Err(format!(
            "native dependency manifest contains an out-of-scope path: {relative}"
        ));
    }
    Ok(path)
}

fn relative_forward(root: &Path, target: &Path) -> Result<String, String> {
    let root = normalize(&fs::canonicalize(root).map_err(|error| error.to_string())?)?;
    let target = normalize(&fs::canonicalize(target).map_err(|error| error.to_string())?)?;
    let prefix = format!("{root}/");
    let rest = strip_prefix(&target, &prefix)
        .ok_or_else(|| format!("native dependency path is outside the vendor root: {target}"))?;
    Ok(rest)
}

fn strip_prefix(path: &str, prefix: &str) -> Option<String> {
    let path_bytes = path.as_bytes();
    let prefix_bytes = prefix.as_bytes();
    if path_bytes.len() < prefix_bytes.len() {
        return None;
    }
    let matched = if cfg!(windows) {
        path_bytes[..prefix_bytes.len()].eq_ignore_ascii_case(prefix_bytes)
    } else {
        path_bytes.starts_with(prefix_bytes)
    };
    if matched {
        Some(path[prefix.len()..].to_owned())
    } else {
        None
    }
}

fn normalize(path: &std::path::Path) -> Result<String, String> {
    let text = path.to_str().ok_or("vendor path is not Unicode")?;
    let text = text
        .strip_prefix("\\\\?\\")
        .or_else(|| text.strip_prefix("//?/"))
        .unwrap_or(text);
    Ok(text.replace('\\', "/"))
}

pub(crate) fn hash_file(path: &std::path::Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(bytes);
    let alphabet = b"0123456789abcdef";
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(alphabet[usize::from(byte >> 4)]));
        hex.push(char::from(alphabet[usize::from(byte & 0x0f)]));
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::{check_vendor, contained_file};

    #[test]
    fn manifest_paths_cannot_escape_the_vendor_root() -> Result<(), String> {
        let root = tempfile::tempdir().map_err(|error| error.to_string())?;
        let vendor = root.path().join("vendor").join("libsqlite3-sys");
        std::fs::create_dir_all(&vendor).map_err(|error| error.to_string())?;
        let Err(error) = contained_file(&vendor, "../checksums.json") else {
            return Err("escaped path was accepted".into());
        };
        if !error.contains("out-of-scope") {
            return Err(error);
        }
        if check_vendor(root.path()).is_ok() {
            return Err("missing checksum manifest was accepted".into());
        }
        Ok(())
    }
}
