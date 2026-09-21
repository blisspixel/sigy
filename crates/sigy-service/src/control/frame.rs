use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Error, Result};

pub(super) async fn read<T: DeserializeOwned>(
    stream: &mut (impl AsyncRead + Unpin),
    maximum: usize,
) -> Result<T> {
    let length =
        usize::try_from(stream.read_u32().await?).map_err(|_| Error::Protocol("frame length"))?;
    if length == 0 || length > maximum {
        return Err(Error::Protocol("frame exceeds its permitted size"));
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

pub(super) async fn write(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
    maximum: usize,
) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    if body.len() > maximum {
        return Err(Error::Protocol("response exceeds its permitted size"));
    }
    let length = u32::try_from(body.len()).map_err(|_| Error::Protocol("frame length"))?;
    stream.write_u32(length).await?;
    stream.write_all(&body).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_length_is_rejected_without_waiting_for_a_body() -> Result<()> {
        let (mut reader, mut writer) = tokio::io::duplex(4);
        writer.write_u32(u32::MAX).await?;
        assert!(matches!(
            read::<serde_json::Value>(&mut reader, 1024).await,
            Err(Error::Protocol(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn truncated_and_invalid_frames_fail() -> Result<()> {
        for bytes in [
            &[0, 0, 0, 2, b'{'][..],
            &[0, 0, 0, 1, 0xff][..],
            &[0, 0, 0, 0][..],
        ] {
            let mut input = bytes;
            assert!(read::<serde_json::Value>(&mut input, 1024).await.is_err());
        }
        Ok(())
    }
}
