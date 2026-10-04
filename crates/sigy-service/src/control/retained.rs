//! Read-only receipts and explicit service-owned retained media admission.

use serde::{Deserialize, Serialize};

use super::Snapshot;
use crate::{Error, Result, storage::Store};

pub use crate::storage::retained_readers::{
    RetainedCitation, RetainedExcerpt, RetainedReadSpec, RetainedReadView,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetainedOperation {
    Start {
        id: String,
        recording_id: String,
        seek_us: u64,
    },
    StartRange {
        id: String,
        recording_id: String,
        seek_us: u64,
        end_us: u64,
    },
    StartFinding {
        id: String,
        monitor_id: String,
        finding_id: String,
    },
    Stop {
        id: String,
        generation: u64,
    },
    Show {
        id: String,
    },
    List {},
}

/// A nonce is an ephemeral, peer-checked byte-stream capability, never a file path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedPage {
    pub entries: Vec<RetainedReadView>,
    pub pipe_nonce: Option<String>,
    pub newly_started: Option<bool>,
}

pub(super) fn apply(store: &Store, command: RetainedOperation) -> Result<Snapshot> {
    let entries = match command {
        RetainedOperation::Start { .. }
        | RetainedOperation::StartRange { .. }
        | RetainedOperation::StartFinding { .. }
        | RetainedOperation::Stop { .. } => {
            return Err(Error::ServiceRequired);
        }
        RetainedOperation::Show { id } => vec![store.retained_reader(&id)?],
        RetainedOperation::List {} => store.retained_readers()?,
    };
    let mut view = super::snapshot(store)?;
    view.retained = Some(Box::new(RetainedPage {
        entries,
        pipe_nonce: None,
        newly_started: None,
    }));
    Ok(view)
}
