//! `FFmpeg` receives one bounded local byte stream and has no network protocol access.

use crate::{Error, Result};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

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
    let mut output = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        stdout.take(65_537).read_to_end(&mut output).await?;
        if output.len() > 65_536 {
            return Err(Error::Acquisition("decoder progress limit"));
        }
        let status = child.wait().await?;
        if !status.success() {
            return Err(Error::Acquisition(
                "media could not be fully decoded; original retained as unverified",
            ));
        }
        let text = std::str::from_utf8(&output)
            .map_err(|_| Error::Acquisition("invalid decoder progress"))?;
        let duration = text
            .lines()
            .filter_map(|line| line.strip_prefix("out_time_us="))
            .filter_map(|value| value.parse::<u64>().ok())
            .max()
            .filter(|value| *value > 0)
            .ok_or(Error::Acquisition("decoder produced no audio"))?;
        if !text.lines().any(|line| line == "progress=end") {
            return Err(Error::Acquisition("decoder did not finish"));
        }
        Ok(duration)
    })
    .await;
    match result {
        Ok(Ok(duration)) => Ok(duration),
        failure => {
            // Kill and reap before another worker can consume this decoder slot.
            if child.try_wait()?.is_none() {
                Box::into_pin(child.kill()).await?;
            }
            child.wait().await?;
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
    if child.try_wait()?.is_none() {
        Box::into_pin(child.kill()).await?;
    }
    child.wait().await?;
    Ok(())
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
    let remaining_seconds = decoded_us.saturating_sub(seek_us) / 1_000_000;
    let read = tokio::time::timeout(Duration::from_secs(remaining_seconds + 15), async {
        read_progress(stdout).await
    })
    .await;
    let progress = match read {
        Ok(Ok(progress)) => progress,
        Ok(Err(error)) => {
            stop(&mut child).await?;
            return Err(error);
        }
        Err(_) => {
            stop(&mut child).await?;
            return Err(Error::Acquisition("playback exceeded its deadline"));
        }
    };
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
    Ok(PlaybackReport {
        playhead_us: seek_us.saturating_add(progress.max_out_us).min(decoded_us),
        progress_advanced: progress.advanced,
    })
}

pub(super) async fn play_reader(
    executable: &str,
    input: impl AsyncRead + Unpin,
    format: &str,
    system_audio: bool,
) -> Result<PlaybackReport> {
    if !matches!(format, "mp3" | "aac" | "flac" | "ogg" | "wav") {
        return Err(Error::Acquisition("unsupported playback format"));
    }
    let system_muxer = if system_audio {
        Some(system_output_muxer(executable).await?)
    } else {
        None
    };
    let mut child = supervise(pipe_playback_command(executable, format, system_muxer))
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
    let progress = match tokio::time::timeout(Duration::from_secs(15 * 60 + 30), async {
        let copied = copy_limited(input, &mut stdin).await;
        drop(stdin);
        copied?;
        read_progress(stdout).await
    })
    .await
    {
        Ok(Ok(progress)) => progress,
        Ok(Err(error)) => {
            stop(&mut child).await?;
            return Err(error);
        }
        Err(_) => {
            stop(&mut child).await?;
            return Err(Error::Acquisition("playback exceeded its deadline"));
        }
    };
    finish_playback(&mut child, system_audio, progress).await
}

async fn finish_playback(
    child: &mut Box<dyn process_wrap::tokio::ChildWrapper>,
    system_audio: bool,
    progress: Progress,
) -> Result<PlaybackReport> {
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
    Ok(PlaybackReport {
        playhead_us: progress.max_out_us,
        progress_advanced: progress.advanced,
    })
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
            "-re",
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

struct Progress {
    max_out_us: u64,
    advanced: bool,
    finished: bool,
}

async fn read_progress(stdout: impl AsyncReadExt + Unpin) -> Result<Progress> {
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut line = String::new();
    let mut total = 0_usize;
    let mut max_out_us = 0_u64;
    let mut previous: Option<u64> = None;
    let mut advanced = false;
    let mut finished = false;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read);
        if total > 65_536 {
            return Err(Error::Acquisition("decoder progress limit"));
        }
        let trimmed = line.trim_end();
        if let Some(value) = trimmed.strip_prefix("out_time_us=") {
            if let Ok(value) = value.parse::<u64>() {
                if previous.is_some_and(|earlier| value > earlier) {
                    advanced = true;
                }
                previous = Some(value);
                max_out_us = max_out_us.max(value);
            }
        } else if trimmed == "progress=end" {
            finished = true;
        }
    }
    Ok(Progress {
        max_out_us,
        advanced,
        finished,
    })
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
