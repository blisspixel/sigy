//! Focused inspection owns a frozen page observation, never a service effect.

use super::{Effect, Explorer, Key};
use crate::explorer::context::StationContext;

impl Explorer {
    #[cfg(test)]
    pub(crate) fn modes_for_context_fixture(&mut self, linear: bool) {
        self.modes.linear = linear;
    }
    pub(super) fn open_context(&mut self) -> Effect {
        if self.pending_search.is_some() || self.pending_favorite.is_some() {
            self.note_message("The cached request is pending. Inspect after it finishes.");
            return Effect::None;
        }
        let Some(mut row) = self.selected().cloned() else {
            self.note_message(
                "No station on this page. C chooses a country; / searches the cache.",
            );
            return Effect::None;
        };
        if row
            .metadata
            .as_ref()
            .is_some_and(|station| station.id != row.id)
        {
            row.metadata = None;
        }
        let library = format!(
            "Library capture snapshot: {} active, {} scheduled, {} interrupted. These counts are not station-scoped.",
            self.captures_active, self.captures_scheduled, self.captures_interrupted
        );
        self.context = Some(StationContext::new(
            row,
            self.page_catalog.clone(),
            self.focus,
            library,
            self.playback.clone(),
        ));
        Effect::None
    }

    pub(super) fn context_key(&mut self, key: &Key) -> Effect {
        match key {
            Key::Escape | Key::Backspace => {
                if let Some(context) = self.context.take() {
                    self.focus = context.return_focus;
                }
            }
            Key::Char('q') | Key::Quit => return Effect::Detach,
            Key::Up | Key::Down => {
                if let Some(context) = &mut self.context {
                    context.scroll(matches!(key, Key::Down));
                }
            }
            _ => (),
        }
        Effect::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::{
        client::operations_for,
        state::{DirectoryCatalog, DirectoryView, Focus, Link, StationRow, tests::loaded},
    };
    use sigy_service::discovery::Station;

    fn station(id: &str) -> Station {
        Station {
            provider: "radio_browser".into(),
            id: id.into(),
            name: "Cafe\u{301} 東京 العربية".into(),
            country: "CA".into(),
            state: "Québec".into(),
            languages: vec![String::new(), "français".into()],
            language_codes: vec!["fr-CA".into()],
            tags: vec!["musique".into()],
            codec: "MP3".into(),
            bitrate_kbps: 128,
            hls: false,
            last_check_ok: Some(true),
            latitude: Some(45.5),
            longitude: Some(-73.5),
            stream_origin: "https://example.invalid".into(),
            observed_ms: 1000,
            refresh_id: "retained-fixture".into(),
        }
    }

    #[test]
    fn second_page_context_freezes_query_identity_and_all_effects_until_back() {
        let mut model = loaded();
        model.search.applied.filter.country = "CA".into();
        model.search.draft.filter.country = "FR".into();
        model.page_cursor = Some("second-page".into());
        model.page_history = vec![None];
        model.next_after = Some("third-page".into());
        model.rows[0].metadata = Some(station(&model.rows[0].id));
        let before = model.clone();
        assert_eq!(model.handle(Key::Enter), Effect::None);
        let frozen = model.context.as_ref().map(|context| context.row.clone());
        for key in [
            'f', 'v', 'r', 'n', 'p', 'g', 'x', 'F', 'C', '/', '1', '7', 'h', 'j', 'k', 'l', 'm',
            'c',
        ]
        .map(Key::Char)
        .into_iter()
        .chain([
            Key::Enter,
            Key::Tab,
            Key::BackTab,
            Key::Left,
            Key::Right,
            Key::Paste("record click".into()),
            Key::ClearInput,
        ]) {
            let effect = model.handle(key);
            assert_eq!(effect, Effect::None);
            assert!(operations_for(&effect, &model).is_empty());
        }
        assert_eq!(
            model.context.as_ref().map(|context| context.row.clone()),
            frozen
        );
        assert_eq!(model.handle(Key::Escape), Effect::None);
        assert!(model.context.is_none());
        assert_eq!(model.current_search(), before.current_search());
        assert_eq!(model.search, before.search);
        assert_eq!(model.selected(), before.selected());
        assert_eq!(model.page_history, before.page_history);
        assert_eq!(model.next_after, before.next_after);
        assert_eq!(model.quota, before.quota);
        assert_eq!(model.recordings, before.recordings);
        assert_eq!(model.playback, before.playback);
        assert_eq!(model.focus, before.focus);
    }

    #[test]
    fn empty_pending_and_mismatched_identity_contexts_are_explicit_and_read_only() {
        let mut model = loaded();
        model.rows.clear();
        assert_eq!(model.handle(Key::Enter), Effect::None);
        assert!(model.context.is_none());
        assert!(model.status.contains("No station"));
        model = loaded();
        model.next_after = Some("next".into());
        let Effect::Search(query) = model.handle(Key::Char('n')) else {
            panic!("page read");
        };
        pending_checks(model, query.generation);
    }

    fn pending_checks(mut model: Explorer, generation: u64) {
        assert_eq!(model.handle(Key::Enter), Effect::None);
        assert!(model.context.is_none());
        assert!(model.fail_search(generation, "fixture stopped"));
        model.rows[0].metadata = Some(station("00000000-0000-4000-8000-000000000099"));
        assert_eq!(model.handle(Key::Enter), Effect::None);
        let Some(context) = &model.context else {
            panic!("preview");
        };
        assert!(context.row.metadata.is_none());
        assert_eq!(model.handle(Key::Backspace), Effect::None);
        assert_eq!(
            model.handle(Key::Char('f')),
            Effect::SetFavorite {
                generation: 1,
                id: model.rows[0].id.clone(),
                favorite: true
            }
        );
        assert_eq!(model.handle(Key::Enter), Effect::None);
        assert!(model.context.is_none());
    }

    #[test]
    fn disconnect_drift_and_delayed_reply_keep_frozen_context_and_return_position() {
        let mut model = loaded();
        let initial = model.selected().cloned();
        model.focus = Focus::Detail;
        model.page_catalog = Some(DirectoryCatalog {
            namespace: "a".repeat(32),
            comparison: "fixture".into(),
            revision: 1,
        });
        model.handle(Key::Enter);
        model.note_disconnect(20_000, "fixture lost service");
        model.note_cursor_changed();
        assert!(!model.apply_search(
            99,
            Vec::<StationRow>::new(),
            DirectoryView {
                cached_stations: 0,
                maximum_stations: 10_000,
                favorite_stations: 0,
                refresh: None
            },
            None
        ));
        assert_eq!(
            model.context.as_ref().map(|context| context.row.clone()),
            initial
        );
        assert_eq!(model.handle(Key::Quit), Effect::Detach);
        assert_eq!(model.handle(Key::Char('q')), Effect::Detach);
        model.handle(Key::Escape);
        assert_eq!(model.focus, Focus::Detail);
        assert_eq!(model.selected().cloned(), initial);
        assert!(model.restart_required());
        assert_eq!(model.link(), &Link::Disconnected);
        model.handle(Key::Char('7'));
        model.handle(Key::Enter);
        assert!(model.context.is_some());
        model.handle(Key::Backspace);
        assert_eq!(model.workspace(), super::super::Workspace::Globe);
    }
}
