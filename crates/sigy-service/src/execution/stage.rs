//! Where a local executor finds the content a spec names by hash.

use std::path::{Path, PathBuf};

use super::spec::{AssetRef, AssetRole, BlobRef, TaskKind, TaskSpec};

pub(crate) const SCRATCH: &str = "analysis-scratch";

/// Maps content hashes to files that already exist on this host: retained library media
/// and pinned profile files. The service builds it from the catalog; the spec never
/// carries these locations. A hash the stage does not map is unavailable.
#[derive(Debug, Clone)]
pub(crate) struct LocalStage {
    library: PathBuf,
    decoder: Option<PathBuf>,
    blobs: Vec<(String, PathBuf)>,
    assets: Vec<(AssetRole, String, PathBuf)>,
}

impl LocalStage {
    /// A stage whose scratch space lives inside this library directory.
    pub(crate) fn new(library: &Path) -> Self {
        Self {
            library: library.to_path_buf(),
            decoder: None,
            blobs: Vec::new(),
            assets: Vec::new(),
        }
    }

    /// The configured local decoder executable.
    #[must_use]
    pub(crate) fn decoder(mut self, path: impl Into<PathBuf>) -> Self {
        self.decoder = Some(path.into());
        self
    }

    /// One retained file that holds the content with this hash.
    #[must_use]
    pub(crate) fn blob(mut self, sha256: &str, path: PathBuf) -> Self {
        self.blobs.push((sha256.to_owned(), path));
        self
    }

    /// One pinned asset: a runtime directory, or a model file.
    #[must_use]
    pub(crate) fn asset(mut self, role: AssetRole, sha256: &str, path: PathBuf) -> Self {
        self.assets.push((role, sha256.to_owned(), path));
        self
    }

    pub(crate) fn decoder_path(&self) -> Option<&Path> {
        self.decoder.as_deref()
    }

    pub(crate) fn blob_path(&self, blob: &BlobRef) -> Option<&Path> {
        self.blobs
            .iter()
            .find(|(sha256, _)| *sha256 == blob.sha256)
            .map(|(_, path)| path.as_path())
    }

    pub(crate) fn asset_path(&self, asset: &AssetRef) -> Option<&Path> {
        self.assets
            .iter()
            .find(|(role, sha256, _)| *role == asset.role && *sha256 == asset.sha256)
            .map(|(_, _, path)| path.as_path())
    }

    /// The private scratch directory of one task generation inside the library.
    pub(crate) fn scratch(&self, spec: &TaskSpec) -> PathBuf {
        let name = match spec.kind {
            TaskKind::Recognition => format!("{}-g{}", spec.task_id, spec.generation),
            TaskKind::Translation => format!("translate-{}-g{}", spec.task_id, spec.generation),
        };
        self.library.join(SCRATCH).join(name)
    }
}
