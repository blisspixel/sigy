//! Versioned, bounded local control. The catalog actor owns all durable mutations.

mod actor;
mod discovery;
mod dvr;
mod endpoint;
mod frame;
mod listen;
mod playlist;
mod podcast;
mod server;

use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    domain::money::Usd,
    sources::{HttpSource, NetworkScope, RedirectPolicy},
    storage::{Store, sources::SourceRevision},
};

pub use discovery::{DirectoryOperation, StationPage};
pub use dvr::{DvrOperation, RecordingOperation, RecordingPage, apply_library};
pub use listen::{ListenOperation, ListenView};
pub use playlist::{PlaylistOperation, PlaylistView};
pub use podcast::{PodcastOperation, PodcastPage, PodcastView};
pub use server::{request, run};

pub const PROTOCOL_VERSION: u32 = 13;
pub const MAX_CLIENTS: usize = 32;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub operation: Operation,
}

impl Request {
    #[must_use]
    pub const fn new(operation: Operation) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            operation,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Radio {
        command: DirectoryOperation,
    },
    Dvr {
        command: DvrOperation,
    },
    Record {
        command: RecordingOperation,
    },
    Status {},
    SetBudget {
        scope: String,
        limit_usd: String,
    },
    Stop {},
    RegisterSource {
        revision_id: String,
        name: String,
        url: String,
        network: NetworkScope,
        #[serde(default)]
        redirects: RedirectPolicy,
    },
    ListSources {
        after: Option<String>,
        limit: u32,
    },
    ShowSource {
        revision_id: String,
    },
    Playlist {
        command: PlaylistOperation,
    },
    Listen {
        command: ListenOperation,
    },
    Podcast {
        command: PodcastOperation,
    },
}

impl std::fmt::Debug for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Radio { .. } => "radio",
            Self::Dvr { .. } => "dvr",
            Self::Record { .. } => "record",
            Self::Status {} => "status",
            Self::SetBudget { .. } => "set_budget",
            Self::Stop {} => "stop",
            Self::RegisterSource { .. } => "register_source",
            Self::ListSources { .. } => "list_sources",
            Self::ShowSource { .. } => "show_source",
            Self::Playlist { .. } => "playlist",
            Self::Listen { .. } => "listen",
            Self::Podcast { .. } => "podcast",
        };
        f.debug_struct("Operation")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub result: std::result::Result<Snapshot, Failure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetView {
    pub scope: String,
    pub limit_usd: String,
    pub settled_usd: String,
    pub reserved_usd: String,
    pub available_usd: String,
    pub frozen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceView {
    pub process_id: u32,
    pub uptime_seconds: u64,
    pub stopping: bool,
    pub maximum_clients: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub directory: Option<crate::storage::discovery::DirectoryStatus>,
    pub directory_refresh: Option<crate::storage::discovery::RefreshStatus>,
    pub station_page: Option<StationPage>,
    pub schema_version: u32,
    pub sqlite_version: String,
    pub provider_dispatch_available: bool,
    pub budgets: Vec<BudgetView>,
    pub service: Option<ServiceView>,
    pub captures: CaptureStatus,
    pub source_page: Option<SourcePage>,
    pub recording_page: Option<RecordingPage>,
    pub dvr: Option<crate::storage::dvr::DvrStatus>,
    pub recording_metadata: Option<crate::recordings::metadata::RecordingEnvelope>,
    pub playlist: Option<PlaylistView>,
    pub directory_click: Option<crate::storage::ClickStatus>,
    pub listen: Option<ListenView>,
    pub podcast_page: Option<PodcastPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceView {
    pub revision_id: String,
    pub kind: String,
    pub name: String,
    pub origin: String,
    pub network: NetworkScope,
    pub redirects: RedirectPolicy,
    pub created_ms: i64,
}

impl From<SourceRevision> for SourceView {
    fn from(revision: SourceRevision) -> Self {
        Self {
            revision_id: revision.id,
            kind: "http_audio".into(),
            name: revision.source.name().into(),
            origin: revision.source.origin(),
            network: revision.source.network(),
            redirects: revision.source.redirects(),
            created_ms: revision.created_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePage {
    pub entries: Vec<SourceView>,
    pub next_after: Option<String>,
    pub newly_created: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureStatus {
    pub dispatch_available: bool,
    pub scheduled: u64,
    pub active: u64,
    pub interrupted: u64,
    pub terminal: u64,
}

/// The same application operation is used by maintenance and the live controller.
/// # Errors
/// Returns validation, accounting, or catalog errors. No provider work is dispatched.
pub fn apply(store: &mut Store, operation: Operation) -> Result<Snapshot> {
    let mut recording_page = None;
    let mut dvr_status = None;
    let mut recording_metadata = None;
    let source_page = match operation {
        Operation::Radio { command } => return discovery::apply(store, command),
        Operation::Playlist { command } => return playlist::apply(store, command),
        Operation::Listen { command } => return listen::apply(store, command),
        Operation::Podcast { command } => return podcast::apply(store, command),
        Operation::Record { command } => {
            if let RecordingOperation::Metadata { id } = &command {
                recording_metadata = Some(crate::recordings::metadata::export(
                    store,
                    &store.recording(id)?,
                )?);
            }
            recording_page = Some(dvr::apply(store, command)?);
            None
        }
        Operation::Dvr { command } => {
            match command {
                DvrOperation::Status {} => (),
                DvrOperation::Configure {
                    quota_bytes,
                    minimum_free_bytes,
                    retention_days,
                    decoder,
                } => store.configure_dvr(
                    quota_bytes,
                    minimum_free_bytes,
                    retention_days,
                    &decoder,
                )?,
                DvrOperation::Prune {} => {
                    return Err(Error::InvalidInput("pruning requires library ownership"));
                }
            }
            dvr_status = Some(store.dvr_status()?);
            None
        }
        Operation::SetBudget { scope, limit_usd } => {
            let amount: Usd = limit_usd.parse()?;
            store.set_budget_limit(&scope, amount)?;
            None
        }
        Operation::RegisterSource {
            revision_id,
            name,
            url,
            network,
            redirects,
        } => {
            let source = HttpSource::new(&name, &url, network)?.with_redirects(redirects)?;
            let admission = store.register_source(&revision_id, &source)?;
            Some(SourcePage {
                entries: vec![admission.revision.into()],
                next_after: None,
                newly_created: Some(admission.newly_created),
            })
        }
        Operation::ShowSource { revision_id } => {
            let revision = store.source(&revision_id)?.ok_or(Error::NotFound)?;
            Some(SourcePage {
                entries: vec![revision.into()],
                next_after: None,
                newly_created: None,
            })
        }
        Operation::ListSources { after, limit } => {
            let entries = store.sources(after.as_deref(), limit)?;
            let next_after = if let Some(last) = entries.last()
                && !store.sources(Some(&last.id), 1)?.is_empty()
            {
                Some(last.id.clone())
            } else {
                None
            };
            Some(SourcePage {
                entries: entries.into_iter().map(Into::into).collect(),
                next_after,
                newly_created: None,
            })
        }
        Operation::Status {} | Operation::Stop {} => None,
    };
    let mut view = snapshot(store)?;
    view.source_page = source_page;
    view.recording_page = recording_page;
    view.dvr = dvr_status;
    view.recording_metadata = recording_metadata;
    Ok(view)
}

fn snapshot(store: &Store) -> Result<Snapshot> {
    store.audit_ledger()?;
    let budgets = store
        .budgets()?
        .into_iter()
        .map(|(scope, balance)| BudgetView {
            scope,
            limit_usd: balance.limit().to_string(),
            settled_usd: balance.settled().to_string(),
            reserved_usd: balance.reserved().to_string(),
            available_usd: balance.available().to_string(),
            frozen: balance.frozen(),
        })
        .collect();
    let captures = store.capture_counts()?;
    Ok(Snapshot {
        directory: None,
        directory_refresh: None,
        station_page: None,
        schema_version: crate::storage::SCHEMA_VERSION,
        sqlite_version: store.sqlite_version()?,
        provider_dispatch_available: false,
        budgets,
        service: None,
        source_page: None,
        recording_page: None,
        dvr: None,
        recording_metadata: None,
        playlist: None,
        directory_click: None,
        listen: None,
        podcast_page: None,
        captures: CaptureStatus {
            dispatch_available: false,
            scheduled: captures.scheduled,
            active: captures.active,
            interrupted: captures.interrupted,
            terminal: captures.terminal,
        },
    })
}
