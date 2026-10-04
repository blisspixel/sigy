//! Bounded immutable registrations and recording observations for one directory identity.

use serde::{Deserialize, Serialize};

use super::{Station, ordered::DirectoryCatalog};
use crate::control::SourceView;

pub const MAX_LINKED_SOURCES: usize = 4;
pub const MAX_LINKED_RECORDINGS: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedStationContext {
    pub provider: String,
    pub station_id: String,
    pub catalog: DirectoryCatalog,
    pub cached: bool,
    pub sources: Vec<LinkedSource>,
    pub more_sources: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedSource {
    pub source: SourceView,
    pub registered_station: Station,
    pub recordings: Vec<LinkedRecording>,
    pub more_recordings: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedRecording {
    pub id: String,
    pub source_revision: String,
    pub state: String,
    pub storage_state: String,
    pub retention: String,
    pub media_bytes: Option<u64>,
    pub decoded_microseconds: Option<u64>,
    pub retained_segments: bool,
    pub released_segments: bool,
    pub has_gaps: bool,
}
