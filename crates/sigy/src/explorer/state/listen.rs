//! Listen workspace: one cached favorites page, and stations this client started.
//!
//! Recent stations live on this value only. A new client starts empty. Nothing here
//! is written to the catalog.

use super::{
    DirectoryCatalog, DirectoryView, Effect, Explorer, Key, SearchQuery, StationRow, Workspace,
};
use crate::explorer::text::sanitize;
use sigy_service::discovery::StationFilter;

/// Shown when Listen has no playback and no cached favorites.
pub(crate) const EMPTY_LISTEN: &str =
    "Find stations (1) or surprise me (e). f on any station saves it here.";

const RECENT_LIMIT: usize = 16;

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ListenDesk {
    favorites: Vec<StationRow>,
    recent: Vec<StationRow>,
    selection: usize,
    pending: Option<u64>,
}

impl ListenDesk {
    fn cancel(&mut self) {
        self.pending = None;
    }

    fn remember(&mut self, row: StationRow) {
        self.recent.retain(|item| item.id != row.id);
        self.recent.insert(0, row);
        self.recent.truncate(RECENT_LIMIT);
    }

    fn note_favorite(&mut self, id: &str, favorite: bool) {
        for row in self.favorites.iter_mut().chain(self.recent.iter_mut()) {
            if row.id == id {
                row.favorite = favorite;
            }
        }
    }

    fn entries(&self) -> Vec<&StationRow> {
        let mut entries: Vec<&StationRow> = self.favorites.iter().collect();
        for row in &self.recent {
            if !self.favorites.iter().any(|favorite| favorite.id == row.id) {
                entries.push(row);
            }
        }
        entries
    }

    fn restore(&mut self, selected: Option<String>) {
        let ids: Vec<String> = self
            .entries()
            .into_iter()
            .map(|row| row.id.clone())
            .collect();
        if ids.is_empty() {
            self.selection = 0;
            return;
        }
        if let Some(id) = selected
            && let Some(index) = ids.iter().position(|item| item == &id)
        {
            self.selection = index;
            return;
        }
        self.selection = self.selection.min(ids.len() - 1);
    }
}

impl Explorer {
    pub(super) fn cancel_listen_read(&mut self) {
        self.listen.cancel();
    }

    pub(crate) fn listen_favorites(&self) -> &[StationRow] {
        &self.listen.favorites
    }

    pub(crate) fn listen_recent(&self) -> &[StationRow] {
        &self.listen.recent
    }

    pub(crate) fn listen_selection(&self) -> usize {
        self.listen.selection
    }

    /// The cursor row. Favorites come first. A recent station already saved is not repeated.
    pub(super) fn listen_selected(&self) -> Option<&StationRow> {
        let index = self.listen.selection;
        if index < self.listen.favorites.len() {
            return self.listen.favorites.get(index);
        }
        let mut cursor = self.listen.favorites.len();
        for row in &self.listen.recent {
            if self
                .listen
                .favorites
                .iter()
                .any(|favorite| favorite.id == row.id)
            {
                continue;
            }
            if cursor == index {
                return Some(row);
            }
            cursor = cursor.saturating_add(1);
        }
        None
    }

    pub(super) fn listen_name<'a>(&'a self, id: &'a str) -> Option<&'a str> {
        self.listen_name_row(id).map(|row| row.name.as_str())
    }

    /// One favorites cache read. It does not change the Find stations filter or start playback.
    pub(super) fn request_listen_favorites(&mut self) -> Effect {
        if matches!(self.link, super::Link::Disconnected) {
            self.cancel_listen_read();
            self.note_message(
                "Disconnected; the favorites filter was not submitted. r reconnects.",
            );
            return Effect::None;
        }
        self.pending_search = None;
        self.pending_page = None;
        self.pending_random = None;
        self.search.interrupt();
        self.search_generation = self.search_generation.saturating_add(1);
        let generation = self.search_generation;
        self.listen.pending = Some(generation);
        self.note_message("Reading cached favorites. No directory refresh was requested.");
        Effect::Search(SearchQuery {
            generation,
            filter: StationFilter::default(),
            favorites_only: true,
            after: None,
        })
    }

    pub(super) fn listen_pending(&self, generation: u64) -> bool {
        self.listen.pending == Some(generation) && generation >= self.applied_search
    }

    pub(super) fn finish_listen_search(
        &mut self,
        generation: u64,
        rows: Vec<StationRow>,
        directory: DirectoryView,
        catalog: &DirectoryCatalog,
    ) -> bool {
        if !self.listen_pending(generation) {
            return false;
        }
        if !self.catalog_fresh(catalog, true) {
            let _ = self.fail_search(
                generation,
                "Catalog changed while this request was running. Previous favorites kept.",
            );
            self.restart_required = true;
            return false;
        }
        let selected = self.listen_selected().map(|row| row.id.clone());
        self.listen.pending = None;
        self.listen.favorites = rows;
        self.listen.restore(selected);
        self.directory = Some(directory);
        self.install_catalog(catalog);
        if self
            .page_catalog
            .as_ref()
            .is_none_or(|page| page == catalog)
        {
            self.restart_required = false;
        }
        let count = u64::try_from(self.listen.favorites.len()).unwrap_or(u64::MAX);
        self.note_message(&format!(
            "Cached favorites: {}. No directory refresh was requested.",
            crate::copy::station_count(count)
        ));
        self.draw = super::Draw::Needed;
        true
    }

    pub(super) fn remember_played(&mut self, id: &str) {
        let Some(row) = self
            .listen_name_row(id)
            .or_else(|| self.rows.iter().find(|row| row.id == id))
            .cloned()
        else {
            return;
        };
        self.listen.remember(row);
    }

    fn listen_name_row(&self, id: &str) -> Option<&StationRow> {
        self.listen
            .favorites
            .iter()
            .chain(self.listen.recent.iter())
            .find(|row| row.id == id)
    }

    pub(super) fn note_listen_favorite(&mut self, id: &str, favorite: bool) {
        self.listen.note_favorite(id, favorite);
    }

    pub(super) fn listen_key(&mut self, key: &Key) -> Option<Effect> {
        if self.workspace != Workspace::Live || self.at_home() || self.text_entry() {
            return None;
        }
        Some(match key {
            Key::Up => {
                self.move_listen(false);
                Effect::None
            }
            Key::Down => {
                self.move_listen(true);
                Effect::None
            }
            Key::Enter => self.confirm_selected_listen(),
            Key::Char('v') => self.request_listen_favorites(),
            Key::Char('e') => self.surprise_from_listen(),
            _ => return None,
        })
    }

    fn move_listen(&mut self, forward: bool) {
        let len = self.listen.entries().len();
        if len == 0 {
            self.note_message("No station on Listen yet.");
            return;
        }
        let next = if forward {
            self.listen.selection.saturating_add(1).min(len - 1)
        } else {
            self.listen.selection.saturating_sub(1)
        };
        if next == self.listen.selection {
            self.note_message("That is the end of this Listen list.");
            return;
        }
        self.listen.selection = next;
    }

    fn confirm_selected_listen(&mut self) -> Effect {
        let Some(row) = self.listen_selected() else {
            self.note_message("No selected station to play.");
            return Effect::None;
        };
        let station = row.id.clone();
        let name = sanitize(&row.name, 32);
        self.arm_station_play(station, &name)
    }

    fn surprise_from_listen(&mut self) -> Effect {
        let effect = self.explore_random();
        if matches!(effect, Effect::ExploreRandom(_)) {
            self.workspace = Workspace::Globe;
            self.view = super::View::Workspace;
            self.home_index = self.workspace.menu_index();
            self.focus = super::Focus::Results;
            self.keys = super::KeyList::Closed;
            self.menu = super::Menu::Closed;
        }
        effect
    }

    pub(crate) fn listen_headline(&self) -> Option<String> {
        if let Some((phase, request)) = self.retained_label() {
            let name = sanitize(request, 32);
            let phase = sanitize(phase, 16);
            return Some(format!(
                "{name}  retained  {phase}  {}  S stop  H pause  [ ] seek",
                self.retained_elapsed_label()
            ));
        }
        let session = self.playback.as_ref()?;
        let name = self.listen_subject().unwrap_or_else(|| "Live".into());
        let state = sanitize(&session.state, 16);
        Some(format!(
            "{name}  live  {state}  {}  S stop",
            self.live_elapsed_label()
        ))
    }

    pub(crate) fn listen_detail(&self) -> Option<String> {
        if let Some((_, request)) = self.retained_label() {
            return Some(format!("session {}", sanitize(request, 64)));
        }
        let session = self.playback.as_ref()?;
        Some(format!(
            "session {}  revision {}",
            sanitize(&session.id, 64),
            sanitize(&session.source_revision, 64)
        ))
    }

    /// Now playing, then favorites, then recent stations started in this client.
    pub(crate) fn listen_body(&self, limit: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(headline) = self.listen_headline() {
            lines.push(headline);
            if let Some(detail) = self.listen_detail() {
                lines.push(detail);
            }
        } else if self.listen_favorites().is_empty() {
            lines.push(EMPTY_LISTEN.to_owned());
        } else {
            lines.push("Nothing playing.".to_owned());
        }
        self.append_listen_rows(&mut lines);
        lines.truncate(limit);
        lines
    }

    fn append_listen_rows(&self, lines: &mut Vec<String>) {
        if !self.listen_favorites().is_empty() {
            lines.push("Favorites".to_owned());
            for (index, row) in self.listen_favorites().iter().enumerate() {
                lines.push(marked(index == self.listen_selection(), &row.name));
            }
        }
        let recent: Vec<&StationRow> = self
            .listen_recent()
            .iter()
            .filter(|row| {
                !self
                    .listen_favorites()
                    .iter()
                    .any(|favorite| favorite.id == row.id)
            })
            .collect();
        if recent.is_empty() {
            return;
        }
        lines.push("Recent".to_owned());
        let offset = self.listen_favorites().len();
        for (index, row) in recent.iter().enumerate() {
            lines.push(marked(offset + index == self.listen_selection(), &row.name));
        }
    }

    fn live_elapsed_label(&self) -> String {
        let Some(session) = &self.playback else {
            return "elapsed unknown".to_owned();
        };
        let end = if matches!(session.state.as_str(), "running" | "playing") {
            self.now_ms
        } else {
            session.completed_ms.unwrap_or(self.now_ms)
        };
        elapsed_clock(session.started_ms, end).unwrap_or_else(|| "elapsed unknown".to_owned())
    }

    fn retained_elapsed_label(&self) -> String {
        let paused = self
            .retained
            .as_ref()
            .is_some_and(|view| view.phase == "paused");
        let time = if paused {
            self.paused_at_us().or_else(|| self.decoder_output_us())
        } else {
            self.decoder_output_us()
        };
        time.map_or_else(
            || "elapsed unknown".to_owned(),
            |time| format!("elapsed {time} us"),
        )
    }

    #[cfg(test)]
    pub(super) fn listen_read(&self) -> Option<u64> {
        self.listen.pending
    }

    #[cfg(test)]
    pub(super) fn recent_ids(&self) -> Vec<&str> {
        self.listen
            .recent
            .iter()
            .map(|row| row.id.as_str())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn install_listen_favorites_for_test(&mut self, rows: Vec<StationRow>) {
        self.listen.favorites = rows;
        self.listen.selection = 0;
        self.listen.pending = None;
    }

    #[cfg(test)]
    fn stage_recent_for_test(&mut self, row: StationRow) {
        let id = row.id.clone();
        self.rows.push(row);
        self.remember_played(&id);
    }
}

fn marked(selected: bool, name: &str) -> String {
    let name = sanitize(name, 32);
    if selected {
        format!("> {name}")
    } else {
        format!("  {name}")
    }
}

fn elapsed_clock(started_ms: i64, end_ms: i64) -> Option<String> {
    if started_ms < 0 || end_ms < started_ms {
        return None;
    }
    let span = u64::try_from(end_ms - started_ms).ok()?;
    let seconds = span / 1000;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        Some(format!("elapsed {hours}:{minutes:02}:{secs:02}"))
    } else {
        Some(format!("elapsed {minutes}:{secs:02}"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{loaded, row};
    use super::{Effect, Explorer, Key, RECENT_LIMIT, Workspace};
    use crate::explorer::client::operations_for;
    use sigy_service::control::{DirectoryCatalog, DirectoryOperation, Operation};
    use sigy_service::discovery::StationFilter;

    fn catalog() -> DirectoryCatalog {
        DirectoryCatalog {
            namespace: "a".repeat(32),
            revision: 1,
            comparison: sigy_service::discovery::ordered::COMPARISON.into(),
        }
    }

    fn directory() -> super::DirectoryView {
        super::DirectoryView {
            cached_stations: 2,
            maximum_stations: 10_000,
            favorite_stations: 1,
            refresh: None,
        }
    }

    fn favorite(id: &str, name: &str) -> super::StationRow {
        let mut station = row(id, name);
        station.favorite = true;
        station
    }

    fn open_favorites(model: &mut Explorer) -> u64 {
        let Effect::Search(query) = model.handle(Key::Char('2')) else {
            panic!("opening Listen reads favorites");
        };
        assert!(query.favorites_only);
        assert!(query.after.is_none());
        assert_eq!(query.filter, StationFilter::default());
        query.generation
    }

    #[test]
    fn enter_on_a_favorite_matches_explore_play() {
        let base = loaded();
        let mut explore = base.clone();
        assert_eq!(explore.handle(Key::Char('P')), Effect::None);
        let direct = explore.handle(Key::Char('y'));
        assert_eq!(
            direct,
            Effect::Play {
                station: "station-a".into()
            }
        );

        let mut listen = base;
        let before_rows: Vec<String> = listen.rows().iter().map(|item| item.id.clone()).collect();
        assert!(!listen.favorites_only());
        let generation = open_favorites(&mut listen);
        assert!(!listen.favorites_only());
        assert_eq!(
            listen
                .rows()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            before_rows.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert!(!listen.status().contains("confirms"), "{}", listen.status());
        let operations = operations_for(
            &Effect::Search(super::SearchQuery {
                generation,
                filter: StationFilter::default(),
                favorites_only: true,
                after: None,
            }),
            &listen,
        );
        assert_eq!(operations.len(), 1);
        assert!(matches!(
            &operations[0],
            Operation::Radio {
                command: DirectoryOperation::SearchOrdered {
                    favorites_only: true,
                    after: None,
                    ..
                }
            }
        ));
        assert!(operations.iter().all(|operation| !matches!(
            operation,
            Operation::Listen { .. } | Operation::Playback { .. } | Operation::Retained { .. }
        )));
        assert!(listen.apply_ordered_search(
            generation,
            vec![favorite("station-a", "Alpha")],
            directory(),
            None,
            &catalog(),
        ));
        assert_eq!(
            listen
                .rows()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            before_rows.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert!(!listen.favorites_only());
        assert_eq!(listen.handle(Key::Enter), Effect::None);
        assert_eq!(
            listen.status(),
            "Play Alpha? y confirms; n/Esc cancels. Contacts the station; may register its first revision."
        );
        assert_eq!(listen.handle(Key::Char('y')), direct);

        let mut again = loaded();
        let generation = open_favorites(&mut again);
        assert!(again.apply_ordered_search(
            generation,
            vec![favorite("station-a", "Alpha")],
            directory(),
            None,
            &catalog(),
        ));
        assert_eq!(again.handle(Key::Char('P')), Effect::None);
        assert_eq!(again.handle(Key::Char('y')), direct);
    }

    #[test]
    fn opening_again_does_not_read_twice_and_disconnect_reads_nothing() {
        let mut model = loaded();
        let first = open_favorites(&mut model);
        assert_eq!(model.handle(Key::Char('2')), Effect::None);
        assert!(
            model.status().contains("already open"),
            "{}",
            model.status()
        );
        assert_eq!(model.listen_read(), Some(first));
        assert_eq!(model.handle(Key::Char('1')), Effect::None);
        assert_eq!(model.workspace(), Workspace::Explore);
        assert!(model.listen_read().is_none());
        let second = open_favorites(&mut model);
        assert_ne!(second, first);
        assert!(!model.apply_ordered_search(
            first,
            vec![favorite("station-a", "Alpha")],
            directory(),
            None,
            &catalog(),
        ));
        assert_eq!(model.listen_favorites(), &[] as &[super::StationRow]);

        let mut offline = Explorer::new(
            super::super::Modes {
                reduced_motion: true,
                linear: true,
                monochrome: true,
                emphasis: super::super::Emphasis::Standard,
            },
            10_000,
        );
        assert_eq!(offline.handle(Key::Char('2')), Effect::None);
        assert_eq!(offline.workspace(), Workspace::Live);
        assert!(
            offline.status().contains("not submitted"),
            "{}",
            offline.status()
        );
        assert!(offline.listen_read().is_none());
    }

    #[test]
    fn recent_stations_survive_the_screen_and_not_a_new_client() {
        let mut model = loaded();
        model.handle(Key::Char('P'));
        assert_eq!(
            model.handle(Key::Char('y')),
            Effect::Play {
                station: "station-a".into()
            }
        );
        assert_eq!(model.recent_ids(), vec!["station-a"]);
        assert_eq!(model.handle(Key::Char('3')), Effect::None);
        assert_eq!(model.workspace(), Workspace::Recordings);
        assert_eq!(model.recent_ids(), vec!["station-a"]);
        let _generation = open_favorites(&mut model);
        assert_eq!(model.recent_ids(), vec!["station-a"]);
        assert_eq!(model.handle(Key::Char('n')), Effect::None);
        assert_eq!(model.recent_ids(), vec!["station-a"]);
        let fresh = Explorer::new(
            super::super::Modes {
                reduced_motion: true,
                linear: true,
                monochrome: true,
                emphasis: super::super::Emphasis::Standard,
            },
            10_000,
        );
        assert_eq!(fresh.recent_ids(), Vec::<&str>::new());
        assert_eq!(fresh.listen_favorites(), &[] as &[super::StationRow]);
    }

    #[test]
    fn a_discarded_play_is_not_recent_and_the_list_is_bounded() {
        let mut model = loaded();
        model.handle(Key::Char('P'));
        assert_eq!(model.handle(Key::Char('n')), Effect::None);
        assert_eq!(model.recent_ids(), Vec::<&str>::new());

        for index in 0..20 {
            model.stage_recent_for_test(row(&format!("s{index:02}"), "Name"));
        }
        let ids = model.recent_ids();
        assert_eq!(ids.len(), RECENT_LIMIT);
        assert_eq!(ids[0], "s19");
        assert_eq!(ids[RECENT_LIMIT - 1], "s04");
        assert!(!ids.contains(&"s00"));
        model.remember_played("s10");
        assert_eq!(model.recent_ids()[0], "s10");
        assert_eq!(model.recent_ids().len(), RECENT_LIMIT);
    }

    #[test]
    fn a_stale_favorites_page_does_not_replace_find_stations() {
        let mut model = loaded();
        let explore: Vec<String> = model.rows().iter().map(|item| item.id.clone()).collect();
        let generation = open_favorites(&mut model);
        assert!(!model.apply_ordered_search(
            generation.saturating_add(1),
            vec![favorite("other", "Other")],
            directory(),
            None,
            &catalog(),
        ));
        assert_eq!(model.listen_favorites(), &[] as &[super::StationRow]);
        let mut bad = catalog();
        bad.namespace = "not-a-catalog-namespace".into();
        assert!(!model.apply_ordered_search(
            generation,
            vec![favorite("other", "Other")],
            directory(),
            None,
            &bad,
        ));
        assert_eq!(model.listen_favorites(), &[] as &[super::StationRow]);
        assert!(model.restart_required());
        assert!(model.listen_read().is_none());

        let Effect::Search(query) = model.request_listen_favorites() else {
            panic!("refresh after a rejected catalog");
        };
        let generation = query.generation;
        assert!(model.apply_ordered_search(
            generation,
            vec![favorite("station-a", "Alpha")],
            directory(),
            None,
            &catalog(),
        ));
        assert!(!model.restart_required());
        assert_eq!(model.listen_favorites()[0].id, "station-a");
        assert_eq!(
            model.rows().iter().map(|item| &item.id).collect::<Vec<_>>(),
            explore.iter().collect::<Vec<_>>()
        );
        model.handle(Key::Down);
        assert_eq!(
            model.selection(),
            0,
            "Listen arrows do not move Find stations"
        );
        assert_eq!(model.listen_selection(), 0);
        model.note_message("ready");
        model.handle(Key::Down);
        assert!(
            model.status().contains("end of this Listen list"),
            "{}",
            model.status()
        );
    }

    #[test]
    fn favorite_on_listen_uses_the_listen_row_and_refreshes_without_playback() {
        let mut model = loaded();
        let generation = open_favorites(&mut model);
        assert!(model.apply_ordered_search(
            generation,
            vec![favorite("station-a", "Alpha")],
            directory(),
            None,
            &catalog(),
        ));
        let Effect::SetFavorite {
            generation: favorite_generation,
            id,
            favorite,
        } = model.handle(Key::Char('f'))
        else {
            panic!("favorite");
        };
        assert_eq!(id, "station-a");
        assert!(!favorite);
        assert!(model.apply_favorite(favorite_generation, &id, favorite, None));
        assert_eq!(model.listen_favorites().len(), 1);
        assert!(!model.listen_favorites()[0].favorite);
        assert!(!model.favorites_only());
        let Effect::Search(refresh) = model.handle(Key::Char('v')) else {
            panic!("refresh");
        };
        assert!(refresh.favorites_only);
        assert!(refresh.after.is_none());
        assert_ne!(refresh.generation, generation);
        assert!(!model.favorites_only());
        assert!(model.playback().is_none());
    }

    #[test]
    fn an_empty_listen_list_does_not_play_the_find_stations_row() {
        let mut model = loaded();
        let generation = open_favorites(&mut model);
        assert!(model.apply_ordered_search(generation, Vec::new(), directory(), None, &catalog(),));
        assert_eq!(model.listen_favorites(), &[] as &[super::StationRow]);
        assert_eq!(model.recent_ids(), Vec::<&str>::new());
        assert_eq!(
            model.selected().map(|row| row.id.as_str()),
            Some("station-a")
        );
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert_eq!(model.status(), "No selected station to play.");
        assert_eq!(model.handle(Key::Char('y')), Effect::None);
        assert_eq!(model.handle(Key::Enter), Effect::None);
        assert_eq!(model.status(), "No selected station to play.");
        assert!(model.playback().is_none());
    }

    #[test]
    fn a_favorites_page_keeps_the_recent_cursor() {
        let mut model = loaded();
        model.remember_played("station-b");
        assert_eq!(model.recent_ids(), vec!["station-b"]);
        let generation = open_favorites(&mut model);
        assert!(model.apply_ordered_search(
            generation,
            vec![favorite("station-a", "Alpha")],
            directory(),
            None,
            &catalog(),
        ));
        assert_eq!(model.listen_selection(), 1);
        assert_eq!(
            model.selected().map(|row| row.id.as_str()),
            Some("station-a"),
            "Find stations stays on its own cursor"
        );
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert_eq!(
            model.status(),
            "Play Beta? y confirms; n/Esc cancels. Contacts the station; may register its first revision."
        );
        assert_eq!(
            model.handle(Key::Char('y')),
            Effect::Play {
                station: "station-b".into()
            }
        );
    }
}
