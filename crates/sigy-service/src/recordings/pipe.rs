//! One private local byte pipe. The name is a random nonce, never a source URL.

use std::{path::Path, time::Duration};

use interprocess::local_socket::{
    ListenerOptions,
    tokio::{Listener, Stream, prelude::*},
};

use crate::{Error, Result, library::control_directory, local_stream::LocalStream};

pub(crate) struct ListenPipe {
    nonce: String,
    listener: Listener,
    #[cfg(unix)]
    path: std::path::PathBuf,
}

impl ListenPipe {
    pub(crate) fn nonce(&self) -> &str {
        &self.nonce
    }

    /// Dropping the returned writer leaves accepted bytes readable by the
    /// client and starts no detached cleanup owner.
    /// # Errors
    /// Returns a stop, timeout, transport, or peer-check error. The pipe file is removed.
    pub(crate) async fn accept(
        &self,
        stop: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<LocalStream> {
        let stream = LocalStream::new(wait_for_client(&self.listener, stop).await?);
        verify_listen_peer(stream.get())?;
        Ok(stream)
    }
}

impl Drop for ListenPipe {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// # Errors
/// Rejects a directory that is not a private library directory, or a pipe that cannot be created.
pub(crate) fn bind_listen_pipe(directory: &Path) -> Result<ListenPipe> {
    let directory = control_directory(directory)?;
    let nonce = new_nonce()?;
    #[cfg(unix)]
    let path = prepare_socket(&directory, &nonce)?;
    let listener = pipe_options(&directory, &nonce)?.create_tokio()?;
    Ok(ListenPipe {
        nonce,
        listener,
        #[cfg(unix)]
        path,
    })
}

/// # Errors
/// Rejects a nonce that is not the 32-character hex form created by this process.
pub(crate) async fn connect_listen_pipe(directory: &Path, nonce: &str) -> Result<LocalStream> {
    if !valid_listen_nonce(nonce) {
        return Err(Error::InvalidInput("listen pipe"));
    }
    let directory = control_directory(directory)?;
    let stream = LocalStream::new(Stream::connect(pipe_name(&directory, nonce)?).await?);
    verify_listen_peer(stream.get())?;
    Ok(stream)
}

#[must_use]
pub(crate) fn valid_listen_nonce(nonce: &str) -> bool {
    nonce.len() == 32
        && nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn new_nonce() -> Result<String> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| Error::InvalidInput("secure random source unavailable"))?;
    Ok(format!("{:032x}", u128::from_be_bytes(random)))
}

fn pipe_name(directory: &Path, nonce: &str) -> Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        let _ = directory;
        Ok(format!("sigy-listen-{nonce}").to_ns_name::<GenericNamespaced>()?)
    }
    #[cfg(unix)]
    {
        use interprocess::local_socket::GenericFilePath;
        Ok(socket_path(directory, nonce).to_fs_name::<GenericFilePath>()?)
    }
}

fn pipe_options(directory: &Path, nonce: &str) -> Result<ListenerOptions<'static>> {
    let options = ListenerOptions::new().name(pipe_name(directory, nonce)?);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        // Same owner and SYSTEM grant as the control pipe. Network logons are denied.
        let descriptor = SecurityDescriptor::deserialize(widestring::u16cstr!(
            "D:P(D;;GA;;;NU)(A;;GA;;;OW)(A;;GA;;;SY)"
        ))?;
        options.security_descriptor(descriptor)
    };
    #[cfg(unix)]
    let options = {
        #[cfg(target_os = "linux")]
        let options = {
            use interprocess::os::unix::local_socket::ListenerOptionsExt;
            options.mode(0o600)
        };
        options
    };
    Ok(options)
}

#[cfg(unix)]
fn prepare_socket(directory: &Path, nonce: &str) -> Result<std::path::PathBuf> {
    use std::os::unix::fs::FileTypeExt;
    let path = socket_path(directory, nonce);
    crate::library::reject_link(&path)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            return Err(Error::InvalidInput("listen pipe"));
        }
        Ok(_) => return Err(Error::InvalidInput("listen pipe")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    Ok(path)
}

#[cfg(unix)]
fn socket_path(directory: &Path, nonce: &str) -> std::path::PathBuf {
    directory.join(format!("listen-{nonce}.sock"))
}

async fn wait_for_client(
    listener: &Listener,
    stop: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<Stream> {
    let accept = listener.accept();
    tokio::pin!(accept);
    tokio::select! {
        biased;
        () = async { if !*stop.borrow() { let _ = stop.changed().await; } } => {
            Err(Error::Acquisition("listen stopped"))
        }
        result = tokio::time::timeout(Duration::from_secs(5), &mut accept) => {
            match result {
                Ok(Ok(stream)) => Ok(stream),
                Ok(Err(error)) => Err(error.into()),
                Err(_) => Err(Error::Acquisition("listen pipe was not opened")),
            }
        }
    }
}

fn verify_listen_peer(stream: &Stream) -> Result<()> {
    #[cfg(unix)]
    if stream.peer_creds()?.euid() != Some(rustix::process::geteuid().as_raw()) {
        return Err(Error::Protocol(
            "listen pipe peer belongs to a different user",
        ));
    }
    #[cfg(windows)]
    let _ = stream.peer_creds()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dropped_listen_writer_delivers_its_tail_then_end_of_stream() -> TestResult {
        let temporary = tempfile::tempdir()?;
        let library = Library::open(temporary.path(), true)?;
        let pipe = bind_listen_pipe(library.directory())?;
        let (_running, mut stop) = tokio::sync::watch::channel(false);
        let (writer, reader) = tokio::join!(
            pipe.accept(&mut stop),
            connect_listen_pipe(library.directory(), pipe.nonce())
        );
        let mut reader = reader?;
        // Larger than a pipe buffer: the last native write is still pending
        // when the writer is dropped, as at the end of a listen transfer.
        let payload: Vec<u8> = (0..(3 * 1024 * 1024 + 5))
            .map(|index: u32| u8::try_from(index % 241).unwrap_or(0))
            .collect();
        let received =
            crate::local_stream::tests::deliver_then_drop(writer?, &mut reader, payload.clone())
                .await?;
        assert_eq!(received.len(), payload.len());
        assert!(received == payload, "listen bytes differ from the payload");
        Ok(())
    }

    #[tokio::test]
    async fn listen_pipe_refuses_foreign_nonces_and_a_prior_stop() -> TestResult {
        let temporary = tempfile::tempdir()?;
        let library = Library::open(temporary.path(), true)?;
        for nonce in [
            "",
            "ABCDEF0123456789ABCDEF0123456789",
            "0123456789abcdef0123456789abcdeg",
        ] {
            assert!(matches!(
                connect_listen_pipe(library.directory(), nonce).await,
                Err(Error::InvalidInput("listen pipe"))
            ));
        }
        let pipe = bind_listen_pipe(library.directory())?;
        assert!(valid_listen_nonce(pipe.nonce()));
        let (stopping, mut stop) = tokio::sync::watch::channel(false);
        stopping.send(true)?;
        assert!(matches!(
            pipe.accept(&mut stop).await,
            Err(Error::Acquisition("listen stopped"))
        ));
        Ok(())
    }
}
