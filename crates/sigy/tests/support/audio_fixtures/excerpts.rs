//! Independent silent excerpt samples; legacy tail qualification stays separate.

use super::*;

fn frame_window(start_us: u64, end_us: u64, rate: u32) -> TestResultWindow {
    if start_us >= end_us {
        return Err("invalid reference range".into());
    }
    let ceil = |time: u64| -> Result<u64, Box<dyn std::error::Error>> {
        Ok(time
            .checked_mul(u64::from(rate))
            .ok_or("reference overflow")?
            .checked_add(999_999)
            .ok_or("reference overflow")?
            / 1_000_000)
    };
    Ok((ceil(start_us)?, ceil(end_us)?))
}

type TestResultWindow = Result<(u64, u64), Box<dyn std::error::Error>>;

#[test]
fn half_open_sample_onsets_preserve_fractional_and_adjacent_boundaries() -> TestResult {
    assert_eq!(frame_window(125_000, 375_000, 48_000)?, (6_000, 18_000));
    assert_eq!(frame_window(375_000, 1_000_000, 48_000)?, (18_000, 48_000));
    assert_eq!(frame_window(1, 21, 48_000)?, (1, 2));
    assert_eq!(frame_window(1, 20, 48_000)?, (1, 1));
    assert_eq!(frame_window(21, 42, 48_000)?, (2, 3));
    // Absolute endpoints, not ceil(duration): this range contains no onset.
    assert_eq!(frame_window(1, 2, 44_100)?, (1, 1));
    assert!(frame_window(2, 2, 48_000).is_err());
    assert!(frame_window(3, 2, 48_000).is_err());
    assert!(frame_window(u64::MAX - 1, u64::MAX, 192_000).is_err());
    assert_eq!(expected_range()?.len(), 12_000 * 2 * 4);
    Ok(())
}

#[cfg(windows)]
#[test]
#[ignore = "silent excerpt oracle requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn native_protected_excerpt_matches_every_sample_and_excludes_both_sentinels() -> TestResult {
    use super::super::{AudioServer, RunningChild, initialize, success, wait_recording};
    let directory = tempfile::tempdir()?;
    let fixture = AudioServer::start(reference_wave()?)?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "pcm-oracle",
            "--source",
            "radio:v1",
            "--seconds",
            "5",
            "--max-mib",
            "1",
        ],
    )?;
    assert_eq!(
        wait_recording(directory.path(), "pcm-oracle", "completed")?["decoded_microseconds"],
        1_000_000
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(check_excerpt(directory.path()))?;
    check_cli_excerpt(directory.path())?;
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[cfg(windows)]
async fn check_excerpt(directory: &std::path::Path) -> TestResult {
    use sigy_service::{
        control::{self, Operation, RetainedOperation},
        recordings::audio::{
            NativeAudioGroup, PcmDecodeRequest, PcmFormat, PcmReaderRequest, decode_retained_pcm,
        },
    };
    let executable = std::env::var("SIGY_TEST_FFMPEG")?;
    let (spec, nonce) = admit_exact_excerpt(directory).await?;
    let group = NativeAudioGroup::new()?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(2);
    let decode = decode_retained_pcm(
        PcmDecodeRequest {
            reader: PcmReaderRequest {
                executable: &executable,
                spec: &spec,
                format: PcmFormat {
                    rate_hz: 48_000,
                    channels: 2,
                },
                group: &group,
            },
            directory,
            nonce: &nonce,
        },
        sender,
    );
    let collect = async {
        let mut bytes = Vec::with_capacity(EXPECTED_BYTES);
        while let Some(chunk) = receiver.recv().await {
            if chunk.is_empty() || chunk.len() > 8192 || bytes.len() + chunk.len() > EXPECTED_BYTES
            {
                return Err(sigy_service::Error::Acquisition(
                    "excerpt fixture PCM bound",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        tokio::try_join!(decode, collect)
    })
    .await;
    if !matches!(&outcome, Ok(Ok(_))) {
        group.kill()?;
    }
    let closure = group.finish().await?;
    let (decoded, bytes) = outcome??;
    assert_eq!(bytes, expected_range()?);
    assert_eq!(decoded.pcm_bytes, 96_000);
    assert_eq!(decoded.decoder.file_playhead_us, 375_000);
    assert_eq!(decoded.decoder.reported_elapsed_us, 250_000);
    assert_eq!(closure.mechanism, "job_object");
    // Each source sample in the range has magnitude <=0.5. Sentinels are outside.
    assert!(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .all(|sample| f32::from_le_bytes(*sample).abs() <= 0.5)
    );
    wait_excerpt_closed(directory, &spec).await?;
    Box::pin(direct_sample_cases(&executable, &spec)).await?;
    // Replay dispatches neither another decoder nor a protected transport.
    let replay = control::request(
        directory,
        Operation::Retained {
            command: RetainedOperation::StartRange {
                id: "pcm-excerpt".into(),
                recording_id: "pcm-oracle".into(),
                seek_us: 125_000,
                end_us: 375_000,
            },
        },
    )
    .await?
    .retained
    .ok_or("excerpt replay missing")?;
    assert!(replay.pipe_nonce.is_none());
    assert_eq!(replay.entries[0].spec, spec);
    assert!(
        control::request(
            directory,
            Operation::Retained {
                command: RetainedOperation::StartRange {
                    id: "pcm-excerpt".into(),
                    recording_id: "pcm-oracle".into(),
                    seek_us: 125_000,
                    end_us: 375_001,
                }
            }
        )
        .await
        .is_err()
    );
    eprintln!(
        "silent excerpt oracle: 12000 stereo frames, 96000 bytes, both boundary sentinels excluded; original-reader closure inspected separately"
    );
    Ok(())
}

#[cfg(windows)]
fn check_cli_excerpt(directory: &std::path::Path) -> TestResult {
    use super::super::success;
    let args = [
        "listen",
        "file",
        "pcm-oracle",
        "--seek-us",
        "125000",
        "--end-us",
        "375000",
        "--destination",
        "null",
        "--request",
        "cli-excerpt",
    ];
    let played = success(directory, &args)?;
    assert_eq!(played["requested_end_us"], 375_000);
    assert_eq!(played["decoder_completed"], true);
    assert_eq!(played["audio_output"]["pcm_bytes"], 96_000);
    assert_eq!(played["audio_output"]["format"]["rate_hz"], 48_000);
    assert_eq!(
        played["audio_output"]["native_closure"]["mechanism"],
        "job_object"
    );
    assert!(played["decoder_failure"].is_null() && played["native_failure"].is_null());
    assert!(matches!(
        played["reader"]["state"].as_str(),
        Some("completed" | "failed")
    ));
    let replay = success(directory, &args)?;
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["decoder_completed"], false);
    assert!(replay["audio_output"].is_null());
    let refused = super::super::invoke(
        directory,
        &[
            "listen",
            "file",
            "pcm-oracle",
            "--seek-us",
            "1",
            "--end-us",
            "20",
            "--destination",
            "null",
            "--request",
            "cli-empty-samples",
        ],
    )?;
    assert!(!refused.status.success());
    let failed: serde_json::Value = serde_json::from_slice(&refused.stdout)?;
    assert_eq!(failed["decoder_completed"], false);
    assert!(
        failed["operation_failure"]
            .as_str()
            .is_some_and(|text| text.contains("no sample onsets"))
    );
    assert!(failed["decoder_failure"].is_null() && failed["native_failure"].is_null());
    assert!(failed["audio_output"]["native_closure"].is_null());
    assert!(matches!(
        failed["reader"]["state"].as_str(),
        Some("completed" | "failed")
    ));
    Ok(())
}

#[cfg(windows)]
async fn admit_exact_excerpt(
    directory: &std::path::Path,
) -> Result<(sigy_service::control::RetainedReadSpec, String), Box<dyn std::error::Error>> {
    use sigy_service::control::{self, Operation, RetainedOperation};
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut operation = RetainedOperation::StartRange {
        id: "pcm-excerpt".into(),
        recording_id: "pcm-oracle".into(),
        seek_us: 125_000,
        end_us: 375_000,
    };
    loop {
        let snapshot = tokio::time::timeout_at(
            deadline,
            control::request(directory, Operation::Retained { command: operation }),
        )
        .await??;
        let page = snapshot.retained.ok_or("excerpt receipt missing")?;
        let view = page.entries.first().ok_or("excerpt entry missing")?;
        assert_eq!(view.spec.request_id, "pcm-excerpt");
        assert_eq!(view.spec.file_duration_us, 1_000_000);
        assert_eq!(view.spec.file_seek_us, 125_000);
        let excerpt = view
            .spec
            .excerpt
            .as_ref()
            .ok_or("excerpt identity missing")?;
        assert_eq!(excerpt.version, 2);
        assert_eq!(excerpt.timeline_end_us, 375_000);
        assert!(excerpt.citation.is_none());
        if let Some(nonce) = page.pipe_nonce {
            return Ok((view.spec.clone(), nonce));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        operation = RetainedOperation::Show {
            id: "pcm-excerpt".into(),
        };
    }
}

#[cfg(windows)]
async fn direct_sample_cases(
    executable: &str,
    original: &sigy_service::control::RetainedReadSpec,
) -> TestResult {
    use sigy_service::control::RetainedExcerpt;
    // These are caller-owned decoder inputs, not persisted/admitted reader claims.
    // A nonzero recording-time origin must not change the file-relative selection.
    for (origin, start, end, first, after) in [
        (9_000_000, 125_000, 375_000, 6_000, 18_000),
        (0, 375_000, 1_000_000, 18_000, 48_000),
        (0, 1, 21, 1, 2),
        (0, 21, 42, 2, 3),
    ] {
        let mut spec = original.clone();
        spec.timeline_start_us = origin;
        spec.timeline_end_us = origin + 1_000_000;
        spec.file_seek_us = start;
        spec.excerpt = Some(RetainedExcerpt {
            version: 2,
            timeline_end_us: origin + end,
            citation: None,
        });
        let expected = sample_bytes(first, after)?;
        let (decoded, bytes) =
            Box::pin(direct_decode(executable, &spec, reference_wave()?)).await?;
        assert_eq!(bytes, expected);
        assert_eq!(decoded.pcm_bytes, u64::from((after - first) * 8));
    }
    let mut spec = original.clone();
    spec.file_seek_us = 1;
    spec.excerpt = Some(RetainedExcerpt {
        version: 2,
        timeline_end_us: 20,
        citation: None,
    });
    assert!(
        Box::pin(direct_decode(executable, &spec, reference_wave()?))
            .await
            .is_err()
    );
    // The file header declares one second, but actual bytes end before 375 ms.
    // Positive progress and a successful decoder exit cannot substitute for samples.
    spec.file_seek_us = 125_000;
    spec.excerpt = Some(RetainedExcerpt {
        version: 2,
        timeline_end_us: 375_000,
        citation: None,
    });
    let mut short = reference_wave()?;
    short.truncate(44 + 9_600 * 4);
    assert!(
        Box::pin(direct_decode(executable, &spec, short))
            .await
            .is_err()
    );
    assert!(
        Box::pin(direct_decode(executable, &spec, Vec::new()))
            .await
            .is_err()
    );
    Ok(())
}

#[cfg(windows)]
fn sample_bytes(first: u32, after: u32) -> Result<Vec<u8>, std::num::TryFromIntError> {
    let mut output = Vec::with_capacity(usize::try_from((after - first) * 8)?);
    for frame in first..after {
        let pair = match frame {
            5_999 => [32767.0 / 32768.0; 2],
            18_000 => [-1.0; 2],
            _ => [
                f32::from(i16::try_from(frame % 257)?) / 256.0 - 0.5,
                f32::from(i16::try_from(frame % 251)?) / 256.0 - 125.0 / 256.0,
            ],
        };
        for value in pair {
            output.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(output)
}

#[cfg(windows)]
async fn direct_decode(
    executable: &str,
    spec: &sigy_service::control::RetainedReadSpec,
    wave: Vec<u8>,
) -> Result<(sigy_service::recordings::audio::PcmDecoded, Vec<u8>), Box<dyn std::error::Error>> {
    use sigy_service::recordings::audio::{
        NativeAudioGroup, PcmFormat, PcmReaderRequest, decode_pcm_reader,
    };
    use tokio::io::AsyncWriteExt;
    let group = NativeAudioGroup::new()?;
    let (mut writer, input) = tokio::io::duplex(4096);
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(2);
    let work = async {
        let pump = async {
            // Chunk boundaries deliberately split headers and stereo sample frames.
            for chunk in wave.chunks(509) {
                if let Err(error) = writer.write_all(chunk).await {
                    if error.kind() == std::io::ErrorKind::BrokenPipe {
                        break;
                    }
                    return Err(sigy_service::Error::from(error));
                }
            }
            drop(writer);
            Ok(())
        };
        let collect = async {
            let mut bytes = Vec::new();
            while let Some(chunk) = receiver.recv().await {
                if chunk.len() > 8192 || bytes.len() + chunk.len() > 384_000 {
                    return Err(sigy_service::Error::Acquisition(
                        "direct excerpt fixture bound",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        };
        tokio::try_join!(
            decode_pcm_reader(
                PcmReaderRequest {
                    executable,
                    spec,
                    format: PcmFormat {
                        rate_hz: 48_000,
                        channels: 2
                    },
                    group: &group,
                },
                input,
                sender
            ),
            collect,
            pump
        )
    };
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(20), Box::pin(work)).await;
    if !matches!(&outcome, Ok(Ok(_))) {
        group.kill()?;
    }
    assert_eq!(group.finish().await?.mechanism, "job_object");
    let (decoded, bytes, ()) = outcome??;
    Ok((decoded, bytes))
}

#[cfg(windows)]
async fn wait_excerpt_closed(
    directory: &std::path::Path,
    spec: &sigy_service::control::RetainedReadSpec,
) -> TestResult {
    use sigy_service::control::{self, Operation, RetainedOperation};
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let page = tokio::time::timeout_at(
            deadline,
            control::request(
                directory,
                Operation::Retained {
                    command: RetainedOperation::Show {
                        id: spec.request_id.clone(),
                    },
                },
            ),
        )
        .await??
        .retained
        .ok_or("original excerpt reader missing")?;
        let view = page
            .entries
            .first()
            .ok_or("original excerpt reader entry missing")?;
        assert_eq!(&view.spec, spec);
        match view.state.as_str() {
            // Excerpt decoder may close before the whole encoded transfer ends.
            "completed" | "failed" => return Ok(()),
            "running" | "cancelling" => {}
            _ => return Err("original excerpt reader closure remains unproven".into()),
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
