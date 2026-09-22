//! Catalog calls from a running capture. The worker does not open the database.

use crate::storage::captures::CaptureVersion;

pub(crate) enum SealAction {
    Connected,
    Open,
    ReleaseOpen,
    Renew,
    Seal {
        bytes: u64,
        sha256: String,
        format: &'static str,
        decoded_microseconds: u64,
    },
}

pub(crate) enum SealReply {
    Ready(CaptureVersion),
    Opened {
        version: CaptureVersion,
        object_key: String,
        ceiling: u64,
        ordinal: u32,
    },
    BudgetHeld,
}
