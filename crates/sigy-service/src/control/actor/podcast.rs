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

    pub(super) fn start_publisher_text(
        &mut self,
        id: &str,
        subscription_id: &str,
        episode_id: &str,
        kind: &str,
        index: u32,
    ) -> Result<()> {
        if self.text_worker.is_some() {
            return Err(Error::InvalidInput("publisher text is already running"));
        }
        let kind = crate::podcast::TextKind::parse(kind)?;
        let admission = self.library.store_mut().begin_publisher_text(
            id,
            subscription_id,
            episode_id,
            kind,
            index,
        )?;
        let crate::storage::podcast_text::TextAdmission::Fetch { source, media_type } = admission
        else {
            return Ok(());
        };
        let owned = id.to_owned();
        let fetch_id = owned.clone();
        let acquirer = self.acquirer.clone();
        match self.spawn_worker(
            move |mut stop| async move {
                tokio::select! {
                    biased;
                    _ = stop.changed() => Err(Error::Acquisition("publisher text stopped")),
                    result = crate::podcast::fetch_text(&acquirer, &source, kind, &media_type) => result,
                }
            },
            move |result| Message::PublisherTextFinished { id: owned, result },
        ) {
            Ok(worker) => {
                self.text_worker = Some(worker);
                Ok(())
            }
            Err(error) => {
                self.library
                    .store_mut()
                    .fail_publisher_text(&fetch_id, &error)?;
                Err(error)
            }
        }
    }

    pub(super) fn finish_publisher_text(
        &mut self,
        id: &str,
        result: Result<crate::storage::podcast_text::TextDocument>,
    ) -> Result<()> {
        self.text_worker.take();
        match result {
            Ok(document) => self
                .library
                .store_mut()
                .finish_publisher_text(id, &document),
            Err(error) => self.library.store_mut().fail_publisher_text(id, &error),
        }
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
