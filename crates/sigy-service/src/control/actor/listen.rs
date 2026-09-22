use super::{Actor, Message};
use crate::{Error, Result};

impl Actor {
    pub(super) fn start_listen(&mut self, id: &str, revision: &str) -> Result<bool> {
        if !self.library.store_mut().begin_listen(id, revision)? {
            return Ok(false);
        }
        let Some(loaded) = self.library.store().source(revision)? else {
            self.library.store_mut().fail_listen(id, &Error::NotFound)?;
            return Err(Error::NotFound);
        };
        let source = loaded.source;
        let directory = self.library.directory().to_owned();
        let acquirer = self.acquirer.clone();
        let sender = self.sender.upgrade().ok_or(Error::ServiceStopped)?;
        let ready_id = id.to_owned();
        let worker_id = id.to_owned();
        let worker = match self.spawn_worker(
            move |mut signal| async move {
                crate::recordings::stream_revision(
                    directory,
                    source,
                    acquirer,
                    &mut signal,
                    |nonce, format| {
                        let sender = sender.clone();
                        let id = ready_id.clone();
                        async move {
                            sender
                                .send(Message::ListenReady { id, nonce, format })
                                .await
                                .map_err(|_| Error::ServiceStopped)?;
                            Ok(())
                        }
                    },
                )
                .await
            },
            move |result| Message::ListenFinished {
                id: worker_id,
                result,
            },
        ) {
            Ok(worker) => worker,
            Err(error) => {
                self.library.store_mut().fail_listen(id, &error)?;
                return Err(error);
            }
        };
        self.listen_workers.insert(
            id.to_owned(),
            super::LiveListen {
                worker,
                pipe_nonce: None,
                format: None,
            },
        );
        Ok(true)
    }

    pub(super) fn ready_listen(&mut self, id: &str, nonce: String, format: String) {
        let valid = crate::recordings::pipe_nonce_valid(&nonce) && is_audio_format(&format);
        let Some(session) = self.listen_workers.get_mut(id) else {
            return;
        };
        if !valid {
            session.worker.stop.send_replace(true);
            return;
        }
        session.pipe_nonce = Some(nonce);
        session.format = Some(format);
    }

    pub(super) fn stop_listen(&mut self, id: &str) -> Result<()> {
        let _ = self.library.store().listen(id)?;
        if let Some(session) = self.listen_workers.get(id) {
            session.worker.stop.send_replace(true);
            return Ok(());
        }
        if self.library.store().listen(id)?.state == "running" {
            self.library
                .store_mut()
                .interrupt_listen(id, "listen stopped")?;
        }
        Ok(())
    }

    pub(super) fn finish_listen(&mut self, id: &str, result: Result<String>) -> Result<()> {
        self.listen_workers.remove(id);
        match result {
            Ok(format) => self.library.store_mut().finish_listen(id, &format),
            Err(error) if stopped(&error) => self
                .library
                .store_mut()
                .interrupt_listen(id, "listen stopped"),
            Err(error) => self.library.store_mut().fail_listen(id, &error),
        }
    }

    pub(super) fn attach_listen(&self, listen: &mut super::super::ListenView) {
        let Some(session) = self.listen_workers.get(&listen.id) else {
            return;
        };
        listen.pipe_nonce.clone_from(&session.pipe_nonce);
        if listen.format.is_none() {
            listen.format.clone_from(&session.format);
        }
    }
}

fn stopped(error: &Error) -> bool {
    matches!(
        error,
        Error::Acquisition("listen stopped" | "stopped before receiving audio")
    )
}

fn is_audio_format(format: &str) -> bool {
    matches!(format, "mp3" | "aac" | "flac" | "ogg" | "wav")
}
