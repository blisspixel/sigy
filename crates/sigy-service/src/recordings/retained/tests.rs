use super::*;
use crate::library::Library;
use tokio::{io::AsyncReadExt, sync::oneshot};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const TEST_DEADLINE: Duration = Duration::from_secs(5);

fn fixture(directory: &Path, bytes: &[u8]) -> Result<RetainedReadSpec> {
    super::super::checked_directory(directory, true)?;
    let spec = RetainedReadSpec {
        request_id: "reader".into(),
        generation: 1,
        recording_id: "recording".into(),
        source_revision: "revision".into(),
        ordinal: 3,
        object_key: "ab".repeat(16),
        sha256: hex(&Sha256::digest(bytes)),
        format: "wav".into(),
        bytes: bytes.len() as u64,
        timeline_start_us: 7_000_000,
        timeline_end_us: 9_000_000,
        file_seek_us: 200_000,
        file_duration_us: 2_000_000,
        spec_sha256: "cd".repeat(32),
        excerpt: None,
    };
    std::fs::write(
        super::super::media_path(directory, &spec.object_key)?,
        bytes,
    )?;
    Ok(spec)
}

async fn read_fixture(
    directory: PathBuf,
    spec: RetainedReadSpec,
    ownership: Arc<File>,
) -> Result<(RetainedReadReceipt, Vec<u8>)> {
    let (_running, signal) = watch::channel(false);
    let (ready, nonce) = oneshot::channel();
    let client_directory = directory.clone();
    let worker = stream_retained(
        directory,
        spec,
        ownership,
        signal,
        move |nonce| async move { ready.send(nonce).map_err(|_| Error::ServiceStopped) },
    );
    let client = async {
        let nonce = nonce.await.map_err(|_| Error::ServiceStopped)?;
        let Ok(mut input) =
            super::super::pipe::connect_listen_pipe(&client_directory, &nonce).await
        else {
            // A failed verification may close its endpoint before this client
            // connects. Successful transfer assertions still require every byte.
            return Ok::<_, Error>(Vec::new());
        };
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).await?;
        Ok::<_, Error>(bytes)
    };
    let (receipt, bytes) =
        tokio::time::timeout(TEST_DEADLINE, async { tokio::join!(worker, client) })
            .await
            .map_err(|_| Error::Timeout)?;
    Ok((receipt?, bytes?))
}

#[tokio::test]
async fn verified_original_streams_exact_bytes_and_closes_before_receipt() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let bytes: Vec<u8> = (0..(CHUNK_BYTES * 9 + 37))
        .map(|index| u8::try_from(index % 251).unwrap_or(0))
        .collect();
    let spec = fixture(library.directory(), &bytes)?;
    let ownership = library.hold_ownership();
    let path = library.directory().to_owned();
    drop(library);
    let (receipt, observed) = read_fixture(path.clone(), spec.clone(), ownership).await?;
    assert_eq!(observed, bytes);
    assert!(receipt.successful());
    assert_eq!(receipt.reason(), "completed");
    assert_eq!(receipt.id(), "reader");
    assert_eq!(receipt.generation(), 1);
    assert!(receipt.matches(&spec));
    let mut changed = spec;
    changed.ordinal += 1;
    assert!(!receipt.matches(&changed));
    drop(Library::open(&path, false)?);
    Ok(())
}

#[tokio::test]
async fn wrong_checksum_or_size_sends_no_unverified_bytes() -> TestResult {
    for wrong_size in [false, true] {
        let directory = tempfile::tempdir()?;
        let library = Library::open(directory.path(), true)?;
        let mut spec = fixture(library.directory(), b"retained encoded observation")?;
        if wrong_size {
            spec.bytes += 1;
        } else {
            spec.sha256 = "00".repeat(32);
        }
        let (receipt, bytes) = read_fixture(
            library.directory().to_owned(),
            spec.clone(),
            library.hold_ownership(),
        )
        .await?;
        assert!(bytes.is_empty());
        assert!(!receipt.successful());
        assert!(receipt.matches(&spec));
        assert_eq!(
            receipt.reason(),
            if wrong_size {
                "input-size-mismatch"
            } else {
                "input-checksum-mismatch"
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn prior_stop_proves_no_reader_opened_and_keeps_exact_binding() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let spec = fixture(library.directory(), b"original")?;
    let (_stopping, signal) = watch::channel(true);
    let receipt = stream_retained(
        library.directory().to_owned(),
        spec.clone(),
        library.hold_ownership(),
        signal,
        |_| async { Err(Error::Protocol("ready must not be called")) },
    )
    .await?;
    assert_eq!(receipt.reason(), "cancelled");
    assert!(!receipt.successful());
    assert!(receipt.matches(&spec));
    Ok(())
}

#[tokio::test]
async fn stalled_client_cancellation_joins_reader_and_releases_library_ownership() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let bytes = vec![7_u8; CHUNK_BYTES * 256];
    let spec = fixture(library.directory(), &bytes)?;
    let path = library.directory().to_owned();
    let ownership = library.hold_ownership();
    let (stopping, signal) = watch::channel(false);
    let (ready, nonce) = oneshot::channel();
    let worker = tokio::spawn(stream_retained(
        path.clone(),
        spec,
        ownership,
        signal,
        move |nonce| async move { ready.send(nonce).map_err(|_| Error::ServiceStopped) },
    ));
    let nonce = tokio::time::timeout(TEST_DEADLINE, nonce).await??;
    let _client = super::super::pipe::connect_listen_pipe(&path, &nonce).await?;
    drop(library);
    assert!(matches!(
        Library::open(&path, false),
        Err(Error::LibraryBusy)
    ));
    stopping.send_replace(true);
    let receipt = tokio::time::timeout(TEST_DEADLINE, worker).await???;
    assert!(!receipt.successful());
    assert!(matches!(
        receipt.reason(),
        "cancelled" | "retained-pipe-failed"
    ));
    drop(Library::open(&path, false)?);
    Ok(())
}

#[test]
fn expired_deadline_precedes_read_and_fixed_queue_holds_only_two_chunks() -> TestResult {
    let (sender, mut receiver) = mpsc::channel(QUEUED_CHUNKS);
    sender.try_send(vec![0; CHUNK_BYTES])?;
    sender.try_send(vec![0; CHUNK_BYTES])?;
    assert!(matches!(
        sender.try_send(vec![0; CHUNK_BYTES]),
        Err(mpsc::error::TrySendError::Full(_))
    ));
    assert_eq!(receiver.try_recv()?.len(), CHUNK_BYTES);
    let expired = Instant::now()
        .checked_sub(DEADLINE)
        .ok_or("deadline fixture clock")?;
    assert!(matches!(
        check_stop(&AtomicBool::new(false), expired),
        Err(Error::Analysis("deadline"))
    ));
    Ok(())
}

#[test]
fn later_mutation_is_detected_by_independent_second_pass_hash() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("original");
    std::fs::write(&path, b"first observation")?;
    let mut file = File::open(&path)?;
    let mut buffer = [0_u8; CHUNK_BYTES];
    let stop = AtomicBool::new(false);
    let first = read_pass(&mut file, 17, &mut buffer, &stop, Instant::now(), None)?;
    std::fs::write(&path, b"other observation")?;
    file.seek(SeekFrom::Start(0))?;
    let second = read_pass(&mut file, 17, &mut buffer, &stop, Instant::now(), None)?;
    assert_eq!(first, hex(&Sha256::digest(b"first observation")));
    assert_eq!(second, hex(&Sha256::digest(b"other observation")));
    assert_ne!(first, second);
    Ok(())
}

#[tokio::test]
async fn failed_transport_cannot_issue_receipt_before_gated_file_owner_joins() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let spec = fixture(library.directory(), b"gated original")?;
    let media = super::super::media_path(library.directory(), &spec.object_key)?;
    let path = library.directory().to_owned();
    let ownership = library.hold_ownership();
    let (stopping, signal) = watch::channel(false);
    let (started, ready) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let read = crate::processing::supervise(ownership, signal, move |_| {
        let _file = File::open(media)?;
        let _ = started.send(());
        wait.recv()
            .map_err(|_| Error::Analysis("fixture-gate-closed"))?;
        Ok(ReadCompletion(Err(Error::Analysis("cancelled"))))
    });
    let worker = tokio::spawn(async move {
        let result =
            join_transfer(read, async { Err(Error::Analysis("retained-pipe-failed")) }).await?;
        Ok::<_, Error>(receipt(spec, &result))
    });
    tokio::time::timeout(TEST_DEADLINE, ready).await??;
    drop(library);
    stopping.send_replace(true);
    assert!(!worker.is_finished());
    assert!(matches!(
        Library::open(&path, false),
        Err(Error::LibraryBusy)
    ));
    release.send(())?;
    let receipt = tokio::time::timeout(TEST_DEADLINE, worker).await???;
    assert!(!receipt.successful());
    drop(Library::open(&path, false)?);
    Ok(())
}

#[test]
fn malformed_object_hash_capacity_and_coordinates_refuse_before_open() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let valid = fixture(library.directory(), b"original")?;
    assert!(validate(&valid).is_ok());
    for candidate in [0, MAX_RETAINED_READ_BYTES + 1] {
        let mut invalid = valid.clone();
        invalid.bytes = candidate;
        assert!(validate(&invalid).is_err());
    }
    let mut invalid = valid.clone();
    invalid.object_key = "../outside".into();
    assert!(validate(&invalid).is_err());
    invalid = valid.clone();
    invalid.sha256 = "AB".repeat(32);
    assert!(validate(&invalid).is_err());
    invalid = valid.clone();
    invalid.timeline_end_us = invalid.timeline_start_us - 1;
    assert!(validate(&invalid).is_err());
    invalid = valid.clone();
    invalid.file_duration_us = RETAINED_READ_DEADLINE_SECONDS * 1_000_000 + 1;
    invalid.timeline_end_us = invalid.timeline_start_us + invalid.file_duration_us;
    assert!(validate(&invalid).is_err());
    invalid = valid;
    invalid.file_seek_us = invalid.file_duration_us;
    assert!(validate(&invalid).is_err());
    Ok(())
}

#[tokio::test]
async fn nonregular_owned_object_is_refused_before_open_and_sends_no_bytes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let spec = fixture(library.directory(), b"original")?;
    let owned = super::super::media_path(library.directory(), &spec.object_key)?;
    std::fs::remove_file(&owned)?;
    std::fs::create_dir(&owned)?;
    let (receipt, observed) = read_fixture(
        library.directory().to_owned(),
        spec,
        library.hold_ownership(),
    )
    .await?;
    assert!(!receipt.successful());
    assert!(observed.is_empty());
    Ok(())
}

#[tokio::test]
async fn joined_panic_returns_no_completion_capability() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let read = crate::processing::supervise(
        library.hold_ownership(),
        watch::channel(false).1,
        |_| -> Result<ReadCompletion> { panic!("retained fixture panic") },
    );
    assert!(matches!(
        join_transfer(read, async { Ok(()) }).await,
        Err(Error::Analysis("worker-panicked"))
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_at_owned_object_boundary_sends_no_bytes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let bytes = b"linked original";
    let spec = fixture(library.directory(), bytes)?;
    let owned = super::super::media_path(library.directory(), &spec.object_key)?;
    let target = library.directory().join("other");
    std::fs::write(&target, bytes)?;
    std::fs::remove_file(&owned)?;
    std::os::unix::fs::symlink(&target, &owned)?;
    let (receipt, observed) = read_fixture(
        library.directory().to_owned(),
        spec,
        library.hold_ownership(),
    )
    .await?;
    assert!(!receipt.successful());
    assert!(observed.is_empty());
    Ok(())
}
