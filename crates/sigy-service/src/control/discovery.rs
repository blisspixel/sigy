use super::{Operation, Snapshot, SourcePage};
use crate::{
    Error, Result,
    discovery::{RefreshRequest, Station, StationFilter},
    sources::RedirectPolicy,
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
        #[serde(default)]
        favorites_only: bool,
        after: Option<String>,
        limit: u32,
    },
    Show {
        id: String,
    },
    SetFavorite {
        id: String,
        favorite: bool,
    },
    Add {
        id: String,
        revision_id: String,
        #[serde(default)]
        redirects: RedirectPolicy,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationPage {
    pub entries: Vec<Station>,
    pub favorite_ids: Vec<String>,
    pub next_after: Option<String>,
}

impl StationPage {
    fn new(store: &Store, entries: Vec<Station>, next_after: Option<String>) -> Result<Self> {
        let mut favorite_ids = Vec::new();
        for station in &entries {
            if store.is_station_favorite(&station.id)? {
                favorite_ids.push(station.id.clone());
            }
        }
        Ok(Self {
            entries,
            favorite_ids,
            next_after,
        })
    }
}

pub(super) fn apply(store: &mut Store, command: DirectoryOperation) -> Result<Snapshot> {
    let mut view = super::snapshot(store)?;
    match command {
        DirectoryOperation::Refresh { .. } => return Err(Error::ServiceRequired),
        DirectoryOperation::RefreshStatus { id } => {
            view.directory_refresh = Some(store.directory_refresh(&id)?);
        }
        DirectoryOperation::Status {} => (),
        DirectoryOperation::Search {
            filter,
            favorites_only,
            after,
            limit,
        } => {
            let entries =
                store.search_stations(&filter, favorites_only, after.as_deref(), limit)?;
            let next_after = if let Some(last) = entries.last()
                && !store
                    .search_stations(&filter, favorites_only, Some(&last.id), 1)?
                    .is_empty()
            {
                Some(last.id.clone())
            } else {
                None
            };
            view.station_page = Some(StationPage::new(store, entries, next_after)?);
        }
        DirectoryOperation::Show { id } => {
            view.station_page = Some(StationPage::new(store, vec![store.station(&id)?], None)?);
        }
        DirectoryOperation::SetFavorite { id, favorite } => {
            store.set_station_favorite(&id, favorite)?;
            view.station_page = Some(StationPage::new(store, vec![store.station(&id)?], None)?);
        }
        DirectoryOperation::Add {
            id,
            revision_id,
            redirects,
        } => {
            let admission = store.add_station_source(&id, &revision_id, redirects)?;
            view.source_page = Some(SourcePage {
                entries: vec![admission.revision.into()],
                next_after: None,
                newly_created: Some(admission.newly_created),
            });
        }
    }
    view.directory = Some(store.directory_status()?);
    Ok(view)
}

impl From<DirectoryOperation> for Operation {
    fn from(command: DirectoryOperation) -> Self {
        Self::Radio { command }
    }
}
