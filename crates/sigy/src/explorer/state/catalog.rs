//! Catalog revisions fence stale rows independently from client request generations.

use super::{DirectoryCatalog, DirectoryView, Explorer, PageMove, StationRow};

impl Explorer {
    pub(super) fn catalog_fresh(&self, incoming: &DirectoryCatalog, allow_reset: bool) -> bool {
        if incoming.namespace.len() != 32
            || !incoming
                .namespace
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || incoming.comparison.is_empty()
            || incoming.comparison.len() > 64
        {
            return false;
        }
        self.catalog.as_ref().is_none_or(|known| {
            if known.namespace == incoming.namespace {
                known.comparison == incoming.comparison && incoming.revision >= known.revision
            } else {
                allow_reset && !self.retired_catalogs.contains(&incoming.namespace)
            }
        })
    }

    pub(super) fn install_catalog(&mut self, incoming: &DirectoryCatalog) {
        if let Some(known) = &self.catalog
            && known.namespace != incoming.namespace
        {
            self.retired_catalogs.push(known.namespace.clone());
            if self.retired_catalogs.len() > 8 {
                self.retired_catalogs.remove(0);
            }
        }
        self.catalog = Some(incoming.clone());
        if self
            .page_catalog
            .as_ref()
            .is_some_and(|page| page != incoming)
        {
            self.restart_required = true;
        }
    }

    pub const fn restart_required(&self) -> bool {
        self.restart_required
    }

    pub fn note_cursor_changed(&mut self) {
        self.restart_required = true;
        self.note_message("g restarts cached page 1. Catalog changed; previous rows and filters kept. No network request.");
    }

    pub fn catalog_label(&self) -> String {
        self.page_catalog.as_ref().map_or_else(
            || "catalog unknown".into(),
            |catalog| {
                if self.restart_required {
                    format!("g restart | cache r{}", catalog.revision)
                } else {
                    format!("name order | cache r{}", catalog.revision)
                }
            },
        )
    }

    pub fn apply_ordered_search(
        &mut self,
        generation: u64,
        rows: Vec<StationRow>,
        directory: DirectoryView,
        next: Option<String>,
        catalog: &DirectoryCatalog,
    ) -> bool {
        if self.pending_search != Some(generation) || generation < self.applied_search {
            return false;
        }
        let first = matches!(self.pending_page, Some(PageMove::First) | None);
        if !self.catalog_fresh(catalog, first) {
            let _ = self.fail_search(generation, "Catalog changed while this request was running. Previous rows kept; g restarts cached page 1.");
            self.restart_required = true;
            return false;
        }
        if !self.apply_search(generation, rows, directory, next) {
            return false;
        }
        self.install_catalog(catalog);
        self.page_catalog = Some(catalog.clone());
        self.restart_required = false;
        true
    }

    pub fn apply_ordered_favorite(
        &mut self,
        generation: u64,
        id: &str,
        favorite: bool,
        directory: Option<DirectoryView>,
        catalog: &DirectoryCatalog,
    ) -> bool {
        if self.pending_favorite != Some(generation) {
            return false;
        }
        if !self.catalog_fresh(catalog, false) {
            self.pending_favorite = None;
            self.note_message("An older favorite reply was ignored. g rereads cached page 1.");
            return false;
        }
        self.install_catalog(catalog);
        let accepted = self.apply_favorite(generation, id, favorite, directory);
        if accepted && self.restart_required {
            self.note_message(
                "Favorite saved. Catalog changed; previous page kept. g restarts cached page 1.",
            );
        }
        accepted
    }

    pub fn fail_favorite(&mut self, generation: u64, message: &str) -> bool {
        if self.pending_favorite != Some(generation) {
            return false;
        }
        self.pending_favorite = None;
        self.note_message(message);
        true
    }
}
