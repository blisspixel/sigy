use std::sync::mpsc;

use tokio::sync::oneshot;

use super::*;
use crate::library::Library;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

const KEY: &str = "0123456789abcdef0123456789abcdef";
const SECOND_KEY: &str = "abcdef0123456789abcdef0123456789";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const TEST_DEADLINE: Duration = Duration::from_secs(5);

fn fixture(data: &[u8]) -> Result<(tempfile::TempDir, VerificationInput)> {
    let directory = tempfile::tempdir()?;
    crate::recordings::checked_directory(directory.path(), true)?;
    let file = add_file(directory.path(), KEY, data)?;
    let input = VerificationInput {
        bytes: file.bytes,
        files: vec![file],
    };
    Ok((directory, input))
}

fn add_file(directory: &Path, key: &str, data: &[u8]) -> Result<InputFile> {
    std::fs::write(crate::recordings::media_path(directory, key)?, data)?;
    Ok(InputFile {
        key: key.into(),
        sha256: hex(&Sha256::digest(data)),
        bytes: data.len() as u64,
    })
}

fn run(directory: &Path, input: &VerificationInput) -> Result<VerificationReceipt> {
    verify_files(
        directory,
        input,
        &AtomicBool::new(false),
        Instant::now(),
        VERIFY_DEADLINE,
    )
}

fn assert_analysis_error(result: &Result<VerificationReceipt>, expected: &str) {
    assert!(
        matches!(result, Err(Error::Analysis(reason)) if *reason == expected),
        "expected {expected}, received {result:?}"
    );
}

#[test]
fn verifies_known_sha256_and_returns_bound_manifest() -> TestResult {
    let (directory, mut input) = fixture(b"abc")?;
    input.files[0].sha256 = ABC_SHA256.into();
    let receipt = run(directory.path(), &input)?;
    assert_eq!(receipt.bytes, 3);
    assert_eq!(receipt.files, 1);
    assert_eq!(receipt.manifest_sha256, manifest_digest(&input.files)?);
    assert_eq!(
        std::fs::read(crate::recordings::media_path(directory.path(), KEY)?)?,
        b"abc"
    );
    Ok(())
}

#[test]
fn hashes_multiple_chunks_and_binds_manifest_file_order() -> TestResult {
    let data = vec![0xa5; 131_073];
    let (directory, mut input) = fixture(&data)?;
    let second = add_file(directory.path(), SECOND_KEY, b"second file")?;
    input.bytes += second.bytes;
    input.files.push(second);
    let receipt = run(directory.path(), &input)?;
    assert_eq!(receipt.files, 2);
    assert_eq!(receipt.bytes, 131_084);
    assert_eq!(receipt.manifest_sha256, manifest_digest(&input.files)?);
    input.files.reverse();
    let reordered = run(directory.path(), &input)?;
    assert_eq!(reordered.bytes, receipt.bytes);
    assert_ne!(reordered.manifest_sha256, receipt.manifest_sha256);
    Ok(())
}

#[test]
fn changed_content_fails_even_when_length_matches() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let path = crate::recordings::media_path(directory.path(), KEY)?;
    std::fs::write(&path, b"abd")?;
    assert_analysis_error(&run(directory.path(), &input), "input-checksum-mismatch");
    assert_eq!(std::fs::read(path)?, b"abd");
    Ok(())
}

#[test]
fn shortened_and_extended_inputs_fail_without_rewriting_files() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let path = crate::recordings::media_path(directory.path(), KEY)?;
    for changed in [&b"ab"[..], &b"abcd"[..]] {
        std::fs::write(&path, changed)?;
        assert_analysis_error(&run(directory.path(), &input), "input-size-mismatch");
        assert_eq!(std::fs::read(&path)?, changed);
    }
    Ok(())
}

#[test]
fn missing_input_is_refused_without_recreating_it() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let path = crate::recordings::media_path(directory.path(), KEY)?;
    std::fs::remove_file(&path)?;
    assert_analysis_error(&run(directory.path(), &input), "input-unavailable");
    assert!(!path.exists());
    Ok(())
}

#[test]
fn object_keys_cannot_select_arbitrary_paths() -> TestResult {
    let (directory, mut input) = fixture(b"abc")?;
    for invalid in [
        "../outside",
        "..\\outside",
        "C:\\outside",
        "/outside",
        "0123456789ABCDEF0123456789ABCDEF",
        "0123456789abcdef0123456789abcdef.media",
        "",
    ] {
        input.files[0].key = invalid.into();
        assert!(matches!(
            run(directory.path(), &input),
            Err(Error::StorageIntegrity)
        ));
    }
    Ok(())
}

#[test]
fn a_directory_cannot_replace_an_input_file() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let path = crate::recordings::media_path(directory.path(), KEY)?;
    std::fs::remove_file(&path)?;
    std::fs::create_dir(&path)?;
    assert!(matches!(
        run(directory.path(), &input),
        Err(Error::InvalidInput("library-owned file path"))
    ));
    assert!(path.is_dir());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symbolic_link_inputs_are_refused() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let path = crate::recordings::media_path(directory.path(), KEY)?;
    let target = directory.path().join("outside");
    std::fs::write(&target, b"abc")?;
    std::fs::remove_file(&path)?;
    std::os::unix::fs::symlink(&target, &path)?;
    assert!(matches!(
        run(directory.path(), &input),
        Err(Error::InvalidInput("library-owned file path"))
    ));
    assert_eq!(std::fs::read(target)?, b"abc");
    Ok(())
}

#[test]
fn aggregate_input_limits_are_checked_before_filesystem_access() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let missing = directory.path().join("absent");
    let invalid = [
        VerificationInput {
            files: Vec::new(),
            bytes: 3,
        },
        VerificationInput {
            files: input.files.clone(),
            bytes: 0,
        },
        VerificationInput {
            files: input.files.clone(),
            bytes: MAX_INPUT_BYTES + 1,
        },
        VerificationInput {
            files: vec![input.files[0].clone(); MAX_INPUT_FILES + 1],
            bytes: 3,
        },
    ];
    for input in invalid {
        assert_analysis_error(&run(&missing, &input), "input-limit");
    }
    assert!(!missing.exists());
    Ok(())
}

#[test]
fn file_sizes_must_fit_and_exactly_account_for_the_total() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    for bytes in [0, 4] {
        let mut invalid = input.clone();
        invalid.files[0].bytes = bytes;
        assert_analysis_error(&run(directory.path(), &invalid), "input-limit");
    }
    let mut leftover = input.clone();
    leftover.bytes = 4;
    assert_analysis_error(&run(directory.path(), &leftover), "input-limit");
    let mut repeated = input;
    repeated.files.push(repeated.files[0].clone());
    assert_analysis_error(&run(directory.path(), &repeated), "input-limit");
    Ok(())
}

#[test]
fn oversized_manifest_is_refused_before_filesystem_access() -> TestResult {
    let (directory, mut input) = fixture(b"abc")?;
    input.files[0].sha256 = "a".repeat(65_536);
    let missing = directory.path().join("absent");
    assert_analysis_error(&run(&missing, &input), "input-manifest-limit");
    assert!(!missing.exists());
    Ok(())
}

#[test]
fn cancelled_and_expired_work_does_not_open_inputs() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let missing = directory.path().join("absent");
    assert_analysis_error(
        &verify_files(
            &missing,
            &input,
            &AtomicBool::new(true),
            Instant::now(),
            VERIFY_DEADLINE,
        ),
        "cancelled",
    );
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .ok_or("test clock range")?;
    for (began, deadline) in [
        (Instant::now(), Duration::ZERO),
        (expired, Duration::from_millis(1)),
    ] {
        assert_analysis_error(
            &verify_files(&missing, &input, &AtomicBool::new(false), began, deadline),
            "deadline",
        );
    }
    Ok(())
}

#[tokio::test]
async fn async_verification_reads_local_files_and_honors_preset_cancellation() -> TestResult {
    let (directory, input) = fixture(b"abc")?;
    let library = Library::open(directory.path(), true)?;
    let (sender, signal) = watch::channel(false);
    let receipt = verify(
        directory.path().into(),
        input.clone(),
        library.hold_ownership(),
        signal.clone(),
    )
    .await?;
    assert_eq!(receipt.bytes, 3);
    assert_eq!(receipt.manifest_sha256, manifest_digest(&input.files)?);
    sender.send_replace(true);
    assert_analysis_error(
        &verify(
            directory.path().join("absent"),
            input,
            library.hold_ownership(),
            signal,
        )
        .await,
        "cancelled",
    );
    Ok(())
}

fn gated_work(
    started: oneshot::Sender<()>,
    stopped: oneshot::Sender<()>,
    release: mpsc::Receiver<()>,
) -> impl FnOnce(&AtomicBool) -> Result<VerificationReceipt> + Send + 'static {
    move |stop| {
        started
            .send(())
            .map_err(|()| Error::Analysis("test-start-receiver-closed"))?;
        let began = Instant::now();
        let mut stopped = Some(stopped);
        loop {
            if stop.load(Ordering::Acquire)
                && let Some(stopped) = stopped.take()
            {
                let _ = stopped.send(());
            }
            match release.recv_timeout(Duration::from_millis(10)) {
                Ok(()) => break,
                Err(mpsc::RecvTimeoutError::Timeout) if began.elapsed() < TEST_DEADLINE => (),
                Err(_) => return Err(Error::Analysis("test-release-unavailable")),
            }
        }
        check_stop(stop, began, TEST_DEADLINE)?;
        Ok(VerificationReceipt {
            bytes: 1,
            files: 1,
            manifest_sha256: "a".repeat(64),
        })
    }
}

async fn wait_for_library_release(directory: &Path) -> TestResult {
    tokio::time::timeout(TEST_DEADLINE, async {
        loop {
            match Library::open(directory, false) {
                Ok(library) => {
                    drop(library);
                    return Ok(());
                }
                Err(Error::LibraryBusy) => tokio::task::yield_now().await,
                Err(error) => return Err(error),
            }
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn cancellation_retains_ownership_until_blocking_work_finishes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let ownership = library.hold_ownership();
    let (sender, signal) = watch::channel(false);
    let (started, ready) = oneshot::channel();
    let (stopped, observed_stop) = oneshot::channel();
    let (release, wait) = mpsc::channel();
    let worker = tokio::spawn(supervise(
        ownership,
        signal,
        gated_work(started, stopped, wait),
    ));
    tokio::time::timeout(TEST_DEADLINE, ready).await??;
    drop(library);
    sender.send_replace(true);
    tokio::time::timeout(TEST_DEADLINE, observed_stop).await??;
    assert!(!worker.is_finished());
    assert!(matches!(
        Library::open(directory.path(), false),
        Err(Error::LibraryBusy)
    ));
    release.send(())?;
    assert_analysis_error(
        &tokio::time::timeout(TEST_DEADLINE, worker).await??,
        "cancelled",
    );
    drop(Library::open(directory.path(), false)?);
    Ok(())
}

#[tokio::test]
async fn aborting_supervisor_cannot_release_active_blocking_ownership() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let ownership = library.hold_ownership();
    let (_sender, signal) = watch::channel(false);
    let (started, ready) = oneshot::channel();
    let (stopped, observed_stop) = oneshot::channel();
    let (release, wait) = mpsc::channel();
    let worker = tokio::spawn(supervise(
        ownership,
        signal,
        gated_work(started, stopped, wait),
    ));
    tokio::time::timeout(TEST_DEADLINE, ready).await??;
    drop(library);
    worker.abort();
    assert!(
        tokio::time::timeout(TEST_DEADLINE, worker)
            .await?
            .is_err_and(|error| error.is_cancelled())
    );
    tokio::time::timeout(TEST_DEADLINE, observed_stop).await??;
    assert!(matches!(
        Library::open(directory.path(), false),
        Err(Error::LibraryBusy)
    ));
    release.send(())?;
    wait_for_library_release(directory.path()).await?;
    Ok(())
}

#[tokio::test]
async fn panicking_blocking_work_reports_failure_and_releases_ownership() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = Library::open(directory.path(), true)?;
    let ownership = library.hold_ownership();
    let (_sender, signal) = watch::channel(false);
    drop(library);
    let result = supervise(ownership, signal, |_| panic!("fixture worker failure")).await;
    assert_analysis_error(&result, "worker-panicked");
    drop(Library::open(directory.path(), false)?);
    Ok(())
}
