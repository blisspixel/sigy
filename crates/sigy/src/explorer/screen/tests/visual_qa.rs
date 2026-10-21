//! Explicit, offline production-render gallery. All observations are synthetic.

use super::*;
use ratatui::style::Modifier;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const PALETTE: [&str; 16] = [
    "#0c0f12", "#c50f1f", "#13a10e", "#c19c00", "#0037da", "#881798", "#3a96dd", "#cccccc",
    "#87919d", "#e74856", "#16c60c", "#f9f1a5", "#3b78ff", "#b4009e", "#61d6d6", "#ffffff",
];

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn spin_visibility_tracks_canvas_and_modal_ownership() {
    use ratatui::layout::Rect;
    let mut model = sample();
    let area = Rect::new(0, 0, 80, 24);
    assert!(!super::super::globe_visible(&model, area));
    assert!(super::super::globe_flourish_canvas(&model, area).is_none());
    model.handle(Key::Char('7'));
    assert!(super::super::globe_visible(&model, area));
    let shown = super::super::globe_flourish_canvas(&model, area);
    assert!(
        shown.is_some_and(|(width, height)| width >= 20 && height >= 8),
        "{shown:?}"
    );
    model.handle(Key::Char('?'));
    assert!(!super::super::globe_visible(&model, area));
    assert!(super::super::globe_flourish_canvas(&model, area).is_none());
    model.handle(Key::Escape);
    assert!(super::super::globe_visible(&model, area));
    for (width, height) in [(20, 8), (59, 24), (80, 15)] {
        assert!(!super::super::globe_visible(
            &model,
            Rect::new(0, 0, width, height)
        ));
    }
    for key in [Key::Char('F'), Key::Char('C'), Key::Enter] {
        let mut overlay = model.clone();
        overlay.handle(key);
        assert!(!super::super::globe_visible(&overlay, area));
        overlay.handle(Key::Escape);
        // The country picker returns to its draft filter editor first.
        overlay.handle(Key::Escape);
        assert!(super::super::globe_visible(&overlay, area));
    }
}

#[test]
#[ignore = "explicit offline rotating globe render measurement"]
fn spin_render_measurement() -> Result<()> {
    use std::time::{Duration, Instant};
    let directory = output_directory()?.join("spin");
    std::fs::create_dir_all(&directory)?;
    for (width, height) in [(80, 24), (200, 60)] {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: false,
                linear: false,
                monochrome: false,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            10_000,
        );
        let mut desk = sample_desk();
        desk.stations[0].coordinates = Some(Coordinates {
            longitude: -109.05,
            latitude: 35.68,
        });
        model.apply_desk(desk);
        let now = Instant::now();
        model.configure_motion(true, false, now);
        model.handle(Key::Char('7'));
        model.handle(Key::Char('c'));
        model.handle(Key::Char('r'));
        model.advance_spin(now, true);
        let mut samples = Vec::new();
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        for tick in 1..=20 {
            model.advance_spin(now + Duration::from_millis(tick * 100), true);
            let start = Instant::now();
            terminal.draw(|frame| render(frame, &model))?;
            samples.push(start.elapsed().as_micros());
            assert!(
                terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .any(|cell| cell.symbol() == "@")
            );
        }
        let mut ordered = samples.clone();
        ordered.sort_unstable();
        let report = serde_json::json!({
            "cols":width,"rows":height,"samples_us":samples,"p95_us":ordered[18],
            "maximum_us":ordered[19],"frames":model.animation_frames(),
            "scope":"full color production render with TestBackend draw/diff; terminal I/O excluded"
        });
        std::fs::write(
            directory.join(format!("{width}x{height}.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        std::fs::write(
            directory.join(format!("{width}x{height}.svg")),
            svg(&model, width, height)?,
        )?;
        println!("{report}");
    }
    Ok(())
}

#[test]
fn svg_escapes_text_and_preserves_rgb_and_indexed_colors() {
    assert_eq!(escape("<&\"'東京"), "&lt;&amp;&quot;&#39;東京");
    assert_eq!(color(Color::Rgb(1, 2, 255), false), "#0102ff");
    assert_eq!(color(Color::Indexed(16), false), "#000000");
    assert_eq!(color(Color::Indexed(231), false), "#ffffff");
    assert_eq!(color(Color::Indexed(232), false), "#080808");
    assert_eq!(color(Color::Indexed(255), false), "#eeeeee");
}

#[test]
fn svg_cell_styles_preserve_rgb_inversion_and_underline() -> Result<()> {
    let mut cells = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 1, 1));
    cells[(0, 0)]
        .set_symbol("&")
        .set_fg(Color::Rgb(1, 2, 255))
        .set_bg(Color::Indexed(232));
    cells[(0, 0)].modifier = Modifier::REVERSED | Modifier::UNDERLINED;
    let output = svg_cells(&cells, 1, 1)?;
    assert!(output.contains("height=\"20\" fill=\"#0102ff\""));
    assert!(output.contains("fill=\"#080808\" font-weight"));
    assert!(output.contains("text-decoration=\"underline\">&amp;</text>"));
    Ok(())
}

#[test]
fn svg_renders_untrusted_production_cells_with_bounded_structure() -> Result<()> {
    let mut model = sample();
    let mut desk = sample_desk();
    desk.stations[0].name = "<&\"'東京".into();
    model.apply_desk(desk);
    let output = svg(&model, 80, 24)?;
    assert!(
        output.contains("&lt;")
            && output.contains("&amp;")
            && output.contains("&quot;")
            && output.contains("&#39;")
    );
    assert!(!output.contains("<&\"'"));
    assert!(output.contains("width=\"824\" height=\"504\""));
    assert!(output.matches("<rect ").count() <= 1 + 80 * 24);
    assert!(output.matches("<text ").count() <= 80 * 24);
    assert!(output.contains("fill=\"#d9dfe7\""));
    Ok(())
}

#[test]
fn visual_case_matrix_covers_real_workspaces_and_modals_within_cap() -> Result<()> {
    let cases = cases()?;
    assert_eq!(
        cases.iter().map(|(_, _, sizes)| sizes.len()).sum::<usize>(),
        78
    );
    for workspace in Workspace::ALL {
        assert!(
            cases.iter().any(
                |(name, model, _)| *name == workspace.label() && model.workspace() == workspace
            )
        );
    }
    let finding = cases
        .iter()
        .find(|(name, _, _)| *name == "Findings")
        .ok_or("finding fixture")?;
    assert_eq!(finding.1.findings.query(), "world-news one");
    assert!(!finding.1.findings.editing());
    for expected in [
        "filters",
        "filters-invalid",
        "countries",
        "context",
        "linked-pending",
        "linked-loaded",
        "linked-failed",
        "monitor-detail",
        "empty",
        "unknown",
        "disconnected",
        "search-pending",
        "catalog-stale",
        "linear",
        "linear-color",
        "high-contrast",
        "monochrome",
        "random-picked",
        "random-empty",
        "random-monochrome",
        "globe-turn",
    ] {
        assert!(cases.iter().any(|(name, _, _)| *name == expected));
    }
    Ok(())
}

#[test]
fn gallery_accent_stays_on_brand_workspace_and_focused_filters() -> Result<()> {
    for (name, model, sizes) in cases()? {
        if model.modes().monochrome || model.modes().emphasis.high() {
            continue;
        }
        let Some(&(width, height)) = sizes
            .iter()
            .find(|(width, height)| *width >= 80 && *height >= 24)
        else {
            continue;
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        terminal.draw(|ui| render(ui, &model))?;
        let buffer = terminal.backend().buffer().clone();
        for y in 0..height {
            let mut text = String::new();
            let mut accent = false;
            for x in 0..width {
                let cell = &buffer[(x, y)];
                text.push_str(cell.symbol());
                if cell.fg == Color::Cyan {
                    accent = true;
                }
                assert!(
                    !(matches!(cell.symbol(), "★" | "*") && cell.fg == Color::Yellow),
                    "{name} paints a favorite with attention"
                );
            }
            if !accent {
                continue;
            }
            let workspace = text.contains("[1 ")
                || text.contains("[2 ")
                || text.contains("[3 ")
                || text.contains("[4 ")
                || text.contains("[5 ")
                || text.contains("[6 ")
                || text.contains("[7 ");
            let focused_editor = name.starts_with("filters") || name == "countries";
            // The globe selected marker uses the accent slot. It is the `@` on the map.
            let selected_marker = text.contains('@');
            assert!(
                text.contains("Sigy") || workspace || focused_editor || selected_marker,
                "{name} accent outside brand, workspace, focused filters, or the selected map marker: {text}"
            );
            assert!(
                !text.contains("Help:"),
                "{name} colors the help line: {text}"
            );
        }
    }
    Ok(())
}

/// Official Catppuccin v1.8.0 hex, used only to review the locked ANSI slots.
/// High contrast uses the theme default foreground. These ratios record the slots
/// that miss 4.5:1 text contrast or 3:1 marker contrast on an example palette.
#[test]
fn example_palettes_keep_the_reviewed_contrast() {
    let latte_base = "#eff1f5";
    let mocha_base = "#1e1e2e";
    let pairs = [
        ("latte text", "#4c4f69", latte_base, 7.06, 4.5),
        ("mocha text", "#cdd6f4", mocha_base, 11.34, 4.5),
        ("latte fail", "#d20f39", latte_base, 4.80, 4.5),
        ("mocha fail", "#f38ba8", mocha_base, 7.08, 4.5),
        ("latte favorite", "#456eff", latte_base, 3.77, 3.0),
        ("mocha favorite", "#74a8fc", mocha_base, 6.83, 3.0),
        ("latte accent", "#179299", latte_base, 3.31, 3.0),
        ("mocha accent", "#94e2d5", mocha_base, 11.01, 3.0),
        ("latte record", "#de293e", latte_base, 4.13, 3.0),
        ("mocha record", "#f37799", mocha_base, 6.18, 3.0),
        ("mocha live", "#a6e3a1", mocha_base, 11.03, 4.5),
        ("mocha attention", "#f9e2af", mocha_base, 12.91, 4.5),
    ];
    for (name, foreground, background, expected, floor) in pairs {
        let ratio = contrast(foreground, background);
        assert!(
            (ratio - expected).abs() < 0.02,
            "{name} measured {ratio:.2}, reviewed {expected:.2}"
        );
        assert!(ratio + 0.001 >= floor, "{name} {ratio:.2} is below {floor}");
    }
    let misses = [
        ("latte live", "#40a02b", latte_base, 2.96),
        ("latte attention", "#df8e1d", latte_base, 2.31),
        ("mocha muted", "#585b70", mocha_base, 2.46),
        ("mocha rim", "#585b70", mocha_base, 2.46),
    ];
    for (name, foreground, background, expected) in misses {
        let ratio = contrast(foreground, background);
        assert!(
            (ratio - expected).abs() < 0.02,
            "{name} measured {ratio:.2}, reviewed {expected:.2}"
        );
        assert!(
            ratio < 3.0,
            "{name} was expected to miss 3.0 and measured {ratio:.2}"
        );
    }
}

fn contrast(foreground: &str, background: &str) -> f64 {
    let (lighter, darker) = {
        let left = luminance(foreground);
        let right = luminance(background);
        if left >= right {
            (left, right)
        } else {
            (right, left)
        }
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn luminance(hex: &str) -> f64 {
    let bytes = hex_channel(hex);
    let channel = |value: u8| {
        let scaled = f64::from(value) / 255.0;
        if scaled <= 0.04045 {
            scaled / 12.92
        } else {
            ((scaled + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(bytes[0]) + 0.7152 * channel(bytes[1]) + 0.0722 * channel(bytes[2])
}

fn hex_channel(hex: &str) -> [u8; 3] {
    let hex = hex.trim_start_matches('#');
    let value = u32::from_str_radix(hex, 16).unwrap_or(0);
    [
        u8::try_from((value >> 16) & 0xff).unwrap_or(0),
        u8::try_from((value >> 8) & 0xff).unwrap_or(0),
        u8::try_from(value & 0xff).unwrap_or(0),
    ]
}

#[test]
fn essential_hints_fit_production_cells_at_minimum_and_normal_widths() {
    for workspace in Workspace::ALL {
        let mut model = sample();
        model.handle(Key::Char(workspace.digit()));
        let hint = super::super::compact_help(&model, 20);
        assert!(ratatui::text::Line::raw(hint.clone()).width() <= 20);
        assert!(frame(&model, 20, 8).0.contains(&hint));
    }
    let mut globe = sample();
    globe.handle(Key::Char('7'));
    let text = frame(&globe, 80, 24).0;
    let help = text
        .lines()
        .find(|line| line.starts_with("Help:"))
        .unwrap_or("");
    assert!(help.ends_with("? more"));
    assert!(ratatui::text::Line::raw(help).width() <= 80);
    let mut context = catalog_sample();
    context.handle(Key::Enter);
    let text = frame(&context, 20, 8).0;
    assert!(text.contains("Esc back q quit"));
    assert!(text.contains("i links Up/Down"));
    context.note_cursor_changed();
    let effect = context.handle(Key::Char('i'));
    assert!(matches!(
        effect,
        crate::explorer::state::Effect::LinkedContext { .. }
    ));
    let changed = frame(&context, 20, 8).0;
    assert!(changed.contains("Sigy | changed") && changed.contains("links pending"));
    assert!(changed.contains("i links Up/Down") && changed.contains("Esc back q quit"));
    let before = context.current_search();
    context.handle(Key::Down);
    context.handle(Key::Escape);
    assert_eq!(context.current_search(), before);
}

#[test]
fn clock_ticks_change_visible_age_without_rewriting_observations() {
    let mut model = sample();
    let rows = model.rows().to_vec();
    model.take_dirty();
    model.tick_clock(10_900);
    assert!(!model.take_dirty());
    model.tick_clock(11_000);
    assert!(model.take_dirty());
    assert_eq!(model.snapshot_age(), "1s");
    assert_eq!(model.rows(), rows);
    model.handle(Key::Enter);
    let context = model.context.clone();
    model.tick_clock(70_000);
    assert_eq!(model.context, context);
    assert_eq!(model.snapshot_age(), "1m");
    model.tick_clock(9_000);
    assert_eq!(model.snapshot_age(), "unknown");
}

#[test]
fn unknown_cache_is_distinct_from_observed_empty_cache() -> Result<()> {
    let mut model = Explorer::new(sample().modes(), 10_000);
    let unknown = frame(&model, 80, 24).0;
    assert!(unknown.contains("coverage has not been observed"));
    assert!(!unknown.contains("No stations are cached yet"));
    model.note_disconnect(11_000, "initial read failed");
    assert!(
        frame(&model, 80, 24)
            .0
            .contains("coverage has not been observed")
    );
    let mut desk = sample_desk();
    desk.stations.clear();
    let unobserved = desk.directory.as_mut().ok_or("fixture directory")?;
    unobserved.cached_stations = 0;
    unobserved.maximum_stations = 0;
    model.apply_desk(desk.clone());
    assert!(
        frame(&model, 80, 24)
            .0
            .contains("coverage has not been observed")
    );
    desk.directory
        .as_mut()
        .ok_or("fixture directory")?
        .maximum_stations = 10_000;
    desk.stations.clear();
    desk.directory
        .as_mut()
        .ok_or("fixture directory")?
        .cached_stations = 0;
    model.apply_desk(desk);
    assert!(
        frame(&model, 80, 24)
            .0
            .contains("No stations are cached yet")
    );
    Ok(())
}

#[test]
fn production_monitor_passage_suffix_remains_reachable_in_screen_scroll() {
    for (width, height) in [(20, 8), (80, 24), (132, 40)] {
        let mut model = sample();
        model.handle(Key::Char('4'));
        model.monitors = crate::explorer::monitor::passage_fixture();
        let mut seen = String::new();
        for _ in 0..160 {
            let text = frame(&model, width, height).0;
            // Remove only row separators to compare wrapped original graphemes.
            seen.push_str(&text.replace('\n', ""));
            model.handle(Key::Down);
        }
        assert!(seen.contains("FINAL-SUFFIX"), "{width}x{height}");
        assert!(seen.contains("Cafe\u{301}"), "{width}x{height}");
        assert!(!seen.contains("preview clipped"));
    }
}

#[test]
fn gallery_monitor_navigation_places_original_in_each_actual_viewport() -> Result<()> {
    let mut original = gallery_sample();
    original.handle(Key::Char('4'));
    original.monitors = crate::explorer::monitor::passage_fixture();
    let before = original.clone();
    for (width, height) in [(20, 8), (80, 24), (132, 40)] {
        let model = case_at_size("monitor-detail", &original, width, height)?;
        assert!(frame(&model, width, height).0.contains("Original:"));
    }
    assert_eq!(original, before);
    Ok(())
}

#[test]
#[ignore = "explicit offline visual artifact generation"]
fn visual_qa_gallery() -> Result<()> {
    let directory = output_directory()?;
    let cases = cases()?;
    let mut manifest = Vec::new();
    let mut html = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>Sigy visual QA</title><style>body{background:#16191d;color:#ddd;font:16px system-ui}img{max-width:100%;border:1px solid #555}section{margin:2rem 0}a{color:#8dc8ff}</style><h1>Sigy production-render visual QA</h1><p>Synthetic offline fixtures. No station contact, audio, device, library or service is opened. SVG is a font-dependent approximation of terminal cells, not a terminal qualification.</p>",
    );
    for (name, model, sizes) in cases {
        for (width, height) in sizes {
            let model = case_at_size(name, &model, width, height)?;
            let stem = format!("{name}-{width}x{height}");
            write_snapshot(
                &directory.join(format!("{stem}.json")),
                &model,
                width,
                height,
            )?;
            let json_path = directory.join(format!("{stem}.json"));
            if std::fs::metadata(&json_path)?.len() > 2 * 1024 * 1024 {
                return Err("visual frame byte cap exceeded".into());
            }
            let json_hash = sha256(&std::fs::read(json_path)?);
            let svg = svg(&model, width, height)?;
            let svg_hash = sha256(svg.as_bytes());
            std::fs::write(directory.join(format!("{stem}.svg")), svg)?;
            write!(
                html,
                "<section><h2>{stem}</h2><a href=\"{stem}.json\">Cell JSON</a><br><img alt=\"{}\" src=\"{stem}.svg\"></section>",
                escape(&stem)
            )?;
            manifest.push(serde_json::json!({"name":name,"cols":width,"rows":height,"json":format!("{stem}.json"),"json_sha256":json_hash,"svg":format!("{stem}.svg"),"svg_sha256":svg_hash,"synthetic":true,"workspace":model.workspace().label(),"linear":model.modes().linear,"monochrome":model.modes().monochrome,"high_contrast":model.modes().emphasis.high()}));
        }
    }
    assert!(manifest.len() <= 78);
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"format":1,"method":"production Ratatui TestBackend","synthetic":true,"source_hashes":{"renderer":sha256(include_bytes!("../../screen.rs")),"state":sha256(include_bytes!("../../state.rs")),"monitor":sha256(include_bytes!("../../monitor.rs")),"context":sha256(include_bytes!("../../context.rs")),"context_state":sha256(include_bytes!("../../state/context.rs")),"context_renderer":sha256(include_bytes!("../context.rs")),"globe":sha256(include_bytes!("../../globe.rs")),"spin":sha256(include_bytes!("../../state/spin.rs")),"gallery":sha256(include_bytes!("visual_qa.rs"))},"frames":manifest}),
        )?,
    )?;
    std::fs::write(directory.join("index.html"), html)?;
    println!(
        "Visual QA gallery: {}",
        directory.join("index.html").display()
    );
    Ok(())
}

type Case = (&'static str, Explorer, Vec<(u16, u16)>);

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    use sha2::Digest;
    let mut output = String::with_capacity(64);
    for byte in &sha2::Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(*byte >> 4)]));
        output.push(char::from(HEX[usize::from(*byte & 15)]));
    }
    output
}

fn cases() -> Result<Vec<Case>> {
    let sizes = vec![(20, 8), (80, 24), (132, 40)];
    let mut cases = Vec::new();
    for workspace in Workspace::ALL {
        let mut model = gallery_sample();
        model.handle(Key::Char(workspace.digit()));
        if workspace == Workspace::Monitors {
            model.monitors.load_ids(vec!["news".into(), "夜間".into()]);
        }
        if workspace == Workspace::Findings {
            let page = crate::explorer::finding::fixture();
            model.handle(Key::Char('/'));
            model.handle(Key::Paste("world-news one".into()));
            model.handle(Key::Enter);
            if !model.findings.load("world-news", "one", &page) {
                return Err("fixture finding identity".into());
            }
        }
        cases.push((workspace.label(), model, sizes.clone()));
    }
    let mut empty = gallery_sample();
    let mut desk = sample_desk();
    desk.stations.clear();
    desk.directory
        .as_mut()
        .ok_or("fixture directory")?
        .cached_stations = 0;
    empty.apply_desk(desk);
    cases.push(("empty", empty, sizes.clone()));
    cases.push((
        "unknown",
        Explorer::new(gallery_sample().modes(), 10_000),
        sizes.clone(),
    ));
    let mut offline = gallery_sample();
    offline.note_disconnect(20_000, "synthetic unavailable service");
    cases.push(("disconnected", offline, sizes.clone()));
    let mut filters = gallery_sample();
    filters.handle(Key::Char('F'));
    cases.push(("filters", filters.clone(), sizes.clone()));
    filters.handle(Key::Down);
    filters.handle(Key::Paste("not-a-country".into()));
    filters.handle(Key::Enter);
    cases.push(("filters-invalid", filters, sizes.clone()));
    let mut monitor = gallery_sample();
    monitor.handle(Key::Char('4'));
    monitor.monitors = crate::explorer::monitor::passage_fixture();
    // The fixture is a loaded detail; ordinary navigation witnesses qualify scroll.
    cases.push(("monitor-detail", monitor, sizes.clone()));
    let mut countries = gallery_sample();
    countries.handle(Key::Char('C'));
    countries.handle(Key::Paste("日本".into()));
    cases.push(("countries", countries, sizes.clone()));
    append_context_cases(&mut cases, &sizes)?;
    for (name, linear) in [("monochrome", false), ("linear", true)] {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear,
                monochrome: true,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            10_000,
        );
        model.apply_desk(sample_desk());
        cases.push((name, model, vec![(80, 24), (132, 40)]));
    }
    let mut linear_color = Explorer::new(
        Modes {
            reduced_motion: true,
            linear: true,
            monochrome: false,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        10_000,
    );
    linear_color.apply_desk(sample_desk());
    cases.push(("linear-color", linear_color, vec![(80, 24), (132, 40)]));
    let mut high_contrast = Explorer::new(
        Modes {
            reduced_motion: true,
            linear: false,
            monochrome: false,
            emphasis: crate::explorer::state::Emphasis::HighContrast,
        },
        10_000,
    );
    high_contrast.apply_desk(sample_desk());
    cases.push(("high-contrast", high_contrast, vec![(80, 24), (132, 40)]));
    append_random_cases(&mut cases)?;
    let mut keys = gallery_sample();
    keys.handle(Key::Char('?'));
    cases.push(("keys", keys, vec![(80, 24)]));
    cases.push(("layout", gallery_sample(), vec![(60, 20), (90, 30)]));
    append_globe_turn(&mut cases);
    Ok(cases)
}

fn append_globe_turn(cases: &mut Vec<Case>) {
    let mut turn = Explorer::new(
        Modes {
            reduced_motion: false,
            linear: false,
            monochrome: false,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        10_000,
    );
    turn.apply_desk(sample_desk());
    let now = std::time::Instant::now();
    turn.configure_motion(true, false, now);
    turn.handle(Key::Char('7'));
    turn.handle(Key::Char('l'));
    turn.advance_spin(now + std::time::Duration::from_millis(200), true);
    cases.push(("globe-turn", turn, vec![(80, 24)]));
}

fn append_random_cases(cases: &mut Vec<Case>) -> Result<()> {
    use crate::explorer::{client, state::Effect};
    for (name, empty, monochrome, sizes) in [
        (
            "random-picked",
            false,
            false,
            vec![(20, 8), (80, 24), (132, 40)],
        ),
        ("random-empty", true, false, vec![(80, 24)]),
        ("random-monochrome", false, true, vec![(80, 24), (132, 40)]),
    ] {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            1000,
        );
        model.apply_desk(sample_desk());
        let Effect::ExploreRandom(query) = model.handle(Key::Char('e')) else {
            return Err("random fixture request".into());
        };
        let mut result = client::random_tests::result()?;
        if empty {
            result.station = None;
            result.eligible_count = 0;
        }
        if !client::adopt_random(
            &mut model,
            &query,
            Ok((false, client::random_tests::snapshot(&result)?)),
        ) {
            return Err("random fixture result".into());
        }
        cases.push((name, model, sizes));
    }
    Ok(())
}

fn case_at_size(name: &str, original: &Explorer, width: u16, height: u16) -> Result<Explorer> {
    let mut model = original.clone();
    if name == "monitor-detail" {
        for _ in 0..256 {
            // Actual rendering establishes the viewport used by keyboard scrolling.
            frame(&model, width, height);
            if model
                .monitors
                .lines(1)
                .first()
                .is_some_and(|line| line.contains("Original:"))
            {
                return Ok(model);
            }
            model.handle(Key::Down);
        }
        return Err("monitor passage not reachable in visual fixture".into());
    }
    Ok(model)
}

fn append_context_cases(cases: &mut Vec<Case>, sizes: &[(u16, u16)]) -> Result<()> {
    let mut context = catalog_sample();
    context.handle(Key::Enter);
    cases.push(("context", context.clone(), sizes.to_vec()));
    let crate::explorer::state::Effect::LinkedContext {
        generation,
        id,
        catalog,
    } = context.handle(Key::Char('i'))
    else {
        return Err("fixture linked request".into());
    };
    cases.push(("linked-pending", context.clone(), sizes.to_vec()));
    let mut failed = context.clone();
    failed.fail_linked_context(generation, "synthetic read refused");
    cases.push(("linked-failed", failed, sizes.to_vec()));
    let page = linked_page(&id, &catalog)?;
    if !context.apply_linked_context(generation, &id, &catalog, page) {
        return Err("fixture linked identity".into());
    }
    cases.push(("linked-loaded", context, sizes.to_vec()));
    let mut pending = catalog_sample();
    pending.handle(Key::Char('/'));
    pending.handle(Key::Paste("東京".into()));
    pending.handle(Key::Enter);
    cases.push(("search-pending", pending, sizes.to_vec()));
    let mut stale = catalog_sample();
    stale.note_cursor_changed();
    cases.push(("catalog-stale", stale, sizes.to_vec()));
    Ok(())
}

fn catalog_sample() -> Explorer {
    let mut model = gallery_sample();
    let mut desk = sample_desk();
    desk.catalog = Some(sigy_service::control::DirectoryCatalog {
        namespace: "a".repeat(32),
        revision: 1,
        comparison: sigy_service::discovery::ordered::COMPARISON.into(),
    });
    model.apply_desk(desk);
    model
}

fn gallery_sample() -> Explorer {
    let mut model = Explorer::new(
        Modes {
            reduced_motion: true,
            linear: false,
            monochrome: false,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        10_000,
    );
    let mut desk = sample_desk();
    desk.schema_version = sigy_service::storage::SCHEMA_VERSION;
    desk.stations[0].coordinates = Some(Coordinates {
        longitude: -109.05,
        latitude: 35.68,
    });
    model.apply_desk(desk);
    model
}

fn linked_page(
    id: &str,
    catalog: &sigy_service::control::DirectoryCatalog,
) -> Result<sigy_service::discovery::linked::LinkedStationContext> {
    Ok(serde_json::from_value(serde_json::json!({
        "provider":"radio_browser","station_id":id,"catalog":catalog,"cached":true,"more_sources":true,
        "sources":[{"source":{"revision_id":"station:v1","kind":"http_audio","name":"東京 العربية","origin":"https://radio.example","network":{"kind":"public_internet"},"redirects":"deny","created_ms":5000},
        "registered_station":{"provider":"radio_browser","id":id,"name":"東京 العربية","country":"JP","state":"","languages":["日本語"],"language_codes":["ja"],"tags":[],"codec":"WAV","bitrate_kbps":0,"hls":false,"last_check_ok":null,"latitude":null,"longitude":null,"stream_origin":"https://radio.example","observed_ms":4000,"refresh_id":"fixture"},
        "recordings":[{"id":"original","source_revision":"station:v1","state":"completed","storage_state":"retained","retention":"kept","media_bytes":20,"decoded_microseconds":1_000_000,"retained_segments":true,"released_segments":true,"has_gaps":true}],"more_recordings":true}]
    }))?)
}

fn output_directory() -> Result<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/visual-qa");
    std::fs::create_dir_all(&root)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let directory = root.join(format!("{stamp}-{}", std::process::id()));
    std::fs::create_dir(&directory)?;
    Ok(directory)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn svg(model: &Explorer, width: u16, height: u16) -> Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| render(frame, model))?;
    svg_cells(terminal.backend().buffer(), width, height)
}

fn svg_cells(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) -> Result<String> {
    let mut output = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"#0c0f12\"/><g font-family=\"Cascadia Mono,DejaVu Sans Mono,monospace\" font-size=\"15\">",
        u32::from(width) * 10 + 24,
        u32::from(height) * 20 + 24,
        u32::from(width) * 10 + 24,
        u32::from(height) * 20 + 24
    );
    for y in 0..height {
        let mut until = 0;
        for x in 0..width {
            if x < until {
                continue;
            }
            let cell = &buffer[(x, y)];
            let span = ratatui::text::Span::raw(cell.symbol()).width().max(1);
            until = x.saturating_add(u16::try_from(span)?);
            let mut fg = color(cell.fg, false);
            let mut bg = color(cell.bg, true);
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let px = u32::from(x) * 10 + 12;
            let py = u32::from(y) * 20 + 12;
            write!(
                output,
                "<rect x=\"{px}\" y=\"{py}\" width=\"{}\" height=\"20\" fill=\"{bg}\"/>",
                span * 10
            )?;
            if !cell.modifier.contains(Modifier::HIDDEN) {
                write!(
                    output,
                    "<text x=\"{px}\" y=\"{}\" fill=\"{fg}\" font-weight=\"{}\" text-decoration=\"{}\">{}</text>",
                    py + 15,
                    if cell.modifier.contains(Modifier::BOLD) {
                        "bold"
                    } else {
                        "normal"
                    },
                    if cell.modifier.contains(Modifier::UNDERLINED) {
                        "underline"
                    } else {
                        "none"
                    },
                    escape(cell.symbol())
                )?;
            }
        }
    }
    output.push_str("</g></svg>");
    Ok(output)
}

fn color(color: Color, background: bool) -> String {
    let index = match color {
        Color::Reset => return if background { "#0c0f12" } else { "#d9dfe7" }.into(),
        Color::Rgb(r, g, b) => return format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(index) => index,
    };
    if index < 16 {
        return PALETTE[usize::from(index)].into();
    }
    if index >= 232 {
        let gray = 8 + 10 * (index - 232);
        return format!("#{gray:02x}{gray:02x}{gray:02x}");
    }
    let i = index - 16;
    let levels = [0, 95, 135, 175, 215, 255];
    format!(
        "#{:02x}{:02x}{:02x}",
        levels[usize::from(i / 36)],
        levels[usize::from(i / 6 % 6)],
        levels[usize::from(i % 6)]
    )
}
