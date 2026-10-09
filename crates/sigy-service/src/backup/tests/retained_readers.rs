//! Real backup copies of sealed media protected by durable restart holds.

use super::*;
use crate::storage::dvr::{OPEN_SEGMENT_CEILING, SegmentOpen, SegmentSeal};

fn segmented(root: &Path) -> Result<Library> {
    let mut library = library(root)?;
    let executable = std::env::current_exe()?;
    library.store_mut().configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or(Error::StorageIntegrity)?,
    )?;
    crate::recordings::checked_directory(library.directory(), true)?;
    let job = library
        .store_mut()
        .admit_recording(
            "segmented",
            "radio:v1",
            60,
            OPEN_SEGMENT_CEILING * 4,
            Retention::Temporary,
            false,
        )?
        .ok_or(Error::StorageIntegrity)?;
    let mut version = library.store_mut().connect_recording(&job.version)?;
    for bytes in [
        b"first sealed object".as_slice(),
        b"released object",
        b"third sealed object",
    ] {
        let SegmentOpen::Opened {
            version: opened,
            object_key,
            ..
        } = library.store_mut().open_segment(&version)?
        else {
            return Err(Error::StorageIntegrity);
        };
        fs::write(crate::recordings::media_path(root, &object_key)?, bytes)?;
        version = library.store_mut().seal_segment(
            &opened,
            &SegmentSeal {
                bytes: u64::try_from(bytes.len()).map_err(|_| Error::StorageIntegrity)?,
                sha256: hex(&Sha256::digest(bytes)),
                format: "wav",
                decoded_microseconds: 1_000_000,
            },
        )?;
    }
    for id in ["reader1", "reader2", "reader3", "reader4"] {
        library
            .store_mut()
            .admit_retained_reader(id, "segmented", 250_000, 10)?;
    }
    let release = library
        .store()
        .next_segment_release(false, crate::storage::Store::clock_ms()? + 15 * 86_400_000)?
        .ok_or(Error::StorageIntegrity)?;
    if release.ordinal != 1 {
        return Err(Error::StorageIntegrity);
    }
    fs::remove_file(crate::recordings::media_path(root, &release.object_key)?)?;
    library.store_mut().mark_segment_released(&release)?;
    let SegmentOpen::Opened { object_key, .. } = library.store_mut().open_segment(&version)? else {
        return Err(Error::StorageIntegrity);
    };
    fs::write(
        crate::recordings::media_path(root, &object_key)?,
        b"unpublished open tail",
    )?;
    // Reopen and apply the production restart transition. No fabricated close proof.
    drop(library);
    let mut library = Library::open(root, false)?;
    if library.store_mut().recover_retained_readers(20)? != 4 {
        return Err(Error::StorageIntegrity);
    }
    Ok(library)
}

#[test]
fn reserved_sealed_backup_restores_media_history_quota_and_four_restart_holds() -> TestResult {
    let root = tempfile::tempdir()?;
    let source = root.path().join("library");
    let mut original = segmented(&source)?;
    publish(&mut original, "retained", b"ordinary retained media")?;
    publish(&mut original, "deleting", b"excluded deleting media")?;
    original.store_mut().begin_delete("deleting", false)?;
    publish(&mut original, "deleted", b"excluded deleted media")?;
    original.store_mut().begin_delete("deleted", false)?;
    original.store_mut().finish_delete("deleted")?;
    let recordings = serde_json::to_value(original.store().recordings(None, 10)?)?;
    let quota = serde_json::to_value(original.store().dvr_status()?)?;
    let holds = original.store().retained_readers()?;
    let manifest = backup(&original, &root.path().join("backup"))?;
    assert_eq!(manifest.media.len(), 3);
    assert_eq!(manifest.media_bytes, 19 + 19 + 23);
    assert_eq!(verify(&root.path().join("backup"))?, manifest);
    let held_owner = original.hold_ownership();
    drop(original);
    assert!(matches!(
        Library::open(&source, false),
        Err(Error::LibraryBusy)
    ));
    drop(held_owner);
    let restored_path = root.path().join("restored");
    restore(&root.path().join("backup"), &restored_path)?;
    let mut restored = Library::open(&restored_path, false)?;
    assert_eq!(
        serde_json::to_value(restored.store().recordings(None, 10)?)?,
        recordings
    );
    let mut expected_quota = quota.clone();
    expected_quota["decoder"] = serde_json::Value::Null;
    assert_eq!(
        serde_json::to_value(restored.store().dvr_status()?)?,
        expected_quota
    );
    assert_eq!(restored.store().retained_readers()?, holds);
    assert_eq!(
        restored.store().recording("segmented")?.storage_state,
        "reserved"
    );
    assert!(
        restored
            .store_mut()
            .admit_retained_reader("fifth", "segmented", 0, 21)
            .is_err()
    );
    assert!(matches!(
        restored.store().retained_reader("fifth"),
        Err(Error::NotFound)
    ));
    for object in &manifest.media {
        let bytes = fs::read(media_file(&restored_path, &object.key)?)?;
        assert_eq!(bytes.len() as u64, object.bytes);
        assert_eq!(hex(&Sha256::digest(&bytes)), object.sha256);
    }
    restored.store().audit_retained_readers()?;
    Ok(())
}

fn protected_object(manifest: &BackupManifest, library: &Library) -> Result<BackupObject> {
    let key = library.store().retained_reader("reader1")?.spec.object_key;
    manifest
        .media
        .iter()
        .find(|object| object.key == key)
        .cloned()
        .ok_or(Error::StorageIntegrity)
}

#[test]
fn omitted_or_self_consistently_changed_protected_backup_cannot_restore() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = segmented(&root.path().join("library"))?;
    let manifest = backup(&original, &root.path().join("backup"))?;
    let protected = protected_object(&manifest, &original)?;
    drop(original);
    for changed in [false, true] {
        let name = if changed { "changed" } else { "omitted" };
        let copy = root.path().join(name);
        copy_tree(&root.path().join("backup"), &copy)?;
        let mut manifest = manifest.clone();
        if changed {
            let bytes = b"different protected bytes";
            fs::write(media_file(&copy, &protected.key)?, bytes)?;
            let object = manifest
                .media
                .iter_mut()
                .find(|object| object.key == protected.key)
                .ok_or("protected")?;
            object.bytes = u64::try_from(bytes.len())?;
            object.sha256 = hex(&Sha256::digest(bytes));
        } else {
            manifest.media.retain(|object| object.key != protected.key);
            fs::remove_file(media_file(&copy, &protected.key)?)?;
        }
        manifest.media_bytes = manifest.media.iter().map(|object| object.bytes).sum();
        fs::write(copy.join(MANIFEST), serde_json::to_vec(&manifest)?)?;
        // The outer manifest is consistent; the immutable catalog still refuses it.
        verify(&copy)?;
        let destination = root.path().join(format!("restored-{name}"));
        assert!(matches!(
            restore(&copy, &destination),
            Err(Error::InvalidInput(
                "restored catalog references missing media"
            ))
        ));
        assert!(!destination.exists());
        assert!(
            !root
                .path()
                .join(format!(".restored-{name}.restoring"))
                .exists()
        );
    }
    Ok(())
}

#[test]
fn protected_reserved_source_missing_or_changed_never_writes_valid_manifest() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = segmented(&root.path().join("library"))?;
    let spec = original.store().retained_reader("reader1")?.spec;
    let path = media_file(original.directory(), &spec.object_key)?;
    fs::write(&path, b"wrong")?;
    assert!(matches!(
        backup(&original, &root.path().join("changed")),
        Err(Error::Analysis("backup-media-mismatch"))
    ));
    assert!(!root.path().join("changed").join(MANIFEST).exists());
    fs::remove_file(path)?;
    assert!(matches!(
        backup(&original, &root.path().join("missing")),
        Err(Error::Analysis("backup-media-unavailable"))
    ));
    assert!(!root.path().join("missing").join(MANIFEST).exists());
    Ok(())
}
