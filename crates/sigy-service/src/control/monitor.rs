//! Monitor versions and proposals over local IPC. Nothing here captures, transcribes,
//! contacts a network or spends money.

use serde::{Deserialize, Serialize};

use super::{Snapshot, snapshot};
use crate::{
    Result,
    monitor::{ActionOrigin, MonitorAction, MonitorSpec, MonitorVersion, MonitorView, Proposal},
    storage::{Store, monitors::VersionWrite},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum MonitorOperation {
    /// Create a monitor with its first user version.
    Create {
        id: String,
        spec: Box<MonitorSpec>,
    },
    /// Append a user version after the version the user last saw.
    Revise {
        id: String,
        expected_version: u32,
        spec: Box<MonitorSpec>,
    },
    /// Record a proposal and its policy decision. Only what the current version allows
    /// is applied; everything else is refused and kept.
    Propose {
        id: String,
        action_id: String,
        origin: ActionOrigin,
        proposal: Proposal,
    },
    Show {
        id: String,
        #[serde(default)]
        version: Option<u32>,
    },
    Actions {
        id: String,
        #[serde(default)]
        after: Option<u32>,
    },
    List {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MonitorPage {
    Monitor {
        monitor: MonitorView,
        created: Option<bool>,
    },
    Version {
        version: MonitorVersion,
    },
    Action {
        action: MonitorAction,
    },
    Actions {
        id: String,
        actions: Vec<MonitorAction>,
    },
    List {
        ids: Vec<String>,
    },
}

pub(super) fn apply(store: &mut Store, command: MonitorOperation) -> Result<Snapshot> {
    let now = crate::storage::now_ms()?;
    let page = match command {
        MonitorOperation::Create { id, spec } => {
            let written = store.create_monitor(&id, &spec, now)?;
            MonitorPage::Monitor {
                monitor: store.monitor(&id)?,
                created: Some(written == VersionWrite::Created),
            }
        }
        MonitorOperation::Revise {
            id,
            expected_version,
            spec,
        } => {
            let written = store.revise_monitor(&id, expected_version, &spec, now)?;
            MonitorPage::Monitor {
                monitor: store.monitor(&id)?,
                created: Some(written == VersionWrite::Created),
            }
        }
        MonitorOperation::Propose {
            id,
            action_id,
            origin,
            proposal,
        } => MonitorPage::Action {
            action: store.propose_monitor_action(&id, &action_id, origin, &proposal, now)?,
        },
        MonitorOperation::Show { id, version: None } => MonitorPage::Monitor {
            monitor: store.monitor(&id)?,
            created: None,
        },
        MonitorOperation::Show {
            id,
            version: Some(version),
        } => MonitorPage::Version {
            version: store.monitor_version(&id, version)?,
        },
        MonitorOperation::Actions { id, after } => MonitorPage::Actions {
            actions: store.monitor_actions(&id, after)?,
            id,
        },
        MonitorOperation::List {} => MonitorPage::List {
            ids: store.monitor_ids()?,
        },
    };
    let mut view = snapshot(store)?;
    view.monitor = Some(Box::new(page));
    Ok(view)
}
