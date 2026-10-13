use super::{DirectoryView, Effect, Explorer, Focus, PageMove, StationRow, Workspace};
use sigy_service::discovery::ordered::RandomStation;

impl Explorer {
    pub const fn random_count(&self) -> Option<u32> {
        self.random_count
    }

    pub fn result_label(&self) -> String {
        self.random_count.map_or_else(
            || format!("page {}", self.page_number()),
            |count| format!("random pick / {count} eligible"),
        )
    }

    pub(super) fn explore_random(&mut self) -> Effect {
        self.search.draft = self.search.applied.clone();
        match self.submit_search(PageMove::First) {
            Effect::Search(query) => {
                self.pending_random = Some(query.generation);
                self.note_message(
                    "Choosing from cached stations with coordinates. No stream contact.",
                );
                Effect::ExploreRandom(query)
            }
            effect => effect,
        }
    }

    pub fn apply_random_station(
        &mut self,
        generation: u64,
        result: &RandomStation,
        rows: Vec<StationRow>,
        directory: DirectoryView,
    ) -> bool {
        if self.pending_random != Some(generation) || self.pending_search != Some(generation) {
            return false;
        }
        let valid = result.eligible_count <= sigy_service::discovery::MAX_CACHED_STATIONS
            && match result.station.as_ref() {
                Some(station) => {
                    result.eligible_count > 0
                        && station.latitude.zip(station.longitude).is_some_and(
                            |(latitude, longitude)| {
                                sigy_service::domain::geo::LonLat::new(longitude, latitude).is_ok()
                            },
                        )
                        && rows.len() == 1
                        && rows[0].id == station.id
                }
                None => result.eligible_count == 0 && rows.is_empty() && !result.favorite,
            };
        if !valid {
            let _ = self.fail_search(
                generation,
                "Invalid random station result; previous selection kept.",
            );
            return false;
        }
        if !self.apply_ordered_search(generation, rows, directory, None, &result.catalog) {
            return false;
        }
        self.pending_random = None;
        self.random_count = Some(result.eligible_count);
        self.selection = 0;
        self.focus = Focus::Results;
        self.workspace = Workspace::Globe;
        if let Some(coordinates) = self.selected().and_then(|row| row.coordinates) {
            self.globe.center = (coordinates.longitude, coordinates.latitude);
            self.note_message("Random cached station. Directory coordinates are approximate. e picks again; g returns to ordered results; P requests play.");
        } else {
            self.note_message("No cached stations with coordinates match these filters. g returns to ordered results; R requests a directory page.");
        }
        true
    }
}
