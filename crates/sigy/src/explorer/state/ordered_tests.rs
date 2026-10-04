//! Hand-declared received order and hostile catalog/request interleavings.

use super::*;

fn catalog(revision: u64) -> DirectoryCatalog {
    DirectoryCatalog {
        namespace: "a".repeat(32),
        revision,
        comparison: sigy_service::discovery::ordered::COMPARISON.into(),
    }
}

fn directory() -> DirectoryView {
    DirectoryView {
        cached_stations: 37,
        maximum_stations: 10_000,
        favorite_stations: 0,
        refresh: None,
    }
}

fn model() -> Explorer {
    let mut model = Explorer::new(
        Modes {
            reduced_motion: true,
            linear: true,
            monochrome: true,
        },
        1000,
    );
    model.note_link(Link::LocalCatalog);
    model
}

fn query(model: &mut Explorer, key: char) -> SearchQuery {
    let Effect::Search(query) = model.handle(Key::Char(key)) else {
        panic!("explicit cached search");
    };
    query
}

fn rows(names: &[&str], offset: usize) -> Vec<StationRow> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| StationRow {
            id: format!("00000000-0000-4000-8000-{:012}", offset + index),
            name: (*name).into(),
            favorite: false,
            directory_health: Health::Unknown,
            directory_languages: "mixed directory labels".into(),
            observed_ms: 1000,
            hls: false,
            coordinates: None,
            metadata: None,
        })
        .collect()
}

#[test]
fn three_received_name_pages_preserve_native_names_exact_order_and_back_cursors() {
    // This declared order is independent of any application comparator or sort.
    let names = [
        "Alpha",
        "ALPHA",
        "Beta",
        "Cafe\u{301}",
        "Café",
        "Charlie",
        "Delta",
        "Echo",
        "Foxtrot",
        "Golf",
        "Hotel",
        "India",
        "Juliet",
        "Kilo",
        "Lima",
        "Mike",
        "November",
        "Oscar",
        "Papa",
        "Quebec",
        "Romeo",
        "Sierra",
        "Straße",
        "STRASSE",
        "Tango",
        "Uniform",
        "Victor",
        "Whiskey",
        "Xray",
        "Yankee",
        "Zulu",
        "Åland",
        "Москва",
        "العربية",
        "हिन्दी",
        "中文",
        "日本語",
    ];
    let mut model = model();
    let mut received = Vec::new();
    for (index, expected) in names.chunks(16).enumerate() {
        let query = query(&mut model, if index == 0 { 'g' } else { 'n' });
        assert_eq!(
            query.after.as_deref(),
            match index {
                0 => None,
                1 => Some("opaque-one"),
                _ => Some("opaque-two"),
            }
        );
        let next = match index {
            0 => Some("opaque-one".into()),
            1 => Some("opaque-two".into()),
            _ => None,
        };
        assert!(model.apply_ordered_search(
            query.generation,
            rows(expected, index * 16),
            directory(),
            next,
            &catalog(1)
        ));
        assert_eq!(model.rows().len(), expected.len());
        assert_eq!(model.page_number(), index + 1);
        received.extend(model.rows().iter().map(|row| row.name.clone()));
    }
    assert_eq!(received, names);
    assert!(!model.has_next_page());
    let previous = query(&mut model, 'p');
    assert_eq!(previous.after.as_deref(), Some("opaque-one"));
    assert!(model.apply_ordered_search(
        previous.generation,
        rows(&names[16..32], 16),
        directory(),
        Some("opaque-two".into()),
        &catalog(1)
    ));
    assert_eq!(model.page_number(), 2);
    assert_eq!(model.captures_active(), 0);
    assert!(model.playback().is_none());
}

#[test]
fn newer_favorite_revision_fences_a_search_with_the_same_pending_request_generation() {
    let mut model = model();
    let initial = query(&mut model, 'g');
    assert!(model.apply_ordered_search(
        initial.generation,
        rows(&["Original"], 0),
        directory(),
        Some("opaque".into()),
        &catalog(1)
    ));
    let delayed = query(&mut model, 'n');
    let Effect::SetFavorite {
        generation,
        id,
        favorite,
    } = model.handle(Key::Char('f'))
    else {
        panic!("favorite");
    };
    assert!(model.apply_ordered_favorite(generation, &id, favorite, None, &catalog(2)));
    assert!(!model.apply_ordered_search(
        delayed.generation,
        rows(&["Older renamed row"], 1),
        directory(),
        None,
        &catalog(1)
    ));
    assert_eq!(model.rows()[0].name, "Original");
    assert!(model.rows()[0].favorite);
    assert_eq!(model.page_cursor(), None);
    assert!(!model.search.pending());
    assert!(model.restart_required());
    assert_eq!(model.handle(Key::Char('n')), Effect::None);
    let restart = query(&mut model, 'g');
    assert!(restart.after.is_none());
    assert!(model.apply_ordered_search(
        restart.generation,
        rows(&["Current renamed row"], 0),
        directory(),
        None,
        &catalog(2)
    ));
    assert!(!model.restart_required());
}

#[test]
fn namespace_reset_and_delayed_favorite_cannot_revert_new_catalog_rows() {
    let mut model = model();
    let initial = query(&mut model, 'g');
    assert!(model.apply_ordered_search(
        initial.generation,
        rows(&["Before restore"], 0),
        directory(),
        None,
        &catalog(8)
    ));
    let Effect::SetFavorite {
        generation,
        id,
        favorite,
    } = model.handle(Key::Char('f'))
    else {
        panic!("favorite");
    };
    let restart = query(&mut model, 'g');
    let mut restored = catalog(0);
    restored.namespace = "b".repeat(32);
    assert!(model.apply_ordered_search(
        restart.generation,
        rows(&["Restored"], 0),
        directory(),
        None,
        &restored
    ));
    assert!(!model.apply_ordered_favorite(generation, &id, favorite, None, &catalog(9)));
    assert_eq!(model.rows()[0].name, "Restored");
    assert!(!model.rows()[0].favorite);
    let next = query(&mut model, 'g');
    assert!(!model.apply_ordered_search(
        next.generation,
        rows(&["Retired"], 0),
        directory(),
        None,
        &catalog(10)
    ));
    assert_eq!(model.rows()[0].name, "Restored");
}

#[test]
fn explicit_restart_preserves_applied_filters_and_station_identity_after_refusal() {
    let mut model = model();
    model.search.draft.filter.country = "CA".into();
    let Effect::Search(initial) = model.submit_search(PageMove::First) else {
        panic!("scope");
    };
    assert!(model.apply_ordered_search(
        initial.generation,
        rows(&["Selected"], 0),
        directory(),
        Some("opaque".into()),
        &catalog(1)
    ));
    let selected = model.selected().map(|row| row.id.clone());
    let pending = query(&mut model, 'n');
    assert!(model.fail_search(
        pending.generation,
        "ordered station cursor scope changed; restart search"
    ));
    model.note_cursor_changed();
    model.search.draft.filter.country = "FR".into();
    let restart = query(&mut model, 'g');
    assert_eq!(restart.filter.country, "CA");
    assert!(restart.after.is_none());
    assert!(model.apply_ordered_search(
        restart.generation,
        rows(&["Renamed"], 0),
        directory(),
        None,
        &catalog(2)
    ));
    assert_eq!(model.selected().map(|row| row.id.clone()), selected);
    assert_eq!(model.current_search().filter.country, "CA");
}
