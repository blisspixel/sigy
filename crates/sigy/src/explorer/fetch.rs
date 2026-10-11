//! Explicit directory work stays service-owned; this client observes one finite request.

use std::{path::Path, time::Duration};

use sigy_service::{
    Error,
    control::{self, DirectoryOperation, Snapshot},
    discovery::RefreshRequest,
    sources::NetworkScope,
};
use tokio::sync::mpsc;

use super::{
    client,
    state::{Explorer, SearchQuery},
};

#[derive(Debug)]
enum Update {
    Progress(String),
    Finished(Result<Box<Snapshot>, Error>, String),
}

#[derive(Default)]
pub(super) struct Fetcher {
    pending: Option<(SearchQuery, mpsc::Receiver<Update>)>,
}

impl Fetcher {
    pub(super) fn start(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        directory: &Path,
        model: &mut Explorer,
        query: &SearchQuery,
    ) {
        if self.pending.is_some() {
            let _ = model.fail_search(query.generation, "A directory request is still being observed. Its ID remains in the directory status.");
            return;
        }
        let directory = directory.to_path_buf();
        let query = query.clone();
        let id = match crate::radio::request_id("explorer") {
            Ok(id) => id,
            Err(error) => {
                let _ = model.fail_search(query.generation, &error.to_string());
                return;
            }
        };
        let (send, receive) = mpsc::channel(2);
        self.pending = Some((query.clone(), receive));
        runtime.spawn(async move {
            let result = observe(&directory, &query, &id, &send).await;
            let (result, message) = match result {
                Ok((snapshot, message)) => (Ok(Box::new(snapshot)), message),
                Err(error) => {
                    let message = format!("Refresh {id}: {error}. r rereads its durable state.");
                    (Err(error), message)
                }
            };
            let _ = send.send(Update::Finished(result, message)).await;
        });
    }

    pub(super) fn poll(&mut self, model: &mut Explorer) {
        let Some((query, receive)) = &mut self.pending else {
            return;
        };
        match receive.try_recv() {
            Ok(Update::Progress(message)) => {
                if model.search_is_current(query.generation) {
                    model.note_message(&message);
                }
            }
            Ok(Update::Finished(result, message)) => {
                let failed_current = result.is_err() && model.search_is_current(query.generation);
                let adopted =
                    client::adopt_search(model, query, result.map(|snapshot| (true, *snapshot)));
                if (adopted || failed_current) && !message.is_empty() {
                    model.note_message(&message);
                }
                self.pending = None;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                let _ = model.fail_search(query.generation, "Directory observation ended. r reloads its durable service status; no request is replayed.");
                self.pending = None;
            }
            Err(mpsc::error::TryRecvError::Empty) => (),
        }
    }
}

async fn observe(
    directory: &Path,
    query: &SearchQuery,
    id: &str,
    send: &mpsc::Sender<Update>,
) -> Result<(Snapshot, String), Error> {
    observe_request(
        directory,
        query,
        id,
        send,
        RefreshRequest {
            filter: query.filter.clone(),
            limit: 100,
            offset: 0,
            mirror: None,
            network: NetworkScope::PublicInternet {},
        },
        Duration::from_secs(15),
    )
    .await
}

async fn observe_request(
    directory: &Path,
    query: &SearchQuery,
    id: &str,
    send: &mpsc::Sender<Update>,
    request: RefreshRequest,
    wait: Duration,
) -> Result<(Snapshot, String), Error> {
    let deadline = tokio::time::Instant::now() + wait;
    let mut snapshot = control::request(
        directory,
        DirectoryOperation::Refresh {
            id: id.to_owned(),
            request,
        }
        .into(),
    )
    .await?;
    let message = loop {
        let refresh = snapshot
            .directory_refresh
            .as_ref()
            .ok_or(Error::Protocol("refresh status missing"))?;
        if refresh.id != id {
            return Err(Error::Protocol("refresh identity mismatch"));
        }
        if refresh.state != "running" {
            break format!(
                "Refresh {id}: {}. {}",
                refresh.state,
                refresh
                    .failure
                    .as_deref()
                    .unwrap_or("Applied filters reread from the cache.")
            );
        }
        if tokio::time::Instant::now() >= deadline {
            break format!(
                "Refresh {id} continues in the service. r reads directory status; cached matches are shown."
            );
        }
        let _ = send.try_send(Update::Progress(format!("Refresh {id} is running. Browsing stays available; closing the explorer leaves it running.")));
        tokio::time::sleep_until(
            (tokio::time::Instant::now() + Duration::from_millis(250)).min(deadline),
        )
        .await;
        let next = tokio::time::timeout_at(
            deadline,
            control::request(
                directory,
                DirectoryOperation::RefreshStatus { id: id.to_owned() }.into(),
            ),
        )
        .await;
        match next {
            Ok(result) => snapshot = result?,
            Err(_) => {
                break format!(
                    "Refresh {id} continues in the service. r reads directory status; cached matches are shown."
                );
            }
        }
    };
    let mut matches = control::request(directory, client::search_operation(query)).await?;
    matches.directory_refresh = snapshot.directory_refresh;
    Ok((matches, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::state::{Effect, Key, Link, Modes};
    use sigy_service::{discovery::StationFilter, library::Library};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::oneshot,
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn model() -> Explorer {
        let mut model = Explorer::new(Modes::resolve(true, true, true), 1);
        model.note_link(Link::LocalCatalog);
        model
    }

    fn request(model: &mut Explorer) -> SearchQuery {
        model.handle(Key::Char('R'));
        let Effect::Fetch(query) = model.handle(Key::Char('y')) else {
            panic!("confirmed fetch must produce an effect");
        };
        query
    }

    #[tokio::test]
    async fn failed_and_disconnected_observation_preserves_rows_and_ignores_obsolete_progress()
    -> TestResult {
        let mut model = model();
        let query = request(&mut model);
        let (send, receive) = mpsc::channel(2);
        let mut fetcher = Fetcher {
            pending: Some((query.clone(), receive)),
        };
        send.send(Update::Progress("still observing".into()))
            .await?;
        fetcher.poll(&mut model);
        assert_eq!(model.status(), "still observing");
        send.send(Update::Finished(
            Err(Error::Protocol("fixture refusal")),
            "Refresh known-id failed".into(),
        ))
        .await?;
        fetcher.poll(&mut model);
        assert_eq!(model.status(), "Refresh known-id failed");
        assert!(!model.search_is_current(query.generation));
        assert!(fetcher.pending.is_none());
        model.note_link(Link::LocalCatalog);
        let old = request(&mut model);
        let (send, receive) = mpsc::channel(2);
        fetcher.pending = Some((old.clone(), receive));
        model.handle(Key::Char('x'));
        let status = model.status().to_owned();
        send.send(Update::Progress("obsolete".into())).await?;
        fetcher.poll(&mut model);
        assert_eq!(model.status(), status);
        drop(send);
        fetcher.poll(&mut model);
        assert_eq!(model.status(), status);
        assert!(fetcher.pending.is_none());
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn observer_timeout_keeps_the_exact_admitted_request_running_and_reads_the_cache()
    -> TestResult {
        let temporary = tempfile::tempdir()?;
        let directory = temporary.path();
        let library = Library::open(directory, true)?;
        let (stop, signal) = oneshot::channel();
        let service = tokio::spawn(control::run(library, async {
            let _ = signal.await;
        }));
        ready(directory).await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut header = Vec::new();
            while header.len() < 4096 && !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await?);
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]")
                .await?;
            Ok::<_, std::io::Error>(String::from_utf8_lossy(&header).into_owned())
        });
        let query = SearchQuery {
            generation: 1,
            filter: StationFilter {
                country: "CA".into(),
                tag: "jazz".into(),
                ..StationFilter::default()
            },
            favorites_only: false,
            after: None,
        };
        let request = RefreshRequest {
            filter: query.filter.clone(),
            limit: 100,
            offset: 0,
            mirror: Some(format!("http://fixture.invalid:{port}")),
            network: NetworkScope::PinnedAddress {
                address: "127.0.0.1".parse()?,
            },
        };
        let (send, _receive) = mpsc::channel(2);
        let (snapshot, message) = observe_request(
            directory,
            &query,
            "observer-timeout",
            &send,
            request,
            Duration::ZERO,
        )
        .await?;
        assert!(message.contains("observer-timeout continues"));
        assert_eq!(
            snapshot
                .directory_refresh
                .as_ref()
                .map(|refresh| refresh.state.as_str()),
            Some("running")
        );
        assert!(snapshot.ordered_station_page.is_some());
        let header = server.await??;
        assert!(header.contains("countrycode=CA"), "{header}");
        assert!(header.contains("tag=jazz"), "{header}");
        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = control::request(
                    directory,
                    DirectoryOperation::RefreshStatus {
                        id: "observer-timeout".into(),
                    }
                    .into(),
                )
                .await?;
                if snapshot
                    .directory_refresh
                    .as_ref()
                    .is_some_and(|refresh| refresh.state != "running")
                {
                    break Ok::<_, Error>(snapshot);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;
        assert_eq!(
            completed
                .directory_refresh
                .as_ref()
                .map(|refresh| refresh.state.as_str()),
            Some("completed")
        );
        let _ = stop.send(());
        service.await??;
        Ok(())
    }

    async fn ready(directory: &Path) -> TestResult {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control::request(directory, control::Operation::Status {})
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        Ok(())
    }
}
