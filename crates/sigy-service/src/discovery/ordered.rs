//! Name order is pinned Unicode key order, not locale or popularity order.

use super::Station;
use serde::{Deserialize, Serialize};

pub const COMPARISON: &str = "unicode-17-nfc-full-casefold-utf8-uuid-v1";
pub const MAX_CURSOR_BYTES: usize = 8192;
pub const MAX_PAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryCatalog {
    pub namespace: String,
    pub revision: u64,
    pub comparison: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedStationPage {
    pub entries: Vec<Station>,
    pub favorite_ids: Vec<String>,
    pub next_after: Option<String>,
    pub catalog: DirectoryCatalog,
}

/// One uniform pick from the complete eligible portion of the partial local cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomStation {
    pub station: Option<Station>,
    pub favorite: bool,
    pub eligible_count: u32,
    pub catalog: DirectoryCatalog,
}
