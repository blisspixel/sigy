use super::{Actor, Message};
use crate::{
    Error, Result,
    discovery::{RefreshBatch, RefreshRequest, radio_browser},
};

impl Actor {
    pub(super) fn start_directory(&mut self, id: &str, request: RefreshRequest) -> Result<()> {
        if !self.library.store_mut().begin_refresh(id, &request)? {
            return Ok(());
        }
        let id = id.to_owned();
        let acquirer = self.acquirer.clone();
        self.directory_worker = Some(self.spawn_worker(
            move |mut stop| async move {
                tokio::select! {
                    biased;
                    _ = stop.changed() => Err(Error::Acquisition("directory refresh stopped")),
                    result = radio_browser::refresh(&acquirer, &request) => result,
                }
            },
            move |result| Message::DirectoryFinished { id, result },
        )?);
        Ok(())
    }

    pub(super) fn finish_directory(
        &mut self,
        id: &str,
        result: Result<RefreshBatch>,
    ) -> Result<()> {
        self.directory_worker.take();
        match result.and_then(|batch| self.library.store_mut().finish_refresh(id, batch)) {
            Ok(()) => Ok(()),
            Err(error) => self.library.store_mut().fail_refresh(id, &error),
        }
    }
}
