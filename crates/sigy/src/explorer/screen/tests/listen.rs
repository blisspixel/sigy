//! Listen body: now playing, cached favorites, and retained controls.

use super::{frame, sample, sample_desk};
use crate::explorer::state::{EMPTY_LISTEN, Effect, Focus, Key, PlaybackView, Workspace};

#[test]
fn listen_shows_playing_idle_favorites_and_the_empty_sentence() {
    let mut playing = sample();
    playing.place_for_actions(Workspace::Live, Focus::Results);
    let text = frame(&playing, 80, 24).0;
    assert!(text.contains("listen-1"), "{text}");
    assert!(text.contains("elapsed 0:06"), "{text}");
    assert!(text.contains("S stop"), "{text}");
    assert!(!text.contains(EMPTY_LISTEN), "{text}");
    assert!(!text.contains("H pause"), "{text}");

    let mut idle = sample();
    let Some(station) = idle.rows().first().cloned() else {
        panic!("sample station");
    };
    let mut desk = sample_desk();
    desk.playback = None;
    idle.apply_desk(desk);
    idle.install_listen_favorites_for_test(vec![station]);
    idle.place_for_actions(Workspace::Live, Focus::Results);
    let text = frame(&idle, 80, 24).0;
    assert!(text.contains("Nothing playing."), "{text}");
    assert!(text.contains("Favorites"), "{text}");
    assert!(text.contains("KTNN"), "{text}");
    assert!(!text.contains(EMPTY_LISTEN), "{text}");
    assert!(!text.contains("H pause"), "{text}");

    let mut empty = sample();
    let mut desk = sample_desk();
    desk.playback = None;
    empty.apply_desk(desk);
    empty.place_for_actions(Workspace::Live, Focus::Results);
    let text = frame(&empty, 80, 24).0;
    assert!(text.contains(EMPTY_LISTEN), "{text}");
    assert!(!text.contains("Nothing playing."), "{text}");
    assert!(!text.contains("sigy listen file RECORDING"), "{text}");
}

#[test]
fn retained_controls_appear_only_while_a_retained_listen_is_active() {
    let mut model = sample();
    model.place_for_actions(Workspace::Live, Focus::Results);
    let hint = super::super::hint_text(&model, 80);
    assert!(hint.contains("P play"), "{hint}");
    assert!(hint.contains("S stop"), "{hint}");
    assert!(hint.contains("f favorite"), "{hint}");
    assert!(!hint.contains("H pause"), "{hint}");
    assert!(!hint.contains("[/] 10s"), "{hint}");
    let hidden = crate::explorer::actions::overlay_lines(&model);
    assert!(hidden.iter().all(|line| !line.contains("H pause")));
    assert!(hidden.iter().all(|line| !line.contains("[ ] seek")));
    assert_eq!(model.handle(Key::Char('H')), Effect::HoldRetained);
    assert_eq!(
        model.handle(Key::Char('[')),
        Effect::SeekRetained { forward: false }
    );
    assert_eq!(model.handle(Key::Char('S')), Effect::StopListen);

    model.begin_retained("reader-0123456789abcdef0123456789abcdef");
    let (text, _) = frame(&model, 100, 40);
    assert!(text.contains("retained"), "{text}");
    assert!(text.contains("H pause"), "{text}");
    assert!(text.contains("[ ] seek"), "{text}");
    let hint = super::super::hint_text(&model, 120);
    assert!(hint.contains("H pause"), "{hint}");
    assert!(hint.contains("[/] 10s"), "{hint}");
    let shown = crate::explorer::actions::overlay_lines(&model);
    assert!(shown.iter().any(|line| line.contains("H pause")));
    assert!(shown.iter().any(|line| line.contains("[ ] seek")));
    assert_eq!(model.handle(Key::Char('H')), Effect::HoldRetained);
    assert_eq!(
        model.handle(Key::Char(']')),
        Effect::SeekRetained { forward: true }
    );
    assert_eq!(model.handle(Key::Char('S')), Effect::StopRetained);
}

#[test]
fn an_inverted_listen_clock_says_elapsed_is_unknown() {
    let mut model = sample();
    model.place_for_actions(Workspace::Live, Focus::Results);
    model.observe_playback(PlaybackView {
        id: "listen-1".into(),
        state: "running".into(),
        format: Some("wav".into()),
        source_revision: "rev-1".into(),
        failure: None,
        started_ms: 50_000,
        completed_ms: None,
    });
    let text = frame(&model, 80, 24).0;
    assert!(text.contains("elapsed unknown"), "{text}");
    assert!(text.contains("listen-1"), "{text}");
}
