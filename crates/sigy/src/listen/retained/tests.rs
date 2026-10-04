use super::*;
use clap::Parser;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn boxed_null_failure_keeps_its_concrete_evidence_at_the_client_boundary() {
    let typed = Box::new(recordings::audio::ExcerptNullError {
        reason: "native closure unproven".into(),
        operation_failure: None,
        completed_decoder: Some(RetainedPlaybackReport {
            file_playhead_us: 375_000,
            reported_elapsed_us: 250_000,
            boundary_tolerance_us: 21,
            progress_advanced: true,
        }),
        decoder_failure: None,
        native_closure: None,
        native_failure: Some("native closure unproven".into()),
    });
    let erased: Failure = typed;
    let observed = failed_decode(erased.as_ref());
    assert!(observed.decoder.is_some() && observed.decoder_failure.is_none());
    assert_eq!(
        observed.native_failure.as_deref(),
        Some("native closure unproven")
    );
}

#[test]
fn excerpt_parser_preserves_end_and_requires_explicit_finding_request() -> TestResult {
    let cli = crate::Cli::try_parse_from([
        "sigy",
        "listen",
        "file",
        "recording-1",
        "--seek-us",
        "6000000",
        "--end-us",
        "7000000",
        "--destination",
        "null",
        "--request",
        "excerpt-1",
    ])?;
    assert!(
        matches!(cli.command, crate::Command::Listen { command: super::super::ListenCommand::File {
        id, seek_us: 6_000_000, end_us: Some(7_000_000), request: Some(request), destination,
    }} if id == "recording-1" && request == "excerpt-1" && destination == "null")
    );
    let cli = crate::Cli::try_parse_from([
        "sigy",
        "listen",
        "finding",
        "monitor-1",
        "finding-1",
        "--destination",
        "null",
        "--request",
        "citation-1",
    ])?;
    assert!(
        matches!(cli.command, crate::Command::Listen { command: super::super::ListenCommand::Finding {
        monitor, finding, request, destination,
    }} if monitor == "monitor-1" && finding == "finding-1" && request == "citation-1" && destination == "null")
    );
    for args in [
        vec!["sigy", "listen", "finding", "monitor-1", "finding-1"],
        vec!["sigy", "listen", "file", "recording-1", "--end-us", "-1"],
        vec![
            "sigy",
            "listen",
            "finding",
            "monitor-1",
            "finding-1",
            "--request",
            "bad request",
        ],
    ] {
        assert!(crate::Cli::try_parse_from(args).is_err());
    }
    Ok(())
}

fn excerpt_receipt(citation: bool) -> RetainedReadView {
    let mut view = receipt();
    view.spec.excerpt = Some(control::RetainedExcerpt {
        version: 2,
        timeline_end_us: 7_000_000,
        citation: citation.then(|| control::RetainedCitation {
            monitor_id: "monitor-1".into(),
            finding_id: "finding-1".into(),
            transcript_id: "transcript-1".into(),
            transcript_revision: 2,
            translation_revision: 3,
            cue_ordinal: 4,
        }),
    });
    view
}

#[test]
fn replay_scope_refuses_mode_end_and_citation_drift_before_decoder_use() -> TestResult {
    let range = Scope::File {
        recording: "recording-1",
        seek_us: 6_000_000,
        end_us: Some(7_000_000),
    };
    let legacy = Scope::File {
        recording: "recording-1",
        seek_us: 6_000_000,
        end_us: None,
    };
    let finding = Scope::Finding {
        monitor: "monitor-1",
        finding: "finding-1",
    };
    range.validate(&excerpt_receipt(false))?;
    finding.validate(&excerpt_receipt(true))?;
    assert!(legacy.validate(&excerpt_receipt(false)).is_err());
    assert!(range.validate(&receipt()).is_err());
    assert!(range.validate(&excerpt_receipt(true)).is_err());
    assert!(finding.validate(&excerpt_receipt(false)).is_err());
    assert!(
        Scope::Finding {
            monitor: "other",
            finding: "finding-1"
        }
        .validate(&excerpt_receipt(true))
        .is_err()
    );
    assert!(
        Scope::Finding {
            monitor: "monitor-1",
            finding: "other"
        }
        .validate(&excerpt_receipt(true))
        .is_err()
    );
    assert!(
        Scope::File {
            recording: "recording-1",
            seek_us: 6_000_000,
            end_us: Some(7_000_001)
        }
        .validate(&excerpt_receipt(false))
        .is_err()
    );
    assert!(
        Scope::File {
            recording: "recording-1",
            seek_us: 7,
            end_us: Some(7)
        }
        .operation("reader".into())
        .is_err()
    );
    for end in [6_000_000, 15_000_001, u64::MAX] {
        let mut bad = excerpt_receipt(true);
        bad.spec
            .excerpt
            .as_mut()
            .ok_or("excerpt missing")?
            .timeline_end_us = end;
        assert!(finding.validate(&bad).is_err());
    }
    let mut bad = excerpt_receipt(true);
    bad.spec.excerpt.as_mut().ok_or("excerpt missing")?.version = 3;
    assert!(finding.validate(&bad).is_err());
    let mut bad = excerpt_receipt(true);
    bad.spec
        .excerpt
        .as_mut()
        .ok_or("excerpt missing")?
        .citation
        .as_mut()
        .ok_or("citation missing")?
        .transcript_revision = 0;
    assert!(finding.validate(&bad).is_err());
    Ok(())
}

#[test]
fn excerpt_receipt_reports_selected_end_separately_from_original_object_and_output() -> TestResult {
    let result = receipt_replay(excerpt_receipt(true), PlaybackDestination::Null);
    let mut bytes = Vec::new();
    render_play(&mut bytes, &result, None, true, true)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["requested_end_us"], 7_000_000);
    assert_eq!(json["reader"]["spec"]["timeline_end_us"], 15_000_000);
    assert_eq!(
        json["reader"]["spec"]["excerpt"]["citation"]["finding_id"],
        "finding-1"
    );
    assert_eq!(json["replayed"], true);
    assert_eq!(json["decoder_completed"], false);
    assert!(json["audio_output"].is_null());
    let mut bytes = Vec::new();
    render_page(
        &mut bytes,
        &RetainedPage {
            entries: vec![excerpt_receipt(false)],
            pipe_nonce: None,
            newly_started: None,
        },
        false,
    )?;
    assert!(String::from_utf8(bytes)?.contains("6000000..7000000 us"));
    Ok(())
}

#[test]
fn null_native_failure_preserves_completed_decoder_without_inventing_output_failure() -> TestResult
{
    let error = recordings::audio::ExcerptNullError {
        reason: "silent PCM completed".into(),
        operation_failure: None,
        completed_decoder: Some(RetainedPlaybackReport {
            file_playhead_us: 2_000_000,
            reported_elapsed_us: 1_000_000,
            boundary_tolerance_us: 21,
            progress_advanced: true,
        }),
        decoder_failure: None,
        native_closure: None,
        native_failure: Some("drain unproven".into()),
    };
    let observed = failed_decode(&error);
    assert!(observed.decoder.is_some() && observed.decoder_failure.is_none());
    assert!(observed.output_failure.is_none());
    assert_eq!(observed.native_failure.as_deref(), Some("drain unproven"));
    let result = PlayResult {
        receipt: excerpt_receipt(false),
        destination: PlaybackDestination::Null,
        decoder: observed.decoder,
        decoder_failure: observed.decoder_failure,
        output_failure: observed.output_failure,
        native_failure: observed.native_failure,
        operation_failure: observed.operation_failure,
        audio_output: observed.audio_output,
        replayed: false,
    };
    let mut bytes = Vec::new();
    render_play(&mut bytes, &result, None, true, true)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["decoder_completed"], true);
    assert!(json["decoder_failure"].is_null() && json["output_failure"].is_null());
    assert_eq!(json["native_failure"], "drain unproven");
    assert!(json["audio_output"]["native_closure"].is_null());
    let mut bytes = Vec::new();
    render_play(&mut bytes, &result, None, true, false)?;
    let text = String::from_utf8(bytes)?;
    assert!(text.contains("Native decoder closure unproven"));
    assert!(!text.contains("Native group closure observed"));
    let error = recordings::audio::ExcerptNullError {
        reason: "short input".into(),
        completed_decoder: None,
        operation_failure: Some("short input".into()),
        decoder_failure: Some("short input".into()),
        native_failure: None,
        native_closure: Some(recordings::audio::AudioClosure {
            mechanism: "fixture-empty-group".into(),
            peak_memory_bytes: None,
            cpu_time_us: None,
        }),
    };
    let observed = failed_decode(&error);
    assert!(observed.decoder.is_none() && observed.native_failure.is_none());
    assert_eq!(observed.decoder_failure.as_deref(), Some("short input"));
    assert_eq!(
        observed.audio_output.ok_or("null proof missing")?["native_closure"]["mechanism"],
        "fixture-empty-group"
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn output_failure_preserves_completed_decoder_and_observed_native_closure() -> TestResult {
    let error = crate::audio::play::OutputError {
        reason: "audio-device-failed".into(),
        native_closure: Some(Box::new(sigy_service::recordings::audio::AudioClosure {
            mechanism: "job_object".into(),
            peak_memory_bytes: Some(1234),
            cpu_time_us: Some(5678),
        })),
        decoder: Some(RetainedPlaybackReport {
            file_playhead_us: 10_000_000,
            reported_elapsed_us: 9_000_000,
            boundary_tolerance_us: 100_000,
            progress_advanced: true,
        }),
        decoder_failure: None,
    };
    let failed = failed_decode(&error);
    assert!(failed.decoder.is_some());
    assert!(failed.decoder_failure.is_none());
    assert!(
        failed
            .output_failure
            .as_deref()
            .is_some_and(|message| message.contains("audio-device-failed"))
    );
    assert_eq!(
        failed
            .audio_output
            .as_ref()
            .ok_or("missing output observation")?["native_closure"]["peak_memory_bytes"],
        1234
    );
    let result = PlayResult {
        receipt: receipt(),
        destination: PlaybackDestination::System,
        decoder: failed.decoder,
        decoder_failure: failed.decoder_failure,
        output_failure: failed.output_failure,
        native_failure: None,
        operation_failure: None,
        audio_output: failed.audio_output,
        replayed: false,
    };
    let mut bytes = Vec::new();
    render_play(&mut bytes, &result, None, false, true)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["decoder_completed"], true);
    assert!(json["decoder_failure"].is_null());
    assert!(!json["output_failure"].is_null());
    assert_eq!(
        json["audio_output"]["native_closure"]["mechanism"],
        "job_object"
    );
    let generic = failed_decode(&std::io::Error::other("decoder failed"));
    assert!(generic.decoder.is_none() && generic.output_failure.is_none());
    assert_eq!(generic.decoder_failure.as_deref(), Some("decoder failed"));
    Ok(())
}

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
            excerpt: None,
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
        command: super::super::ListenCommand::File { id, seek_us: 6_000_000, end_us: None, request: Some(request), destination }
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
        output_failure: None,
        native_failure: None,
        operation_failure: None,
        audio_output: None,
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
        output_failure: None,
        native_failure: None,
        operation_failure: None,
        audio_output: None,
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
