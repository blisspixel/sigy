//! `FFmpeg` receives one bounded local byte stream and has no network protocol access.

use crate::{Error, Result};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

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
