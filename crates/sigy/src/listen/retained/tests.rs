use super::*;
use clap::Parser;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn only_exact_local_or_remote_missing_receipt_allows_fresh_admission() {
    assert!(is_not_found(&sigy_service::Error::NotFound));
    assert!(is_not_found(&sigy_service::Error::Remote(
        sigy_service::Error::NotFound.to_string()
    )));
    for error in [
        sigy_service::Error::StorageIntegrity,
        sigy_service::Error::LibraryBusy,
        sigy_service::Error::Remote("resource not found, because integrity failed".into()),
        sigy_service::Error::Remote(sigy_service::Error::StorageIntegrity.to_string()),
    ] {
        assert!(!is_not_found(&error));
    }
    assert!(!is_not_found(&std::io::Error::other(
        sigy_service::Error::NotFound.to_string()
    )));
}

fn receipt() -> RetainedReadView {
    RetainedReadView {
        spec: control::RetainedReadSpec {
            request_id: "reader-1".into(),
            generation: 1,
            recording_id: "recording-1".into(),
            source_revision: "source:1".into(),
            ordinal: 2,
            object_key: "a".repeat(32),
            sha256: "b".repeat(64),
            format: "wav".into(),
            bytes: 128,
            timeline_start_us: 5_000_000,
            timeline_end_us: 15_000_000,
            file_seek_us: 1_000_000,
            file_duration_us: 10_000_000,
            spec_sha256: "c".repeat(64),
        },
        state: "failed".into(),
        seek_us: 6_000_000,
        admitted_ms: 1,
        updated_ms: 2,
        completion_reason: Some("client-disconnected".into()),
        recovery_reason: None,
    }
}

#[test]
fn parser_preserves_exact_reader_requests_and_generation() -> TestResult {
    let cli = crate::Cli::try_parse_from([
        "sigy",
        "listen",
        "file",
        "recording-1",
        "--seek-us",
        "6000000",
        "--request",
        "reader-1",
        "--destination",
        "null",
    ])?;
    assert!(matches!(cli.command, crate::Command::Listen {
        command: super::super::ListenCommand::File { id, seek_us: 6_000_000, request: Some(request), destination }
    } if id == "recording-1" && request == "reader-1" && destination == "null"));
    let cli = crate::Cli::try_parse_from([
        "sigy",
        "listen",
        "reader",
        "stop",
        "reader-1",
        "--generation",
        "3",
    ])?;
    assert!(matches!(cli.command, crate::Command::Listen {
        command: super::super::ListenCommand::Reader { command: ReaderCommand::Stop { id, generation: 3 } }
    } if id == "reader-1"));
    assert!(crate::Cli::try_parse_from(["sigy", "listen", "reader", "stop", "reader-1"]).is_err());
    assert!(
        crate::Cli::try_parse_from([
            "sigy",
            "listen",
            "reader",
            "stop",
            "reader-1",
            "--generation",
            "-1"
        ])
        .is_err()
    );
    assert!(reader_id(&"a".repeat(128)).is_ok());
    assert!(reader_id(&"a".repeat(129)).is_err());
    for malformed in ["", "a b", "a;whoami", "عربية", "a\u{1b}"] {
        assert!(reader_id(malformed).is_err());
    }
    for action in ["show", "list"] {
        let mut args = vec!["sigy", "listen", "reader", action];
        if action == "show" {
            args.push("reader-1");
        }
        crate::Cli::try_parse_from(args)?;
    }
    Ok(())
}

#[tokio::test]
async fn absent_decoder_refuses_before_reader_admission_or_source_contact() -> TestResult {
    let directory = tempfile::tempdir()?;
    drop(sigy_service::library::Library::open(
        directory.path(),
        true,
    )?);
    let error = play(
        directory.path(),
        "missing",
        PlaybackDestination::Null,
        0,
        Some("reader-1"),
    )
    .await
    .err()
    .ok_or("decoder-less playback admitted")?;
    assert!(error.to_string().contains("no decoder is configured"));
    let snapshot = super::super::view(
        directory.path(),
        Operation::Retained {
            command: RetainedOperation::List {},
        },
    )
    .await?;
    assert!(snapshot.retained.ok_or("reader list")?.entries.is_empty());
    Ok(())
}

#[test]
fn receipt_identity_and_prefix_gap_math_are_checked_independently() -> TestResult {
    let view = receipt();
    validate_scope(&view, "recording-1", 6_000_000)?;
    let mut bad = view.clone();
    bad.spec.file_seek_us = 6_000_000;
    assert!(validate_scope(&bad, "recording-1", 6_000_000).is_err());
    assert!(validate_scope(&view, "other", 6_000_000).is_err());
    bad = view.clone();
    bad.spec.timeline_start_us = u64::MAX;
    assert!(validate_scope(&bad, "recording-1", 6_000_000).is_err());
    let page = RetainedPage {
        entries: vec![view.clone()],
        pipe_nonce: None,
        newly_started: Some(false),
    };
    assert_eq!(one(&page, "reader-1")?, view);
    assert!(one(&page, "other").is_err());
    let duplicate = RetainedPage {
        entries: vec![view.clone(), view],
        pipe_nonce: None,
        newly_started: None,
    };
    assert!(one(&duplicate, "reader-1").is_err());
    Ok(())
}

#[test]
fn reported_progress_is_unclamped_and_transfer_failure_is_separate() -> TestResult {
    let result = PlayResult {
        receipt: receipt(),
        destination: PlaybackDestination::Null,
        decoder: Some(RetainedPlaybackReport {
            file_playhead_us: 10_050_000,
            reported_elapsed_us: 9_050_000,
            boundary_tolerance_us: 100_000,
            progress_advanced: true,
        }),
        decoder_failure: None,
        replayed: false,
    };
    let mut bytes = Vec::new();
    render_play(&mut bytes, &result, Some("session-1"), false, true)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["playhead_us"], 15_050_000);
    assert_eq!(json["requested_end_us"], 15_000_000);
    assert_eq!(json["reported_elapsed_us"], 9_050_000);
    assert_eq!(json["reader"]["state"], "failed");
    assert_eq!(json["decoder_completed"], true);
    assert_eq!(json["detached"], false);
    let mut bytes = Vec::new();
    render_play(&mut bytes, &result, None, true, false)?;
    let text = String::from_utf8(bytes)?;
    assert!(text.contains("client-disconnected") && text.contains("does not prove audible"));
    assert!(check_outcome(&result).is_err());
    let mut result = result;
    for expected_close in ["cancelled", "retained-pipe-failed"] {
        result.receipt.completion_reason = Some(expected_close.into());
        check_outcome(&result)?;
    }
    for integrity_failure in ["input-checksum-mismatch", "retained-read-failed", "unknown"] {
        result.receipt.completion_reason = Some(integrity_failure.into());
        assert!(check_outcome(&result).is_err());
    }
    Ok(())
}

#[test]
fn replay_and_recovery_render_no_decoder_success_and_sanitize_plain_text() -> TestResult {
    let mut view = receipt();
    view.spec.request_id = "reader\u{1b}[2J".into();
    view.state = "recovery_held".into();
    view.recovery_reason = Some("unproven\u{1b}]52;c;x\u{7}".into());
    let page = RetainedPage {
        entries: vec![view.clone()],
        pipe_nonce: None,
        newly_started: None,
    };
    let mut bytes = Vec::new();
    render_page(&mut bytes, &page, false)?;
    let text = String::from_utf8(bytes)?;
    assert!(text.contains("Protection held"));
    assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
    let result = PlayResult {
        receipt: view,
        destination: PlaybackDestination::Null,
        decoder: None,
        decoder_failure: None,
        replayed: true,
    };
    bytes = Vec::new();
    render_play(&mut bytes, &result, None, true, true)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["replayed"], true);
    assert_eq!(json["decoder_completed"], false);
    assert!(json["playhead_us"].is_null() && json["reported_elapsed_us"].is_null());
    Ok(())
}
