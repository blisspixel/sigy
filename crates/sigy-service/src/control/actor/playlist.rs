use super::{Actor, Message};
use crate::{Error, Result, sources::playlist::ResolvedPlaylist};

impl Actor {
    pub(super) fn start_playlist(&mut self, id: &str, revision: &str) -> Result<()> {
        if !self.library.store_mut().begin_playlist(id, revision)? {
            return Ok(());
        }
        // A directory HLS flag is a hint. The document decides: a master playlist lists
        // candidates, and a media playlist fails without storing any.
        let source = self
            .library
            .store()
            .source(revision)?
            .ok_or(Error::NotFound)?
            .source;
        let acquirer = self.acquirer.clone();
        let request_id = id.to_owned();
        self.playlist_worker = Some(self.spawn_worker(
            move |mut stop| async move {
                tokio::select! {
                    biased;
                    _ = stop.changed() => Err(Error::Acquisition("playlist resolve stopped")),
                    result = crate::sources::playlist::resolve(&acquirer, &source) => result,
                }
            },
            move |result| Message::PlaylistFinished {
                id: request_id,
                result,
            },
        )?);
        Ok(())
    }

    pub(super) fn finish_playlist(
        &mut self,
        id: &str,
        result: Result<ResolvedPlaylist>,
    ) -> Result<()> {
        self.playlist_worker.take();
        match result.and_then(|resolved| self.library.store_mut().finish_playlist(id, &resolved)) {
            Ok(()) => Ok(()),
            Err(error) => self.library.store_mut().fail_playlist(id, &error),
        }
    }
}
