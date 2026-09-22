use super::Snapshot;
use crate::{Error, Result, storage::Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenOperation {
    Start { id: String, revision_id: String },
    Stop { id: String },
    Status { id: String },
}

/// One listen receipt. `pipe_nonce` exists only while this process is streaming.
/// It is not a source URL and is not stored in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListenView {
    pub id: String,
    pub source_revision: String,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub format: Option<String>,
    pub failure: Option<String>,
    pub pipe_nonce: Option<String>,
    pub newly_started: Option<bool>,
}

pub(super) fn apply(store: &mut Store, command: ListenOperation) -> Result<Snapshot> {
    let mut view = super::snapshot(store)?;
    match command {
        ListenOperation::Start { .. } | ListenOperation::Stop { .. } => {
            return Err(Error::ServiceRequired);
        }
        ListenOperation::Status { id } => {
            view.listen = Some(load(store, &id)?);
        }
    }
    Ok(view)
}

pub(super) fn load(store: &Store, id: &str) -> Result<ListenView> {
    let record = store.listen(id)?;
    Ok(ListenView {
        id: record.id,
        source_revision: record.source_revision,
        state: record.state,
        started_ms: record.started_ms,
        completed_ms: record.completed_ms,
        format: record.format,
        failure: record.failure,
        pipe_nonce: None,
        newly_started: None,
    })
}
