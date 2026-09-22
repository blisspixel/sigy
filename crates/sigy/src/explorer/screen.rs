//! List layout. Reduced motion, linear order, and monochrome draw no animation.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;

use crate::explorer::state::{Explorer, Focus, Health, Workspace};
use crate::explorer::text::{age_label, known, sanitize};

pub const HELP_PRIMARY: &str = "Help: / search, arrows select, v filter, f favorite, q quit";
pub const HELP_SELECTION: &str = "Selection does not start audio, capture, refresh, or a click.";
pub const RECOVERY: &str = "Too small\nq quits\nService\nstays up";

pub fn render(frame: &mut Frame<'_>, model: &Explorer) {
    let area = frame.area();
    if area.width < 20 || area.height < 8 {
        frame.render_widget(Paragraph::new(RECOVERY), area);
        return;
    }
    if area.height < 16 || area.width < 60 {
        let sections = compact_sections(model);
        render_sections(frame, area, &sections);
        return;
    }
    let sections = full_sections(model);
    render_sections(frame, area, &sections);
}

fn render_sections(frame: &mut Frame<'_>, area: Rect, sections: &[Section]) {
    let constraints: Vec<Constraint> = sections
        .iter()
        .map(|section| match section.height {
            Height::Fixed(lines) => Constraint::Length(lines),
            Height::Fill => Constraint::Min(1),
        })
        .collect();
    let rects = Layout::vertical(constraints).split(area);
    for (section, rect) in sections.iter().zip(rects.iter()) {
        let text = focused_text(model_prefix(section), section);
        frame.render_widget(Paragraph::new(text).style(section.style), *rect);
    }
}

fn model_prefix(section: &Section) -> String {
    section.text.clone()
}

fn focused_text(text: String, section: &Section) -> String {
    if section.linear_focus {
        format!("Focus {text}")
    } else {
        text
    }
}

struct Section {
    height: Height,
    text: String,
    style: Style,
    linear_focus: bool,
}

enum Height {
    Fixed(u16),
    Fill,
}

fn full_sections(model: &Explorer) -> Vec<Section> {
    vec![
        line(connection_line(model), false, model),
        line(
            workspace_line(model),
            model.focus() == Focus::Workspaces,
            model,
        ),
        line(cache_line(model), false, model),
        line(search_line(model), model.focus() == Focus::Search, model),
        fill(body(model, 8), model.focus() == Focus::Results, model),
        block(identity(model), model.focus() == Focus::Detail, model),
        line(
            playback_line(model),
            model.focus() == Focus::Playback,
            model,
        ),
        line(recording_line(model), false, model),
        line(quota_line(model), false, model),
        block(help_text(), model.focus() == Focus::Help, model),
        line(model.status().to_owned(), false, model),
    ]
}

fn compact_sections(model: &Explorer) -> Vec<Section> {
    vec![
        line(connection_line(model), false, model),
        line(search_line(model), model.focus() == Focus::Search, model),
        line(
            identity_primary(model),
            model.focus() == Focus::Detail,
            model,
        ),
        line(
            playback_line(model),
            model.focus() == Focus::Playback,
            model,
        ),
        line(HELP_PRIMARY.to_owned(), model.focus() == Focus::Help, model),
        line(model.status().to_owned(), false, model),
        fill(body(model, 4), model.focus() == Focus::Results, model),
    ]
}

fn line(text: String, focused: bool, model: &Explorer) -> Section {
    section(Height::Fixed(1), text, focused, model)
}

fn block(text: String, focused: bool, model: &Explorer) -> Section {
    let height = u16::try_from(text.lines().count().max(1)).unwrap_or(1);
    section(Height::Fixed(height), text, focused, model)
}

fn fill(text: String, focused: bool, model: &Explorer) -> Section {
    section(Height::Fill, text, focused, model)
}

fn section(height: Height, text: String, focused: bool, model: &Explorer) -> Section {
    let linear_focus = focused && model.modes().linear;
    Section {
        height,
        text,
        style: focus_style(model, focused && !linear_focus),
        linear_focus,
    }
}

fn focus_style(model: &Explorer, focused: bool) -> Style {
    if !focused {
        return Style::default();
    }
    if model.modes().monochrome {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    }
}

fn connection_line(model: &Explorer) -> String {
    let link = match model.link() {
        crate::explorer::state::Link::LocalCatalog => "local catalog".to_owned(),
        crate::explorer::state::Link::Service {
            process_id,
            stopping,
        } => {
            if *stopping {
                format!("service {process_id} stopping")
            } else {
                format!("service {process_id}")
            }
        }
        crate::explorer::state::Link::Disconnected => {
            format!("disconnected, snapshot {}", model.snapshot_age())
        }
    };
    let motion = on_off(model.modes().reduced_motion);
    let linear = on_off(model.modes().linear);
    let mono = on_off(model.modes().monochrome);
    let frames = model.animation_frames();
    format!("Sigy | {link} | motion {motion} | linear {linear} | mono {mono} | frames {frames}")
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn workspace_line(model: &Explorer) -> String {
    [
        Workspace::Explore,
        Workspace::Live,
        Workspace::Recordings,
        Workspace::Monitors,
        Workspace::Findings,
        Workspace::System,
    ]
    .into_iter()
    .map(|workspace| {
        if workspace == model.workspace() {
            format!("[{}]", workspace.label())
        } else {
            workspace.label().to_owned()
        }
    })
    .collect::<Vec<_>>()
    .join(" ")
}

fn cache_line(model: &Explorer) -> String {
    let Some(directory) = model.directory() else {
        return "Partial cache unknown | refresh none | observed unknown | favorites unknown"
            .into();
    };
    let refresh = directory.refresh.as_ref().map_or_else(
        || "refresh none".to_owned(),
        |refresh| {
            format!(
                "{} {}",
                sanitize(&refresh.id, 16),
                sanitize(&refresh.state, 16)
            )
        },
    );
    let observed = model.selected().map_or_else(
        || "observed unknown".to_owned(),
        |row| format!("observed {}", age_label(row.observed_ms, model.now_ms())),
    );
    format!(
        "Partial cache {}/{} | {refresh} | {observed} | favorites {}",
        directory.cached_stations, directory.maximum_stations, directory.favorite_stations
    )
}

fn search_line(model: &Explorer) -> String {
    let filter = if model.favorites_only() {
        " favorites"
    } else {
        ""
    };
    format!("Search: [{}]{filter}", model.query())
}

fn identity(model: &Explorer) -> String {
    let Some(row) = model.selected() else {
        return "Selected source: none\ndirectory health: unknown\ndirectory languages: unknown"
            .into();
    };
    let hls = if row.hls {
        " Directory marks HLS. This list does not open it."
    } else {
        ""
    };
    format!(
        "Selected source: {}{hls}\ndirectory health: {}\ndirectory languages: {}",
        sanitize(&row.name, 60),
        row.directory_health.label(),
        known(&sanitize(&row.directory_languages, 60))
    )
}

fn identity_primary(model: &Explorer) -> String {
    identity(model)
        .lines()
        .next()
        .unwrap_or("Selected source: none")
        .to_owned()
}

fn playback_line(model: &Explorer) -> String {
    model.playback().map_or_else(
        || "Playback: none".to_owned(),
        |session| {
            let format = session.format.as_deref().unwrap_or("unknown");
            let failure = session
                .failure
                .as_deref()
                .map(|detail| format!(" failure {}", sanitize(detail, 40)))
                .unwrap_or_default();
            format!(
                "Playback: listen {} {} format {format} revision {}{failure}",
                sanitize(&session.id, 24),
                sanitize(&session.state, 16),
                sanitize(&session.source_revision, 24)
            )
        },
    )
}

fn recording_line(model: &Explorer) -> String {
    model
        .recordings()
        .iter()
        .find(|recording| recording.active())
        .map_or_else(
            || "Recording: none".to_owned(),
            |recording| {
                format!(
                    "Recording: {} {} continues in the service",
                    sanitize(&recording.id, 24),
                    sanitize(&recording.state, 16)
                )
            },
        )
}

fn quota_line(model: &Explorer) -> String {
    model.quota().map_or_else(
        || "Quota: unknown".to_owned(),
        |quota| {
            format!(
                "Quota: charged {} reserved {} available {} of {}",
                quota.charged, quota.reserved, quota.available, quota.quota
            )
        },
    )
}

fn help_text() -> String {
    format!("{HELP_PRIMARY}\n{HELP_SELECTION}")
}

fn body(model: &Explorer, limit: usize) -> String {
    let lines: Vec<String> = if model.workspace().available() {
        match model.workspace() {
            Workspace::Explore => explore_lines(model),
            Workspace::Live => live_lines(model),
            Workspace::Recordings => recording_lines(model),
            Workspace::System => system_lines(model),
            Workspace::Monitors | Workspace::Findings => unavailable_lines(model.workspace()),
        }
    } else {
        unavailable_lines(model.workspace())
    };
    lines.into_iter().take(limit).collect::<Vec<_>>().join("\n")
}

fn unavailable_lines(workspace: Workspace) -> Vec<String> {
    match workspace {
        Workspace::Findings => {
            vec!["Findings unavailable. No finding operation exists yet.".into()]
        }
        Workspace::Monitors => {
            vec!["Monitors unavailable. No monitor operation exists yet.".into()]
        }
        Workspace::Explore | Workspace::Live | Workspace::Recordings | Workspace::System => {
            vec!["This workspace has no operation yet.".into()]
        }
    }
}

fn explore_lines(model: &Explorer) -> Vec<String> {
    let mut lines = vec!["Globe and map unavailable. This view is the list.".into()];
    if model.rows().is_empty() {
        lines.push(
            "No cached stations. radio search reads this cache. radio refresh is separate.".into(),
        );
        return lines;
    }
    let start = visible_start(model.selection(), model.rows().len(), 8);
    for (offset, row) in model.rows().iter().enumerate().skip(start).take(8) {
        let marker = if offset == model.selection() {
            ">"
        } else {
            " "
        };
        let favorite = if row.favorite { " favorite" } else { "" };
        let health = health_mark(row.directory_health);
        lines.push(format!(
            "{marker} {}{favorite} | {health} | {}",
            sanitize(&row.name, 24),
            sanitize(&row.id, 36)
        ));
    }
    lines
}

fn health_mark(health: Health) -> &'static str {
    match health {
        Health::Unknown => "directory health unknown",
        Health::Succeeded => "directory health succeeded",
        Health::Failed => "directory health failed",
    }
}

fn visible_start(selection: usize, len: usize, window: usize) -> usize {
    if len <= window {
        return 0;
    }
    selection.saturating_sub(window.saturating_sub(1))
}

fn live_lines(model: &Explorer) -> Vec<String> {
    let mut lines = vec![
        "Playback is the listen receipt, not directory health and not a recording.".into(),
        "listen file and listen source are CLI operations. Selection does not start them.".into(),
    ];
    if model.playback().is_none() {
        lines.push("No listen receipt is loaded in this client.".into());
    }
    lines
}

fn recording_lines(model: &Explorer) -> Vec<String> {
    if model.recordings().is_empty() {
        return vec!["No recordings. record start is a separate CLI operation.".into()];
    }
    model
        .recordings()
        .iter()
        .take(8)
        .map(|recording| {
            let format = recording.format.as_deref().unwrap_or("unknown");
            format!(
                "{} {} {} format {format}",
                sanitize(&recording.id, 24),
                sanitize(&recording.state, 16),
                sanitize(&recording.storage_state, 16)
            )
        })
        .collect()
}

fn system_lines(model: &Explorer) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Schema {} | SQLite {} | provider dispatch {}",
            model.schema_version(),
            model.sqlite_version(),
            if model.provider_dispatch_available() {
                "available"
            } else {
                "unavailable"
            }
        ),
        format!(
            "Captures scheduled {} active {} interrupted {} terminal {} | recording dispatch {}",
            model.captures_scheduled(),
            model.captures_active(),
            model.captures_interrupted(),
            model.captures_terminal(),
            if model.dispatch_available() {
                "available"
            } else {
                "unavailable"
            }
        ),
    ];
    if model.budgets().is_empty() {
        lines.push("Budgets: none loaded".into());
    }
    for budget in model.budgets().iter().take(4) {
        let frozen = if budget.frozen { " frozen" } else { "" };
        lines.push(format!(
            "Budget {} limit {} settled {} reserved {} available {}{frozen}",
            sanitize(&budget.scope, 24),
            budget.limit_usd,
            budget.settled_usd,
            budget.reserved_usd,
            budget.available_usd
        ));
    }
    if let Some(refresh) = model
        .directory()
        .and_then(|directory| directory.refresh.as_ref())
    {
        let failure = refresh
            .failure
            .as_deref()
            .map(|detail| format!(" reason {}", sanitize(detail, 80)))
            .unwrap_or_default();
        lines.push(format!(
            "Refresh {} {} accepted {} skipped {}{failure}",
            sanitize(&refresh.id, 32),
            sanitize(&refresh.state, 16),
            refresh.accepted,
            refresh.skipped
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{HELP_PRIMARY, HELP_SELECTION, render};
    use crate::explorer::state::{
        BudgetLine, Desk, DirectoryView, Explorer, Health, Link, Modes, PlaybackView, QuotaView,
        RecordingLine, RefreshView, StationRow, Workspace,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    fn sample() -> Explorer {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome: true,
            },
            10_000,
        );
        model.apply_desk(Desk {
            link: Link::Service {
                process_id: 42,
                stopping: false,
            },
            observed_ms: 10_000,
            schema_version: 11,
            sqlite_version: "3.53.4".into(),
            provider_dispatch_available: false,
            budgets: vec![BudgetLine {
                scope: "global".into(),
                limit_usd: "0.000000".into(),
                settled_usd: "0.000000".into(),
                reserved_usd: "0.000000".into(),
                available_usd: "0.000000".into(),
                frozen: false,
            }],
            captures_active: 1,
            captures_scheduled: 0,
            captures_interrupted: 0,
            captures_terminal: 0,
            dispatch_available: true,
            directory: Some(DirectoryView {
                cached_stations: 1,
                maximum_stations: 10_000,
                favorite_stations: 1,
                refresh: Some(RefreshView {
                    id: "refresh-demo".into(),
                    state: "completed".into(),
                    accepted: 1,
                    skipped: 0,
                    failure: None,
                }),
            }),
            stations: vec![StationRow {
                id: "00000000-0000-4000-8000-000000000001".into(),
                name: "KTNN \u{6771}\u{4eac}".into(),
                favorite: true,
                directory_health: Health::Succeeded,
                directory_languages: "navajo".into(),
                observed_ms: 4_000,
                hls: false,
            }],
            quota: Some(QuotaView {
                quota: 50,
                charged: 5,
                reserved: 5,
                available: 45,
            }),
            recordings: vec![RecordingLine {
                id: "rec-1".into(),
                state: "running".into(),
                storage_state: "reserved".into(),
                format: None,
            }],
            playback: Some(PlaybackView {
                id: "listen-1".into(),
                state: "running".into(),
                format: Some("wav".into()),
                source_revision: "rev-1".into(),
                failure: None,
            }),
        });
        model
    }

    fn frame(model: &Explorer, width: u16, height: u16) -> (String, bool) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap_or_else(|error| match error {});
        let mut colored = false;
        let drawn = terminal.draw(|ui| {
            render(ui, model);
            let buffer = ui.buffer_mut();
            colored = buffer
                .content()
                .iter()
                .any(|cell| cell.fg != Color::Reset || cell.bg != Color::Reset);
        });
        if drawn.is_err() {
            return (String::new(), colored);
        }
        let mut lines = Vec::new();
        let view = terminal.backend().buffer().clone();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(view[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_owned());
        }
        (lines.join("\n"), colored)
    }

    #[test]
    fn eighty_by_twenty_four_keeps_search_identity_playback_and_help() {
        let model = sample();
        let (text, colored) = frame(&model, 80, 24);
        assert!(text.contains("Search:"), "{text}");
        assert!(text.contains("Selected source:"), "{text}");
        assert!(text.contains("directory health: succeeded"), "{text}");
        assert!(text.contains("directory languages: navajo"), "{text}");
        assert!(text.contains("Playback: listen listen-1"), "{text}");
        assert!(text.contains(HELP_PRIMARY), "{text}");
        assert!(text.contains(HELP_SELECTION), "{text}");
        assert!(text.contains("Partial cache 1/10000"), "{text}");
        assert!(text.contains("refresh-demo completed"), "{text}");
        assert!(text.contains("observed 6s"), "{text}");
        assert!(text.contains("favorites 1"), "{text}");
        assert!(text.contains("Recording: rec-1 running"), "{text}");
        assert!(text.contains("Quota: charged 5"), "{text}");
        assert!(text.contains("service 42"), "{text}");
        assert!(text.contains("motion on"), "{text}");
        assert!(text.contains("Monitors unavailable"), "{text}");
        assert!(text.contains("Findings unavailable"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(!colored);
        let playback = text
            .lines()
            .find(|line| line.contains("Playback:"))
            .unwrap_or("");
        assert!(!playback.contains("directory health"));
        assert!(text.contains("Globe and map unavailable"));
    }

    #[test]
    fn linear_order_prefixes_focus_and_still_has_no_color() {
        let mut linear = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: true,
                monochrome: true,
            },
            10_000,
        );
        linear.apply_desk(sample_desk());
        linear.handle(crate::explorer::state::Key::Char('/'));
        let (text, colored) = frame(&linear, 80, 24);
        assert!(text.contains("Focus Search:"), "{text}");
        let search = text.find("Search:").unwrap_or(usize::MAX);
        let identity = text.find("Selected source:").unwrap_or(0);
        let playback = text.find("Playback:").unwrap_or(0);
        let help = text.find("Help:").unwrap_or(0);
        assert!(search < identity);
        assert!(identity < playback);
        assert!(playback < help);
        assert!(!colored);
        assert_eq!(linear.animation_frames(), 0);
    }

    fn sample_desk() -> Desk {
        sample_desk_from(&sample())
    }

    fn sample_desk_from(model: &Explorer) -> Desk {
        Desk {
            link: model.link().clone(),
            observed_ms: model.now_ms(),
            schema_version: model.schema_version(),
            sqlite_version: model.sqlite_version().to_owned(),
            provider_dispatch_available: model.provider_dispatch_available(),
            budgets: model.budgets().to_vec(),
            captures_active: model.captures_active(),
            captures_scheduled: model.captures_scheduled(),
            captures_interrupted: model.captures_interrupted(),
            captures_terminal: model.captures_terminal(),
            dispatch_available: model.dispatch_available(),
            directory: model.directory().cloned(),
            stations: model.rows().to_vec(),
            quota: model.quota(),
            recordings: model.recordings().to_vec(),
            playback: model.playback().cloned(),
        }
    }

    #[test]
    fn unavailable_workspace_is_visible_and_tiny_terminal_recovers() {
        let mut model = sample();
        model.handle(crate::explorer::state::Key::Char('4'));
        assert_eq!(model.workspace(), Workspace::Monitors);
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("Monitors unavailable. No monitor operation exists yet."));
        let (small, _) = frame(&model, 10, 4);
        assert!(small.contains("Too small"), "{small}");
        assert!(small.contains("q quits"), "{small}");
    }

    #[test]
    fn colored_mode_uses_color_only_when_monochrome_is_off() {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: false,
                linear: false,
                monochrome: false,
            },
            10_000,
        );
        model.apply_desk(sample_desk());
        model.handle(crate::explorer::state::Key::Char('/'));
        let (_, colored) = frame(&model, 80, 24);
        assert!(colored);
        assert_eq!(model.animation_frames(), 0);
    }
}
