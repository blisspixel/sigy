//! Recordings list, then the timeline. Selection starts no playback.

use super::{frame, sample, sample_desk};
use crate::explorer::actions::{self, Slot};
use crate::explorer::state::{Effect, Explorer, Focus, HoldSpan, Key, Modes, SealMap, Workspace};
use sigy_service::storage::dvr::RecordingInterval;

const PLAY_BLOCK: &str =
    "Only a retained recording that is not capturing can play. Selection does not start audio.";

#[test]
fn eighty_by_twenty_four_lists_twelve_recordings_without_a_microsecond_range() {
    let model = listed(16);
    let text = frame(&model, 80, 24).0;
    let shown = text.lines().filter(|line| line.contains("rev-")).count();
    assert!(shown >= 12, "{shown} rows\n{text}");
    assert!(text.contains("Enter opens the timeline"), "{text}");
    assert!(text.contains("Selected recording: row-00"), "{text}");
    assert!(!text.contains(")us"), "{text}");
    assert!(!text.contains("[0,"), "{text}");
    assert!(!text.contains("10000000"), "{text}");
    assert!(!text.contains("Cache:"), "{text}");
    assert!(!text.contains("Search:"), "{text}");
    assert!(text.contains("> rev-00"), "{text}");
    for line in text.lines().filter(|line| line.contains("rev-")) {
        assert!(line.chars().count() <= 80, "{line}");
    }
    let rows = body(&model, 80, 16);
    assert!(rows.contains("REC"), "{rows}");
    assert!(rows.contains("completed"), "{rows}");
    assert!(!rows.contains("running"), "{rows}");
    assert!(!rows.contains("ago"), "{rows}");
    assert!(!rows.contains("Hold "), "{rows}");
}

#[test]
fn the_selected_row_stays_on_screen_and_enter_returns_to_it() {
    let mut model = listed(16);
    for _ in 0..15 {
        assert_eq!(model.handle(Key::Down), Effect::None);
    }
    assert_eq!(model.recording_selection(), 15);
    let text = frame(&model, 80, 24).0;
    let Some(line) = text.lines().find(|line| line.contains("rev-15")) else {
        panic!("last row missing from {text}");
    };
    assert!(line.starts_with("> rev-15"), "{line}");
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(model.recording_detail());
    let detail = frame(&model, 80, 24).0;
    assert!(detail.contains(")us"), "{detail}");
    assert!(detail.contains("Metadata snapshot"), "{detail}");
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(model.recording_detail());
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(!model.recording_detail());
    assert!(!model.at_home());
    assert_eq!(model.recording_selection(), 15);
    let restored = frame(&model, 80, 24).0;
    assert!(restored.contains("> rev-15"), "{restored}");
    assert!(!restored.contains(")us"), "{restored}");
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(model.at_home());
}

#[test]
fn backspace_and_digit_three_leave_the_timeline_for_the_list() {
    let mut model = listed(2);
    model.handle(Key::Down);
    model.handle(Key::Enter);
    assert!(model.recording_detail());
    assert_eq!(model.handle(Key::Backspace), Effect::None);
    assert!(!model.recording_detail());
    assert_eq!(model.recording_selection(), 1);
    model.handle(Key::Enter);
    assert_eq!(model.handle(Key::Char('3')), Effect::None);
    assert!(!model.recording_detail());
    assert_eq!(model.workspace(), Workspace::Recordings);
    assert_eq!(model.recording_selection(), 1);
}

#[test]
fn an_empty_list_does_not_open_a_timeline() {
    let mut model = sample();
    let mut desk = sample_desk();
    desk.recordings.clear();
    model.apply_desk(desk);
    assert_eq!(model.handle(Key::Char('3')), Effect::None);
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(!model.recording_detail());
    assert_eq!(
        model.status(),
        "No recordings yet. This view never starts one."
    );
    let text = frame(&model, 80, 24).0;
    assert!(
        text.contains("No recordings yet. This view never starts one."),
        "{text}"
    );
    assert!(
        text.contains("radio add registers a station as a source revision first."),
        "{text}"
    );
    assert!(!text.contains(")us"), "{text}");
}

#[test]
fn a_cleared_page_closes_a_timeline_that_no_longer_has_a_row() {
    let mut model = playable();
    model.handle(Key::Enter);
    assert!(model.recording_detail());
    let mut desk = sample_desk();
    desk.recordings.clear();
    model.apply_desk(desk);
    assert!(!model.recording_detail());
}

#[test]
fn a_cited_recording_opens_on_its_timeline() {
    let mut model = sample();
    let record = model.recordings()[0].clone();
    model.open_finding_recording(record);
    assert!(model.recording_detail());
    assert_eq!(
        model.status(),
        "Cited recording metadata selected. No playback or processing started."
    );
    let text = frame(&model, 80, 24).0;
    assert!(text.contains(")us"), "{text}");
    assert!(text.contains("[0,"), "{text}");
    assert_eq!(model.handle(Key::Char('3')), Effect::None);
    assert!(!model.recording_detail());
    let list = frame(&model, 80, 24).0;
    assert!(!list.contains(")us"), "{list}");
}

#[test]
fn the_list_uses_the_revision_id_and_strips_controls() {
    let mut model = sample();
    let mut desk = sample_desk();
    desk.recordings[0].id = "rec\nsecret".into();
    desk.recordings[0].source_revision = "rev\n\u{202e}one".into();
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    let text = frame(&model, 80, 24).0;
    assert!(text.contains("revone"), "{text}");
    assert!(text.contains("Selected recording: recsecret"), "{text}");
    assert!(!text.contains('\u{202e}'), "{text}");
    assert!(
        text.chars()
            .filter(|character| *character != '\n')
            .all(|character| !character.is_control()),
        "{text:?}"
    );
    assert!(
        !text
            .lines()
            .any(|line| line.contains("secret") && !line.contains("recsecret")),
        "{text}"
    );
    let rows = body(&model, 80, 16);
    assert!(rows.contains("revone"), "{rows}");
    assert!(!rows.contains("recsecret"), "{rows}");
    assert!(!rows.contains(")us"), "{rows}");

    let mut plain = sample();
    let mut desk = sample_desk();
    desk.recordings[0].id = "rec-plain".into();
    desk.recordings[0].source_revision.clear();
    plain.apply_desk(desk);
    plain.handle(Key::Char('3'));
    let rows = body(&plain, 80, 16);
    assert!(rows.contains("rec-plain"), "{rows}");
}

#[test]
fn comma_on_the_list_keeps_the_range_off_the_rows() {
    let mut model = playable();
    let before = body(&model, 80, 16);
    assert!(!before.contains("Sealed interval"), "{before}");
    assert!(!before.contains("Hold "), "{before}");
    assert!(!before.contains(")us"), "{before}");
    assert_eq!(model.handle(Key::Char(',')), Effect::None);
    assert_eq!(model.interval_index(), 0);
    assert!(!model.recording_detail());
    assert_eq!(body(&model, 80, 16), before);
    assert!(
        model.status().contains("Sealed interval 1 of 2"),
        "{}",
        model.status()
    );
    assert_eq!(model.handle(Key::Char('.')), Effect::None);
    assert_eq!(model.interval_index(), 1);
    assert_eq!(body(&model, 80, 16), before);
    assert!(
        model.status().contains("Sealed interval 2 of 2"),
        "{}",
        model.status()
    );
    assert_eq!(model.handle(Key::Enter), Effect::None);
    let detail = body(&model, 80, 16);
    assert!(detail.contains(")us"), "{detail}");
    assert!(detail.contains("Sealed interval 2 of 2"), "{detail}");
    assert!(detail.contains("Hold "), "{detail}");
    assert_eq!(model.interval_index(), 1);
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert_eq!(model.interval_index(), 1);
    model.handle(Key::Down);
    assert!(model.recording_detail());
    assert_eq!(model.interval_index(), 0);
    let reset = body(&model, 80, 16);
    assert!(reset.contains("Sealed interval 1 of 2"), "{reset}");
}

#[test]
fn narrow_list_hides_the_timeline_until_enter() {
    let model = listed(4);
    let text = frame(&model, 40, 10).0;
    assert!(!text.contains(")us"), "{text}");
    assert!(!text.contains("Recording row-00: metadata"), "{text}");
    assert!(!text.contains("########"), "{text}");
}

#[test]
fn linear_marks_the_open_timeline_and_not_the_identity_line() {
    let timeline = detail_text(true, false);
    assert!(timeline.contains("Focus Timeline:"), "{timeline}");
    assert!(!timeline.contains("Focus Selected"), "{timeline}");
    assert!(timeline.contains("Selected recording: rec-1"), "{timeline}");

    let mut model = Explorer::new(
        Modes {
            reduced_motion: true,
            linear: true,
            monochrome: false,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        10_000,
    );
    model.apply_desk(sample_desk());
    model.place_for_actions(Workspace::Recordings, Focus::Results);
    let list = frame(&model, 80, 24).0;
    assert!(list.contains("Focus Recordings:"), "{list}");
    assert!(!list.contains(")us"), "{list}");
    assert!(!list.contains("Focus Selected"), "{list}");
}

#[test]
fn linear_and_monochrome_keep_the_plain_timeline_glyphs() {
    let plain = "# retained, x unavailable, ! gap, . not yet published, + mixed";
    for (linear, monochrome) in [(true, false), (false, true)] {
        let text = detail_text(linear, monochrome);
        assert!(text.contains(plain), "{text}");
        assert!(!text.contains('█'), "{text}");
        assert!(!text.contains('░'), "{text}");
        assert!(!text.contains('╳'), "{text}");
        assert!(!text.contains('·'), "{text}");
    }
    let colored = detail_text(false, false);
    assert!(colored.contains('█'), "{colored}");
    assert!(
        colored.contains("█ retained  ░ unavailable  ╳ gap  · not yet published  + mixed"),
        "{colored}"
    );
}

#[test]
fn the_menu_lists_pause_and_seek_without_moving_play_or_latest() {
    let model = playable();
    let labels: Vec<String> = actions::menu_items(&model)
        .iter()
        .map(actions::menu_label)
        .collect();
    assert_eq!(
        labels,
        [
            "P Play",
            "L Latest",
            "K Hold",
            "H Pause",
            "[ ] Seek",
            "I Interval",
            "Copy CLI command",
        ]
        .map(str::to_owned)
    );
    let lines = actions::overlay_lines(&model);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Enter opens the timeline")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("Enter inspect")),
        "{lines:?}"
    );
}

#[test]
fn menu_rows_and_direct_keys_share_the_recording_confirmation() {
    let mut direct = capturing();
    assert_eq!(direct.handle(Key::Char('P')), Effect::None);
    let mut via = capturing();
    via.handle(Key::Char(' '));
    assert!(via.menu_open());
    assert_eq!(via.handle(Key::Enter), Effect::None);
    assert!(via.menu_open());
    assert_eq!(via.status(), direct.status());
    assert_eq!(via.status(), PLAY_BLOCK);

    same_confirmation('P', Slot::Play, "Play");
    same_confirmation('L', Slot::Latest, "Latest");
    same_confirmation('I', Slot::Interval, "Interval");
    same_confirmation('K', Slot::Hold, "Hold");
    same_effect(Key::Char('H'), Slot::Pause, "Pause", &Effect::HoldRetained);
    same_effect(
        Key::Char('['),
        Slot::Seek,
        "Seek",
        &Effect::SeekRetained { forward: false },
    );
    let mut forward = playable();
    assert_eq!(
        forward.handle(Key::Char(']')),
        Effect::SeekRetained { forward: true }
    );
    let mut open = playable();
    open.handle(Key::Char(' '));
    assert_eq!(open.handle(Key::Char('[')), Effect::None);
    assert!(open.menu_open());
    assert_eq!(open.handle(Key::Char('H')), Effect::HoldRetained);
    assert!(!open.menu_open());
}

#[test]
fn space_opens_on_the_list_and_on_the_timeline() {
    let mut list = playable();
    assert_eq!(list.handle(Key::Char(' ')), Effect::None);
    assert!(list.menu_open());
    assert!(!list.recording_detail());
    assert_eq!(list.handle(Key::Escape), Effect::None);
    assert!(!list.menu_open());
    list.handle(Key::Enter);
    assert!(list.recording_detail());
    assert_eq!(list.handle(Key::Char(' ')), Effect::None);
    assert!(list.menu_open());
    assert!(list.recording_detail());
}

#[test]
fn escape_closes_the_menu_before_it_leaves_the_timeline() {
    let mut model = playable();
    model.handle(Key::Enter);
    model.handle(Key::Char(' '));
    assert!(model.menu_open());
    assert!(model.recording_detail());
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(!model.menu_open());
    assert!(model.recording_detail());
    assert!(!model.at_home());
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(!model.recording_detail());
    assert!(!model.at_home());
    assert_eq!(model.recording_selection(), 0);
}

#[test]
fn reload_keeps_the_open_timeline_and_interval() {
    let mut model = playable();
    assert_eq!(model.handle(Key::Char('.')), Effect::None);
    model.handle(Key::Enter);
    assert!(model.recording_detail());
    assert_eq!(model.interval_index(), 1);
    let kept = model.recordings()[0].clone();
    let mut desk = sample_desk();
    desk.recordings = vec![kept];
    model.apply_desk(desk);
    assert!(model.recording_detail());
    assert_eq!(model.interval_index(), 1);
    assert_eq!(model.recording_selection(), 0);
    let rows = body(&model, 80, 16);
    assert!(rows.contains("Sealed interval 2 of 2"), "{rows}");
}

#[test]
fn a_dropped_selection_stays_on_a_remaining_timeline() {
    let mut model = playable();
    model.handle(Key::Enter);
    assert!(model.recording_detail());
    let mut desk = sample_desk();
    desk.recordings[0].id = "other-rec".into();
    desk.recordings[0].source_revision = "other-rev".into();
    model.apply_desk(desk);
    assert!(model.recording_detail());
    assert_eq!(model.recording_selection(), 0);
    assert_eq!(model.interval_index(), 0);
    let text = frame(&model, 80, 24).0;
    assert!(text.contains(")us"), "{text}");
    assert!(text.contains("Selected recording: other-rec"), "{text}");
    assert_eq!(model.handle(Key::Escape), Effect::None);
    assert!(!model.recording_detail());
    let rows = body(&model, 80, 16);
    assert!(rows.contains("other-rev"), "{rows}");
    assert!(!rows.contains(")us"), "{rows}");
}

#[test]
fn a_long_revision_stays_on_one_row() {
    let mut model = sample();
    let mut desk = sample_desk();
    desk.recordings[0].source_revision = "r".repeat(80);
    desk.recordings[0].state = "completed-and-then-some".into();
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    let text = frame(&model, 80, 24).0;
    let Some(row) = text.lines().find(|line| line.contains("rrrr")) else {
        panic!("revision row missing from {text}");
    };
    assert!(row.chars().count() <= 80, "{row}");
    assert!(row.contains(&"r".repeat(32)), "{row}");
    assert!(!row.contains(&"r".repeat(33)), "{row}");
    assert!(row.contains("completed-and-th"), "{row}");
    assert!(!row.contains("completed-and-the"), "{row}");
}

#[test]
fn a_truncated_row_does_not_invent_a_clock() {
    let mut model = sample();
    let mut desk = sample_desk();
    desk.recordings[0].timeline = crate::explorer::timeline::truncated_fixture();
    desk.recordings[0].source_revision = "rev-cut".into();
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    let rows = body(&model, 80, 16);
    assert!(rows.contains("rev-cut"), "{rows}");
    assert!(rows.contains("unavailable"), "{rows}");
    assert!(!rows.contains("0:10"), "{rows}");
    assert!(!rows.contains("us"), "{rows}");
    assert!(!rows.contains("Timeline unavailable"), "{rows}");
    let text = frame(&model, 80, 24).0;
    assert!(!text.contains(")us"), "{text}");
    assert!(!text.contains("0:10"), "{text}");
}

#[test]
fn a_confirmation_consumes_enter_and_space() {
    let mut model = playable();
    assert_eq!(model.handle(Key::Char('P')), Effect::None);
    assert!(model.status().contains("y confirms"), "{}", model.status());
    assert_eq!(model.handle(Key::Enter), Effect::None);
    assert!(!model.recording_detail());
    assert!(!model.menu_open());
    assert!(model.status().contains("y confirms"), "{}", model.status());
    assert_eq!(model.handle(Key::Char(' ')), Effect::None);
    assert!(!model.menu_open());
    assert!(model.status().contains("y confirms"), "{}", model.status());
}

fn listed(count: usize) -> Explorer {
    let mut model = sample();
    let mut desk = sample_desk();
    let seed = desk.recordings[0].clone();
    desk.recordings = (0..count)
        .map(|index| {
            let mut line = seed.clone();
            line.id = format!("row-{index:02}");
            line.source_revision = format!("rev-{index:02}");
            line.state = if index % 2 == 0 {
                "running".to_owned()
            } else {
                "completed".to_owned()
            };
            line
        })
        .collect();
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    model
}

fn capturing() -> Explorer {
    let mut model = sample();
    model.handle(Key::Char('3'));
    model
}

fn playable() -> Explorer {
    let mut model = sample();
    let mut desk = sample_desk();
    let mut line = desk.recordings[0].clone();
    line.id = "rec-kept".into();
    line.source_revision = "rev-kept".into();
    line.state = "completed".into();
    line.storage_state = "retained".into();
    line.seal = SealMap {
        state: "completed".into(),
        open_ceiling: 0,
        open_object_key: None,
        intervals: vec![
            interval(0, 5_000_000, "a"),
            interval(5_000_000, 9_000_000, "b"),
        ],
        gaps: Vec::new(),
        truncated: false,
    };
    line.hold_total = 1;
    line.shown_holds = vec![HoldSpan {
        ordinal: 1,
        start_us: 0,
        end_us: 5_000_000,
    }];
    desk.recordings = vec![line];
    model.apply_desk(desk);
    model.handle(Key::Char('3'));
    model
}

fn interval(start: u64, end: u64, key: &str) -> RecordingInterval {
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

fn detail_text(linear: bool, monochrome: bool) -> String {
    let mut model = Explorer::new(
        Modes {
            reduced_motion: true,
            linear,
            monochrome,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        10_000,
    );
    model.apply_desk(sample_desk());
    model.place_for_actions(Workspace::Recordings, Focus::Results);
    model.handle(Key::Enter);
    frame(&model, 80, 24).0
}

fn same_confirmation(key: char, slot: Slot, name: &str) {
    let mut direct = playable();
    assert_eq!(direct.handle(Key::Char(key)), Effect::None);
    let mut via = playable();
    via.handle(Key::Char(' '));
    move_menu(&mut via, slot, name);
    assert_eq!(via.handle(Key::Enter), Effect::None);
    assert!(!via.menu_open(), "{name}");
    assert_eq!(via.status(), direct.status(), "{name}");
    assert!(
        via.dialog_copy() == direct.dialog_copy(),
        "{name}: dialog copy differs"
    );
    assert!(
        direct.status().contains("y confirms"),
        "{name} {}",
        direct.status()
    );
}

fn same_effect(key: Key, slot: Slot, name: &str, expected: &Effect) {
    let mut direct = playable();
    assert_eq!(&direct.handle(key), expected, "{name}");
    let mut via = playable();
    via.handle(Key::Char(' '));
    move_menu(&mut via, slot, name);
    assert_eq!(&via.handle(Key::Enter), expected, "{name}");
    assert!(!via.menu_open(), "{name}");
}

fn move_menu(model: &mut Explorer, slot: Slot, name: &str) {
    let Some(index) = actions::menu_items(model)
        .iter()
        .position(|item| item.slot == slot)
    else {
        panic!("missing {name}");
    };
    for _ in 0..index {
        model.handle(Key::Down);
    }
    assert_eq!(model.menu_cursor(), Some(index), "{name}");
}

fn body(model: &Explorer, width: usize, height: usize) -> String {
    super::super::recording_lines(model, width, height)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
