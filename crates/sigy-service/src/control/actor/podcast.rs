use super::{Actor, Message};
use crate::{Error, Result, storage::podcasts::PodcastPolls};

impl Actor {
    pub(super) fn start_podcast_refresh(&mut self, id: &str, subscription_id: &str) -> Result<()> {
        let Some((polls, source)) = self.library.store().podcast_authority(subscription_id)? else {
            return Err(Error::NotFound);
        };
        if polls != PodcastPolls::Active {
            return Err(Error::InvalidInput("podcast polling is stopped"));
        }
        if !self
            .library
            .store_mut()
            .begin_podcast_refresh(id, subscription_id)?
        {
            return Ok(());
        }
        let id = id.to_owned();
        let acquirer = self.acquirer.clone();
        self.podcast_worker = Some(self.spawn_worker(
            move |mut stop| async move {
                tokio::select! {
                    biased;
                    _ = stop.changed() => Err(Error::Acquisition("podcast refresh stopped")),
                    result = crate::podcast::fetch(&acquirer, &source) => result,
                }
            },
            move |result| Message::PodcastFinished { id, result },
        )?);
        Ok(())
    }

    pub(super) fn finish_podcast(
        &mut self,
        id: &str,
        result: Result<crate::podcast::FeedCommit>,
    ) -> Result<()> {
        self.podcast_worker.take();
        match result.and_then(|commit| self.library.store_mut().finish_podcast_refresh(id, &commit))
        {
            Ok(()) => Ok(()),
            Err(error) => self.library.store_mut().fail_podcast_refresh(id, &error),
        }
    }
}
