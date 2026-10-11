//! A bounded clock-convention check, not broad decoder qualification.

use super::{NativeAudioGroup, ProgressLimits, read_progress};
use crate::{Error, Result};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

#[cfg(test)]
mod tests;

pub(super) async fn check(executable: &str, group: &NativeAudioGroup) -> Result<()> {
    let work = async {
        let mut command = tokio::process::Command::new(executable);
        command
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "quiet",
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
                "wav",
                "-i",
                "pipe:0",
                "-map",
                "0:a:0",
                "-threads",
                "1",
                "-progress",
                "pipe:1",
                "-f",
                "null",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = group.spawn(command)?;
        let mut input = child
            .stdin
            .take()
            .ok_or(Error::Acquisition("decoder calibration input missing"))?;
        let output = child
            .stdout
            .take()
            .ok_or(Error::Acquisition("decoder calibration output missing"))?;
        let bytes = wave();
        let write = async move {
            input.write_all(&bytes).await?;
            input.shutdown().await?;
            drop(input);
            Ok::<_, Error>(())
        };
        let ((), progress) =
            tokio::try_join!(write, read_progress(output, ProgressLimits::verification()))?;
        if !child.wait().await?.success() || !progress.finished || progress.max_out_us != 1_000_000
        {
            return Err(Error::Acquisition(
                "incompatible decoder progress: use a tested FFmpeg 9.x build; clock calibration failed",
            ));
        }
        Ok(())
    };
    match tokio::time::timeout(Duration::from_secs(5), work).await {
        Ok(Ok(())) => Ok(()),
        failure => {
            let _ = group.kill();
            group.finish().await?;
            failure.unwrap_or(Err(Error::Acquisition(
                "decoder clock calibration exceeded five seconds",
            )))
        }
    }
}

fn wave() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8044);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&8036_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    for value in [1_u16, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [8000_u32, 8000] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [1_u16, 8] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.resize(8044, 128);
    bytes
}
