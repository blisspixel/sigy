use super::{Actor, Message};
use crate::{Error, Result, discovery::ClickRequest};

impl Actor {
    pub(super) fn start_click(&mut self, id: &str, request: ClickRequest) -> Result<()> {
        if !self.library.store_mut().begin_click(id, &request)? {
            return Ok(());
        }
        let id = id.to_owned();
        let acquirer = self.acquirer.clone();
        self.click_worker = Some(self.spawn_worker(
            move |mut stop| async move {
                tokio::select! {
                    biased;
                    _ = stop.changed() => Err(Error::Acquisition("directory click stopped")),
                    result = crate::discovery::radio_browser::click(&acquirer, &request) => result,
                }
            },
            move |result| Message::ClickFinished { id, result },
        )?);
        Ok(())
    }

    pub(super) fn finish_click(&mut self, id: &str, result: Result<String>) -> Result<()> {
        self.click_worker.take();
        match result {
            Ok(origin) => self.library.store_mut().finish_click(id, &origin),
            Err(error) => self.library.store_mut().fail_click(id, &error),
        }
    }
}
