use super::*;

#[tokio::test]
async fn only_exact_joined_completion_releases_and_terminal_replay_keeps_history() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut library = crate::library::Library::open(directory.path(), true)?;
    crate::recordings::checked_directory(directory.path(), true)?;
    configure(library.store_mut())?;
    published(library.store_mut(), "recording")?;
    let (spec, _) = library
        .store_mut()
        .admit_retained_reader("reader", "recording", 0, 10)?;
    let mut different = spec.clone();
    different.object_key = "b".repeat(32);
    let (_stop, signal) = tokio::sync::watch::channel(false);
    let wrong = crate::recordings::retained::stream_retained(
        directory.path().to_path_buf(),
        different,
        library.hold_ownership(),
        signal,
        |_| async { Ok(()) },
    )
    .await?;
    assert!(
        library
            .store_mut()
            .finish_retained_reader(&wrong, 11)
            .is_err()
    );
    assert_eq!(library.store().retained_reader("reader")?.state, "running");
    let (_stop, signal) = tokio::sync::watch::channel(false);
    let receipt = crate::recordings::retained::stream_retained(
        directory.path().to_path_buf(),
        spec.clone(),
        library.hold_ownership(),
        signal,
        |_| async { Ok(()) },
    )
    .await?;
    let finished = library.store_mut().finish_retained_reader(&receipt, 9)?;
    assert_eq!(finished.state, "failed");
    assert_eq!(finished.updated_ms, 10);
    assert_eq!(
        finished.completion_reason.as_deref(),
        Some("input-unavailable")
    );
    assert_eq!(
        library.store_mut().finish_retained_reader(&receipt, 13)?,
        finished
    );
    assert_eq!(
        library.store_mut().finish_retained_reader(&receipt, -1)?,
        finished
    );
    library.store_mut().begin_delete("recording", false)?;
    library.store_mut().finish_delete("recording")?;
    assert_eq!(
        library
            .store_mut()
            .admit_retained_reader("reader", "recording", 0, -1)?,
        (spec, false)
    );
    assert_eq!(library.store().retained_reader("reader")?, finished);
    assert!(
        library
            .store_mut()
            .admit_retained_reader("new", "recording", 0, 14)
            .is_err()
    );
    Ok(())
}
