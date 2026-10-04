//! Silent native PCM qualification with an independent sample/range oracle.

use super::TestResult;

#[path = "audio_fixtures/excerpts.rs"]
mod excerpts;

const SAMPLE_RATE: u32 = 48_000;
const SOURCE_FRAMES: u32 = 48_000;
const FIRST_FRAME: u32 = 6_000;
const END_FRAME: u32 = 18_000;
const EXPECTED_BYTES: usize = 96_000;

fn reference_wave() -> Result<Vec<u8>, std::num::TryFromIntError> {
    // One second, stereo signed 16-bit PCM. This literal header does not use
    // the production decoder's format or duration arithmetic.
    let mut bytes = Vec::with_capacity(192_044);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&192_036_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&192_000_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&192_000_u32.to_le_bytes());
    for frame in 0..SOURCE_FRAMES {
        let left = (i32::try_from(frame % 257)? - 128) * 128;
        let right = (i32::try_from(frame % 251)? - 125) * 128;
        for sample in [left, right] {
            let sample = match frame {
                5_999 => i16::MAX,
                18_000 => i16::MIN,
                _ => i16::try_from(sample)?,
            };
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    Ok(bytes)
}

fn expected_range() -> Result<Vec<u8>, std::num::TryFromIntError> {
    // Separate oracle: normalized binary fractions for absolute source frames
    // 6000 through 17999, not a decoder progress value or WAV parser result.
    let mut bytes = Vec::with_capacity(EXPECTED_BYTES);
    for source in FIRST_FRAME..END_FRAME {
        let left = f32::from(i16::try_from(source % 257)?) / 256.0 - 0.5;
        let right = f32::from(i16::try_from(source % 251)?) / 256.0 - 125.0 / 256.0;
        bytes.extend_from_slice(&left.to_le_bytes());
        bytes.extend_from_slice(&right.to_le_bytes());
    }
    Ok(bytes)
}

#[test]
fn independent_stereo_wave_and_range_oracle_have_declared_boundaries() -> TestResult {
    let wave = reference_wave()?;
    let expected = expected_range()?;
    assert_eq!(wave.len(), 192_044);
    assert_eq!(expected.len(), EXPECTED_BYTES);
    assert_eq!(&wave[40..44], &192_000_u32.to_le_bytes());
    assert_eq!(&wave[44 + 5_999 * 4..44 + 6_000 * 4], &[255, 127, 255, 127]);
    assert_eq!(&wave[44 + 18_000 * 4..44 + 18_001 * 4], &[0, 128, 0, 128]);
    // Hand-calculated values anchor the oracle independently of its loop.
    assert_eq!(&expected[..4], &(-0.152_343_75_f32).to_le_bytes());
    assert_eq!(&expected[4..8], &0.398_437_5_f32.to_le_bytes());
    assert_eq!(
        &expected[95_992..95_996],
        &(-0.464_843_75_f32).to_le_bytes()
    );
    assert_eq!(&expected[95_996..], &0.207_031_25_f32.to_le_bytes());
    Ok(())
}

#[cfg(windows)]
fn expected_native_tail() -> Result<Vec<u8>, std::num::TryFromIntError> {
    // Current production playback ends at this sealed file's EOF, not an
    // arbitrary cited end. It includes the later sentinel at source frame18000.
    let mut bytes = Vec::with_capacity(336_000);
    for source in FIRST_FRAME..SOURCE_FRAMES {
        let pair = if source == 18_000 {
            [-1.0, -1.0]
        } else {
            [
                f32::from(i16::try_from(source % 257)?) / 256.0 - 0.5,
                f32::from(i16::try_from(source % 251)?) / 256.0 - 125.0 / 256.0,
            ]
        };
        for value in pair {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(bytes)
}

#[cfg(windows)]
#[test]
#[ignore = "silent PCM oracle requires SIGY_TEST_FFMPEG; run cargo verify-media"]
fn native_protected_pcm_matches_every_stereo_sample_and_proves_group_closure() -> TestResult {
    use super::{AudioServer, RunningChild, initialize, success, wait_recording};
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
    let recording = wait_recording(directory.path(), "pcm-oracle", "completed")?;
    assert_eq!(recording["decoded_microseconds"], 1_000_000);
    let expected = expected_native_tail()?;
    assert_eq!(expected.len(), 336_000);
    assert_eq!(&expected[335_992..335_996], &0.269_531_25_f32.to_le_bytes());
    assert_eq!(&expected[335_996..], &(-0.261_718_75_f32).to_le_bytes());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(check_native_pcm(directory.path(), &expected))?;
    let reader = success(
        directory.path(),
        &["listen", "reader", "show", "pcm-reader"],
    )?;
    assert_eq!(reader["entries"][0]["state"], "completed");
    assert_eq!(reader["entries"][0]["spec"]["file_seek_us"], 125_000);
    assert_eq!(reader["entries"][0]["spec"]["file_duration_us"], 1_000_000);
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    if std::env::var("SIGY_TEST_SYSTEM_AUDIO").as_deref() == Ok("1") {
        qualify_system_output()?;
    }
    Ok(())
}

#[cfg(windows)]
fn quiet_wave() -> Result<Vec<u8>, std::num::TryFromIntError> {
    let mut bytes = reference_wave()?;
    for sample in bytes[44..].as_chunks_mut::<2>().0 {
        let value = i16::from_le_bytes([sample[0], sample[1]]) / 2048;
        sample.copy_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

#[cfg(windows)]
#[test]
fn optional_output_source_has_independent_low_digital_amplitude_bound() -> TestResult {
    let bytes = quiet_wave()?;
    assert_eq!(bytes.len(), 192_044);
    let mut peak = 0_u16;
    for sample in bytes[44..].as_chunks::<2>().0 {
        let value = i16::from_le_bytes([sample[0], sample[1]]);
        assert!(value.unsigned_abs() <= 16);
        peak = peak.max(value.unsigned_abs());
    }
    assert_eq!(peak, 16);
    // 16/32768 = 0.00048828125 peak digital amplitude. Device gain and
    // acoustic delivery are unknown; this does not measure loudness.
    Ok(())
}

#[cfg(windows)]
fn private_receipt() -> Result<std::fs::File, Box<dyn std::error::Error>> {
    let requested = std::path::PathBuf::from(
        std::env::var_os("SIGY_AUDIO_QUALIFY_RECEIPT")
            .ok_or("opt-in output requires SIGY_AUDIO_QUALIFY_RECEIPT")?,
    );
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace path")?;
    let private = workspace.join(".agents").canonicalize()?;
    let parent = requested.parent().ok_or("receipt parent")?.canonicalize()?;
    if !requested.is_absolute() || !parent.starts_with(&private) {
        return Err(
            "output receipt must be an absolute path inside existing .agents directory".into(),
        );
    }
    Ok(std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&requested)?)
}

#[cfg(windows)]
fn qualify_system_output() -> TestResult {
    use super::{AudioServer, RunningChild, initialize, success, wait_recording};
    use sha2::{Digest, Sha256};
    let mut receipt = private_receipt()?;
    let directory = tempfile::tempdir()?;
    let bytes = quiet_wave()?;
    let source_sha256 =
        Sha256::digest(&bytes)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
                hex
            });
    let fixture = AudioServer::start(bytes)?;
    initialize(directory.path(), &fixture)?;
    let mut service = RunningChild::start(directory.path())?;
    success(
        directory.path(),
        &[
            "record",
            "start",
            "quiet-oracle",
            "--source",
            "radio:v1",
            "--seconds",
            "5",
            "--max-mib",
            "1",
        ],
    )?;
    let recording = wait_recording(directory.path(), "quiet-oracle", "completed")?;
    assert_eq!(recording["decoded_microseconds"], 1_000_000);
    let arguments = [
        "listen",
        "file",
        "quiet-oracle",
        "--destination",
        "system",
        "--request",
        "quiet-system-once",
    ];
    let mut evidence = serde_json::json!({
        "reviewed": "2026-10-04", "experiment": "explicit Windows default-endpoint output",
        "source_frames": 48000, "source_rate_hz": 48000, "source_channels": 2,
        "source_peak_integer": 16, "source_peak_denominator": 32768,
        "source_sha256": source_sha256, "acoustic_delivery_proven": false,
    });
    save_receipt(&mut receipt, &evidence)?;
    let output = observed_output(directory.path(), &arguments, &mut receipt, &mut evidence)?;
    validate_system_output(&output)?;
    let replay = success(directory.path(), &arguments)?;
    evidence["replay"] = replay.clone();
    save_receipt(&mut receipt, &evidence)?;
    assert_eq!(replay["replayed"], true);
    assert!(replay["audio_output"].is_null());
    assert_eq!(replay["decoder_completed"], false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let stopped = runtime.block_on(stop_without_output(directory.path()))?;
    evidence["stop_before_output"] = serde_json::to_value(stopped)?;
    save_receipt(&mut receipt, &evidence)?;
    eprintln!(
        "explicit Windows output: source_peak=16/32768; receipt stored privately; underrun_frames={}",
        output["audio_output"]["sink"]["underrun_frames"]
    );
    success(directory.path(), &["service", "stop"])?;
    service.wait()?;
    Ok(())
}

#[cfg(windows)]
fn save_receipt(file: &mut std::fs::File, evidence: &serde_json::Value) -> TestResult {
    use std::io::{Seek, Write};
    let bytes = serde_json::to_vec_pretty(evidence)?;
    file.seek(std::io::SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.set_len(u64::try_from(bytes.len())?)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn observed_output(
    directory: &std::path::Path,
    arguments: &[&str],
    receipt: &mut std::fs::File,
    evidence: &mut serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let played = match super::invoke(directory, arguments) {
        Ok(played) => played,
        Err(error) => {
            evidence["execution_error"] = error.to_string().into();
            save_receipt(receipt, evidence)?;
            return Err(error.into());
        }
    };
    evidence["process_success"] = played.status.success().into();
    evidence["process_status"] = played.status.to_string().into();
    // Save raw bytes privately before parsing, including malformed output and
    // native refusal. A process exit alone is not a hardware cleanup receipt.
    evidence["stdout_bytes"] = serde_json::to_value(&played.stdout)?;
    evidence["stderr_bytes"] = serde_json::to_value(&played.stderr)?;
    save_receipt(receipt, evidence)?;
    let output: serde_json::Value = serde_json::from_slice(&played.stdout)?;
    evidence["output"] = output.clone();
    save_receipt(receipt, evidence)?;
    assert!(
        played.status.success(),
        "selected Windows output profile refused; inspect private receipt"
    );
    Ok(output)
}

#[cfg(windows)]
fn validate_system_output(output: &serde_json::Value) -> TestResult {
    assert_eq!(output["decoder_completed"], true);
    assert!(output["decoder_failure"].is_null());
    assert!(output["output_failure"].is_null());
    let audio = &output["audio_output"];
    assert_eq!(audio["sink"]["status"], "drained");
    assert_eq!(audio["sink"]["audibility_proven"], false);
    assert_eq!(audio["sink"]["presentation_is_estimated"], true);
    let frames = audio["sink"]["decoded_frames"]
        .as_u64()
        .ok_or("decoded frames missing")?;
    assert_eq!(audio["sink"]["content_frames"].as_u64(), Some(frames));
    assert!(audio["sink"]["underrun_frames"].as_u64().is_some());
    assert!(audio["sink"]["drain_zero_frames"].as_u64().is_some());
    let rate = audio["ready"]["rate_hz"].as_u64().ok_or("rate missing")?;
    let channels = audio["ready"]["channels"]
        .as_u64()
        .ok_or("channels missing")?;
    assert!((8000..=192_000).contains(&rate));
    assert!((1..=2).contains(&channels));
    // Independent one-second source oracle, including negotiated resampling.
    assert_eq!(frames, rate);
    assert_eq!(audio["ready"]["sample_format"], "f32le");
    assert_eq!(output["reported_elapsed_us"], 1_000_000);
    let pcm_bytes = frames
        .checked_mul(channels)
        .and_then(|value| value.checked_mul(4))
        .ok_or("PCM byte overflow")?;
    // Production checks the actual forwarded byte count against decoder bytes
    // and frame alignment. The report exposes frames, not a second byte counter.
    assert!(
        pcm_bytes
            <= audio["ready"]["maximum_pcm_bytes"]
                .as_u64()
                .ok_or("PCM limit missing")?
    );
    assert_eq!(audio["native_closure"]["mechanism"], "job_object");
    assert!(
        audio["native_closure"]["peak_memory_bytes"]
            .as_u64()
            .is_some()
    );
    assert!(audio["native_closure"]["cpu_time_us"].as_u64().is_some());
    assert!(matches!(
        output["reader"]["state"].as_str(),
        Some("completed" | "failed")
    ));
    if output["reader"]["state"] == "failed" {
        assert!(matches!(
            output["reader"]["completion_reason"].as_str(),
            Some("cancelled" | "retained-pipe-failed")
        ));
    }
    Ok(())
}

#[cfg(windows)]
async fn stop_without_output(
    directory: &std::path::Path,
) -> Result<sigy_service::control::RetainedReadView, Box<dyn std::error::Error>> {
    use sigy_service::control::{self, Operation, RetainedOperation};
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let start = tokio::time::timeout_at(
        deadline,
        control::request(
            directory,
            Operation::Retained {
                command: RetainedOperation::Start {
                    id: "quiet-stopped-before-output".into(),
                    recording_id: "quiet-oracle".into(),
                    seek_us: 0,
                },
            },
        ),
    )
    .await??;
    let admitted = start.retained.ok_or("stop fixture admission missing")?;
    assert_eq!(admitted.entries.len(), 1);
    let expected = admitted.entries[0].spec.clone();
    let mut operation = RetainedOperation::Stop {
        id: expected.request_id.clone(),
        generation: expected.generation,
    };
    loop {
        let snapshot = tokio::time::timeout_at(
            deadline,
            control::request(directory, Operation::Retained { command: operation }),
        )
        .await??;
        let page = snapshot.retained.ok_or("stop fixture receipt missing")?;
        assert_eq!(page.entries.len(), 1);
        let view = &page.entries[0];
        assert_eq!(view.spec, expected);
        if view.state == "failed" {
            assert!(matches!(
                view.completion_reason.as_deref(),
                Some("cancelled" | "retained-pipe-failed")
            ));
            return Ok(view.clone());
        }
        assert!(matches!(view.state.as_str(), "running" | "cancelling"));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        operation = RetainedOperation::Show {
            id: expected.request_id.clone(),
        };
    }
}

#[cfg(windows)]
async fn check_native_pcm(directory: &std::path::Path, expected: &[u8]) -> TestResult {
    use sigy_service::recordings::audio::{
        NativeAudioGroup, PcmDecodeRequest, PcmFormat, PcmReaderRequest, decode_retained_pcm,
    };
    let executable = std::env::var("SIGY_TEST_FFMPEG")?;
    let group = NativeAudioGroup::new()?;
    let (spec, nonce) = ready_reader(directory).await?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let decode = decode_retained_pcm(
        PcmDecodeRequest {
            reader: PcmReaderRequest {
                executable: &executable,
                spec: &spec,
                format: PcmFormat {
                    rate_hz: SAMPLE_RATE,
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
        let mut bytes = Vec::with_capacity(336_000);
        while let Some(chunk) = receiver.recv().await {
            if chunk.is_empty() || chunk.len() > 8192 || bytes.len() + chunk.len() > 336_000 {
                return Err(sigy_service::Error::Acquisition("fixture PCM bound"));
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
    assert_eq!(bytes.as_slice(), expected);
    assert_eq!(decoded.pcm_bytes, 336_000);
    assert_eq!(decoded.decoder.reported_elapsed_us, 875_000);
    assert_eq!(decoded.decoder.file_playhead_us, 1_000_000);
    assert_eq!(closure.mechanism, "job_object");
    assert!(closure.peak_memory_bytes.is_some());
    assert!(closure.cpu_time_us.is_some());
    eprintln!(
        "silent PCM oracle: frames=42000 bytes={} peak_memory_bytes={:?} cpu_time_us={:?}",
        bytes.len(),
        closure.peak_memory_bytes,
        closure.cpu_time_us
    );
    wait_original_closed(directory, &spec).await?;
    Ok(())
}

#[cfg(windows)]
async fn wait_original_closed(
    directory: &std::path::Path,
    expected: &sigy_service::control::RetainedReadSpec,
) -> TestResult {
    use sigy_service::control::{self, Operation, RetainedOperation};
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let snapshot = tokio::time::timeout_at(
            deadline,
            control::request(
                directory,
                Operation::Retained {
                    command: RetainedOperation::Show {
                        id: "pcm-reader".into(),
                    },
                },
            ),
        )
        .await??;
        let page = snapshot.retained.ok_or("original reader receipt missing")?;
        if page.entries.len() != 1 {
            return Err("original reader receipt count".into());
        }
        assert_eq!(&page.entries[0].spec, expected);
        match page.entries[0].state.as_str() {
            "completed" => return Ok(()),
            "running" => {}
            _ => return Err("original reader did not complete successfully".into()),
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[cfg(windows)]
async fn ready_reader(
    directory: &std::path::Path,
) -> Result<(sigy_service::control::RetainedReadSpec, String), Box<dyn std::error::Error>> {
    use sigy_service::control::{self, Operation, RetainedOperation};
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut operation = RetainedOperation::Start {
        id: "pcm-reader".into(),
        recording_id: "pcm-oracle".into(),
        seek_us: 125_000,
    };
    loop {
        let snapshot = tokio::time::timeout_at(
            deadline,
            control::request(directory, Operation::Retained { command: operation }),
        )
        .await??;
        let page = snapshot.retained.ok_or("PCM fixture receipt missing")?;
        if page.entries.len() != 1 {
            return Err("PCM fixture receipt count".into());
        }
        let entry = &page.entries[0];
        if entry.spec.request_id != "pcm-reader" || entry.state != "running" {
            return Err("PCM fixture receipt identity/state".into());
        }
        if let Some(nonce) = page.pipe_nonce {
            return Ok((entry.spec.clone(), nonce));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        operation = RetainedOperation::Show {
            id: "pcm-reader".into(),
        };
    }
}
