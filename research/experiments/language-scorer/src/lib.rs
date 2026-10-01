//! Shared offline validation contracts for bounded research tools.
pub mod judge_records;
pub mod judge_scoring;
pub mod metrics;
pub mod records;
pub mod scoring;
pub mod selection;
pub mod translation_metric;
pub mod translation_records;
pub mod translation_scoring;

impl selection::Selection {
    /// Inspect one frozen asset without permitting selection mutation.
    #[must_use]
    pub fn asset(&self, clip_id: &str) -> Option<&selection::ManifestEntry> {
        self.by_id.get(clip_id)
    }
}

/// Encode a digest using the scorer's canonical lowercase hexadecimal format.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    selection::hex(bytes)
}
