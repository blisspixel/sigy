//! Selection menu, confirmation dialogs, and the tight-window fallback.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use sigy_service::storage::dvr::RecordingInterval;

use crate::explorer::state::{
    Coordinates, Effect, Explorer, Key, Modes, RecordingLine, SealMap, Workspace,
};

#[test]
fn space_opens_the_explore_menu_and_leaves_the_selection() {
    let mut model = two_stations();
    let status = model.status().to_owned();
    let selection = model.selection();
    assert_eq!(model.handle(Key::Char(' ')), Effect::None);
    assert!(model.menu_open());
    assert_eq!(model.menu_cursor(), Some(0));
    assert_eq!(model.selection(), selection);
    assert_eq!(model.status(), status);
    let text = super::frame(&model, 80, 24).0;
    for label in [
        "R Fetch",
        "f Favorite",
        "P Play",
        "Enter Inspect",
        "i Linked history",
        "Copy CLI command",
    ] {
        assert!(text.contains(label), "{label} missing from {text}");
    }
    assert!(text.contains("Enter inspects this station before linked history."));
    assert!(text.contains("Home / Find stations"), "{text}");
    assert_eq!(model.handle(Key::Down), Effect::None);
    assert_eq!(model.selection(), selection);
    assert_eq!(model.menu_cursor(), Some(1));
    assert_eq!(model.handle(Key::Char('0')), Effect::None);
    assert!(!model.at_home());
    assert!(model.menu_open());
    assert_eq!(model.handle(Key::Char('q')), Effect::Detach);
}

#[test]
fn escape_and_backspace_close_the_menu_without_a_status_change() {
    let mut model = two_stations();
    let status = model.status().to_owned();
    let selection = model.selection();
    model.handle(Key::Char(' '));
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(!model.menu_open());
    assert_eq!(model.selection(), selection);
    assert_eq!(model.status(), status);
    assert!(!model.at_home());
    model.handle(Key::Char(' '));
    model.handle(Key::Down);
    assert_eq!(model.handle(Key::Backspace), Effect::None);
    assert!(!model.menu_open());
    assert_eq!(model.selection(), selection);
    assert_eq!(model.status(), status);
    assert_eq!(model.handle(Key::Down), Effect::None);
    assert_eq!(model.selection(), 1);
}

#[test]
fn menu_play_matches_the_direct_key_and_enter_does_not_confirm() {
    let mut direct = super::sample();
    assert_eq!(direct.handle(Key::Char('P')), Effect::None);
    assert!(direct.status().contains("y confirms"));
    let direct_play = direct.handle(Key::Char('y'));
    let mut via = super::sample();
    via.handle(Key::Char(' '));
    assert_eq!(via.handle(Key::Char('P')), Effect::None);
    assert!(!via.menu_open());
    assert!(via.status().contains("y confirms"));
    assert_eq!(via.handle(Key::Enter), Effect::None);
    assert_eq!(via.handle(Key::Backspace), Effect::None);
    assert!(via.status().contains("y confirms"));
    assert!(!via.status().contains("discarded"));
    let via_play = via.handle(Key::Char('y'));
    assert_eq!(direct_play, via_play);
    assert!(matches!(via_play, Effect::Play { .. }));
}

#[test]
fn enter_on_the_play_row_confirms_only_with_y() {
    let mut model = super::sample();
    model.handle(Key::Char(' '));
    model.handle(Key::Down);
    model.handle(Key::Down);
    assert_eq!(model.menu_cursor(), Some(2));
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(model.status().contains("y confirms"));
    assert_eq!(model.handle(Key::Char('n')), Effect::None);
    assert!(model.status().contains("Playback request discarded."));
    assert!(!model.menu_open());
    assert!(!model.at_home());
}

#[test]
fn menu_fetch_matches_the_direct_key() {
    let mut direct = super::sample();
    assert_eq!(direct.handle(Key::Char('R')), Effect::None);
    let direct_fetch = direct.handle(Key::Char('y'));
    let mut via = super::sample();
    via.handle(Key::Char(' '));
    assert_eq!(via.handle(Key::Char('R')), Effect::None);
    assert!(via.status().contains("y confirms"));
    assert_eq!(via.handle(Key::Enter), Effect::None);
    assert_eq!(via.handle(Key::Backspace), Effect::None);
    assert!(via.status().contains("y confirms"));
    assert!(!via.status().contains("discarded"));
    let via_fetch = via.handle(Key::Char('y'));
    assert_eq!(direct_fetch, via_fetch);
    assert!(matches!(via_fetch, Effect::Fetch(_)));
    let text = super::frame(&confirming_fetch(), 80, 24).0;
    assert!(text.contains("y Fetch"), "{text}");
    assert!(text.contains("No station is contacted."), "{text}");
    assert!(text.contains("Applied directory page"), "{text}");
}

#[test]
fn menu_hold_matches_the_direct_key() {
    let mut direct = sealed();
    assert_eq!(direct.handle(Key::Char('K')), Effect::None);
    assert!(direct.status().contains("y confirms"));
    let direct_hold = direct.handle(Key::Char('y'));
    let mut via = sealed();
    via.handle(Key::Char(' '));
    assert_eq!(via.handle(Key::Char('K')), Effect::None);
    assert!(!via.menu_open());
    assert_eq!(via.handle(Key::Enter), Effect::None);
    assert_eq!(via.handle(Key::Backspace), Effect::None);
    assert!(via.status().contains("y confirms"));
    assert!(!via.status().contains("discarded"));
    let via_hold = via.handle(Key::Char('y'));
    assert_eq!(direct_hold, via_hold);
    match via_hold {
        Effect::ProtectInterval {
            generation,
            id,
            start_us,
            end_us,
        } => {
            assert_eq!(generation, 1);
            assert_eq!(id, "rec-sealed");
            assert_eq!(start_us, 5_000_000);
            assert_eq!(end_us, 9_000_000);
        }
        other => panic!("expected one hold, got {other:?}"),
    }
    let text = super::frame(&held(), 80, 24).0;
    assert!(text.contains("y Hold"), "{text}");
    assert!(text.contains("Records one hold"), "{text}");
    assert!(text.contains("rec-sealed"), "{text}");
}

#[test]
fn menu_favorite_and_inspect_match_the_direct_keys() {
    let mut direct = super::sample();
    let Effect::SetFavorite { id, favorite, .. } = direct.handle(Key::Char('f')) else {
        panic!("direct favorite");
    };
    let mut via = super::sample();
    via.handle(Key::Char(' '));
    assert_eq!(via.handle(Key::Down), Effect::None);
    assert_eq!(via.menu_cursor(), Some(1));
    match via.handle(Key::Enter) {
        Effect::SetFavorite {
            id: via_id,
            favorite: via_favorite,
            ..
        } => {
            assert_eq!(via_id, id);
            assert_eq!(via_favorite, favorite);
        }
        other => panic!("menu favorite, got {other:?}"),
    }
    assert!(!via.menu_open());

    let mut direct_inspect = super::sample();
    assert_eq!(direct_inspect.handle(Key::Enter), Effect::None);
    assert!(direct_inspect.context.is_some());
    let mut via_inspect = super::sample();
    via_inspect.handle(Key::Char(' '));
    for _ in 0..3 {
        via_inspect.handle(Key::Down);
    }
    assert_eq!(via_inspect.menu_cursor(), Some(3));
    assert_eq!(via_inspect.handle(Key::Enter), Effect::None);
    assert!(!via_inspect.menu_open());
    assert_eq!(
        via_inspect
            .context
            .as_ref()
            .map(|context| context.row.id.as_str()),
        direct_inspect
            .context
            .as_ref()
            .map(|context| context.row.id.as_str())
    );
}

#[test]
fn a_disconnected_favorite_stays_on_the_menu() {
    let mut model = super::sample();
    model.note_disconnect(20_000, "fixture");
    model.handle(Key::Char(' '));
    assert_eq!(model.handle(Key::Char('f')), Effect::None);
    assert!(model.menu_open());
    assert!(model.status().contains("not submitted"));
    assert!(!matches!(
        model.handle(Key::Char('f')),
        Effect::SetFavorite { .. }
    ));
    let mut direct = super::sample();
    direct.note_disconnect(20_000, "fixture");
    assert_eq!(direct.handle(Key::Char('f')), Effect::None);
    assert!(direct.status().contains("not submitted"));
}

#[test]
fn a_disconnected_fetch_confirmation_submits_nothing() {
    let mut via = Explorer::new(plain_modes(false, true), 10_000);
    via.handle(Key::Char(' '));
    assert_eq!(via.handle(Key::Char('R')), Effect::None);
    let effect = via.handle(Key::Char('y'));
    assert!(!matches!(effect, Effect::Fetch(_)), "{effect:?}");
    assert!(via.status().contains("not submitted"), "{}", via.status());
    let mut direct = Explorer::new(plain_modes(false, true), 10_000);
    assert_eq!(direct.handle(Key::Char('R')), Effect::None);
    let effect = direct.handle(Key::Char('y'));
    assert!(!matches!(effect, Effect::Fetch(_)), "{effect:?}");
    assert!(direct.status().contains("not submitted"));
}

#[test]
fn an_empty_station_list_keeps_play_on_the_menu() {
    let mut model = super::sample();
    let mut desk = super::sample_desk();
    desk.stations.clear();
    model.apply_desk(desk);
    model.handle(Key::Char(' '));
    assert_eq!(model.handle(Key::Char('P')), Effect::None);
    assert!(model.menu_open());
    assert!(model.status().contains("No selected station to play."));
    model.handle(Key::Down);
    model.handle(Key::Down);
    model.handle(Key::Down);
    model.handle(Key::Down);
    model.handle(Key::Down);
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(!model.menu_open());
    assert_eq!(model.status(), "sigy play STATION_UUID");
}

#[test]
fn copy_writes_the_selected_station_command() {
    let mut model = super::sample();
    model.handle(Key::Char(' '));
    for _ in 0..5 {
        model.handle(Key::Down);
    }
    assert_eq!(model.menu_cursor(), Some(5));
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(!model.menu_open());
    assert_eq!(
        model.status(),
        "sigy play 00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(model.selection(), 0);
}

#[test]
fn a_tight_window_uses_the_status_line_and_does_not_trap_escape() {
    let mut model = two_stations();
    let selection = model.selection();
    model.note_window(39, 11);
    assert_eq!(model.handle(Key::Char(' ')), Effect::None);
    assert!(!model.menu_open());
    assert!(model.status().contains("Actions:"), "{}", model.status());
    assert!(model.status().contains("R Fetch"));
    let text = super::frame(&model, 39, 11).0;
    assert!(!text.contains("y Play"), "{text}");
    assert!(!text.contains('┌'), "{text}");
    model.note_window(80, 24);
    model.handle(Key::Char(' '));
    assert!(model.menu_open());
    model.note_window(39, 11);
    assert!(!model.menu_open());
    assert_eq!(model.selection(), selection);
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(!model.at_home());
    assert_eq!(model.workspace(), Workspace::Explore);
    let Effect::Search(query) = model.handle(Key::Char('2')) else {
        panic!("Listen reads cached favorites");
    };
    assert!(query.favorites_only);
    assert!(query.after.is_none());
    assert_eq!(
        query.filter,
        sigy_service::discovery::StationFilter::default()
    );
    assert_eq!(model.workspace(), Workspace::Live);
}

#[test]
fn confirmations_fall_back_below_forty_by_twelve_and_draw_at_the_boundary() {
    let mut small = super::sample();
    small.handle(Key::Char('P'));
    let narrow = super::frame(&small, 39, 11).0;
    assert!(small.status().contains("y confirms"));
    assert!(!narrow.contains("y Play"), "{narrow}");
    assert!(!words(&narrow).contains("Nothing is recorded."), "{narrow}");
    let short = super::frame(&small, 40, 11).0;
    assert!(!short.contains("y Play"), "{short}");
    let boundary = super::frame(&small, 40, 12).0;
    assert!(boundary.contains("y Play"), "{boundary}");
    assert!(
        words(&boundary).contains("Nothing is recorded."),
        "{boundary}"
    );
    assert!(boundary.contains("KTNN"), "{boundary}");
    let wide = super::frame(&small, 80, 24).0;
    assert!(wide.contains("y Play"), "{wide}");
    assert!(words(&wide).contains("Nothing is recorded."), "{wide}");
    assert!(wide.contains("KTNN"), "{wide}");
    assert!(wide.contains("y confirms"), "{wide}");
    assert!(matches!(small.handle(Key::Char('y')), Effect::Play { .. }));
}

#[test]
fn the_selected_menu_row_is_reversed_and_linear_has_no_border() {
    for linear in [false, true] {
        for monochrome in [false, true] {
            let mut model = styled(linear, monochrome);
            model.handle(Key::Char(' '));
            let text = super::frame(&model, 80, 24).0;
            assert!(text.contains("R Fetch"), "{linear} {monochrome} {text}");
            assert_eq!(
                label_reversed(&model, 80, 24, "R Fetch"),
                Some(true),
                "{linear} {monochrome}"
            );
            assert_eq!(
                label_reversed(&model, 80, 24, "P Play"),
                Some(false),
                "{linear} {monochrome}"
            );
            let boxed = text.contains('┌') || text.contains('─') || text.contains('│');
            assert_eq!(boxed, !linear, "{linear} {monochrome} {text}");
        }
    }
}

#[test]
fn space_stays_closed_on_home_live_monitors_system_filters_and_keys() {
    let mut home = super::sample();
    home.show_home();
    let status = home.status().to_owned();
    assert_eq!(home.handle(Key::Char(' ')), Effect::None);
    assert!(!home.menu_open());
    assert!(home.at_home());
    assert_eq!(home.status(), status);
    for digit in ['2', '4', '6'] {
        let mut model = super::sample();
        model.handle(Key::Char(digit));
        let before = model.status().to_owned();
        let selection = model.selection();
        assert_eq!(model.handle(Key::Char(' ')), Effect::None);
        assert!(!model.menu_open(), "{digit}");
        assert_eq!(model.status(), before);
        assert_eq!(model.selection(), selection);
    }
    let mut filters = super::sample();
    filters.handle(Key::Char('F'));
    assert_eq!(filters.handle(Key::Char(' ')), Effect::None);
    assert!(!filters.menu_open());
    assert!(super::super::editing(&filters));
    let mut keys = super::sample();
    keys.handle(Key::Char('?'));
    assert_eq!(keys.handle(Key::Char(' ')), Effect::None);
    assert!(keys.keys_open());
    assert!(!keys.menu_open());
}

#[test]
fn globe_menu_centers_only_with_coordinates_and_hides_the_spin() {
    let mut placed = super::sample();
    let mut desk = super::sample_desk();
    desk.stations[0].coordinates = Some(Coordinates {
        latitude: 35.68,
        longitude: 139.69,
    });
    placed.apply_desk(desk);
    placed.handle(Key::Char('7'));
    let area = Rect::new(0, 0, 160, 48);
    assert!(super::super::globe_visible(&placed, area));
    placed.handle(Key::Char(' '));
    assert!(!super::super::globe_visible(&placed, area));
    assert!(super::super::globe_flourish_canvas(&placed, area).is_none());
    assert_eq!(placed.handle(Key::Char('c')), Effect::None);
    assert!(!placed.menu_open());
    assert!(placed.status().contains("Centered"));
    let mut bare = super::sample();
    bare.handle(Key::Char('7'));
    bare.handle(Key::Char(' '));
    let text = super::frame(&bare, 80, 24).0;
    assert!(text.contains("c Center on globe"), "{text}");
    assert!(text.contains("no directory coordinates"), "{text}");
    assert_eq!(bare.handle(Key::Char('c')), Effect::None);
    assert!(bare.menu_open());
    assert!(bare.status().contains("no directory coordinates"));
}

#[test]
fn an_open_station_blocks_fetch_and_favorite_until_escape() {
    let mut model = super::sample();
    model.handle(Key::Enter);
    model.handle(Key::Char(' '));
    assert!(model.menu_open());
    assert_eq!(model.handle(Key::Char('R')), Effect::None);
    assert!(model.menu_open());
    assert!(model.status().contains("before a fetch"));
    assert_eq!(model.handle(Key::Char('f')), Effect::None);
    assert!(model.menu_open());
    assert!(model.status().contains("before a favorite"));
    assert!(!matches!(
        model.handle(Key::Char('f')),
        Effect::SetFavorite { .. }
    ));
    assert_eq!(model.handle(Key::Char('P')), Effect::None);
    assert!(!model.menu_open());
    assert!(model.status().contains("y confirms"));
    assert_eq!(model.handle(Key::Char('n')), Effect::None);
    assert!(model.status().contains("Playback request discarded."));
    assert!(model.context.is_some());
}

#[test]
fn the_key_overlay_names_space_and_the_footer_does_not() {
    let model = super::sample();
    let help = super::super::hint_text(&model, 80);
    assert!(help.ends_with("? more"), "{help}");
    assert!(!help.contains("Space"), "{help}");
    let mut open = model.clone();
    open.handle(Key::Char('?'));
    let overlay = super::frame(&open, 100, 64).0;
    assert!(
        overlay.contains("Space opens actions for the selection"),
        "{overlay}"
    );
    assert!(overlay.contains("Copy CLI command writes it on the status line"));
    assert!(overlay.contains("i linked history"));
}

#[test]
fn latest_ignores_the_open_tail_under_the_cursor() {
    let mut model = open_tail();
    assert_eq!(model.interval_index(), 2);
    model.handle(Key::Char(' '));
    assert_eq!(model.handle(Key::Char('.')), Effect::None);
    assert_eq!(model.handle(Key::Down), Effect::None);
    assert_eq!(model.interval_index(), 2);
    assert!(model.menu_open());
    let latest = popup_line(&model, "L Latest");
    assert!(!latest.contains("open tail"), "{latest}");
    let interval = popup_line(&model, "I Interval");
    assert!(interval.contains("open tail"), "{interval}");
    let hold = popup_line(&model, "K Hold");
    assert!(hold.contains("open tail"), "{hold}");
    assert_eq!(model.handle(Key::Char('I')), Effect::None);
    assert!(model.menu_open());
    assert!(model.status().contains("open tail"));
    assert!(!matches!(
        model.handle(Key::Char('I')),
        Effect::PlayInterval { .. }
    ));
    assert_eq!(model.handle(Key::Char('K')), Effect::None);
    assert!(model.menu_open());
    assert!(model.status().contains("open tail"));
    assert!(!matches!(
        model.handle(Key::Char('K')),
        Effect::ProtectInterval { .. }
    ));

    let mut direct = open_tail();
    assert_eq!(direct.handle(Key::Char('L')), Effect::None);
    let direct_play = confirm_latest(&mut direct);

    let mut by_key = open_tail();
    by_key.handle(Key::Char(' '));
    assert_eq!(by_key.handle(Key::Char('L')), Effect::None);
    assert!(!by_key.menu_open());
    let by_key_play = confirm_latest(&mut by_key);

    let mut by_row = open_tail();
    by_row.handle(Key::Char(' '));
    assert_eq!(by_row.handle(Key::Down), Effect::None);
    assert_eq!(by_row.menu_cursor(), Some(1));
    assert_eq!(by_row.interval_index(), 2);
    assert_eq!(by_row.handle(Key::Enter), Effect::None);
    assert!(!by_row.menu_open());
    let by_row_play = confirm_latest(&mut by_row);
    assert_same_latest(&direct_play, &by_key_play);
    assert_same_latest(&direct_play, &by_row_play);
}

#[test]
fn hold_dialog_strips_controls_and_keeps_the_real_id() {
    let mut model = super::sample();
    let mut desk = super::sample_desk();
    let mut line = sealed_line();
    line.id = "rec\n\u{202e}sealed".into();
    desk.recordings[0] = line;
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    let _ = model.handle(Key::Char('.'));
    model.handle(Key::Char(' '));
    assert_eq!(model.handle(Key::Char('K')), Effect::None);
    let Some(dialog) = model.dialog_copy() else {
        panic!("hold dialog");
    };
    assert_eq!(dialog.target, "recsealed [5000000, 9000000)");
    assert!(!dialog.target.chars().any(char::is_control));
    match model.handle(Key::Char('y')) {
        Effect::ProtectInterval {
            id,
            start_us,
            end_us,
            ..
        } => {
            assert_eq!(id, "rec\n\u{202e}sealed");
            assert_eq!(start_us, 5_000_000);
            assert_eq!(end_us, 9_000_000);
        }
        other => panic!("expected the real recording id, got {other:?}"),
    }
}

#[test]
fn copy_names_the_recording_and_a_finding_without_a_citation_stays_put() {
    let mut recordings = sealed();
    recordings.handle(Key::Char(' '));
    let Some(copy) = crate::explorer::actions::menu_items(&recordings)
        .iter()
        .position(|item| item.slot == crate::explorer::actions::Slot::Copy)
    else {
        panic!("recordings menu has no copy row");
    };
    for _ in 0..copy {
        recordings.handle(Key::Down);
    }
    assert_eq!(recordings.menu_cursor(), Some(copy));
    assert_eq!(recordings.handle(Key::Enter), Effect::None);
    assert!(!recordings.menu_open());
    assert_eq!(recordings.status(), "sigy listen file rec-sealed");

    let mut findings = super::sample();
    findings.handle(Key::Char('5'));
    findings.handle(Key::Char(' '));
    assert!(findings.menu_open());
    assert_eq!(findings.handle(Key::Enter), Effect::None);
    assert!(findings.menu_open());
    assert!(findings.status().contains("Enter exactly two IDs"));
    assert_eq!(findings.handle(Key::Char('o')), Effect::None);
    assert!(findings.menu_open());
    assert!(findings.status().contains("no loaded citation"));
    assert!(!matches!(
        findings.handle(Key::Char('P')),
        Effect::PlayFinding { .. }
    ));
    assert!(findings.menu_open());
    assert!(findings.status().contains("no cited interval"));
    assert_eq!(findings.handle(Key::Down), Effect::None);
    assert_eq!(findings.menu_cursor(), Some(3));
    assert_eq!(findings.handle(Key::Enter), Effect::None);
    assert!(!findings.menu_open());
    assert_eq!(findings.status(), "sigy listen file RECORDING");
}

fn confirm_latest(model: &mut Explorer) -> Effect {
    assert!(
        model
            .status()
            .contains("Play latest sealed interval [5000000, 9000000)")
    );
    let Some(dialog) = model.dialog_copy() else {
        panic!("latest dialog");
    };
    assert!(
        dialog.target.contains("rec-sealed from 5000000"),
        "{}",
        dialog.target
    );
    assert!(!dialog.target.contains("9000000"), "{}", dialog.target);
    assert_eq!(dialog.buttons, "y Play   n Cancel");
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert_eq!(model.handle(Key::Backspace), Effect::None);
    assert!(model.status().contains("y confirms"));
    assert!(!model.status().contains("discarded"));
    model.handle(Key::Char('y'))
}

fn assert_same_latest(direct: &Effect, via: &Effect) {
    match (direct, via) {
        (
            Effect::PlayLatest {
                id: direct_id,
                start_us: direct_start,
                ..
            },
            Effect::PlayLatest { id, start_us, .. },
        ) => {
            assert_eq!(id, direct_id);
            assert_eq!(start_us, direct_start);
            assert_eq!(*start_us, 5_000_000);
            assert_eq!(id, "rec-sealed");
        }
        (direct, via) => panic!("expected latest playback, got {direct:?} and {via:?}"),
    }
}

fn words(text: &str) -> String {
    text.chars()
        .map(|character| {
            if matches!(
                character,
                '┌' | '┐' | '└' | '┘' | '─' | '│' | '├' | '┤' | '┬' | '┴' | '┼'
            ) {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn confirming_fetch() -> Explorer {
    let mut model = super::sample();
    model.handle(Key::Char('R'));
    model
}

fn held() -> Explorer {
    let mut model = sealed();
    model.handle(Key::Char('K'));
    model
}

fn sealed() -> Explorer {
    let mut model = super::sample();
    let mut desk = super::sample_desk();
    desk.recordings[0] = sealed_line();
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    let _ = model.handle(Key::Char('.'));
    model
}

fn open_tail() -> Explorer {
    let mut model = super::sample();
    let mut desk = super::sample_desk();
    let mut line = sealed_line();
    line.seal
        .intervals
        .push(one_interval(9_000_000, 12_000_000, "c"));
    desk.recordings[0] = line;
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    let _ = model.handle(Key::Char('.'));
    let _ = model.handle(Key::Char('.'));
    model
}

fn popup_line(model: &Explorer, label: &str) -> String {
    let width = 80u16;
    let height = 24u16;
    let buffer = draw(model, width, height);
    let chars: Vec<char> = label.chars().collect();
    for y in 0..height {
        for x in 0..width {
            if !starts_at(&buffer, x, y, width, &chars) {
                continue;
            }
            let mut text = String::new();
            let mut column = x;
            while column < width {
                let symbol = buffer[(column, y)].symbol();
                if symbol == "│" {
                    break;
                }
                text.push_str(symbol);
                column = column.saturating_add(1);
            }
            return text.trim_end().to_owned();
        }
    }
    String::new()
}

fn sealed_line() -> RecordingLine {
    RecordingLine {
        id: "rec-sealed".into(),
        source_revision: String::new(),
        state: "running".into(),
        storage_state: "reserved".into(),
        format: Some("wav".into()),
        timeline: crate::explorer::timeline::Timeline::default(),
        seal: SealMap {
            state: "completed".into(),
            open_ceiling: 1,
            open_object_key: Some("c".repeat(32)),
            intervals: vec![
                one_interval(0, 5_000_000, "a"),
                one_interval(5_000_000, 9_000_000, "b"),
            ],
            gaps: Vec::new(),
            truncated: false,
        },
        hold_total: 0,
        shown_holds: Vec::new(),
    }
}

fn one_interval(start: u64, end: u64, key: &str) -> RecordingInterval {
    RecordingInterval {
        ordinal: 0,
        decoded_start_us: start,
        decoded_end_us: end,
        byte_start: 0,
        byte_end: 100,
        object_key: key.repeat(32),
        sha256: "ab".repeat(32),
        format: "wav".into(),
        ceiling_bytes: 100,
        released: false,
    }
}

fn two_stations() -> Explorer {
    let mut model = super::sample();
    let mut desk = super::sample_desk();
    let mut extra = desk.stations[0].clone();
    extra.id = "00000000-0000-4000-8000-000000000002".into();
    extra.name = "Second".into();
    desk.stations.push(extra);
    model.apply_desk(desk);
    model
}

fn styled(linear: bool, monochrome: bool) -> Explorer {
    let mut model = Explorer::new(plain_modes(linear, monochrome), 10_000);
    model.apply_desk(super::sample_desk());
    model
}

fn plain_modes(linear: bool, monochrome: bool) -> Modes {
    Modes {
        reduced_motion: true,
        linear,
        monochrome,
        emphasis: crate::explorer::state::Emphasis::Standard,
    }
}

fn label_reversed(model: &Explorer, width: u16, height: u16, label: &str) -> Option<bool> {
    let buffer = draw(model, width, height);
    let chars: Vec<char> = label.chars().collect();
    for y in 0..height {
        for x in 0..width {
            if !starts_at(&buffer, x, y, width, &chars) {
                continue;
            }
            return Some(chars.iter().enumerate().all(|(index, _)| {
                let column = x.saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
                buffer[(column, y)].modifier.contains(Modifier::REVERSED)
            }));
        }
    }
    None
}

fn starts_at(buffer: &Buffer, x: u16, y: u16, width: u16, chars: &[char]) -> bool {
    chars.iter().enumerate().all(|(index, character)| {
        let Some(column) = x.checked_add(u16::try_from(index).unwrap_or(u16::MAX)) else {
            return false;
        };
        column < width && is_char(&buffer[(column, y)], *character)
    })
}

fn is_char(cell: &Cell, character: char) -> bool {
    let mut encoded = [0_u8; 4];
    cell.symbol() == character.encode_utf8(&mut encoded)
}

fn draw(model: &Explorer, width: u16, height: u16) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).unwrap_or_else(|error| match error {});
    let _ = terminal.draw(|ui| super::super::render(ui, model));
    terminal.backend().buffer().clone()
}
