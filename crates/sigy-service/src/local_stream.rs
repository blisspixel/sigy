//! Owned local streams whose drop never starts detached native cleanup.
//!
//! On Windows, Interprocess 2.4.4 hands a written pipe that was never flushed
//! to a detached linger thread when it is dropped. That thread blocks in a real
//! flush until the peer reads, then closes the pipe outside the runtime.
//! Every control and listen stream in this crate is owned here instead.
//! Dropping it first clears the dirty flag through the supported
//! `assume_flushed` API, so the pipe closes on the runtime that owns it.
//!
//! Clearing the flag does not cancel a native write that is already pending.
//! Mio keeps that write's buffer and handle until it completes, so bytes the
//! stream has accepted still reach a reading peer before end of stream. Bytes
//! the peer never reads are abandoned with the pipe. Streams are never split
//! or cloned, so the flag cannot be shared with another owner.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use interprocess::local_socket::tokio::Stream;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};

use crate::{Error, Result};

pub(crate) struct LocalStream(Stream);

impl LocalStream {
    pub(crate) const fn new(stream: Stream) -> Self {
        Self(stream)
    }

    pub(crate) const fn get(&self) -> &Stream {
        &self.0
    }

    /// Waits for the peer to close after a complete exchange. The caller bounds
    /// this wait with its absolute deadline. A further byte is a protocol fault.
    /// # Errors
    /// Returns a transport error, or a protocol error for bytes after the exchange.
    pub(crate) async fn peer_closed(&mut self) -> Result<()> {
        let mut trailing = [0_u8; 1];
        if self.0.read(&mut trailing).await? == 0 {
            Ok(())
        } else {
            Err(Error::Protocol("bytes after a complete local exchange"))
        }
    }
}

impl Drop for LocalStream {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let Stream::NamedPipe(pipe) = &self.0;
            pipe.inner().assume_flushed();
        }
    }
}

impl AsyncRead for LocalStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl AsyncWrite for LocalStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

#[cfg(test)]
pub(crate) mod tests;
