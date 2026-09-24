use super::{Operation, Snapshot, SourcePage};
use crate::{Error, Result, storage::Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaylistOperation {
    Resolve {
        id: String,
        revision_id: String,
    },
    Status {
        id: String,
    },
    Accept {
        id: String,
        index: u32,
        revision_id: String,
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistEntryView {
    pub index: u32,
    pub origin: String,
    pub kind: crate::sources::CandidateKind,
    /// Declared HLS variant bandwidth in bits per second. Publisher text.
    pub bandwidth: Option<u64>,
    /// Declared HLS codecs. Publisher text, not a decoder result.
    pub codecs: Option<String>,
    /// Whether the declared codecs are all audio. None when undeclared.
    pub audio_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistAcceptanceView {
    pub index: u32,
    pub child_revision: String,
    pub parent_revision: String,
    pub document_sha256: String,
}

/// Ordinary playlist view. Entry paths and queries stay in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistView {
    pub id: String,
    pub parent_revision: String,
    pub state: String,
    pub started_ms: i64,
    pub completed_ms: Option<i64>,
    pub document_sha256: Option<String>,
    pub final_origin: Option<String>,
    pub failure: Option<String>,
    pub entries: Vec<PlaylistEntryView>,
    pub acceptances: Vec<PlaylistAcceptanceView>,
}

pub(super) fn apply(store: &mut Store, command: PlaylistOperation) -> Result<Snapshot> {
    let mut view = super::snapshot(store)?;
    match command {
        PlaylistOperation::Resolve { .. } => return Err(Error::ServiceRequired),
        PlaylistOperation::Status { id } => {
            view.playlist = Some(load(store, &id)?);
        }
        PlaylistOperation::Accept {
            id,
            index,
            revision_id,
            name,
        } => {
            let admission = store.accept_playlist_entry(&id, index, &revision_id, &name)?;
            view.playlist = Some(load(store, &id)?);
            view.source_page = Some(SourcePage {
                entries: vec![admission.revision.into()],
                next_after: None,
                newly_created: Some(admission.newly_created),
            });
        }
    }
    Ok(view)
}

pub(super) fn load(store: &Store, id: &str) -> Result<PlaylistView> {
    let record = store.playlist(id)?;
    Ok(PlaylistView {
        id: record.id,
        parent_revision: record.parent_revision,
        state: record.state,
        started_ms: record.started_ms,
        completed_ms: record.completed_ms,
        document_sha256: record.document_sha256,
        final_origin: record.final_origin,
        failure: record.failure,
        entries: record
            .entries
            .into_iter()
            .map(|entry| PlaylistEntryView {
                index: entry.index,
                origin: entry.origin,
                kind: entry.kind,
                bandwidth: entry.bandwidth,
                codecs: entry.codecs,
                audio_only: entry.audio_only,
            })
            .collect(),
        acceptances: record
            .acceptances
            .into_iter()
            .map(|acceptance| PlaylistAcceptanceView {
                index: acceptance.index,
                child_revision: acceptance.child_revision,
                parent_revision: acceptance.parent_revision,
                document_sha256: acceptance.document_sha256,
            })
            .collect(),
    })
}

impl From<PlaylistOperation> for Operation {
    fn from(command: PlaylistOperation) -> Self {
        Self::Playlist { command }
    }
}
