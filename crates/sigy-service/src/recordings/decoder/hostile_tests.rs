use super::{
    complete, copy_limited,
    progress::{Limits, read_progress},
};
use crate::recordings::audio::NativeAudioGroup;
use std::{io::Write, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn unknown_initial_timestamps_never_invent_progress_or_replace_measured_final_time()
-> TestResult {
    let mut initial = b"out_time_us=N/A\nprogress=continue\n".repeat(3);
    let unknown = read_progress(initial.as_slice(), Limits::exact(100, 4)).await?;
    assert_eq!(unknown.max_out_us, 0);
    assert!(!unknown.advanced && !unknown.finished);
    initial.extend_from_slice(b"out_time_us=100\nprogress=end\n");
    let complete = read_progress(initial.as_slice(), Limits::exact(100, 4)).await?;
    assert_eq!(complete.max_out_us, 100);
    assert!(complete.advanced && complete.finished);
    assert!(
        read_progress(initial.as_slice(), Limits::exact(100, 3))
            .await
            .is_err()
    );
    for input in [
        b"out_time_us=N/A\nprogress=end\n".as_slice(),
        b"out_time_us=N/A\nout_time_us=1\nprogress=end\n",
        b"out_time_us=1\nout_time_us=N/A\nprogress=end\n",
        b"out_time_us=0\nprogress=continue\nout_time_us=N/A\nprogress=continue\n",
        b"out_time_us=N/Ax\nprogress=continue\n",
        b"out_time_us=n/a\nprogress=continue\n",
        b"out_time_us= N/A\nprogress=continue\n",
        b"out_time_us=-1\nprogress=continue\n",
        b"out_time_us=+1\nprogress=continue\n",
    ] {
        assert!(read_progress(input, Limits::exact(100, 4)).await.is_err());
    }
    Ok(())
}

#[test]
fn retained_pipe_seek_is_unthrottled_and_output_bounded_without_changing_live_pacing() -> TestResult
{
    let retained =
        super::pipe_playback_command("fixture", "wav", None, Some((59_250_000, 60_000_000)));
    let args: Vec<_> = retained
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(!args.iter().any(|arg| arg == "-re"));
    let input = args
        .iter()
        .position(|arg| arg == "-i")
        .ok_or("input missing")?;
    let seek = args
        .iter()
        .position(|arg| arg == "-ss")
        .ok_or("seek missing")?;
    let duration = args
        .iter()
        .position(|arg| arg == "-t")
        .ok_or("duration missing")?;
    assert!(seek > input && duration > input);
    assert_eq!(args[input + 1], "pipe:0");
    assert_eq!(args[seek + 1], "59.250000");
    assert_eq!(args[duration + 1], "0.750000");
    let live = super::pipe_playback_command("fixture", "wav", None, None);
    let args: Vec<_> = live
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let pacing = args
        .iter()
        .position(|arg| arg == "-re")
        .ok_or("live pacing missing")?;
    let input = args
        .iter()
        .position(|arg| arg == "-i")
        .ok_or("input missing")?;
    assert!(pacing < input);
    assert!(!args.iter().any(|arg| matches!(arg.as_str(), "-ss" | "-t")));
    Ok(())
}

#[tokio::test]
async fn transport_stream_is_retained_only_without_expanding_direct_formats() -> TestResult {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("absent-decoder");
    let executable = missing.to_str().ok_or("fixture path")?;
    let direct = super::play_reader(executable, tokio::io::empty(), "mpegts", false)
        .await
        .err()
        .ok_or("direct TS admitted")?;
    assert!(matches!(
        direct,
        crate::Error::Acquisition("unsupported playback format")
    ));
    let retained = super::play_range_reader(
        executable,
        tokio::io::empty(),
        "mpegts",
        false,
        0,
        1_000_000,
        1,
    )
    .await
    .err()
    .ok_or("missing decoder started")?;
    assert!(matches!(
        retained,
        crate::Error::Acquisition("cannot start configured FFmpeg decoder")
    ));
    Ok(())
}

#[tokio::test]
async fn hostile_progress_refuses_bad_times_frames_and_unbounded_lines() -> TestResult {
    for bytes in [
        b"out_time_us=-1\nprogress=end\n".as_slice(),
        b"out_time_us=+1\nprogress=end\n",
        b"out_time_us=101\nprogress=end\n",
        b"out_time_us=2\nprogress=continue\nout_time_us=1\nprogress=end\n",
        b"out_time_us=1\nout_time_us=2\nprogress=end\n",
        b"progress=end\n",
        b"out_time_us=1\nprogress=bad\n",
        b"out_time_us=1\nprogress=end\nout_time_us=1\n",
        b"out_time_us=1\nprogress=end",
        b"out_time_us=18446744073709551616\nprogress=end\n",
    ] {
        assert!(read_progress(bytes, Limits::exact(100, 512)).await.is_err());
    }
    let long = vec![b'x'; 65_537];
    let error = read_progress(long.as_slice(), Limits::exact(100, 512))
        .await
        .err()
        .ok_or("line accepted")?;
    assert!(error.to_string().contains("line limit"));
    let many = b"ignored=x\n".repeat(7000);
    assert!(
        read_progress(many.as_slice(), Limits::exact(100, 512))
            .await
            .is_err()
    );
    let legal = b"bitrate=N/A\nout_time_us=0\nprogress=continue\nout_time_us=100\nprogress=end\n";
    let progress = read_progress(legal.as_slice(), Limits::exact(100, 512)).await?;
    assert_eq!(progress.max_out_us, 100);
    assert!(progress.advanced && progress.finished);
    Ok(())
}

#[tokio::test]
async fn single_final_positive_observation_advances_from_zero_without_clamping() -> TestResult {
    let progress = read_progress(
        b"out_time_us=800000\nprogress=end\n".as_slice(),
        Limits::exact(900_000, 1),
    )
    .await?;
    assert_eq!(progress.max_out_us, 800_000);
    assert!(progress.finished && progress.advanced);
    for bytes in [
        b"out_time_us=0\nprogress=end\n".as_slice(),
        b"out_time_us=0\nprogress=continue\nout_time_us=0\nprogress=end\n",
    ] {
        let progress = read_progress(bytes, Limits::exact(900_000, 2)).await?;
        assert_eq!(progress.max_out_us, 0);
        assert!(progress.finished && !progress.advanced);
    }
    assert!(
        read_progress(
            b"out_time_us=800000\nprogress=continue\nout_time_us=799999\nprogress=end\n".as_slice(),
            Limits::exact(900_000, 2),
        )
        .await
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn range_tolerance_preserves_raw_excess_and_refuses_early_output() -> TestResult {
    let progress = read_progress(
        b"out_time_us=1100000\nprogress=end\n".as_slice(),
        Limits::exact(1_100_000, 512),
    )
    .await?;
    super::require_range_progress(&progress, 1_000_000)?;
    assert_eq!(progress.max_out_us, 1_100_000);
    assert!(
        read_progress(
            b"out_time_us=1100001\nprogress=end\n".as_slice(),
            Limits::exact(1_100_000, 512)
        )
        .await
        .is_err()
    );
    let short = read_progress(
        b"out_time_us=899999\nprogress=end\n".as_slice(),
        Limits::exact(1_100_000, 512),
    )
    .await?;
    assert!(super::require_range_progress(&short, 1_000_000).is_err());
    Ok(())
}

struct FailedInput;

impl tokio::io::AsyncRead for FailedInput {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
        _buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Err(std::io::Error::other("fixture input read failure")))
    }
}

#[tokio::test]
async fn range_pump_only_accepts_output_broken_pipe_and_checks_input_bounds() -> TestResult {
    let mut output = tokio::io::sink();
    assert!(
        super::copy_range(FailedInput, &mut output, 1024)
            .await
            .is_err()
    );
    assert!(
        super::copy_range(b"123".as_slice(), &mut output, 2)
            .await
            .is_err()
    );
    assert!(
        super::copy_range(b"".as_slice(), &mut output, 2)
            .await
            .is_err()
    );
    let (mut output, peer) = tokio::io::duplex(1);
    drop(peer);
    super::copy_range(b"123".as_slice(), &mut output, 3).await?;
    Ok(())
}

#[tokio::test]
async fn long_legal_progress_exceeds_old_total_cap_with_derived_work_bounds() -> TestResult {
    let mut bytes = Vec::new();
    for ordinal in 0..=1000 {
        writeln!(
            bytes,
            "bitrate=N/A\ntotal_size=N/A\nout_time_us={}\nspeed=1.0x\nprogress={}",
            ordinal * 100_000,
            if ordinal == 1000 { "end" } else { "continue" }
        )?;
    }
    assert!(bytes.len() > 65_536);
    let progress = read_progress(
        bytes.as_slice(),
        Limits::playback(100_000_000, 100_000_000)?,
    )
    .await?;
    assert_eq!(progress.max_out_us, 100_000_000);
    assert!(progress.finished && progress.advanced);
    // The work budget includes time spent discarding input before output-side seek.
    let repeated = b"out_time_us=0\nprogress=continue\n".repeat(1000);
    assert!(
        read_progress(repeated.as_slice(), Limits::playback(100_000, 100_000_000)?)
            .await
            .is_ok()
    );
    assert!(
        read_progress(repeated.as_slice(), Limits::playback(100_000, 100_000)?)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn frame_size_and_frame_flood_are_independently_bounded() -> TestResult {
    let mut large_frame = b"ignored=".to_vec();
    large_frame.extend(vec![b'x'; 4000]);
    large_frame.extend_from_slice(b"\nignored=");
    large_frame.extend(vec![b'y'; 4000]);
    large_frame.extend_from_slice(b"\nignored=");
    large_frame.extend(vec![b'z'; 200]);
    large_frame.extend_from_slice(b"\nout_time_us=1\nprogress=end\n");
    let error = read_progress(large_frame.as_slice(), Limits::exact(1, 10))
        .await
        .err()
        .ok_or("large frame accepted")?;
    assert!(error.to_string().contains("frame limit"));
    let flood = b"out_time_us=1\nprogress=continue\n".repeat(6);
    let error = read_progress(flood.as_slice(), Limits::exact(1, 5))
        .await
        .err()
        .ok_or("frame flood accepted")?;
    assert!(error.to_string().contains("frame count limit"));
    let mut frame = b"ignored=".to_vec();
    frame.extend(vec![b'x'; 480]);
    frame.extend_from_slice(b"\nout_time_us=1\nprogress=continue\n");
    let lifetime = frame.repeat(18_004);
    let error = read_progress(
        lifetime.as_slice(),
        Limits::playback(1_800_000_000, 1_800_000_000)?,
    )
    .await
    .err()
    .ok_or("lifetime bytes accepted")?;
    assert!(matches!(
        error,
        crate::Error::Acquisition("decoder progress limit")
    ));
    Ok(())
}

#[tokio::test]
async fn wall_horizon_allows_stall_frames_without_expanding_output_clock() -> TestResult {
    let mut legal = b"out_time_us=0\nprogress=continue\n".repeat(300);
    legal.extend_from_slice(b"out_time_us=2000000\nprogress=end\n");
    let progress =
        read_progress(legal.as_slice(), Limits::playback(2_000_000, 32_000_000)?).await?;
    assert!(progress.finished);
    assert_eq!(progress.max_out_us, 2_000_000);
    let flood = b"out_time_us=0\nprogress=continue\n".repeat(325);
    let error = read_progress(flood.as_slice(), Limits::playback(2_000_000, 32_000_000)?)
        .await
        .err()
        .ok_or("wall frame flood accepted")?;
    assert!(matches!(
        error,
        crate::Error::Acquisition("decoder progress frame count limit")
    ));
    assert!(
        read_progress(
            b"out_time_us=2100001\nprogress=end\n".as_slice(),
            Limits::playback(2_000_000, 32_000_000)?
        )
        .await
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn progress_and_encoded_pump_make_progress_under_duplex_backpressure() -> TestResult {
    let (mut encoder, decoder) = tokio::io::duplex(16);
    let (mut progress_writer, progress_reader) = tokio::io::duplex(16);
    let payload = vec![7_u8; 4096];
    let work = async {
        let pump = copy_limited(payload.as_slice(), &mut encoder);
        let read = read_progress(progress_reader, Limits::exact(10_000, 512));
        let receiver = async {
            let mut decoder = decoder;
            let mut chunk = [0_u8; 16];
            let mut total = 0_usize;
            for ordinal in 1..=256 {
                decoder.read_exact(&mut chunk).await?;
                assert!(chunk.iter().all(|byte| *byte == 7));
                total += chunk.len();
                progress_writer
                    .write_all(format!("out_time_us={ordinal}\nprogress=continue\n").as_bytes())
                    .await?;
            }
            progress_writer
                .write_all(b"out_time_us=256\nprogress=end\n")
                .await?;
            drop(progress_writer);
            Ok::<_, Box<dyn std::error::Error>>(total)
        };
        let (pump, read, total) = tokio::join!(pump, read, receiver);
        pump?;
        let progress = read?;
        assert!(progress.finished && progress.advanced);
        assert_eq!(total?, 4096);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(3), work).await??;
    Ok(())
}

#[test]
fn decoder_waiting_child() {
    if std::env::var("SIGY_DECODER_WAITING_CHILD").as_deref() == Ok("1") {
        let _ = writeln!(std::io::stdout(), "decoder fixture ready");
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(10));
    }
}

#[tokio::test]
async fn progress_eof_does_not_leave_live_child_outside_deadline() -> TestResult {
    let executable = std::env::current_exe()?;
    let group = NativeAudioGroup::for_decoder()?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args([
            "--exact",
            "recordings::decoder::hostile_tests::decoder_waiting_child",
            "--nocapture",
        ])
        .env("SIGY_DECODER_WAITING_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = group.spawn(command)?;
    let mut stdout = child.stdout.take().ok_or("fixture stdout")?;
    let mut byte = [0_u8; 1];
    let mut readiness = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !readiness.ends_with(b"decoder fixture ready\n")
            && !readiness.ends_with(b"decoder fixture ready\r\n")
        {
            if readiness.len() == 4096 || stdout.read(&mut byte).await? == 0 {
                return Err(std::io::Error::other("fixture never became ready"));
            }
            readiness.push(byte[0]);
        }
        Ok::<_, std::io::Error>(())
    })
    .await??;
    drop(stdout);
    assert!(child.try_wait()?.is_none());
    let progress = read_progress(
        b"out_time_us=1\nprogress=end\n".as_slice(),
        Limits::exact(1, 512),
    );
    let start = tokio::time::Instant::now();
    let error = complete(
        &group,
        &mut child,
        progress,
        start + Duration::from_millis(100),
        false,
    )
    .await
    .err()
    .ok_or("live child succeeded")?;
    assert!(error.to_string().contains("deadline"));
    assert!(child.try_wait()?.is_some());
    assert!(start.elapsed() < Duration::from_secs(3));
    Ok(())
}
