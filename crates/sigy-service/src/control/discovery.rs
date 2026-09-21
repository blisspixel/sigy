use super::{Operation, Snapshot, SourcePage};
use crate::{
    Error, Result,
    discovery::{RefreshRequest, Station, StationFilter},
    storage::Store,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DirectoryOperation {
    Refresh {
        id: String,
        request: RefreshRequest,
    },
    RefreshStatus {
        id: String,
    },
    Status {},
    Search {
        filter: StationFilter,
        after: Option<String>,
        limit: u32,
    },
    Show {
        id: String,
    },
    Add {
        id: String,
        revision_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationPage {
    pub entries: Vec<Station>,
    pub next_after: Option<String>,
}

pub(super) fn apply(store: &mut Store, command: DirectoryOperation) -> Result<Snapshot> {
    let mut view = super::snapshot(store)?;
    view.directory = Some(store.directory_status()?);
    match command {
        DirectoryOperation::Refresh { .. } => return Err(Error::ServiceRequired),
        DirectoryOperation::RefreshStatus { id } => {
            view.directory_refresh = Some(store.directory_refresh(&id)?);
        }
        DirectoryOperation::Status {} => (),
        DirectoryOperation::Search {
            filter,
            after,
            limit,
        } => {
            let entries = store.search_stations(&filter, after.as_deref(), limit)?;
            let next_after = if let Some(last) = entries.last()
                && !store
                    .search_stations(&filter, Some(&last.id), 1)?
                    .is_empty()
            {
                Some(last.id.clone())
            } else {
                None
            };
            view.station_page = Some(StationPage {
                entries,
                next_after,
            });
        }
        DirectoryOperation::Show { id } => {
            view.station_page = Some(StationPage {
                entries: vec![store.station(&id)?],
                next_after: None,
            });
        }
        DirectoryOperation::Add { id, revision_id } => {
            let admission = store.add_station_source(&id, &revision_id)?;
            view.source_page = Some(SourcePage {
                entries: vec![admission.revision.into()],
                next_after: None,
                newly_created: Some(admission.newly_created),
            });
        }
    }
    Ok(view)
}

impl From<DirectoryOperation> for Operation {
    fn from(command: DirectoryOperation) -> Self {
        Self::Radio { command }
    }
}
