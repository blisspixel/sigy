//! Actual media hashes and legal citation admission, with synthetic model output.

use super::*;
use crate::{
    monitor::{FindingCite, FindingOriginal, MonitorSpec, MonitorTerm},
    recognition::{
        LocalAsrOutcome, LocalAsrRequest, ReapedLocalAsr, RecognitionCoverage, RecognitionCue,
        RecognitionOutput,
    },
    translation::{
        TranslatedCue, TranslationOutcome, TranslationProfile, TranslationRequest,
        TranslationResult,
    },
};
use tokio::io::AsyncReadExt;

const ORIGINAL: &[u8] = b"retained bytes for exact citation backup; no decoded-audio claim";

fn recognize(library: &mut Library) -> Result<()> {
    library
        .store_mut()
        .admit_analysis("spoken", "spoken", false, 10)?;
    library.store_mut().publish_analysis("spoken", 1)?;
    let request = LocalAsrRequest {
        id: "asr".into(),
        analysis_id: "spoken".into(),
        analysis_revision: 1,
        profile: "synthetic-storage-fixture-v1".into(),
        profile_sha256: "b".repeat(64),
        parent_revision: 0,
    };
    let work = library
        .store_mut()
        .admit_local_asr(&request, 20)?
        .1
        .ok_or(Error::StorageIntegrity)?;
    let chunk = work.input.chunks.first().ok_or(Error::StorageIntegrity)?;
    let output = RecognitionOutput {
        profile_sha256: work.job.request.profile_sha256.clone(),
        manifest_sha256: work.job.manifest_sha256.clone(),
        coverages: vec![RecognitionCoverage {
            ordinal: chunk.ordinal,
            interval_ordinal: chunk.interval_ordinal,
            start_us: chunk.start_us,
            end_us: chunk.end_us,
            source_sha256: chunk.source_sha256.clone(),
            decoded_sha256: "c".repeat(64),
            sample_rate: 16_000,
            sample_count: 16_000,
        }],
        cues: vec![RecognitionCue {
            ordinal: 0,
            start_us: 125_000,
            end_us: 375_000,
            script: "Una feria mundial".into(),
        }],
    };
    let reaped = ReapedLocalAsr::synthetic_fixture(&work, LocalAsrOutcome::Succeeded(output));
    library.store_mut().finish_local_asr(&work, &reaped, 21)?;
    Ok(())
}

fn translate(library: &mut Library) -> Result<()> {
    let root = library.directory();
    let mut profile = TranslationProfile {
        id: "mt".into(),
        engine: crate::translation::LLAMA_CPP_COMPLETION.into(),
        template: crate::translation::HY_MT2_PLAIN.into(),
        runtime_dir: root.join("runtime").display().to_string(),
        executable: "llama-completion.exe".into(),
        runtime_sha256: "4".repeat(64),
        runtime_files: 3,
        runtime_bytes: 300,
        model_path: root.join("model.gguf").display().to_string(),
        model_sha256: "5".repeat(64),
        model_bytes: 1000,
        languages: "es".into(),
        threads: 2,
        memory_bytes: 1 << 30,
        cue_deadline_ms: 60_000,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.identity()?;
    library.store_mut().add_translation_profile(&profile, 22)?;
    let request = TranslationRequest {
        id: "translate".into(),
        transcript_id: "spoken".into(),
        transcript_revision: 1,
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
    };
    let (job, work) = library.store_mut().admit_translation(&request, 30)?;
    let outcome = TranslationOutcome::synthetic_fixture(
        &job,
        TranslationResult::Succeeded(vec![TranslatedCue {
            ordinal: 0,
            state: "translated".into(),
            english: Some("A world fair".into()),
            reason: None,
        }]),
    );
    library
        .store_mut()
        .finish_translation(&work.ok_or(Error::StorageIntegrity)?, &outcome, 31)?;
    Ok(())
}

fn cited_library(root: &Path) -> Result<Library> {
    let mut library = library(root)?;
    publish(&mut library, "spoken", ORIGINAL)?;
    recognize(&mut library)?;
    translate(&mut library)?;
    library.store_mut().create_monitor(
        "desk",
        &MonitorSpec {
            name: "Citation fixture".into(),
            goal: "Inspect the exact original".into(),
            terms: vec![MonitorTerm {
                language: "es".into(),
                text: "feria".into(),
            }],
            sources: vec!["radio:v1".into()],
            candidate_sources: Vec::new(),
            schedules: Vec::new(),
            daily_audio_seconds: 60,
            total_audio_seconds: 60,
            recognition_profile: None,
            translation_profile: None,
            capture: None,
        },
        35,
    )?;
    library.store_mut().publish_finding(
        "desk",
        "passage",
        &FindingCite {
            transcript_id: "spoken".into(),
            transcript_revision: 1,
            translation_revision: 1,
            cue_ordinal: 0,
            original: FindingOriginal::Retained,
        },
        40,
    )?;
    Ok(library)
}

async fn complete_original(library: &mut Library) -> TestResult {
    let (spec, _) =
        library
            .store_mut()
            .admit_retained_range("closed-range", "spoken", 125_000, 375_000, 50)?;
    let directory = library.directory().to_owned();
    let (ready, endpoint) = tokio::sync::oneshot::channel();
    let (_stop, signal) = tokio::sync::watch::channel(false);
    let stream = crate::recordings::retained::stream_retained(
        directory.clone(),
        spec,
        library.hold_ownership(),
        signal,
        move |nonce| async move { ready.send(nonce).map_err(|_| Error::ServiceStopped) },
    );
    let consume = async {
        let nonce = endpoint.await.map_err(|_| Error::ServiceStopped)?;
        let mut input = crate::recordings::connect_retained_for_test(&directory, &nonce).await?;
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).await?;
        Ok::<_, Error>(bytes)
    };
    let (receipt, bytes) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::try_join!(stream, consume)
    })
    .await??;
    assert_eq!(bytes, ORIGINAL);
    library.store_mut().finish_retained_reader(&receipt, 51)?;
    Ok(())
}

fn restart_holds(library: &mut Library) -> Result<()> {
    // These admissions represent a crash after durable commit, before actor dispatch.
    library
        .store_mut()
        .admit_retained_finding("cited", "desk", "passage", 60)?;
    library
        .store_mut()
        .admit_retained_range("cancelled-range", "spoken", 375_000, 500_000, 61)?;
    library
        .store_mut()
        .cancel_retained_reader("cancelled-range", 1, 62)?;
    library
        .store_mut()
        .admit_retained_reader("legacy", "spoken", 0, 63)?;
    library
        .store_mut()
        .admit_retained_range("fourth", "spoken", 0, 125_000, 64)?;
    assert_eq!(library.store_mut().recover_retained_readers(70)?, 4);
    Ok(())
}

#[tokio::test]
async fn exact_excerpt_and_citation_history_restore_with_bytes_and_four_restart_holds() -> TestResult
{
    let root = tempfile::tempdir()?;
    let mut original = cited_library(&root.path().join("library"))?;
    complete_original(&mut original).await?;
    restart_holds(&mut original)?;
    let readers = original.store().retained_readers()?;
    let reader_json = serde_json::to_string(&readers)?;
    let finding = original.store().finding("desk", "passage")?;
    let recordings = serde_json::to_value(original.store().recordings(None, 10)?)?;
    let manifest = backup(&original, &root.path().join("backup"))?;
    assert_eq!(manifest.media.len(), 1);
    assert_eq!(manifest.media_bytes, u64::try_from(ORIGINAL.len())?);
    assert_eq!(verify(&root.path().join("backup"))?, manifest);
    drop(original);
    let destination = root.path().join("restored");
    restore(&root.path().join("backup"), &destination)?;
    let mut restored = Library::open(&destination, false)?;
    assert_eq!(
        serde_json::to_string(&restored.store().retained_readers()?)?,
        reader_json
    );
    assert_eq!(restored.store().finding("desk", "passage")?, finding);
    assert_eq!(
        serde_json::to_value(restored.store().recordings(None, 10)?)?,
        recordings
    );
    for object in &manifest.media {
        assert_eq!(fs::read(media_file(&destination, &object.key)?)?, ORIGINAL);
    }
    let cited = readers
        .iter()
        .find(|view| view.spec.request_id == "cited")
        .ok_or("cited receipt")?;
    assert_eq!(
        restored
            .store_mut()
            .admit_retained_finding("cited", "desk", "passage", -1)?,
        (cited.spec.clone(), false)
    );
    assert_eq!(
        (
            cited.spec.file_seek_us,
            cited.spec.playback_end_us()?,
            cited.spec.playback_duration_us()?
        ),
        (125_000, 375_000, 250_000)
    );
    assert!(
        restored
            .store_mut()
            .admit_retained_range("fifth", "spoken", 125_000, 375_000, 71)
            .is_err()
    );
    assert!(matches!(
        restored.store().retained_reader("fifth"),
        Err(Error::NotFound)
    ));
    assert!(restored.store_mut().begin_delete("spoken", false).is_err());
    assert!(matches!(
        restored.store_mut().cancel_retained_reader("cited", 2, 72),
        Err(Error::RequestState)
    ));
    assert_eq!(restored.store().retained_reader("cited")?, *cited);
    assert_eq!(restored.store_mut().recover_retained_readers(73)?, 0);
    restored.store().audit_retained_readers()?;
    Ok(())
}

fn damage_catalog(path: &Path, bounds: bool) -> Result<()> {
    let connection = rusqlite::Connection::open(path)?;
    let identity: String = connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE name='retained_readers_identity'",
        [],
        |row| row.get(0),
    )?;
    connection.execute_batch(
        "DROP TRIGGER retained_readers_identity; PRAGMA ignore_check_constraints=ON;",
    )?;
    if bounds {
        connection.execute(
            "UPDATE retained_readers SET excerpt_end_us=seek_us WHERE id='cited'",
            [],
        )?;
    } else {
        connection.execute(
            "UPDATE retained_readers SET spec_sha256=?1 WHERE id='cited'",
            ["a".repeat(64)],
        )?;
    }
    connection.execute_batch(&identity)?;
    connection.execute_batch("PRAGMA ignore_check_constraints=OFF;")?;
    Ok(())
}

#[test]
fn valid_sqlite_wrong_excerpt_hash_after_open_cannot_create_backup_destination() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut original = cited_library(&root.path().join("library"))?;
    original
        .store_mut()
        .admit_retained_finding("cited", "desk", "passage", 50)?;
    // Deliberate local fault injection under the current library owner.
    damage_catalog(&original.directory().join(CATALOG), false)?;
    assert!(original.store().integrity_ok()?);
    let destination = root.path().join("refused");
    assert!(matches!(
        backup(&original, &destination),
        Err(Error::StorageIntegrity)
    ));
    assert!(!destination.exists());
    Ok(())
}

#[test]
fn self_consistent_outer_backup_hash_cannot_restore_invalid_excerpt_hash_or_bounds() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut original = cited_library(&root.path().join("library"))?;
    original
        .store_mut()
        .admit_retained_finding("cited", "desk", "passage", 50)?;
    let manifest = backup(&original, &root.path().join("backup"))?;
    drop(original);
    for bounds in [false, true] {
        let name = if bounds { "bounds" } else { "hash" };
        let source = root.path().join(name);
        copy_tree(&root.path().join("backup"), &source)?;
        damage_catalog(&source.join(CATALOG), bounds)?;
        let mut changed = manifest.clone();
        changed.catalog = hash_file(&source.join(CATALOG), MAX_CATALOG_BYTES)?;
        fs::write(source.join(MANIFEST), serde_json::to_vec(&changed)?)?;
        assert_eq!(verify(&source)?, changed);
        let destination = root.path().join(format!("restored-{name}"));
        let outcome = restore(&source, &destination);
        // SQLite rejects the violated range CHECK before the identity audit.
        assert!(
            if bounds {
                matches!(outcome, Err(Error::CatalogIntegrity))
            } else {
                matches!(outcome, Err(Error::StorageIntegrity))
            },
            "{name}: {outcome:?}"
        );
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
