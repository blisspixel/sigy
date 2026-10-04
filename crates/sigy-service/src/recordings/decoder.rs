//! `FFmpeg` receives one bounded local byte stream and has no network protocol access.

use crate::{Error, Result};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(test)]
mod hostile_tests;
mod progress;

use progress::{Limits as ProgressLimits, Progress, read_progress};

/// Provisional profile ceiling, not measured sample accuracy. Preserve raw progress.
pub(super) const PROGRESS_TOLERANCE_US: u64 = 100_000;

pub(super) async fn verify(executable: &str, path: &Path, format: &str) -> Result<u64> {
    let input = std::fs::File::open(path)?;
    let mut command = CommandWrap::with_new(executable, |command| {
        command
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostats",
                "-xerror",
                "-max_alloc",
                "16777216",
                "-threads",
                "1",
                "-stats_period",
                "0.5",
                "-filter_threads",
                "1",
                "-protocol_whitelist",
                "pipe",
                "-f",
                format,
                "-i",
                "pipe:0",
                "-map",
                "0:a:0",
                "-vn",
                "-sn",
                "-dn",
                "-threads",
                "1",
                "-progress",
                "pipe:1",
                "-f",
                "null",
                "-",
            ])
            .stdin(Stdio::from(input))
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    });
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CreationFlags, JobObject};
        command
            .wrap(CreationFlags(
                windows::Win32::System::Threading::CREATE_NO_WINDOW,
            ))
            .wrap(JobObject);
    }
    #[cfg(unix)]
    {
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
    }
    let mut child = command
        .spawn()
        .map_err(|_| Error::Acquisition("cannot start configured FFmpeg decoder"))?;
    let stdout = child
        .stdout()
        .take()
        .ok_or(Error::Acquisition("decoder progress pipe unavailable"))?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let progress = read_progress(stdout, ProgressLimits::verification()).await?;
        let status = child.wait().await?;
        if !status.success() {
            return Err(Error::Acquisition(
                "media could not be fully decoded; original retained as unverified",
            ));
        }
        if progress.max_out_us == 0 {
            return Err(Error::Acquisition("decoder produced no audio"));
        }
        if !progress.finished {
            return Err(Error::Acquisition("decoder did not finish"));
        }
        Ok(progress.max_out_us)
    })
    .await;
    match result {
        Ok(Ok(duration)) => Ok(duration),
        failure => {
            // Kill and reap before another worker can consume this decoder slot.
            stop(&mut child).await?;
            failure.unwrap_or(Err(Error::Acquisition("decoder exceeded 30 seconds")))
        }
    }
}

/// Selects a local playback muxer from `ffmpeg -devices` text.
///
/// Capture-only devices are ignored. The result is an output format name, not a hardware device.
pub(super) fn audio_output_muxer(device_list: &str) -> Option<&'static str> {
    const CANDIDATES: &[&str] = if cfg!(windows) {
        &["wasapi", "dsound"]
    } else if cfg!(target_os = "macos") {
        &["coreaudio", "sdl"]
    } else {
        &["pulse", "alsa", "oss"]
    };
    let present: Vec<&str> = device_list
        .lines()
        .filter_map(|line| {
            let bytes = line.as_bytes();
            if bytes.get(2) == Some(&b'E') {
                line[3..].split_whitespace().next()
            } else {
                None
            }
        })
        .collect();
    CANDIDATES
        .iter()
        .copied()
        .find(|candidate| present.contains(candidate))
}

pub(super) struct PlaybackReport {
    pub playhead_us: u64,
    pub reported_elapsed_us: u64,
    pub boundary_tolerance_us: u64,
    pub progress_advanced: bool,
}

fn supervise(mut command: process_wrap::tokio::CommandWrap) -> process_wrap::tokio::CommandWrap {
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CreationFlags, JobObject};
        command
            .wrap(CreationFlags(
                windows::Win32::System::Threading::CREATE_NO_WINDOW,
            ))
            .wrap(JobObject);
    }
    #[cfg(unix)]
    {
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
    }
    command
}

async fn stop(child: &mut Box<dyn process_wrap::tokio::ChildWrapper>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        if child.try_wait()?.is_none() {
            Box::into_pin(child.kill()).await?;
        }
        child.wait().await?;
        Ok(())
    })
    .await
    .unwrap_or(Err(Error::Acquisition("decoder stop remains unproven")))
}

pub(super) async fn play_file(
    executable: &str,
    path: &Path,
    format: &str,
    system_audio: bool,
    seek_us: u64,
    decoded_us: u64,
) -> Result<PlaybackReport> {
    if !crate::storage::dvr::retained_format(format) {
        return Err(Error::Acquisition("unsupported playback format"));
    }
    if seek_us >= decoded_us {
        return Err(Error::Acquisition("seek is outside the retained audio"));
    }
    let remaining_us = decoded_us - seek_us;
    let work_seconds = remaining_us / 1_000_000 + 15;
    let deadline = tokio::time::Instant::now()
        .checked_add(Duration::from_secs(work_seconds))
        .ok_or(Error::Acquisition("invalid playback deadline"))?;
    let work_us = work_seconds
        .checked_mul(1_000_000)
        .ok_or(Error::Acquisition("invalid playback deadline"))?;
    let progress_limits = ProgressLimits::playback(remaining_us, work_us)?;
    if !path.is_file() {
        return Err(Error::Acquisition("retained media is missing"));
    }
    let system_muxer = if system_audio {
        Some(system_output_muxer(executable).await?)
    } else {
        None
    };
    let mut child = supervise(playback_command(
        executable,
        path,
        format,
        seek_us,
        system_muxer,
    ))
    .spawn()
    .map_err(|_| Error::Acquisition("cannot start configured FFmpeg decoder"))?;
    let stdout = child
        .stdout()
        .take()
        .ok_or(Error::Acquisition("decoder progress pipe unavailable"))?;
    let progress = complete(
        &mut child,
        read_progress(stdout, progress_limits),
        deadline,
        system_audio,
    )
    .await?;
    require_range_progress(&progress, remaining_us)?;
    Ok(PlaybackReport {
        playhead_us: seek_us
            .checked_add(progress.max_out_us)
            .ok_or(Error::Acquisition("decoder progress overflow"))?,
        reported_elapsed_us: progress.max_out_us,
        boundary_tolerance_us: PROGRESS_TOLERANCE_US,
        progress_advanced: progress.advanced,
    })
}

pub(super) async fn play_reader(
    executable: &str,
    input: impl AsyncRead + Unpin,
    format: &str,
    system_audio: bool,
) -> Result<PlaybackReport> {
    play_stream(executable, input, format, system_audio, None).await
}

pub(super) async fn play_range_reader(
    executable: &str,
    input: impl AsyncRead + Unpin,
    format: &str,
    system_audio: bool,
    seek_us: u64,
    end_us: u64,
    maximum_bytes: u64,
) -> Result<PlaybackReport> {
    if seek_us >= end_us
        || end_us > 30 * 60 * 1_000_000
        || maximum_bytes == 0
        || maximum_bytes > 512 * 1024 * 1024
    {
        return Err(Error::Acquisition("invalid retained playback range"));
    }
    play_stream(
        executable,
        input,
        format,
        system_audio,
        Some((seek_us, end_us, maximum_bytes)),
    )
    .await
}

async fn play_stream(
    executable: &str,
    input: impl AsyncRead + Unpin,
    format: &str,
    system_audio: bool,
    range: Option<(u64, u64, u64)>,
) -> Result<PlaybackReport> {
    let supported = if range.is_some() {
        crate::storage::dvr::retained_format(format)
    } else {
        matches!(format, "mp3" | "aac" | "flac" | "ogg" | "wav")
    };
    if !supported {
        return Err(Error::Acquisition("unsupported playback format"));
    }
    let system_muxer = if system_audio {
        Some(system_output_muxer(executable).await?)
    } else {
        None
    };
    let bounds = range.map(|(seek, end, _)| (seek, end));
    let mut child = supervise(pipe_playback_command(
        executable,
        format,
        system_muxer,
        bounds,
    ))
    .spawn()
    .map_err(|_| Error::Acquisition("cannot start configured FFmpeg decoder"))?;
    let mut stdin = child
        .stdin()
        .take()
        .ok_or(Error::Acquisition("decoder audio pipe unavailable"))?;
    let stdout = child
        .stdout()
        .take()
        .ok_or(Error::Acquisition("decoder progress pipe unavailable"))?;
    let (seek_us, end_us, maximum_bytes) = range.unwrap_or((
        0,
        15 * 60 * 1_000_000,
        crate::sources::http::MAXIMUM_BODY_BYTES,
    ));
    // Output-side seek and input stalls can emit zero-time frames until the wall deadline.
    let work_seconds = end_us / 1_000_000 + 30;
    let progress_limits = ProgressLimits::playback(end_us - seek_us, work_seconds * 1_000_000)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(work_seconds);
    let work = async {
        let pump = async {
            let copied = if range.is_some() {
                Box::pin(copy_range(input, &mut stdin, maximum_bytes)).await
            } else {
                Box::pin(copy_limited(input, &mut stdin)).await
            };
            drop(stdin);
            copied
        };
        let ((), progress) = tokio::try_join!(pump, read_progress(stdout, progress_limits))?;
        Ok(progress)
    };
    let progress = complete(&mut child, work, deadline, system_audio).await?;
    if range.is_some() {
        require_range_progress(&progress, end_us - seek_us)?;
    }
    Ok(PlaybackReport {
        playhead_us: seek_us
            .checked_add(progress.max_out_us)
            .ok_or(Error::Acquisition("decoder progress overflow"))?,
        reported_elapsed_us: progress.max_out_us,
        boundary_tolerance_us: PROGRESS_TOLERANCE_US,
        progress_advanced: progress.advanced,
    })
}

fn require_range_progress(progress: &Progress, duration_us: u64) -> Result<()> {
    if progress.max_out_us < duration_us.saturating_sub(PROGRESS_TOLERANCE_US) {
        return Err(Error::Acquisition(
            "decoder output ended before requested range",
        ));
    }
    Ok(())
}

async fn copy_range(
    mut input: impl AsyncRead + Unpin,
    output: &mut (impl AsyncWrite + Unpin),
    maximum_bytes: u64,
) -> Result<()> {
    let mut chunk = [0_u8; 8192];
    let mut total = 0_u64;
    loop {
        let read = input.read(&mut chunk).await?;
        if read == 0 {
            return if total == 0 {
                Err(Error::Acquisition("listen produced no audio"))
            } else {
                Ok(())
            };
        }
        total += u64::try_from(read).map_err(|_| Error::Acquisition("listen byte limit"))?;
        if total > maximum_bytes {
            return Err(Error::Acquisition("listen byte limit"));
        }
        if let Err(error) = output.write_all(&chunk[..read]).await {
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                // An output-limited decoder can stop consuming before encoded EOF.
                // Successful exit and requested-duration progress are checked separately.
                return Ok(());
            }
            return Err(error.into());
        }
    }
}

async fn complete(
    child: &mut Box<dyn process_wrap::tokio::ChildWrapper>,
    work: impl std::future::Future<Output = Result<Progress>>,
    deadline: tokio::time::Instant,
    system_audio: bool,
) -> Result<Progress> {
    let result = tokio::time::timeout_at(deadline, async {
        let progress = work.await?;
        let status = child.wait().await?;
        if !status.success() {
            return Err(Error::Acquisition(if system_audio {
                "cannot open system audio"
            } else {
                "playback did not finish"
            }));
        }
        if !progress.finished || progress.max_out_us == 0 {
            return Err(Error::Acquisition("decoder produced no audio"));
        }
        Ok(progress)
    })
    .await
    .unwrap_or(Err(Error::Acquisition("playback exceeded its deadline")));
    if result.is_err() {
        stop(child).await?;
    }
    result
}

async fn copy_limited(
    input: impl AsyncRead + Unpin,
    output: &mut (impl AsyncWrite + Unpin),
) -> Result<()> {
    let copied = tokio::io::copy(
        &mut input.take(crate::sources::http::MAXIMUM_BODY_BYTES + 1),
        output,
    )
    .await
    .map_err(|_| Error::Acquisition("listen audio pipe closed"))?;
    if copied == 0 {
        return Err(Error::Acquisition("listen produced no audio"));
    }
    if copied > crate::sources::http::MAXIMUM_BODY_BYTES {
        return Err(Error::Acquisition("listen byte limit"));
    }
    Ok(())
}

fn pipe_playback_command(
    executable: &str,
    format: &str,
    system_muxer: Option<&str>,
    range: Option<(u64, u64)>,
) -> process_wrap::tokio::CommandWrap {
    CommandWrap::with_new(executable, |command| {
        command.args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-xerror",
            "-max_alloc",
            "16777216",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-protocol_whitelist",
            "pipe",
            "-stats_period",
            "0.1",
        ]);
        if range.is_none() {
            command.arg("-re");
        }
        command.args([
            "-f",
            format,
            "-i",
            "pipe:0",
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-threads",
            "1",
            "-progress",
            "pipe:1",
        ]);
        if let Some((seek_us, end_us)) = range {
            let seek = timestamp(seek_us);
            let duration = timestamp(end_us - seek_us);
            command.args(["-ss", seek.as_str(), "-t", duration.as_str()]);
        }
        if let Some(muxer) = system_muxer {
            command.args(["-f", muxer, "default"]);
        } else {
            command.args(["-f", "null", "-"]);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    })
}

fn timestamp(microseconds: u64) -> String {
    format!(
        "{}.{:06}",
        microseconds / 1_000_000,
        microseconds % 1_000_000
    )
}

fn playback_command(
    executable: &str,
    path: &Path,
    format: &str,
    seek_us: u64,
    system_muxer: Option<&str>,
) -> process_wrap::tokio::CommandWrap {
    let timestamp = format!("{}.{:06}", seek_us / 1_000_000, seek_us % 1_000_000);
    CommandWrap::with_new(executable, |command| {
        command
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostats",
                "-xerror",
                "-max_alloc",
                "16777216",
                "-threads",
                "1",
                "-filter_threads",
                "1",
                "-protocol_whitelist",
                "file",
                "-stats_period",
                "0.1",
                "-re",
            ])
            .args(
                (seek_us > 0)
                    .then_some(["-ss", timestamp.as_str()])
                    .into_iter()
                    .flatten(),
            )
            .args(["-f", format, "-i"])
            .arg(path)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-sn",
                "-dn",
                "-threads",
                "1",
                "-progress",
                "pipe:1",
            ]);
        if let Some(muxer) = system_muxer {
            command.args(["-f", muxer, "default"]);
        } else {
            command.args(["-f", "null", "-"]);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    })
}

async fn system_output_muxer(executable: &str) -> Result<&'static str> {
    let mut child = supervise(CommandWrap::with_new(executable, |command| {
        command
            .args(["-hide_banner", "-devices"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    }))
    .spawn()
    .map_err(|_| Error::Acquisition("cannot start configured FFmpeg decoder"))?;
    let stdout = child
        .stdout()
        .take()
        .ok_or(Error::Acquisition("decoder progress pipe unavailable"))?;
    let mut output = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        stdout.take(65_537).read_to_end(&mut output).await?;
        if output.len() > 65_536 {
            return Err(Error::Acquisition("decoder progress limit"));
        }
        child.wait().await?;
        Ok(output)
    })
    .await;
    let output = match read {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            stop(&mut child).await?;
            return Err(error);
        }
        Err(_) => {
            stop(&mut child).await?;
            return Err(Error::Acquisition("playback exceeded its deadline"));
        }
    };
    let text = std::str::from_utf8(&output)
        .map_err(|_| Error::Acquisition("configured FFmpeg has no local audio output device"))?;
    audio_output_muxer(text).ok_or(Error::Acquisition(
        "configured FFmpeg has no local audio output device",
    ))
}

#[cfg(test)]
mod tests {
    use super::audio_output_muxer;

    #[test]
    fn platform_output_muxer_ignores_capture_devices() {
        let list = "\
---
 D  dshow           DirectShow capture
  E wasapi          WASAPI output
  E dsound          DirectSound output
  E pulse           PulseAudio output
  E alsa            ALSA output
  E coreaudio       CoreAudio output
";
        let selected = audio_output_muxer(list);
        #[cfg(windows)]
        assert_eq!(selected, Some("wasapi"));
        #[cfg(target_os = "macos")]
        assert_eq!(selected, Some("coreaudio"));
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(selected, Some("pulse"));
    }

    #[test]
    fn capture_only_list_has_no_playback_muxer() {
        let list = "\
---
 D  dshow           DirectShow capture
 D  openal          OpenAL audio capture device
";
        assert_eq!(audio_output_muxer(list), None);
    }
}
