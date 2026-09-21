use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use interprocess::local_socket::{
    ListenerOptions, Name,
    tokio::{Listener, Stream, prelude::*},
};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    library::{control_directory, reject_link},
};

const RECORD: &str = "service.json";

#[derive(Debug, Serialize)]
pub(super) struct Endpoint {
    version: u32,
    process_id: u32,
    nonce: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointRecord {
    version: u32,
    process_id: u32,
    nonce: String,
}

impl Endpoint {
    pub fn new() -> Result<Self> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| Error::Protocol("OS random source unavailable"))?;
        let nonce = format!("{:032x}", u128::from_be_bytes(random));
        Ok(Self {
            version: super::PROTOCOL_VERSION,
            process_id: std::process::id(),
            nonce,
        })
    }

    pub fn load(directory: &Path) -> Result<Self> {
        let path = directory.join(RECORD);
        reject_link(&path)?;
        let mut bytes = Vec::new();
        File::open(path)?.take(1025).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 {
            return Err(Error::Protocol("endpoint record size"));
        }
        let record: EndpointRecord = serde_json::from_slice(&bytes)?;
        if record.version != super::PROTOCOL_VERSION
            || record.process_id == 0
            || record.nonce.len() != 32
            || !record.nonce.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Protocol("endpoint record"));
        }
        Ok(Self {
            version: record.version,
            process_id: record.process_id,
            nonce: record.nonce,
        })
    }

    pub fn publish(&self, directory: &Path) -> Result<Publication> {
        let target = directory.join(RECORD);
        reject_link(&target)?;
        let temporary = directory.join(format!("service-{}.tmp", self.nonce));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec(self)?)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &target)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        Ok(Publication(target))
    }

    #[cfg(windows)]
    fn name(&self, _directory: &Path) -> Result<Name<'static>> {
        use interprocess::local_socket::GenericNamespaced;
        Ok(format!("sigy-{}", self.nonce).to_ns_name::<GenericNamespaced>()?)
    }

    #[cfg(unix)]
    fn name(&self, directory: &Path) -> Result<Name<'static>> {
        use interprocess::local_socket::GenericFilePath;
        Ok(directory
            .join("control.sock")
            .to_fs_name::<GenericFilePath>()?)
    }

    pub fn listen(&self, directory: &Path) -> Result<Listener> {
        let options = ListenerOptions::new().name(self.name(directory)?);
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::{
                local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
            };
            // No inherited Everyone/Anonymous read grants. Network logons are
            // denied; Interprocess also sets PIPE_REJECT_REMOTE_CLIENTS.
            let descriptor = SecurityDescriptor::deserialize(widestring::u16cstr!(
                "D:P(D;;GA;;;NU)(A;;GA;;;OW)(A;;GA;;;SY)"
            ))?;
            options.security_descriptor(descriptor)
        };
        #[cfg(unix)]
        let options = {
            use std::os::unix::fs::FileTypeExt;
            let socket = directory.join("control.sock");
            match fs::symlink_metadata(&socket) {
                Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(socket)?,
                Ok(_) => return Err(Error::InvalidInput("control socket path")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
            // The library lock is held before removing a stale socket.
            #[cfg(target_os = "linux")]
            let options = {
                use interprocess::os::unix::local_socket::ListenerOptionsExt;
                options.mode(0o600)
            };
            options
        };
        Ok(options.create_tokio()?)
    }

    pub async fn connect(&self, directory: &Path) -> Result<Stream> {
        let stream = Stream::connect(self.name(directory)?).await?;
        #[cfg(windows)]
        if stream.peer_creds()?.pid() != Some(self.process_id) {
            return Err(Error::Protocol("service process identity mismatch"));
        }
        verify_peer(&stream)?;
        Ok(stream)
    }
}

pub(super) fn verify_peer(stream: &Stream) -> Result<()> {
    #[cfg(unix)]
    if stream.peer_creds()?.euid() != Some(rustix::process::geteuid().as_raw()) {
        return Err(Error::Protocol("control peer belongs to a different user"));
    }
    #[cfg(windows)]
    let _ = stream.peer_creds()?;
    Ok(())
}

pub(super) fn directory(path: &Path) -> Result<PathBuf> {
    control_directory(path)
}

/// Drop before releasing library ownership, so an old instance cannot remove
/// a newer instance's endpoint record.
pub(super) struct Publication(PathBuf);

impl Drop for Publication {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
