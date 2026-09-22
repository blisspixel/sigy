//! Versioned export snapshots. The catalog remains authoritative for changing retention.

use crate::{
    Error, Result,
    sources::{HttpHop, NetworkScope, RedirectPolicy},
    storage::{
        Store,
        dvr::{Recording, Retention},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingEnvelope {
    pub schema: String,
    pub schema_version: u32,
    pub recording_id: String,
    pub source: SourceMetadata,
    pub capture: CaptureMetadata,
    pub payload: PayloadMetadata,
    pub storage: StorageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMetadata {
    pub adapter: String,
    pub revision_id: String,
    pub name: String,
    pub origin: String,
    pub network: NetworkScope,
    pub redirects: RedirectPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureMetadata {
    pub state: String,
    pub revision: i64,
    pub generation: i64,
    pub planned_start_unix_ms: i64,
    pub planned_end_unix_ms: i64,
    pub maximum_bytes: u64,
    pub end_reason: Option<String>,
    pub failure_detail: Option<String>,
    pub http_route: Option<Vec<HttpHop>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icy_observations: Vec<IcyObservationMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IcyObservationMetadata {
    pub audio_offset: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PayloadMetadata {
    EncodedAudio {
        format: Option<String>,
        decoded_microseconds: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageMetadata {
    pub state: String,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
    pub retention_at_export: Retention,
    pub processing_receipt: Option<String>,
}

/// # Errors
/// Returns missing source/capture records or catalog integrity failures.
pub fn export(store: &Store, recording: &Recording) -> Result<RecordingEnvelope> {
    let source = store
        .source(&recording.source_revision)?
        .ok_or(Error::SourceIntegrity)?;
    let job = store
        .capture(&recording.id)?
        .ok_or(Error::CaptureIntegrity)?;
    let icy_observations = store
        .recording_observations(&recording.id)?
        .into_iter()
        .map(|(audio_offset, text)| IcyObservationMetadata { audio_offset, text })
        .collect::<Vec<_>>();
    Ok(RecordingEnvelope {
        schema: "sigy.recording".into(),
        schema_version: if icy_observations.is_empty() { 2 } else { 3 },
        recording_id: recording.id.clone(),
        source: SourceMetadata {
            adapter: "http_audio".into(),
            revision_id: source.id,
            name: source.source.name().into(),
            origin: source.source.origin(),
            network: source.source.network(),
            redirects: source.source.redirects(),
        },
        capture: CaptureMetadata {
            state: recording.state.clone(),
            revision: job.version.revision(),
            generation: job.version.generation(),
            planned_start_unix_ms: job.plan.starts_ms(),
            planned_end_unix_ms: job.plan.ends_ms(),
            maximum_bytes: recording.maximum_bytes,
            end_reason: recording.end_reason.clone(),
            failure_detail: recording.failure_detail.clone(),
            http_route: store.recording_route(&recording.id)?,
            icy_observations,
        },
        payload: PayloadMetadata::EncodedAudio {
            format: recording.format.clone(),
            decoded_microseconds: recording.decoded_microseconds,
        },
        storage: StorageMetadata {
            state: recording.storage_state.clone(),
            bytes: recording.media_bytes,
            sha256: recording.sha256.clone(),
            retention_at_export: recording.retention,
            processing_receipt: recording.processing_receipt.clone(),
        },
    })
}
