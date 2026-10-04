//! Synthetic presentation observations, not recording or file-availability evidence.

use super::*;
use sigy_service::{control::DirectoryCatalog, discovery::linked::LinkedStationContext};

fn observed_page(
    id: &str,
    catalog: &DirectoryCatalog,
) -> Result<LinkedStationContext, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "provider":"radio_browser","station_id":id,"catalog":catalog,"cached":true,"more_sources":true,
        "sources":[{
            "source":{"revision_id":"station:v1","kind":"http_audio","name":"東京 العربية","origin":"https://radio.example","network":{"kind":"public_internet"},"redirects":"deny","created_ms":5000},
            "registered_station":{"provider":"radio_browser","id":id,"name":"東京 العربية","country":"JP","state":"","languages":["日本語"],"language_codes":["ja"],"tags":[],"codec":"WAV","bitrate_kbps":0,"hls":false,"last_check_ok":null,"latitude":null,"longitude":null,"stream_origin":"https://radio.example","observed_ms":4000,"refresh_id":"fixture"},
            "recordings":[{"id":"retained-original","source_revision":"station:v1","state":"completed","storage_state":"retained","retention":"kept","media_bytes":20,"decoded_microseconds":1_000_000,"retained_segments":true,"released_segments":true,"has_gaps":true}],"more_recordings":true
        }]
    }))
}

fn scroll_text(model: &mut Explorer, width: u16, height: u16) -> String {
    let mut seen = String::new();
    for _ in 0..180 {
        seen.push_str(&frame(model, width, height).0);
        model.handle(Key::Down);
    }
    seen
}

fn assert_compact_phase(model: &Explorer, phase: &str) {
    let text = frame(model, 20, 8).0;
    assert!(text.contains(phase), "{text}");
    assert!(text.contains("Esc back q quit") && text.contains("i links Up/Down"));
    let id = model.selected().map_or("", |row| row.id.as_str());
    assert!(!id.is_empty() && text.replace('\n', "").contains(id));
    assert!(
        text.lines()
            .all(|line| ratatui::text::Line::raw(line).width() <= 20)
    );
}

#[test]
fn compact_linked_feedback_follows_real_request_adoption_failure_and_fencing()
-> Result<(), Box<dyn std::error::Error>> {
    let mut model = sample();
    let catalog = DirectoryCatalog {
        namespace: "a".repeat(32),
        revision: 5,
        comparison: sigy_service::discovery::ordered::COMPARISON.into(),
    };
    let mut desk = sample_desk_from(&model);
    desk.catalog = Some(catalog.clone());
    model.apply_desk(desk);
    model.handle(Key::Enter);
    assert_compact_phase(&model, "links unread");
    let crate::explorer::state::Effect::LinkedContext { generation, id, .. } =
        model.handle(Key::Char('i'))
    else {
        return Err("linked request".into());
    };
    assert_compact_phase(&model, "links pending");
    model.take_dirty();
    assert!(!model.apply_linked_context(
        generation + 1,
        &id,
        &catalog,
        observed_page(&id, &catalog)?
    ));
    assert!(!model.take_dirty());
    assert_compact_phase(&model, "links pending");
    assert!(model.apply_linked_context(generation, &id, &catalog, observed_page(&id, &catalog)?));
    assert_compact_phase(&model, "links loaded");
    let previous = model
        .context
        .as_ref()
        .and_then(|context| context.linked.clone());
    let crate::explorer::state::Effect::LinkedContext { generation, .. } =
        model.handle(Key::Char('i'))
    else {
        return Err("repeat request".into());
    };
    assert_compact_phase(&model, "links pending");
    model.fail_linked_context(generation + 1, "stale failure");
    assert_compact_phase(&model, "links pending");
    model.fail_linked_context(generation, "synthetic read refused");
    assert_compact_phase(&model, "links failed");
    assert_eq!(
        model
            .context
            .as_ref()
            .and_then(|context| context.linked.clone()),
        previous
    );
    let crate::explorer::state::Effect::LinkedContext { generation, .. } =
        model.handle(Key::Char('i'))
    else {
        return Err("third request".into());
    };
    let mut invalid = observed_page(&id, &catalog)?;
    invalid.station_id = "00000000-0000-4000-8000-000000000099".into();
    assert!(!model.apply_linked_context(generation, &id, &catalog, invalid));
    assert_compact_phase(&model, "links failed");
    assert_eq!(
        model
            .context
            .as_ref()
            .and_then(|context| context.linked.clone()),
        previous
    );
    Ok(())
}

#[test]
fn linked_replies_request_redraw_after_consumed_dirty_state_and_stale_replies_are_inert()
-> Result<(), Box<dyn std::error::Error>> {
    let mut model = sample();
    let catalog = DirectoryCatalog {
        namespace: "a".repeat(32),
        revision: 5,
        comparison: sigy_service::discovery::ordered::COMPARISON.into(),
    };
    let mut desk = sample_desk_from(&model);
    desk.catalog = Some(catalog.clone());
    model.apply_desk(desk);
    model.handle(Key::Enter);
    let crate::explorer::state::Effect::LinkedContext { generation, id, .. } =
        model.handle(Key::Char('i'))
    else {
        panic!("read");
    };
    frame(&model, 80, 24);
    assert!(model.take_dirty());
    assert!(!model.take_dirty());
    let before = model.context.clone();
    assert!(!model.apply_linked_context(
        generation + 1,
        &id,
        &catalog,
        observed_page(&id, &catalog)?
    ));
    model.fail_linked_context(generation + 1, "stale failure");
    assert_eq!(model.context, before);
    assert!(!model.take_dirty());
    assert!(model.apply_linked_context(generation, &id, &catalog, observed_page(&id, &catalog)?));
    assert!(model.take_dirty());
    assert!(!model.take_dirty());
    assert!(
        frame(&model, 80, 24)
            .0
            .contains("Exact immutable registrations")
    );
    let crate::explorer::state::Effect::LinkedContext { generation, .. } =
        model.handle(Key::Char('i'))
    else {
        panic!("repeat read");
    };
    assert!(model.take_dirty());
    model.fail_linked_context(generation, "read refused");
    assert!(model.take_dirty());
    assert!(!model.take_dirty());
    assert!(
        frame(&model, 80, 24)
            .0
            .contains("Linked history unavailable")
    );
    assert!(
        model
            .context
            .as_ref()
            .is_some_and(|context| context.linked.is_some())
    );
    Ok(())
}

#[test]
fn linked_context_pending_loaded_and_failed_render_without_losing_native_identity_or_back()
-> Result<(), Box<dyn std::error::Error>> {
    for (width, height) in [(80, 24), (132, 40)] {
        let mut model = sample();
        let mut desk = sample_desk_from(&model);
        let catalog = DirectoryCatalog {
            namespace: "a".repeat(32),
            revision: 5,
            comparison: sigy_service::discovery::ordered::COMPARISON.into(),
        };
        desk.catalog = Some(catalog.clone());
        model.apply_desk(desk);
        let before = model.current_search();
        let identity = model.selected().ok_or("station")?.id.clone();
        assert_eq!(
            model.handle(Key::Enter),
            crate::explorer::state::Effect::None
        );
        let crate::explorer::state::Effect::LinkedContext { generation, .. } =
            model.handle(Key::Char('i'))
        else {
            panic!("explicit read");
        };
        requested_snapshot(
            &model,
            &format!("linked-pending-{width}x{height}.json"),
            width,
            height,
        )?;
        assert!(frame(&model, width, height).0.contains("pending"));
        assert!(model.apply_linked_context(
            generation,
            &identity,
            &catalog,
            observed_page(&identity, &catalog)?
        ));
        let seen = scroll_text(&mut model, width, height);
        for expected in [
            "東京",
            "العربية",
            "Catalog sealed media",
            "gaps true",
            "File availability was not probed",
            "More recordings",
            "More registrations",
        ] {
            assert!(
                seen.contains(expected),
                "missing {expected} at{width}x{height}"
            );
        }
        for _ in 0..180 {
            model.handle(Key::Up);
        }
        for _ in 0..25 {
            if frame(&model, width, height).0.contains("registered name") {
                break;
            }
            model.handle(Key::Down);
        }
        requested_snapshot(
            &model,
            &format!("linked-loaded-{width}x{height}.json"),
            width,
            height,
        )?;
        let crate::explorer::state::Effect::LinkedContext { generation, .. } =
            model.handle(Key::Char('i'))
        else {
            panic!("refresh read");
        };
        model.fail_linked_context(generation, "catalog changed");
        for _ in 0..180 {
            model.handle(Key::Up);
        }
        requested_snapshot(
            &model,
            &format!("linked-failed-{width}x{height}.json"),
            width,
            height,
        )?;
        let seen = scroll_text(&mut model, width, height);
        assert!(
            seen.contains("unavailable")
                && seen.contains("Previous observation kept")
                && seen.contains("東京")
        );
        model.handle(Key::Escape);
        assert_eq!(model.current_search(), before);
        assert_eq!(
            model.selected().map(|row| row.id.as_str()),
            Some(identity.as_str())
        );
    }
    Ok(())
}
