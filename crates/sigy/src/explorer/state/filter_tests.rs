//! Query scope, failed edits and paging through the same service operation.

use super::{
    DirectoryView, Effect, Explorer, Key, Link, SearchQuery,
    tests::{loaded, row},
};
use crate::explorer::client::operations_for;
use sigy_service::{
    control::{DirectoryOperation, Operation},
    discovery::StationFilter,
};

fn apply(model: &mut Explorer, query: &SearchQuery, id: &str, next: Option<&str>) -> bool {
    model.apply_search(
        query.generation,
        vec![row(id, id)],
        DirectoryView {
            cached_stations: 40,
            maximum_stations: 10_000,
            favorite_stations: 0,
            refresh: None,
        },
        next.map(str::to_owned),
    )
}

fn country(model: &mut Explorer, code: &str) -> SearchQuery {
    model.handle(Key::Char('F'));
    model.handle(Key::Tab);
    model.handle(Key::ClearInput);
    model.handle(Key::Paste(code.into()));
    let Effect::Search(query) = model.handle(Key::Enter) else {
        panic!("valid country applies");
    };
    query
}

#[test]
fn combined_filters_change_results_only_after_their_matching_response() {
    let mut model = loaded();
    for key in [
        Key::Char('F'),
        Key::Tab,
        Key::Paste("ca".into()),
        Key::Tab,
        Key::Paste("FRENCH".into()),
        Key::Tab,
        Key::Paste("news".into()),
        Key::Tab,
        Key::Char(' '),
        Key::Tab,
        Key::Char(' '),
    ] {
        assert_eq!(model.handle(key), Effect::None);
    }
    let Effect::Search(query) = model.handle(Key::Enter) else {
        panic!("explicit apply reads cache");
    };
    assert_eq!(
        query.filter,
        StationFilter {
            country: "CA".into(),
            language: "FRENCH".into(),
            tag: "news".into(),
            healthy_only: true,
            ..StationFilter::default()
        }
    );
    assert!(query.favorites_only);
    assert_eq!(model.current_search().filter, StationFilter::default());
    let operations = operations_for(&Effect::Search(query.clone()), &model);
    assert!(
        matches!(operations.as_slice(), [Operation::Radio { command: DirectoryOperation::SearchOrdered { filter, favorites_only: true, after: None, limit: 16 } }] if *filter == query.filter)
    );
    assert!(apply(&mut model, &query, "station-a", Some("station-a")));
    assert_eq!(model.current_search().filter, query.filter);
    assert_eq!(model.captures_active(), 1);
    assert!(model.playback().is_none());
    model.handle(Key::Escape);
    assert_eq!(model.search.draft, model.search.applied);
}

#[test]
fn delayed_responses_cannot_change_the_applied_scope_or_selection() {
    let mut model = loaded();
    let initial = country(&mut model, "CA");
    assert!(apply(&mut model, &initial, "station-a", None));
    model.handle(Key::Escape);
    let older = country(&mut model, "FR");
    model.handle(Key::ClearInput);
    model.handle(Key::Paste("US".into()));
    let Effect::Search(newer) = model.handle(Key::Enter) else {
        panic!("new scope applies");
    };
    assert_eq!(model.current_search().filter.country, "CA");
    assert!(apply(&mut model, &newer, "station-b", Some("station-b")));
    assert!(!apply(&mut model, &older, "station-a", None));
    assert_eq!(model.current_search().filter.country, "US");
    assert_eq!(
        model.selected().map(|row| row.id.as_str()),
        Some("station-b")
    );
}

#[test]
fn reload_and_paging_use_applied_filters_while_drafts_can_be_cancelled() {
    let mut model = loaded();
    let initial = country(&mut model, "CA");
    assert!(apply(&mut model, &initial, "a", Some("b")));
    model.handle(Key::Escape);
    let Effect::Search(next) = model.handle(Key::Char('n')) else {
        panic!("next page reads");
    };
    assert_eq!(next.filter.country, "CA");
    assert!(apply(&mut model, &next, "c", Some("d")));
    model.handle(Key::Char('F'));
    model.handle(Key::Paste("unapplied".into()));
    model.handle(Key::Tab);
    model.handle(Key::ClearInput);
    model.handle(Key::Paste("FR".into()));
    let reload = operations_for(&Effect::Reload, &model);
    assert!(reload.iter().any(|operation| matches!(operation, Operation::Radio { command: DirectoryOperation::SearchOrdered { filter, after: Some(after), .. } } if filter.country == "CA" && filter.name.is_empty() && after == "b")));
    model.handle(Key::Escape);
    assert_eq!(model.query(), "");
    let Effect::Search(reset) = model.handle(Key::Char('x')) else {
        panic!("reset reads first page");
    };
    assert_eq!(reset.filter, StationFilter::default());
    assert_eq!(reset.after, None);
    assert!(apply(&mut model, &reset, "c", None));
    assert_eq!(model.page_number(), 1);
    assert_eq!(model.selected().map(|row| row.id.as_str()), Some("c"));
}

#[test]
fn invalid_or_offline_filters_preserve_the_last_rows_and_scope() {
    let mut model = loaded();
    model.handle(Key::Char('F'));
    model.handle(Key::Tab);
    model.handle(Key::Paste("Congo".into()));
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(model.status().contains("unknown or ambiguous"));
    assert_eq!(model.current_search().filter, StationFilter::default());
    let old = model.rows().to_vec();
    model.note_disconnect(20_000, "fixture disconnect");
    model.handle(Key::ClearInput);
    model.handle(Key::Paste("ca".into()));
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert_eq!(model.rows(), old);
    model.handle(Key::Escape);
    assert_eq!(model.search.draft, model.search.applied);
    model.note_link(Link::LocalCatalog);
    let Effect::Search(query) = model.handle(Key::Char('x')) else {
        panic!("reconnected cache can read");
    };
    assert!(apply(&mut model, &query, "a", None));
}

#[test]
fn completed_reads_clear_pending_status_without_hiding_a_new_invalid_draft() {
    let mut model = loaded();
    let first = country(&mut model, "CA");
    assert!(apply(&mut model, &first, "a", None));
    assert!(model.status().starts_with("Cached page read: 1 stations."));
    assert!(!model.search.pending());
    model.handle(Key::ClearInput);
    model.handle(Key::Paste("FR".into()));
    let Effect::Search(pending) = model.handle(Key::Enter) else {
        panic!("valid draft reads");
    };
    model.handle(Key::ClearInput);
    model.handle(Key::Paste("Congo".into()));
    assert_eq!(model.handle(Key::Enter), Effect::None);
    let invalid = model.status().to_owned();
    assert!(apply(&mut model, &pending, "b", None));
    assert_eq!(model.current_search().filter.country, "FR");
    assert_eq!(model.status(), invalid);
    assert!(model.search.validation.is_some());
    assert!(!model.search.pending());
}

#[test]
fn text_editing_preserves_graphemes_and_obeys_the_service_byte_bound() {
    let mut model = loaded();
    model.handle(Key::Char('/'));
    model.handle(Key::Paste("東京a\u{301}".into()));
    model.handle(Key::Backspace);
    assert_eq!(model.query(), "東京");
    model.handle(Key::ClearInput);
    model.handle(Key::Paste(format!("\u{1b}{}", "東".repeat(128))));
    assert_eq!(model.query().len(), 126);
    let Effect::Search(query) = model.handle(Key::Enter) else {
        panic!("bounded Unicode query reads");
    };
    assert!(query.filter.validate().is_ok());
    assert_eq!(model.captures_active(), 1);
    assert!(model.playback().is_none());
}

#[test]
fn offline_reference_selection_changes_only_draft_until_explicit_apply() {
    let mut model = loaded();
    let rows = model.rows().to_vec();
    assert_eq!(model.handle(Key::Char('C')), Effect::None);
    assert!(model.search.picker.as_ref().is_some_and(|picker| {
        picker
            .page
            .as_ref()
            .is_some_and(|page| page.total_candidates >= 249)
    }));
    assert_eq!(model.handle(Key::Paste("Congo".into())), Effect::None);
    assert_eq!(model.handle(Key::Down), Effect::None);
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert_eq!(model.search.draft.filter.country, "CG");
    assert_eq!(model.current_search().filter.country, "");
    assert_eq!(model.rows(), rows);
    assert!(model.playback().is_none());
    let Effect::Search(query) = model.handle(Key::Enter) else {
        panic!("explicit apply reads cache");
    };
    assert_eq!(query.filter.country, "CG");
    assert_eq!(query.after, None);
    assert_eq!(model.rows(), rows);
    assert!(apply(&mut model, &query, "cg", None));
    model.handle(Key::Escape);
    model.handle(Key::Char('C'));
    assert!(
        model
            .search
            .picker
            .as_ref()
            .is_some_and(|picker| picker.query == "Congo" && picker.selected == 1)
    );
    model.handle(Key::ClearInput);
    model.handle(Key::Paste("Canada".into()));
    model.handle(Key::Escape);
    assert_eq!(model.search.draft.filter.country, "CG");
    model.handle(Key::Escape);
    assert_eq!(model.search.draft, model.search.applied);
}

#[test]
fn empty_cache_still_selects_every_reference_country_and_name_resolves() {
    let mut model = loaded();
    let Effect::Search(current) = model.handle(Key::Char('x')) else {
        panic!("explicit cache read");
    };
    assert!(model.apply_search(
        current.generation,
        Vec::new(),
        DirectoryView {
            cached_stations: 0,
            maximum_stations: 10_000,
            favorite_stations: 0,
            refresh: None
        },
        None
    ));
    model.handle(Key::Char('C'));
    model.handle(Key::Paste("中国".into()));
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert_eq!(model.search.draft.filter.country, "CN");
    assert!(model.rows().is_empty());
    model.handle(Key::Escape);
    let query = country(&mut model, "Canada");
    assert_eq!(query.filter.country, "CA");
}

#[test]
fn pending_station_response_cannot_relabel_a_new_country_draft_or_newer_results() {
    let mut model = loaded();
    let initial = country(&mut model, "CA");
    model.handle(Key::Escape);
    model.handle(Key::Char('C'));
    model.handle(Key::Paste("Congo".into()));
    model.handle(Key::Down);
    model.handle(Key::Enter);
    assert_eq!(model.search.draft.filter.country, "CG");
    assert!(apply(&mut model, &initial, "ca", None));
    assert_eq!(model.current_search().filter.country, "CA");
    assert_eq!(model.search.draft.filter.country, "CG");
    let Effect::Search(newer) = model.handle(Key::Enter) else {
        panic!("country apply");
    };
    assert!(!apply(&mut model, &initial, "old", None));
    assert_eq!(model.current_search().filter.country, "CA");
    assert!(apply(&mut model, &newer, "cg", None));
    assert_eq!(model.current_search().filter.country, "CG");
    assert_eq!(model.selected().map(|row| row.id.as_str()), Some("cg"));
    assert!(model.playback().is_none());
}

#[test]
fn failed_reads_are_fenced_and_preserve_rows_draft_and_applied_scope() {
    let mut model = loaded();
    let first = country(&mut model, "CA");
    assert!(apply(&mut model, &first, "a", Some("b")));
    model.handle(Key::Escape);
    let failed = country(&mut model, "FR");
    let old = model.rows().to_vec();
    assert!(model.fail_search(failed.generation, "fixture cache failure"));
    assert_eq!(model.rows(), old);
    assert_eq!(model.current_search().filter.country, "CA");
    assert_eq!(model.search.draft.filter.country, "FR");
    assert!(model.search.failed);
    assert!(!model.search.pending());
    assert!(!apply(&mut model, &failed, "b", None));
    model.handle(Key::ClearInput);
    model.handle(Key::Paste("US".into()));
    let Effect::Search(newer) = model.handle(Key::Enter) else {
        panic!("corrected draft reads");
    };
    assert!(!model.search.failed);
    assert!(!model.fail_search(failed.generation, "stale failure"));
    assert!(model.search.pending());
    assert!(apply(&mut model, &newer, "c", None));
    assert_eq!(model.current_search().filter.country, "US");
}
