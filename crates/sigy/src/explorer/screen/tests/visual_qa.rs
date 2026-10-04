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
        64
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
        "monochrome",
    ] {
        assert!(cases.iter().any(|(name, _, _)| *name == expected));
    }
    Ok(())
}

#[test]
fn essential_hints_fit_production_cells_at_minimum_and_normal_widths() {
    for workspace in Workspace::ALL {
        let mut model = sample();
        model.handle(Key::Char(workspace.digit()));
        let hint = super::super::compact_help(&model, 20);
        assert!(ratatui::text::Line::raw(hint).width() <= 20);
        assert!(frame(&model, 20, 8).0.contains(hint));
    }
    let mut globe = sample();
    globe.handle(Key::Char('7'));
    let text = frame(&globe, 80, 24).0;
    let help = text
        .lines()
        .find(|line| line.starts_with("Help:"))
        .unwrap_or("");
    assert!(help.ends_with("q quit"));
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
            manifest.push(serde_json::json!({"name":name,"cols":width,"rows":height,"json":format!("{stem}.json"),"json_sha256":json_hash,"svg":format!("{stem}.svg"),"svg_sha256":svg_hash,"synthetic":true,"workspace":model.workspace().label(),"linear":model.modes().linear,"monochrome":model.modes().monochrome}));
        }
    }
    assert!(manifest.len() <= 64);
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"format":1,"method":"production Ratatui TestBackend","synthetic":true,"source_hashes":{"renderer":sha256(include_bytes!("../../screen.rs")),"state":sha256(include_bytes!("../../state.rs")),"monitor":sha256(include_bytes!("../../monitor.rs")),"context":sha256(include_bytes!("../../context.rs")),"context_state":sha256(include_bytes!("../../state/context.rs")),"context_renderer":sha256(include_bytes!("../context.rs")),"gallery":sha256(include_bytes!("visual_qa.rs"))},"frames":manifest}),
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
            },
            10_000,
        );
        model.apply_desk(sample_desk());
        cases.push((name, model, vec![(80, 24), (132, 40)]));
    }
    Ok(cases)
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
