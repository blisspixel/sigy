//! Backup and restore on real catalogs and media files in temporary directories.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::*;
use crate::{
    sources::{HttpHop, HttpSource, NetworkScope},
    storage::dvr::{Publication, Retention},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type Damage<'a> = &'a dyn Fn(&Path) -> std::io::Result<()>;

fn library(root: &Path) -> Result<Library> {
    let mut library = Library::open(root, true)?;
    let executable = std::env::current_exe()?;
    library.store_mut().configure_dvr(
        10_000,
        64 * 1024 * 1024,
        14,
        executable
            .to_str()
            .ok_or(Error::InvalidInput("test executable"))?,
    )?;
    library.store_mut().register_source(
        "radio:v1",
        &HttpSource::new(
            "Test radio",
            "https://example.com/audio",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    Ok(library)
}

fn publish(library: &mut Library, id: &str, bytes: &[u8]) -> Result<PathBuf> {
    let sha = hex(&Sha256::digest(bytes));
    let job = library
        .store_mut()
        .admit_recording(id, "radio:v1", 60, 600, Retention::Kept, false)?
        .ok_or(Error::StorageIntegrity)?;
    let length = u64::try_from(bytes.len()).map_err(|_| Error::StorageIntegrity)?;
    library.store_mut().publish_recording(
        &job.version,
        &Publication {
            bytes: length,
            sha256: sha,
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![HttpHop {
                origin: "https://example.com".into(),
                peer: std::net::SocketAddr::from(([8, 8, 8, 8], 443)),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
            gap: None,
        },
    )?;
    let key = library
        .store()
        .recording(id)?
        .intervals
        .first()
        .ok_or(Error::StorageIntegrity)?
        .object_key
        .clone();
    crate::recordings::checked_directory(library.directory(), true)?;
    let path = crate::recordings::media_path(library.directory(), &key)?;
    fs::write(&path, bytes)?;
    Ok(path)
}

fn populated(root: &Path) -> Result<Library> {
    let mut library = library(root)?;
    publish(&mut library, "one", b"first retained recording bytes")?;
    publish(
        &mut library,
        "two",
        "second recording, not ASCII: \u{6771}\u{4eac}".as_bytes(),
    )?;
    Ok(library)
}

#[test]
fn backup_and_restore_reproduce_the_catalog_and_every_retained_object() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = populated(&root.path().join("library"))?;
    let manifest = backup(&original, &root.path().join("backup"))?;
    assert_eq!(manifest.format, BACKUP_FORMAT);
    assert_eq!(manifest.media.len(), 2);
    assert_eq!(verify(&root.path().join("backup"))?, manifest);
    let before = original.store().recordings(None, 10)?;
    drop(original);

    let restored_path = root.path().join("restored");
    restore(&root.path().join("backup"), &restored_path)?;
    let restored = Library::open(&restored_path, false)?;
    assert_eq!(
        serde_json::to_value(restored.store().recordings(None, 10)?)?,
        serde_json::to_value(&before)?
    );
    for recording in &before {
        let interval = recording.intervals.first().ok_or("interval")?;
        let bytes = fs::read(crate::recordings::media_path(
            &restored_path,
            &interval.object_key,
        )?)?;
        assert_eq!(hex(&Sha256::digest(&bytes)), interval.sha256);
    }
    assert!(restored.store().integrity_ok()?);
    Ok(())
}

#[test]
fn existing_destinations_are_refused_and_nothing_is_overwritten() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = populated(&root.path().join("library"))?;
    fs::create_dir(root.path().join("taken"))?;
    assert!(backup(&original, &root.path().join("taken")).is_err());
    backup(&original, &root.path().join("backup"))?;
    assert!(restore(&root.path().join("backup"), &root.path().join("library")).is_err());
    assert!(restore(&root.path().join("backup"), &root.path().join("taken")).is_err());
    assert!(fs::read_dir(root.path().join("taken"))?.next().is_none());
    Ok(())
}

#[test]
fn a_missing_or_changed_source_object_fails_the_backup() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut original = library(&root.path().join("library"))?;
    let path = publish(&mut original, "one", b"original bytes")?;
    fs::write(&path, b"changed bytes!")?;
    assert!(matches!(
        backup(&original, &root.path().join("changed")),
        Err(Error::Analysis("backup-media-mismatch"))
    ));
    assert!(!root.path().join("changed").join(MANIFEST).exists());
    fs::remove_file(&path)?;
    assert!(matches!(
        backup(&original, &root.path().join("missing")),
        Err(Error::Analysis("backup-media-unavailable"))
    ));
    Ok(())
}

#[test]
fn tampered_missing_extra_or_malformed_backups_do_not_restore() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = populated(&root.path().join("library"))?;
    let manifest = backup(&original, &root.path().join("backup"))?;
    drop(original);
    let object = manifest.media.first().ok_or("media")?;
    let cases: [(&str, Damage<'_>); 5] = [
        ("changed media", &|dir| {
            fs::write(
                dir.join("media").join(format!("{}.media", object.key)),
                b"x",
            )
        }),
        ("missing media", &|dir| {
            fs::remove_file(dir.join("media").join(format!("{}.media", object.key)))
        }),
        ("extra media", &|dir| {
            fs::write(
                dir.join("media")
                    .join("0123456789abcdef0123456789abcdef.media"),
                b"x",
            )
        }),
        ("changed catalog", &|dir| {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(dir.join(CATALOG))?;
            file.write_all(b"trailing")
        }),
        ("malformed manifest", &|dir| {
            fs::write(dir.join(MANIFEST), b"{\"format\":")
        }),
    ];
    for (index, (name, damage)) in cases.iter().enumerate() {
        let copy = root.path().join(format!("copy-{index}"));
        copy_tree(&root.path().join("backup"), &copy)?;
        damage(&copy)?;
        assert!(verify(&copy).is_err(), "{name}");
        let target = root.path().join(format!("restored-{index}"));
        assert!(restore(&copy, &target).is_err(), "{name}");
        assert!(!target.exists(), "{name} left a library");
        assert!(
            !root
                .path()
                .join(format!(".restored-{index}.restoring"))
                .exists(),
            "{name} left staging"
        );
    }
    Ok(())
}

#[test]
fn a_manifest_that_omits_an_object_the_catalog_needs_is_refused() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = populated(&root.path().join("library"))?;
    let mut manifest = backup(&original, &root.path().join("backup"))?;
    drop(original);
    let removed = manifest.media.remove(0);
    manifest.media_bytes -= removed.bytes;
    fs::remove_file(
        root.path()
            .join("backup")
            .join("media")
            .join(format!("{}.media", removed.key)),
    )?;
    fs::write(
        root.path().join("backup").join(MANIFEST),
        serde_json::to_vec(&manifest)?,
    )?;
    verify(&root.path().join("backup"))?;
    let target = root.path().join("restored");
    assert!(restore(&root.path().join("backup"), &target).is_err());
    assert!(!target.exists());
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
