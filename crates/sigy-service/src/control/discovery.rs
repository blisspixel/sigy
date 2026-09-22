use super::{Operation, Snapshot, SourcePage};
use crate::{
    Error, Result,
    discovery::{RefreshRequest, Station, StationFilter},
    sources::RedirectPolicy,
    storage::{
        Store,
        directory_policy::{DirectoryPolicyRecord, PolicyDraft, SaveResult},
        discovery::RefreshStatus,
    },
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
    Click {
        id: String,
        request: crate::discovery::ClickRequest,
    },
    ClickStatus {
        id: String,
    },
    /// Store one bounded page. Saving it does not fetch.
    SetPolicy {
        id: String,
        request: RefreshRequest,
        interval_ms: i64,
    },
    ShowPolicy {
        id: Option<String>,
    },
    /// Remove the saved page. History and favorites stay.
    ClearPolicy {
        id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDisposition {
    Created,
    Unchanged,
    Revised,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryPolicyView {
    pub id: String,
    pub interval_ms: i64,
    pub revision: i64,
    pub updated_ms: i64,
    pub next_due_ms: i64,
    pub request: RefreshRequest,
    pub last_refresh: Option<RefreshStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryPolicyPage {
    pub policies: Vec<DirectoryPolicyView>,
    pub disposition: Option<PolicyDisposition>,
    /// True for status and policy commands. Search does not repeat this block.
    #[serde(default)]
    pub listed: bool,
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
    let listed = matches!(
        &command,
        DirectoryOperation::Status {}
            | DirectoryOperation::ShowPolicy { .. }
            | DirectoryOperation::SetPolicy { .. }
            | DirectoryOperation::ClearPolicy { .. }
    );
    let (disposition, selected) = match command {
        DirectoryOperation::Refresh { .. } | DirectoryOperation::Click { .. } => {
            return Err(Error::ServiceRequired);
        }
        DirectoryOperation::RefreshStatus { id } => {
            view.directory_refresh = Some(store.directory_refresh(&id)?);
            (None, None)
        }
        DirectoryOperation::ClickStatus { id } => {
            view.directory_click = Some(store.directory_click(&id)?);
            (None, None)
        }
        DirectoryOperation::Status {} => (None, None),
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
            (None, None)
        }
        DirectoryOperation::Show { id } => {
            view.station_page = Some(StationPage::new(store, vec![store.station(&id)?], None)?);
            (None, None)
        }
        DirectoryOperation::SetFavorite { id, favorite } => {
            store.set_station_favorite(&id, favorite)?;
            view.station_page = Some(StationPage::new(store, vec![store.station(&id)?], None)?);
            (None, None)
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
            (None, None)
        }
        DirectoryOperation::SetPolicy {
            id,
            request,
            interval_ms,
        } => {
            let draft = PolicyDraft {
                id,
                request,
                interval_ms,
            };
            let saved = store.save_directory_policy_at(&draft, crate::storage::now_ms()?)?;
            (Some(map_save(saved)), None)
        }
        DirectoryOperation::ShowPolicy { id } => (None, id),
        DirectoryOperation::ClearPolicy { id } => {
            store.clear_directory_policy(&id)?;
            (None, None)
        }
    };
    attach_policies(&mut view, store, disposition, selected.as_deref(), listed)?;
    view.directory = Some(store.directory_status()?);
    Ok(view)
}

fn map_save(result: SaveResult) -> PolicyDisposition {
    match result {
        SaveResult::Created => PolicyDisposition::Created,
        SaveResult::Unchanged => PolicyDisposition::Unchanged,
        SaveResult::Revised => PolicyDisposition::Revised,
    }
}

fn attach_policies(
    view: &mut Snapshot,
    store: &Store,
    disposition: Option<PolicyDisposition>,
    selected: Option<&str>,
    listed: bool,
) -> Result<()> {
    let records = store.directory_policies(crate::storage::now_ms()?)?;
    if let Some(id) = selected
        && !records.iter().any(|record| record.id == id)
    {
        return Err(Error::NotFound);
    }
    let policies = records
        .into_iter()
        .filter(|record| selected.is_none_or(|id| record.id == id))
        .map(policy_view)
        .collect();
    view.directory_policy = Some(DirectoryPolicyPage {
        policies,
        disposition,
        listed,
    });
    Ok(())
}

fn policy_view(record: DirectoryPolicyRecord) -> DirectoryPolicyView {
    DirectoryPolicyView {
        id: record.id,
        interval_ms: record.interval_ms,
        revision: record.revision,
        updated_ms: record.updated_ms,
        next_due_ms: record.next_due_ms,
        request: record.request,
        last_refresh: record.last_refresh,
    }
}

impl From<DirectoryOperation> for Operation {
    fn from(command: DirectoryOperation) -> Self {
        Self::Radio { command }
    }
}
