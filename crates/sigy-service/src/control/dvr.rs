use super::{Operation, Snapshot};
use crate::{
    Error, Result,
    library::Library,
    storage::{
        Store,
        dvr::{Recording, Retention},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DvrOperation {
    Status {},
    Configure {
        quota_bytes: u64,
        minimum_free_bytes: u64,
        retention_days: u32,
        decoder: String,
    },
    Prune {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordingOperation {
    Start {
        id: String,
        source_revision: String,
        seconds: u64,
        maximum_bytes: u64,
        retention: Retention,
        icy: bool,
    },
    /// Record one finite HLS media playlist. A master playlist is rejected.
    Hls {
        id: String,
        source_revision: String,
        seconds: u64,
        maximum_bytes: u64,
        retention: Retention,
    },
    Stop {
        id: String,
    },
    /// Stop receiving and record the uncovered plan as a pause gap. No silence file is written.
    Pause {
        id: String,
    },
    List {
        after: Option<String>,
        limit: u32,
    },
    Show {
        id: String,
    },
    Metadata {
        id: String,
    },
    Retain {
        id: String,
        retention: Retention,
    },
    Processed {
        id: String,
        receipt: String,
    },
    Delete {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingPage {
    pub entries: Vec<Recording>,
    pub next_after: Option<String>,
}

pub(super) fn apply(store: &mut Store, operation: RecordingOperation) -> Result<RecordingPage> {
    let id = match operation {
        RecordingOperation::Start { .. }
        | RecordingOperation::Hls { .. }
        | RecordingOperation::Stop { .. }
        | RecordingOperation::Pause { .. } => {
            return Err(Error::ServiceRequired);
        }
        RecordingOperation::Delete { .. } => {
            return Err(Error::InvalidInput("deletion requires library ownership"));
        }
        RecordingOperation::Show { id } | RecordingOperation::Metadata { id } => id,
        RecordingOperation::Retain { id, retention } => {
            store.retain_recording(&id, retention)?;
            id
        }
        RecordingOperation::Processed { id, receipt } => {
            store.acknowledge_processing(&id, &receipt)?;
            id
        }
        RecordingOperation::List { after, limit } => {
            let entries = store.recordings(after.as_deref(), limit)?;
            let next_after = if let Some(last) = entries.last()
                && !store.recordings(Some(&last.id), 1)?.is_empty()
            {
                Some(last.id.clone())
            } else {
                None
            };
            return Ok(RecordingPage {
                entries,
                next_after,
            });
        }
    };
    Ok(RecordingPage {
        entries: vec![store.recording(&id)?],
        next_after: None,
    })
}

/// The library owner handles both filesystem and catalog effects through this seam.
/// # Errors
/// Returns validation, storage, or accounting errors.
pub fn apply_library(library: &mut Library, operation: Operation) -> Result<Snapshot> {
    let operation = match operation {
        Operation::Dvr {
            command: DvrOperation::Prune {},
        } => {
            crate::recordings::prune(library)?;
            Operation::Dvr {
                command: DvrOperation::Status {},
            }
        }
        Operation::Record {
            command: RecordingOperation::Delete { id },
        } => {
            crate::recordings::delete(library, &id, false)?;
            Operation::Record {
                command: RecordingOperation::Show { id },
            }
        }
        other => other,
    };
    super::apply(library.store_mut(), operation)
}
