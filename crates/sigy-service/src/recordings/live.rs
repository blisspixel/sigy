//! Direct listen transfer. Bytes go to a local pipe. Nothing is published as a recording.

use std::{future::Future, path::PathBuf};

use crate::{
    Error, Result,
    sources::{
        HttpSource,
        http::{HttpAcquirer, TransferEnd},
    },
};

use super::pipe::bind_listen_pipe;

/// # Errors
/// Returns acquisition, pipe, or stop errors. Playlist and ICY responses fail before `ready`.
pub(crate) async fn stream_revision<F, Fut>(
    directory: PathBuf,
    source: HttpSource,
    acquirer: HttpAcquirer,
    stop: &mut tokio::sync::watch::Receiver<bool>,
    ready: F,
) -> Result<String>
where
    F: FnOnce(String, String) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let limits = crate::sources::http::AcquisitionLimits::new(
        crate::sources::http::MAXIMUM_BODY_BYTES,
        crate::sources::http::MAXIMUM_DURATION,
    )?;
    let download = acquirer.open_audio(&source, limits, stop).await?;
    let format = download.format_name().to_owned();
    let pipe = bind_listen_pipe(&directory)?;
    let nonce = pipe.nonce().to_owned();
    ready(nonce, format.clone()).await?;
    let mut writer = pipe.accept(stop).await?;
    let receipt = HttpAcquirer::copy_audio(download, &mut writer, stop).await?;
    if receipt.end == TransferEnd::UserStop {
        return Err(Error::Acquisition("listen stopped"));
    }
    Ok(format)
}
