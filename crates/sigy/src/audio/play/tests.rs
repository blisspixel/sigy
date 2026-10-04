//! Silent parent transport and independently declared output receipts.

use super::*;

type TestResult = Result<(), Failure>;

fn ready() -> Ready {
    Ready {
        protocol: 1,
        rate_hz: 48_000,
        channels: 2,
        sample_format: "f32le".into(),
        ring_frames: 12_000,
        callback_frames_estimate: 480,
        maximum_pcm_bytes: 422_400,
    }
}

fn report() -> Report {
    Report {
        protocol: 1,
        status: Status::Drained,
        error: None,
        decoded_frames: 48_000,
        content_frames: 48_000,
        clipped_samples: 0,
        underrun_frames: 480,
        drain_zero_frames: 480,
        callbacks: 102,
        queue_high_water_frames: 2400,
        predicted_presentation_us: Some(1_020_000),
        presentation_is_estimated: true,
        audibility_proven: false,
    }
}

#[test]
fn partial_failure_preserves_fixed_cause_without_claiming_completion() -> TestResult {
    let mut failed = report();
    failed.status = Status::Failed;
    failed.error = Some("audio-device-failed".into());
    failed.content_frames = 24_000;
    assert_eq!(
        output_failure(&failed, &ready(), 31_000_000)?,
        "Windows output failed (audio-device-failed)"
    );
    assert!(validate_report(&failed, &ready(), 384_000, 31_000_000).is_err());
    for mutate in [
        |receipt: &mut Report| receipt.error = Some("unknown".into()),
        |receipt: &mut Report| receipt.error = Some("audio-device-failed\u{1b}".into()),
        |receipt: &mut Report| receipt.audibility_proven = true,
        |receipt: &mut Report| receipt.presentation_is_estimated = false,
        |receipt: &mut Report| receipt.content_frames = 48_001,
        |receipt: &mut Report| receipt.clipped_samples = 96_001,
        |receipt: &mut Report| receipt.queue_high_water_frames = 12_001,
        |receipt: &mut Report| receipt.underrun_frames = u64::MAX,
        |receipt: &mut Report| receipt.predicted_presentation_us = Some(31_000_001),
    ] {
        let mut bad = failed.clone();
        mutate(&mut bad);
        assert!(output_failure(&bad, &ready(), 31_000_000).is_err());
    }
    Ok(())
}

async fn forwarded(chunks: Vec<Vec<u8>>, maximum: u64) -> (Result<u64, Failure>, Vec<u8>) {
    let (sender, receiver) = mpsc::channel(2);
    let (output, input) = tokio::io::duplex(64);
    let mut ready = ready();
    ready.maximum_pcm_bytes = maximum;
    let produce = async {
        for chunk in chunks {
            if sender.send(chunk).await.is_err() {
                break;
            }
        }
        drop(sender);
    };
    let collect = async {
        let mut bytes = Vec::with_capacity(128);
        let result = input.take(32_769).read_to_end(&mut bytes).await;
        assert!(result.is_ok());
        bytes
    };
    let ((), sent, collected) = tokio::join!(produce, forward(receiver, output, &ready), collect);
    (sent, collected)
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_stereo_preserves_channels_and_emits_one_explicit_end() -> TestResult {
    // Two hand-declared stereo frames: (0.25,-0.75), (1,-1).
    let pcm = [0, 0, 128, 62, 0, 0, 64, 191, 0, 0, 128, 63, 0, 0, 128, 191];
    let chunks = vec![
        pcm[..1].to_vec(),
        pcm[1..7].to_vec(),
        pcm[7..13].to_vec(),
        pcm[13..].to_vec(),
    ];
    let (sent, bytes) = forwarded(chunks, 16).await;
    assert_eq!(sent?, 16);
    assert_eq!(
        bytes,
        [
            8, 0, 0, 0, 0, 0, 128, 62, 0, 0, 64, 191, 8, 0, 0, 0, 0, 0, 128, 63, 0, 0, 128, 191, 0,
            0, 0, 0,
        ]
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn incomplete_empty_excess_and_oversized_pcm_never_emit_success_end() -> TestResult {
    for chunks in [
        vec![],
        vec![vec![]],
        vec![vec![1; 3]],
        vec![vec![1; 17]],
        vec![vec![1; 8193]],
    ] {
        let (sent, bytes) = forwarded(chunks, 16).await;
        assert!(sent.is_err());
        assert!(bytes.is_empty());
    }
    let (sent, bytes) = forwarded(vec![vec![1; 16], vec![1]], 16).await;
    assert!(sent.is_err());
    assert_eq!(bytes.len(), 20);
    assert_eq!(&bytes[..4], &[16, 0, 0, 0]);
    assert_eq!(&bytes[4..], &[1; 16]);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn maximum_chunks_with_partial_carry_stay_within_packet_bound() -> TestResult {
    let (sent, bytes) = forwarded(vec![vec![1; 7], vec![2; 8192], vec![3]], 8200).await;
    assert_eq!(sent?, 8200);
    assert_eq!(bytes.len(), 8212);
    assert_eq!(&bytes[..4], &8192_u32.to_le_bytes());
    assert_eq!(&bytes[4..11], &[1; 7]);
    assert_eq!(&bytes[8196..8200], &8_u32.to_le_bytes());
    assert_eq!(&bytes[8208..], &[0; 4]);
    Ok(())
}

fn event_line(event: &Event) -> Result<Vec<u8>, Failure> {
    let mut bytes = serde_json::to_vec(event)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[tokio::test(flavor = "current_thread")]
async fn strict_messages_accept_exact_byte_cap_and_refuse_malformed_or_incomplete_json()
-> TestResult {
    let mut exact = serde_json::to_vec(&Event::Ready(ready()))?;
    exact.resize(protocol::MESSAGE_BYTES, b' ');
    exact.push(b'\n');
    assert!(matches!(
        message(&mut exact.as_slice()).await?,
        Event::Ready(_)
    ));
    exact.insert(0, b' ');
    assert!(message(&mut exact.as_slice()).await.is_err());
    for mut bytes in [
        br#"{"event":"ready","protocol":1,"extra":true}"#.as_slice(),
        br#"{"event":"unknown"}"#,
        br#"{"event":"report"}"#,
        b"{}\n",
        b"\xff\n",
    ] {
        assert!(message(&mut bytes).await.is_err());
    }
    let mut no_newline = serde_json::to_vec(&Event::Ready(ready()))?;
    assert!(message(&mut no_newline.as_slice()).await.is_err());
    no_newline.push(b'\n');
    assert!(message(&mut no_newline.as_slice()).await.is_ok());
    let mut extra = serde_json::to_value(Event::Ready(ready()))?;
    extra["unknown"] = serde_json::Value::Bool(true);
    let mut bytes = serde_json::to_vec(&extra)?;
    bytes.push(b'\n');
    assert!(message(&mut bytes.as_slice()).await.is_err());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn final_report_requires_one_report_and_clean_eof() -> TestResult {
    let bytes = event_line(&Event::Report(report()))?;
    let actual = final_report(&mut bytes.as_slice()).await?;
    assert_eq!(actual.content_frames, 48_000);
    assert!(
        final_report(&mut event_line(&Event::Ready(ready()))?.as_slice())
            .await
            .is_err()
    );
    for suffix in [
        vec![b'\n'],
        vec![b'x'],
        event_line(&Event::Report(report()))?,
    ] {
        let mut extra = bytes.clone();
        extra.extend_from_slice(&suffix);
        assert!(final_report(&mut extra.as_slice()).await.is_err());
    }
    let mut fault = EofFault { bytes, offset: 0 };
    assert!(final_report(&mut fault).await.is_err());
    Ok(())
}

struct EofFault {
    bytes: Vec<u8>,
    offset: usize,
}

impl AsyncRead for EofFault {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        output: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.offset == this.bytes.len() {
            return std::task::Poll::Ready(Err(std::io::Error::other("fixture transport failure")));
        }
        let count = output.remaining().min(this.bytes.len() - this.offset);
        output.put_slice(&this.bytes[this.offset..this.offset + count]);
        this.offset += count;
        std::task::Poll::Ready(Ok(()))
    }
}

#[test]
fn report_acceptance_requires_exact_content_and_estimated_unheard_presentation() -> TestResult {
    validate_report(&report(), &ready(), 384_000, 31_000_000)?;
    let mut alternatives = Vec::new();
    let mut invalid = report();
    invalid.protocol = 2;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.status = Status::Failed;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.error = Some("audio-device-failed".into());
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.decoded_frames -= 1;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.content_frames -= 1;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.callbacks = 0;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.queue_high_water_frames = 12_001;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.predicted_presentation_us = None;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.predicted_presentation_us = Some(31_000_001);
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.presentation_is_estimated = false;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.audibility_proven = true;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.underrun_frames = u64::MAX;
    alternatives.push(invalid);
    let mut invalid = report();
    invalid.drain_zero_frames = u64::MAX;
    alternatives.push(invalid);
    for invalid in alternatives {
        assert!(validate_report(&invalid, &ready(), 384_000, 31_000_000).is_err());
    }
    assert!(validate_report(&report(), &ready(), 384_001, 31_000_000).is_err());
    assert!(validate_report(&report(), &ready(), 0, 31_000_000).is_err());
    assert!(validate_report(&report(), &ready(), 384_000, 0).is_err());
    assert!(validate_report(&report(), &ready(), 384_000, u64::MAX).is_err());
    Ok(())
}

fn startup_report() -> Report {
    Report {
        protocol: 1,
        status: Status::Failed,
        error: Some("audio-device-unavailable".into()),
        decoded_frames: 0,
        content_frames: 0,
        clipped_samples: 0,
        underrun_frames: 0,
        drain_zero_frames: 0,
        callbacks: 0,
        queue_high_water_frames: 0,
        predicted_presentation_us: None,
        presentation_is_estimated: true,
        audibility_proven: false,
    }
}

#[test]
fn startup_failure_requires_fixed_code_and_exact_zero_work_receipt() -> TestResult {
    let valid = startup_report();
    assert!(startup_failure(&valid)?.contains("audio-device-unavailable"));
    for code in [
        None,
        Some(""),
        Some("unknown"),
        Some("audio-not-real"),
        Some("audio-device-unavailable\x1b[31m"),
        Some("audio-device-unavailable\n"),
        Some("Audio-device-unavailable"),
    ] {
        let mut invalid = startup_report();
        invalid.error = code.map(str::to_owned);
        assert!(startup_failure(&invalid).is_err(), "invalid startup code");
    }
    let mut overlong = startup_report();
    overlong.error = Some("a".repeat(129));
    assert!(startup_failure(&overlong).is_err());
    // Alter each counter separately so one unsupported observation cannot hide
    // behind a different counter or a successful zero-work refusal.
    for field in [
        "decoded_frames",
        "content_frames",
        "clipped_samples",
        "underrun_frames",
        "drain_zero_frames",
        "callbacks",
        "queue_high_water_frames",
    ] {
        let mut value = serde_json::to_value(&valid)?;
        value[field] = 1.into();
        let invalid = serde_json::from_value::<Report>(value)?;
        assert!(startup_failure(&invalid).is_err(), "startup work: {field}");
    }
    for (field, value) in [
        ("protocol", serde_json::json!(2)),
        ("status", serde_json::json!("drained")),
        ("presentation_is_estimated", serde_json::json!(false)),
        ("audibility_proven", serde_json::json!(true)),
        ("predicted_presentation_us", serde_json::json!(0)),
        ("predicted_presentation_us", serde_json::json!(1)),
    ] {
        let mut invalid = serde_json::to_value(&valid)?;
        invalid[field] = value;
        assert!(startup_failure(&serde_json::from_value(invalid)?).is_err());
    }
    let mut valid_fallback = startup_report();
    valid_fallback.error = Some("audio-startup-failed".into());
    assert!(startup_failure(&valid_fallback).is_ok());
    Ok(())
}

#[test]
fn clipping_counts_obey_independent_sample_boundary_and_checked_overflow() -> TestResult {
    // 48000 stereo frames contain exactly 96000 scalar samples.
    for clipped in [0, 1, 95_999, 96_000] {
        let mut valid = report();
        valid.clipped_samples = clipped;
        validate_report(&valid, &ready(), 384_000, 31_000_000)?;
    }
    for clipped in [96_001, u64::MAX] {
        let mut invalid = report();
        invalid.clipped_samples = clipped;
        assert!(validate_report(&invalid, &ready(), 384_000, 31_000_000).is_err());
    }
    let mut overflow = report();
    overflow.decoded_frames = u64::MAX / 2 + 1;
    overflow.content_frames = overflow.decoded_frames;
    assert_eq!(
        validate_report(&overflow, &ready(), 384_000, 31_000_000)
            .err()
            .ok_or("overflow accepted")?
            .to_string(),
        "audio sample counter overflow"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn empty_native_group_closure_seals_later_spawning_without_starting_a_child() -> TestResult {
    let group = NativeAudioGroup::new()?;
    let closure = group.finish().await?;
    assert_eq!(closure.mechanism, "job_object");
    let command = tokio::process::Command::new("never-started-after-audio-seal");
    assert!(matches!(
        group.spawn(command),
        Err(sigy_service::Error::Acquisition("audio group is sealed"))
    ));
    assert!(group.finish().await.is_ok());
    Ok(())
}

#[test]
#[ignore = "fixed silent child fixture, entered only by the bounded parent test"]
fn native_audio_hang_fixture() -> TestResult {
    use std::io::Write;
    if std::env::var("SIGY_TEST_AUDIO_HANG").as_deref() != Ok("1") {
        return Ok(());
    }
    let mut output = std::io::stderr().lock();
    output.write_all(b"audio-hang\n")?;
    output.flush()?;
    // Models a child stuck in a native callback/driver wait without opening a
    // device. The parent must kill and observe closure, not wait for this sleep.
    std::thread::sleep(std::time::Duration::from_secs(30));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn hung_silent_child_is_killed_and_actual_empty_group_is_observed() -> TestResult {
    let group = NativeAudioGroup::new()?;
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            "audio::play::tests::native_audio_hang_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("SIGY_TEST_AUDIO_HANG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = group.spawn(command)?;
    let mut stderr = child.stderr.take().ok_or("fixture stderr missing")?;
    let mut marker = [0; 11];
    let ready = tokio::time::timeout(Duration::from_secs(5), stderr.read_exact(&mut marker)).await;
    // Always request termination and inspect the group, including failed child
    // handshakes. A root process exit or kill request is not closure evidence.
    group.kill()?;
    let closure = tokio::time::timeout(Duration::from_secs(7), group.finish()).await??;
    let status = tokio::time::timeout(Duration::from_secs(3), child.wait()).await??;
    ready??;
    assert_eq!(&marker, b"audio-hang\n");
    assert!(!status.success());
    assert_eq!(closure.mechanism, "job_object");
    assert!(closure.peak_memory_bytes.is_some());
    assert!(closure.cpu_time_us.is_some());
    assert!(matches!(
        group.spawn(tokio::process::Command::new("sealed-hang-fixture")),
        Err(sigy_service::Error::Acquisition("audio group is sealed"))
    ));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn partial_write_or_failed_shutdown_cannot_become_successful_delivery() -> TestResult {
    for (limit, fail_shutdown) in [(10, false), (24, true)] {
        let (sender, receiver) = mpsc::channel(2);
        sender.send(vec![1; 16]).await?;
        drop(sender);
        let mut output = WriteFault {
            bytes: Vec::with_capacity(24),
            limit,
            fail_shutdown,
        };
        assert!(forward(receiver, &mut output, &ready()).await.is_err());
        assert_eq!(output.bytes.len(), limit);
        if fail_shutdown {
            assert_eq!(&output.bytes[20..], &[0; 4]);
        }
    }
    Ok(())
}

struct WriteFault {
    bytes: Vec<u8>,
    limit: usize,
    fail_shutdown: bool,
}

impl AsyncWrite for WriteFault {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let available = this.limit - this.bytes.len();
        if available == 0 {
            return std::task::Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
        }
        let count = bytes.len().min(available);
        this.bytes.extend_from_slice(&bytes[..count]);
        std::task::Poll::Ready(Ok(count))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(if self.fail_shutdown {
            Err(std::io::ErrorKind::BrokenPipe.into())
        } else {
            Ok(())
        })
    }
}
