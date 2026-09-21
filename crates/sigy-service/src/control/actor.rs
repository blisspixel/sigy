use std::{thread, time::Instant};

use tokio::sync::{mpsc, oneshot, watch};

use super::{Failure, Operation, PROTOCOL_VERSION, Response, ServiceView, apply};
use crate::{Error, Result, library::Library};

pub(super) struct Message {
    pub operation: Operation,
    pub reply: oneshot::Sender<Response>,
}

pub(super) fn spawn(
    mut library: Library,
    stopping: watch::Sender<bool>,
) -> Result<(mpsc::Sender<Message>, thread::JoinHandle<()>)> {
    // A lost reply never reverses an admitted mutation. Reconciliation reads the
    // durable state; future non-idempotent operations require explicit keys.
    let (sender, mut receiver) = mpsc::channel::<Message>(super::MAX_CLIENTS);
    let thread = thread::Builder::new()
        .name("sigy-catalog".into())
        .spawn(move || {
            let started = Instant::now();
            let mut stopped = false;
            while let Some(message) = receiver.blocking_recv() {
                let result = if stopped {
                    Err(Error::ServiceStopped)
                } else {
                    let stop = matches!(message.operation, Operation::Stop {});
                    let result =
                        apply(library.store_mut(), message.operation).map(|mut snapshot| {
                            snapshot.service = Some(ServiceView {
                                process_id: std::process::id(),
                                uptime_seconds: started.elapsed().as_secs(),
                                stopping: stop,
                                maximum_clients: super::MAX_CLIENTS,
                            });
                            snapshot
                        });
                    if stop && result.is_ok() {
                        stopped = true;
                        stopping.send_replace(true);
                    }
                    result
                };
                let result = result.map_err(|error| Failure {
                    code: match error {
                        Error::InvalidInput(_) | Error::Money(_) => "invalid_request",
                        Error::Budget(_) => "budget_rejected",
                        Error::ServiceStopped => "stopping",
                        _ => "catalog_failure",
                    }
                    .into(),
                    message: error.to_string(),
                });
                let _ = message.reply.send(Response {
                    version: PROTOCOL_VERSION,
                    result,
                });
            }
        })?;
    Ok((sender, thread))
}
