use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Error, Result};

struct BoundedBody {
    bytes: Vec<u8>,
    maximum: usize,
    exceeded: bool,
}

impl std::io::Write for BoundedBody {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other("control response byte limit"));
        }
        let needed = self.bytes.len() + bytes.len();
        if needed > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(needed)
                .min(self.maximum);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(std::io::Error::other)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encode(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>> {
    let mut body = BoundedBody {
        bytes: Vec::new(),
        maximum,
        exceeded: false,
    };
    let result = serde_json::to_writer(&mut body, value);
    if body.exceeded {
        return Err(Error::Protocol("response exceeds its permitted size"));
    }
    result?;
    Ok(body.bytes)
}

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
    let body = encode(value, maximum)?;
    let length = u32::try_from(body.len()).map_err(|_| Error::Protocol("frame length"))?;
    stream.write_u32(length).await?;
    stream.write_all(&body).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::SerializeSeq;
    use std::cell::Cell;

    struct Counted<'a>(&'a Cell<usize>);

    impl Serialize for Counted<'_> {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let mut sequence = serializer.serialize_seq(Some(1_000_000))?;
            for _ in 0..1_000_000 {
                self.0.set(self.0.get() + 1);
                sequence.serialize_element("station metadata")?;
            }
            sequence.end()
        }
    }

    #[tokio::test]
    async fn response_refusal_stops_encoding_before_transport_or_full_allocation() -> Result<()> {
        let count = Cell::new(0);
        let mut output = Vec::new();
        assert!(matches!(
            write(&mut output, &Counted(&count), 64).await,
            Err(Error::Protocol(_))
        ));
        assert!(count.get() <= 4);
        assert!(output.is_empty());
        assert!(matches!(encode(&true, 0), Err(Error::Protocol(_))));
        Ok(())
    }

    #[tokio::test]
    async fn exact_response_limit_preserves_existing_wire_bytes() -> Result<()> {
        let value = "original \"script\" العربية";
        let expected = serde_json::to_vec(&value)?;
        let mut output = Vec::new();
        write(&mut output, &value, expected.len()).await?;
        let mut input = output.as_slice();
        assert_eq!(read::<String>(&mut input, expected.len()).await?, value);
        assert!(input.is_empty());
        assert_eq!(output.get(4..), Some(expected.as_slice()));
        assert!(matches!(
            encode(&value, expected.len() - 1),
            Err(Error::Protocol(_))
        ));
        Ok(())
    }

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
