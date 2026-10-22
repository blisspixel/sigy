//! List layout. Reduced motion, linear order, and monochrome draw no animation.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
mod context;
mod countries;
mod filters;

use crate::style::{Tone, tone_for_state};

use crate::explorer::state::{Explorer, Focus, Workspace};
use crate::explorer::text::{age_label, bytes_label, known, sanitize};

#[cfg(test)]
pub const HELP_PRIMARY: &str = "Help: / search  F filters  C country  e explore  R fetch  f favorite  n/p page  q quit  ? more";
/// The boundary sentence belongs to a confirm prompt, not to every explorer frame.
#[cfg(test)]
pub const HELP_SELECTION: &str =
    "Enter inspects; P then y plays; S stops. Selection starts no work.";
pub const RECOVERY: &str = "Too small\n^C quits\nService\nstays up";
const FOCUS_PREFIX: &str = "Focus ";
/// Station list columns other than the name: marker, favorite, health and languages.
const LIST_FIXED: usize = 2 + 9 + 3 + 16 + 3 + 19;
/// The station ID column, shown only when it fits whole.
const LIST_ID: usize = 3 + 36;
/// Narrow lists keep the marker, favorite and health columns and drop languages.
const LIST_NARROW: usize = 2 + 9 + 3 + 16;

pub fn render(frame: &mut Frame<'_>, model: &Explorer) {
    let area = frame.area();
    if area.width < 20 || area.height < 8 {
        frame.render_widget(Paragraph::new(RECOVERY), area);
        return;
    }
    if model.keys_open() {
        frame.render_widget(Paragraph::new(key_lines(model)), area);
        return;
    }
    if model.context.is_some() {
        context::render(frame, area, model);
        return;
    }
    if let Some(picker) = &model.search.picker {
        countries::render(frame, area, picker);
        return;
    }
    if model.search.editing_filters() {
        filters::render(frame, area, model);
        return;
    }
    if area.height < 16 || area.width < 60 {
        let sections = compact_sections(model, area);
        render_sections(frame, area, &sections, model);
        return;
    }
    let sections = full_sections(model, area);
    render_sections(frame, area, &sections, model);
}

fn section_rects(area: Rect, sections: &[Section]) -> std::rc::Rc<[Rect]> {
    let constraints: Vec<Constraint> = sections
        .iter()
        .map(|section| match section.height {
            Height::Fixed(lines) => Constraint::Length(lines),
            Height::Fill => Constraint::Min(1),
        })
        .collect();
    Layout::vertical(constraints).split(area)
}

/// Use the same layout as rendering; a workspace can have no drawable canvas.
pub(super) fn globe_visible(model: &Explorer, area: Rect) -> bool {
    if area.width < 60
        || area.height < 16
        || model.workspace() != Workspace::Globe
        || model.keys_open()
        || model.context.is_some()
        || model.search.picker.is_some()
        || model.search.editing_filters()
    {
        return false;
    }
    let sections = full_sections(model, area);
    sections
        .iter()
        .zip(section_rects(area, &sections).iter())
        .any(|(section, rect)| {
            let canvas = globe_rects(*rect)[1];
            section.globe && canvas.width >= 4 && canvas.height >= 2
        })
}

/// Canvas size for the one-session flourish. `None` waits: an overlay or another workspace
/// is up, or the terminal is still the recovery screen. A small canvas is still `Some`.
pub(super) fn globe_flourish_canvas(model: &Explorer, area: Rect) -> Option<(u16, u16)> {
    if model.workspace() != Workspace::Globe
        || model.keys_open()
        || model.context.is_some()
        || model.search.picker.is_some()
        || model.search.editing_filters()
        || area.width < 20
        || area.height < 8
    {
        return None;
    }
    let sections = if area.height < 16 || area.width < 60 {
        compact_sections(model, area)
    } else {
        full_sections(model, area)
    };
    sections
        .iter()
        .zip(section_rects(area, &sections).iter())
        .find_map(|(section, rect)| {
            if !section.globe {
                return None;
            }
            let canvas = globe_rects(*rect)[1];
            Some((canvas.width, canvas.height))
        })
}

fn render_sections(frame: &mut Frame<'_>, area: Rect, sections: &[Section], model: &Explorer) {
    let rects = section_rects(area, sections);
    for (section, rect) in sections.iter().zip(rects.iter()) {
        if section.globe {
            render_globe(frame, *rect, section, model);
            continue;
        }
        if section.beside {
            render_beside(frame, *rect, section, model);
            continue;
        }
        let mut lines = if model.workspace() == Workspace::Explore
            && matches!(section.height, Height::Fill)
        {
            let prefix = if section.linear_focus {
                FOCUS_PREFIX.len()
            } else {
                0
            };
            explore_lines(
                model,
                usize::from(rect.width).saturating_sub(prefix),
                usize::from(rect.height),
            )
        } else if model.workspace() == Workspace::Monitors && matches!(section.height, Height::Fill)
        {
            model
                .monitors
                .lines_at_width(
                    usize::from(rect.height),
                    usize::from(rect.width).saturating_sub(if section.linear_focus {
                        FOCUS_PREFIX.len()
                    } else {
                        0
                    }),
                )
                .into_iter()
                .map(Line::from)
                .collect()
        } else if model.workspace() == Workspace::Recordings
            && matches!(section.height, Height::Fill)
        {
            recording_lines(model, usize::from(rect.width), usize::from(rect.height))
        } else if model.workspace() == Workspace::Findings && matches!(section.height, Height::Fill)
        {
            model
                .findings
                .lines(
                    usize::from(rect.height),
                    usize::from(rect.width),
                    if section.linear_focus {
                        FOCUS_PREFIX.len()
                    } else {
                        0
                    },
                )
                .into_iter()
                .map(Line::from)
                .collect()
        } else {
            section.lines.clone()
        };
        if section.linear_focus
            && let Some(first) = lines.first_mut()
        {
            first.spans.insert(0, Span::raw(FOCUS_PREFIX));
        }
        // A focused body marks its first line, not every row, so the selection stays visible.
        let mut style = section.style;
        if matches!(section.height, Height::Fill) {
            if let Some(first) = lines.first_mut() {
                first.style = first.style.patch(style);
            }
            style = Style::default();
        }
        frame.render_widget(Paragraph::new(lines).style(style), *rect);
    }
}

fn globe_rects(area: Rect) -> std::rc::Rc<[Rect]> {
    let header_height = if area.height >= 6 { 3 } else { 2 };
    Layout::vertical([Constraint::Length(header_height), Constraint::Min(1)]).split(area)
}

fn render_globe(frame: &mut Frame<'_>, area: Rect, section: &Section, model: &Explorer) {
    let rows = globe_rects(area);
    let mut header = super::globe::header_lines(model);
    if section.linear_focus
        && let Some(first) = header.first_mut()
    {
        first.spans.insert(0, Span::raw(FOCUS_PREFIX));
    }
    if let Some(first) = header.first_mut() {
        first.style = first.style.patch(section.style);
    }
    frame.render_widget(Paragraph::new(header), rows[0]);
    frame.render_widget(
        super::globe::GlobeWidget::new(model, color_on(model)),
        rows[1],
    );
}

fn render_beside(frame: &mut Frame<'_>, area: Rect, section: &Section, model: &Explorer) {
    let columns =
        Layout::horizontal([Constraint::Percentage(64), Constraint::Percentage(36)]).split(area);
    let mut lines = pane_lines(
        model,
        usize::from(columns[0].width),
        usize::from(columns[0].height),
    );
    if section.linear_focus
        && let Some(first) = lines.first_mut()
    {
        first.spans.insert(0, Span::raw(FOCUS_PREFIX));
    }
    if let Some(first) = lines.first_mut() {
        let style = focus_style(
            model,
            model.focus() == Focus::Results && !model.modes().linear,
        );
        first.style = first.style.patch(style);
    }
    frame.render_widget(Paragraph::new(lines), columns[0]);
    let mut identity = identity_lines(model);
    if model.focus() == Focus::Detail && !model.modes().linear {
        let style = focus_style(model, true);
        if let Some(first) = identity.first_mut() {
            first.style = first.style.patch(style);
        }
    }
    frame.render_widget(Paragraph::new(identity), columns[1]);
}

fn pane_lines(model: &Explorer, width: usize, height: usize) -> Vec<Line<'static>> {
    if !model.workspace().available() {
        return unavailable_lines(model);
    }
    match model.workspace() {
        Workspace::Explore => explore_lines(model, width, height),
        Workspace::Live => live_lines(model),
        Workspace::Recordings => recording_lines(model, width, height),
        Workspace::System => system_lines(model),
        Workspace::Globe => Vec::new(),
        Workspace::Monitors => model
            .monitors
            .lines_at_width(height, width)
            .into_iter()
            .map(Line::from)
            .collect(),
        Workspace::Findings => model
            .findings
            .lines(height, width, 0)
            .into_iter()
            .map(Line::from)
            .collect(),
    }
}

struct Section {
    height: Height,
    lines: Vec<Line<'static>>,
    style: Style,
    linear_focus: bool,
    globe: bool,
    beside: bool,
}

enum Height {
    Fixed(u16),
    Fill,
}

fn full_sections(model: &Explorer, area: Rect) -> Vec<Section> {
    let width = usize::from(area.width);
    let mut sections = chrome(model, width);
    sections.push(one(cache_line(model), false, model));
    sections.push(many(
        search_lines(model),
        model.focus() == Focus::Search,
        model,
    ));
    let mut body_section = fill(body(model, 12), model.focus() == Focus::Results, model);
    if area.width >= 120 && area.height >= 40 && model.workspace() != Workspace::Globe {
        body_section.beside = true;
        sections.push(body_section);
    } else {
        sections.push(body_section);
        sections.push(many(
            identity_lines(model),
            model.focus() == Focus::Detail,
            model,
        ));
    }
    push_status(&mut sections, model, width, 2);
    sections
}

fn compact_sections(model: &Explorer, area: Rect) -> Vec<Section> {
    let columns = usize::from(area.width);
    let mut sections = vec![one(connection_line(model, true), false, model)];
    if let Some(live) = live_strip(model, columns) {
        sections.push(one(live, model.focus() == Focus::Playback, model));
    }
    if let Some(retained) = retained_strip(model, columns) {
        sections.push(one(retained, false, model));
    }
    sections.push(many(
        search_lines(model),
        model.focus() == Focus::Search,
        model,
    ));
    sections.push(one(
        identity_primary(model),
        model.focus() == Focus::Detail,
        model,
    ));
    sections.push(one(
        plain(&hint_text(model, columns)),
        model.focus() == Focus::Help,
        model,
    ));
    // Two status rows are allowed only when four body rows remain. The narrow
    // recording timeline needs those four rows for its axis, cells and legend.
    let chrome = fixed_rows(&sections);
    let remaining = area.height.saturating_sub(chrome);
    let status_rows = if remaining >= 6 {
        2
    } else {
        usize::from(remaining > 1)
    };
    push_status(&mut sections, model, columns, status_rows);
    sections.push(fill(body(model, 4), model.focus() == Focus::Results, model));
    sections
}

fn fixed_rows(sections: &[Section]) -> u16 {
    sections
        .iter()
        .map(|section| match section.height {
            Height::Fixed(lines) => lines,
            Height::Fill => 0,
        })
        .sum()
}

fn chrome(model: &Explorer, width: usize) -> Vec<Section> {
    let mut sections = vec![
        one(connection_line(model, false), false, model),
        one(
            workspace_line(model, width),
            model.focus() == Focus::Workspaces,
            model,
        ),
    ];
    if let Some(live) = live_strip(model, width) {
        sections.push(one(live, model.focus() == Focus::Playback, model));
    }
    if let Some(retained) = retained_strip(model, width) {
        sections.push(one(retained, false, model));
    }
    sections.push(one(
        plain(&hint_text(model, width)),
        model.focus() == Focus::Help,
        model,
    ));
    sections
}

fn push_status(sections: &mut Vec<Section>, model: &Explorer, width: usize, max_rows: usize) {
    if max_rows == 0 || model.status().split_whitespace().next().is_none() {
        return;
    }
    sections.push(many(
        status_lines(model.status(), width, max_rows),
        false,
        model,
    ));
}

fn one(line: Line<'static>, focused: bool, model: &Explorer) -> Section {
    section(Height::Fixed(1), vec![line], focused, model)
}

#[cfg(test)]
fn compact_help(model: &Explorer, width: u16) -> String {
    hint_text(model, usize::from(width))
}

fn hint_text(model: &Explorer, width: usize) -> String {
    fit_pairs(&hint_pairs(model), width)
}

fn hint_pairs(model: &Explorer) -> Vec<&'static str> {
    let restart = model.restart_required()
        && matches!(model.workspace(), Workspace::Explore | Workspace::Globe);
    if editing(model) {
        return if restart {
            vec!["g first page", "Esc ends edit", "Enter applies"]
        } else {
            vec!["Esc ends edit", "Enter applies"]
        };
    }
    match model.workspace() {
        Workspace::Explore if restart => vec![
            "g first page",
            "/ search",
            "F filters",
            "C country",
            "e explore",
            "R fetch",
            "f favorite",
            "n/p page",
            "q quit",
        ],
        Workspace::Explore => vec![
            "/ search",
            "F filters",
            "C country",
            "e explore",
            "R fetch",
            "f favorite",
            "n/p page",
            "q quit",
        ],
        Workspace::Live | Workspace::System => vec!["r reload", "Tab focus", "q quit"],
        Workspace::Recordings => {
            vec![
                "select row",
                "r reload",
                "P play",
                "S stop",
                "H pause",
                "[/] 10s",
                "q quit",
            ]
        }
        Workspace::Monitors => vec!["inspect row", "r reload", "Esc back", "q quit"],
        Workspace::Findings => {
            vec![
                "/ edit IDs",
                "Enter inspect",
                "o metadata",
                "P play",
                "S stop",
                "H pause",
                "[/] 10s",
                "q quit",
            ]
        }
        Workspace::Globe if restart => vec!["g first page", "Arrows turn", "R fetch", "q quit"],
        Workspace::Globe => vec![
            "Arrows turn",
            "m map",
            "e explore",
            "R fetch",
            "n/p page",
            "q quit",
        ],
    }
}

/// Keep the first pair whole. End with `? more` when that tail fits.
fn fit_pairs(pairs: &[&str], width: usize) -> String {
    if width == 0 || pairs.is_empty() {
        return String::new();
    }
    let mut kept = Vec::new();
    for pair in pairs {
        let mut trial = kept.clone();
        trial.push(*pair);
        if line_width(&help_line(&trial)) <= width {
            kept.push(*pair);
        } else {
            break;
        }
    }
    if !kept.is_empty() {
        return help_line(&kept);
    }
    let first = pairs[0];
    let bare = format!("{first}  ? more");
    if line_width(&bare) <= width {
        return bare;
    }
    let help_first = format!("Help: {first}");
    if line_width(&help_first) <= width {
        return help_first;
    }
    if line_width(first) <= width {
        return first.to_owned();
    }
    if line_width("? more") <= width {
        "? more".to_owned()
    } else {
        String::new()
    }
}

fn help_line(pairs: &[&str]) -> String {
    format!("Help: {}  ? more", pairs.join("  "))
}

fn editing(model: &Explorer) -> bool {
    model.focus() == Focus::Search
        || (model.workspace() == Workspace::Findings && model.findings.editing())
}

fn many(lines: Vec<Line<'static>>, focused: bool, model: &Explorer) -> Section {
    let height = u16::try_from(lines.len().max(1)).unwrap_or(1);
    section(Height::Fixed(height), lines, focused, model)
}

fn fill(lines: Vec<Line<'static>>, focused: bool, model: &Explorer) -> Section {
    let mut section = section(Height::Fill, lines, focused, model);
    section.globe = model.workspace() == Workspace::Globe;
    section
}

fn section(height: Height, lines: Vec<Line<'static>>, focused: bool, model: &Explorer) -> Section {
    let linear_focus = focused && model.modes().linear;
    Section {
        height,
        lines,
        style: focus_style(model, focused && !linear_focus),
        linear_focus,
        globe: false,
        beside: false,
    }
}

fn focus_style(model: &Explorer, focused: bool) -> Style {
    if !focused || model.modes().linear {
        return Style::default();
    }
    // High contrast uses reverse video for focus, the same signal as monochrome.
    // Color modes use bold so a reversed yellow or green cell does not become a block.
    if model.modes().monochrome || model.modes().emphasis.high() {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

fn color_on(model: &Explorer) -> bool {
    !model.modes().monochrome
}

/// Unicode markers stay off in linear and monochrome modes.
/// `NO_COLOR` and `TERM=dumb` select monochrome through `Modes::resolve_with`.
fn marker_glyphs(model: &Explorer) -> bool {
    !model.modes().linear && !model.modes().monochrome
}

fn plain(text: &str) -> Line<'static> {
    Line::from(text.to_owned())
}

fn paint(model: &Explorer, tone: Tone, text: impl Into<String>) -> Span<'static> {
    let text = text.into();
    let color = color_on(model);
    let high = model.modes().emphasis.high();
    if (tone == Tone::Plain || !color) && !high {
        return Span::raw(text);
    }
    if high && !color {
        return if matches!(tone, Tone::Plain | Tone::Muted) {
            Span::raw(text)
        } else {
            Span::styled(text, Style::default().add_modifier(Modifier::BOLD))
        };
    }
    if high {
        return match tone {
            Tone::Plain | Tone::Muted => Span::raw(text),
            Tone::Fail | Tone::Live | Tone::Record => Span::styled(
                text,
                Style::default()
                    .fg(crate::style::role_color(tone))
                    .add_modifier(Modifier::BOLD),
            ),
            Tone::Accent | Tone::Attention | Tone::Ok | Tone::Favorite => {
                Span::styled(text, Style::default().add_modifier(Modifier::BOLD))
            }
        };
    }
    let mut style = Style::default().fg(crate::style::role_color(tone));
    if matches!(tone, Tone::Accent | Tone::Attention) {
        style = style.add_modifier(Modifier::BOLD);
    }
    Span::styled(text, style)
}

fn favorite_mark(model: &Explorer, favorite: bool) -> Span<'static> {
    if !favorite {
        return Span::raw(" ".repeat(9));
    }
    let glyph = if marker_glyphs(model) { " ★" } else { " *" };
    let width = Line::raw(glyph).width();
    paint(
        model,
        Tone::Favorite,
        format!("{glyph}{}", " ".repeat(9usize.saturating_sub(width))),
    )
}

fn live_prefix(model: &Explorer, state: &str) -> Option<Span<'static>> {
    if !is_live_state(state) {
        return None;
    }
    let glyph = if marker_glyphs(model) {
        "● "
    } else {
        "(live) "
    };
    Some(paint(model, Tone::Live, glyph))
}

fn record_suffix(model: &Explorer, state: &str) -> Option<Span<'static>> {
    if !is_live_state(state) {
        return None;
    }
    let glyph = if marker_glyphs(model) {
        " ● REC"
    } else {
        " REC"
    };
    Some(paint(model, Tone::Record, glyph))
}

fn is_live_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "running" | "playing" | "recording"
    )
}

fn connection_line(model: &Explorer, compact: bool) -> Line<'static> {
    let (link, tone) = match model.link() {
        crate::explorer::state::Link::LocalCatalog => {
            ("no service, local catalog".to_owned(), Tone::Attention)
        }
        crate::explorer::state::Link::Service {
            process_id,
            stopping: false,
        } => (format!("service {process_id}"), Tone::Ok),
        crate::explorer::state::Link::Service {
            process_id,
            stopping: true,
        } => (format!("service {process_id} stopping"), Tone::Attention),
        crate::explorer::state::Link::Disconnected => (
            format!("disconnected, snapshot {}", model.snapshot_age()),
            Tone::Fail,
        ),
    };
    let mut spans = vec![paint(model, Tone::Accent, "Sigy")];
    if compact {
        // The compact layout has no tab bar, so the title names the workspace.
        spans.push(paint(
            model,
            Tone::Accent,
            format!(
                " [{} {}]",
                model.workspace().digit(),
                model.workspace().label()
            ),
        ));
    }
    spans.extend([paint(model, Tone::Plain, " | "), paint(model, tone, link)]);
    let suffix = mode_suffix(model);
    if !suffix.is_empty() {
        spans.push(paint(model, Tone::Muted, suffix));
    }
    if let Some(quota) = model.quota()
        && quota_needs_attention(quota.quota, quota.available)
    {
        spans.push(paint(
            model,
            Tone::Attention,
            format!(" | {} free", bytes_label(quota.available)),
        ));
    }
    Line::from(spans)
}

fn mode_suffix(model: &Explorer) -> String {
    let mut parts = Vec::new();
    if model.modes().reduced_motion {
        parts.push("reduced motion on");
    }
    if model.modes().linear {
        parts.push("linear on");
    }
    if model.modes().monochrome {
        parts.push("mono on");
    }
    if model.modes().emphasis.high() {
        parts.push("high contrast on");
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" | {}", parts.join(" | "))
    }
}

fn quota_needs_attention(quota: u64, available: u64) -> bool {
    quota == 0 || available.saturating_mul(5) <= quota
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn workspace_line(model: &Explorer, width: usize) -> Line<'static> {
    let short = width < 80;
    let mut spans = Vec::new();
    for (index, workspace) in Workspace::ALL.into_iter().enumerate() {
        if index > 0 {
            spans.push(paint(model, Tone::Plain, " "));
        }
        let current = workspace == model.workspace();
        let name = if short {
            short_tab(workspace)
        } else {
            workspace.label()
        };
        let label = format!("{} {name}", workspace.digit());
        let label = if current { format!("[{label}]") } else { label };
        let tone = if current {
            Tone::Accent
        } else if workspace.available() {
            Tone::Plain
        } else {
            Tone::Muted
        };
        spans.push(paint(model, tone, label));
    }
    Line::from(spans)
}

fn short_tab(workspace: Workspace) -> &'static str {
    match workspace {
        Workspace::Explore => "Exp",
        Workspace::Live => "Live",
        Workspace::Recordings => "Rec",
        Workspace::Monitors => "Mon",
        Workspace::Findings => "Find",
        Workspace::System => "Sys",
        Workspace::Globe => "Globe",
    }
}

fn cache_line(model: &Explorer) -> Line<'static> {
    let Some(directory) = model.directory() else {
        return plain("Cache: not read yet | Last fetch: none | Favorites: unknown");
    };
    if directory.maximum_stations == 0 {
        return plain("Cache statistics unobserved | r reload");
    }
    let mut spans = vec![paint(
        model,
        Tone::Plain,
        format!(
            "Cache: {} | ",
            crate::copy::cache_span(
                u64::from(directory.cached_stations),
                u64::from(directory.maximum_stations),
            )
        ),
    )];
    match directory.refresh.as_ref() {
        Some(refresh) => {
            let state = sanitize(&refresh.state, 16);
            spans.push(paint(model, Tone::Plain, "Last fetch: "));
            spans.push(paint(model, tone_for_state(&state), state));
            spans.push(paint(
                model,
                Tone::Muted,
                format!(" {}", clip(&refresh.id, 24)),
            ));
        }
        None => spans.push(paint(model, Tone::Plain, "Last fetch: none")),
    }
    let favorites = directory.favorite_stations;
    spans.push(paint(model, Tone::Plain, " | "));
    spans.push(paint(model, Tone::Plain, format!("Favorites: {favorites}")));
    Line::from(spans)
}

fn search_lines(model: &Explorer) -> Vec<Line<'static>> {
    if model.workspace() == Workspace::Findings {
        return vec![plain(
            "Findings: inspect one stored citation by name. Listing is not available yet.",
        )];
    }
    let mut spans = vec![
        paint(model, Tone::Plain, "Search: ["),
        paint(
            model,
            Tone::Plain,
            if model.focus() == Focus::Search {
                model.query()
            } else {
                &model.search.applied.filter.name
            }
            .to_owned(),
        ),
        paint(model, Tone::Plain, "]"),
    ];
    if model.favorites_only() {
        spans.push(paint(model, Tone::Favorite, " favorites"));
    }
    if matches!(model.workspace(), Workspace::Explore | Workspace::Globe) {
        spans.push(paint(
            model,
            Tone::Muted,
            format!(
                " | {} | {} | snapshot {}",
                model.catalog_label(),
                model.result_label(),
                model.snapshot_age()
            ),
        ));
    }
    let mut lines = vec![Line::from(spans)];
    if model.search.applied.filter != sigy_service::discovery::StationFilter::default()
        || model.search.draft != model.search.applied
    {
        lines.push(plain(&filters::summary(&model.search.applied)));
    }
    lines
}

fn identity_lines(model: &Explorer) -> Vec<Line<'static>> {
    if model.workspace() == Workspace::Findings {
        return vec![
            plain("Stored citations: named lookup, / edits MONITOR FINDING."),
            plain("o inspects original recording metadata. Arrows scroll."),
            plain("Classification off. Original script and uncertain English preserved."),
        ];
    }
    if model.workspace() == Workspace::Recordings {
        return vec![
            plain(&format!(
                "Selected recording: {}",
                model
                    .selected_recording()
                    .map_or_else(|| "none".into(), |recording| sanitize(&recording.id, 64))
            )),
            plain("Published media, gaps and unpublished time remain separate."),
        ];
    }
    if model.workspace() == Workspace::Monitors {
        return vec![
            plain(&format!(
                "Selected monitor: {}",
                model
                    .monitors
                    .selected()
                    .map_or_else(|| "none".into(), |id| sanitize(id, 64))
            )),
            plain(&format!(
                "Snapshot: {}. Arrows scroll; Escape returns.",
                model.monitors.snapshot_label()
            )),
            plain("Classification off. Capture schedules and processing are unchanged."),
        ];
    }
    let Some(row) = model.selected() else {
        return vec![
            Line::from(vec![
                paint(model, Tone::Plain, "Selected station: "),
                paint(model, Tone::Plain, "none"),
            ]),
            Line::from(vec![
                paint(model, Tone::Plain, "Directory health: "),
                paint(model, tone_for_state("unknown"), "unknown"),
            ]),
            Line::from(vec![
                paint(model, Tone::Plain, "Directory languages: "),
                paint(model, Tone::Plain, "unknown"),
            ]),
        ];
    };
    let mut health = vec![
        paint(model, Tone::Plain, "Directory health: "),
        paint(
            model,
            tone_for_state(row.directory_health.label()),
            row.directory_health.label(),
        ),
        paint(
            model,
            Tone::Plain,
            match age_label(row.observed_ms, model.now_ms()).as_str() {
                "unknown" => " | observed at an unknown time".to_owned(),
                age => format!(" | observed {age} ago"),
            },
        ),
    ];
    if row.hls {
        health.push(paint(model, Tone::Plain, " | directory marks HLS"));
    }
    vec![
        Line::from(vec![
            paint(model, Tone::Plain, "Selected station: "),
            paint(model, Tone::Plain, sanitize(&row.name, 60)),
        ]),
        Line::from(health),
        Line::from(vec![
            paint(model, Tone::Plain, "Directory languages: "),
            paint(
                model,
                Tone::Plain,
                known(&sanitize(&row.directory_languages, 60)).to_owned(),
            ),
        ]),
    ]
}

fn identity_primary(model: &Explorer) -> Line<'static> {
    identity_lines(model)
        .into_iter()
        .next()
        .unwrap_or_else(|| plain("Selected station: none"))
}

fn retained_strip(model: &Explorer, width: usize) -> Option<Line<'static>> {
    let (phase, request) = model.retained_label()?;
    let text = if phase == "paused" {
        match model.paused_at_us() {
            Some(time) => format!("Retained: paused at {time} us {}", sanitize(request, 64)),
            None => format!("Retained: paused {}", sanitize(request, 64)),
        }
    } else {
        match model.decoder_output_us() {
            Some(time) => format!(
                "Retained: {phase} decoder {time} us {}",
                sanitize(request, 64)
            ),
            None => format!("Retained: {phase} {}", sanitize(request, 64)),
        }
    };
    Some(plain(&clip(&text, width)))
}

fn live_strip(model: &Explorer, width: usize) -> Option<Line<'static>> {
    let full = live_spans(model, true);
    if full.is_empty() {
        return None;
    }
    let line = Line::from(full);
    if width == 0 || line.width() <= width {
        return Some(line);
    }
    let short = live_spans(model, false);
    if short.is_empty() {
        None
    } else {
        Some(Line::from(short))
    }
}

fn live_spans(model: &Explorer, session_id: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if let Some(session) = model.playback() {
        let state = sanitize(&session.state, 16);
        spans.push(paint(model, Tone::Plain, "Listen session: "));
        if let Some(marker) = live_prefix(model, &state) {
            spans.push(marker);
        }
        spans.push(paint(model, tone_for_state(&state), state));
        if session_id {
            spans.push(paint(
                model,
                Tone::Plain,
                format!(" {}", sanitize(&session.id, 24)),
            ));
        }
    }
    if let Some(recording) = model
        .recordings()
        .iter()
        .find(|recording| recording.active())
    {
        if !spans.is_empty() {
            spans.push(paint(model, Tone::Plain, " | "));
        }
        let state = sanitize(&recording.state, 16);
        spans.push(paint(model, Tone::Plain, "Recording: "));
        spans.push(paint(model, Tone::Muted, sanitize(&recording.id, 24)));
        spans.push(paint(model, Tone::Plain, " "));
        spans.push(paint(model, tone_for_state(&state), state.clone()));
        if let Some(marker) = record_suffix(model, &state) {
            spans.push(marker);
        }
    }
    spans
}

fn playback_line(model: &Explorer) -> Line<'static> {
    let Some(session) = model.playback() else {
        return Line::from(vec![
            paint(model, Tone::Plain, "Listen session: "),
            paint(model, Tone::Plain, "none"),
        ]);
    };
    let format = session.format.as_deref().unwrap_or("unknown");
    let state = sanitize(&session.state, 16);
    let mut spans = vec![paint(model, Tone::Plain, "Listen session: ")];
    if let Some(marker) = live_prefix(model, &state) {
        spans.push(marker);
    }
    spans.extend([
        paint(model, tone_for_state(&state), state),
        paint(model, Tone::Plain, format!(" format {format} ")),
        paint(model, Tone::Muted, sanitize(&session.id, 24)),
        paint(model, Tone::Plain, " revision "),
        paint(model, Tone::Muted, sanitize(&session.source_revision, 24)),
    ]);
    if let Some(detail) = session.failure.as_deref() {
        spans.push(paint(
            model,
            Tone::Fail,
            format!(" failure {}", sanitize(detail, 40)),
        ));
    }
    Line::from(spans)
}

fn recording_line(model: &Explorer) -> Line<'static> {
    let Some(recording) = model
        .recordings()
        .iter()
        .find(|recording| recording.active())
    else {
        return Line::from(vec![
            paint(model, Tone::Plain, "Recording: "),
            paint(model, Tone::Plain, "none"),
        ]);
    };
    let state = sanitize(&recording.state, 16);
    let mut spans = vec![
        paint(model, Tone::Plain, "Recording: "),
        paint(model, Tone::Muted, sanitize(&recording.id, 24)),
        paint(model, Tone::Plain, " "),
        paint(model, tone_for_state(&state), state.clone()),
    ];
    if let Some(marker) = record_suffix(model, &state) {
        spans.push(marker);
    }
    spans.push(paint(model, Tone::Plain, " continues in the service"));
    Line::from(spans)
}

fn quota_line(model: &Explorer) -> Line<'static> {
    let Some(quota) = model.quota() else {
        return Line::from(vec![
            paint(model, Tone::Plain, "Quota: "),
            paint(model, Tone::Plain, "unknown"),
        ]);
    };
    Line::from(vec![
        paint(model, Tone::Plain, "Quota: "),
        paint(
            model,
            Tone::Plain,
            format!(
                "{} charged, {} reserved, {} available of {}",
                bytes_label(quota.charged),
                bytes_label(quota.reserved),
                bytes_label(quota.available),
                bytes_label(quota.quota)
            ),
        ),
    ])
}

fn key_lines(model: &Explorer) -> Vec<Line<'static>> {
    let mut lines = vec![
        plain(&format!("Keys: {}", model.workspace().label())),
        plain(""),
        plain("Move"),
        plain("  1-7 workspace    Tab focus"),
        plain("  Globe results: arrows turn the view"),
        plain("  Other views: Left/Right workspace, Up/Down select"),
        plain(""),
        plain("This workspace"),
    ];
    for line in workspace_keys(model.workspace()) {
        lines.push(plain(line));
    }
    lines.push(plain(""));
    lines.push(plain("Close"));
    lines.push(plain("  ? or Esc closes this list    q quits this client"));
    lines
}

fn workspace_keys(workspace: Workspace) -> &'static [&'static str] {
    match workspace {
        Workspace::Explore => &[
            "  / search    F filters    C country    e explore",
            "  g first page    n/p page    f favorite",
            "  R then y fetch    Enter inspect",
            "  P then y plays    S stops",
        ],
        Workspace::Globe => &[
            "  arrows or h/l/j/k turn    m map    c center    r spin",
            "  e explore    R then y fetch    n/p page    g first page",
        ],
        Workspace::Live | Workspace::System => &["  r reload    Tab focus"],
        Workspace::Recordings => &[
            "  arrows select a recording    r reloads its metadata",
            "  P then y plays a retained recording    S stops",
            "  H pauses at the decoder time and the next H resumes",
            "  [ and ] seek 10 s inside this admitted listen",
        ],
        Workspace::Monitors => {
            &["  arrows select or scroll    Enter inspects    Esc back    r reload"]
        }
        Workspace::Findings => &[
            "  / edit IDs    Enter inspects    arrows scroll    o recording metadata",
            "  P then y plays a cited interval    S stops",
            "  H pauses at the decoder time and the next H resumes",
            "  [ and ] seek 10 s inside this admitted listen",
        ],
    }
}

fn body(model: &Explorer, limit: usize) -> Vec<Line<'static>> {
    let lines = if model.workspace().available() {
        match model.workspace() {
            Workspace::Explore => explore_lines(model, 80, limit),
            Workspace::Live => live_lines(model),
            Workspace::Recordings => recording_lines(model, 60, limit),
            Workspace::System => system_lines(model),
            // Drawn as a canvas by `render_sections`; see `explorer::globe`.
            Workspace::Globe => Vec::new(),
            Workspace::Monitors => model
                .monitors
                .lines(limit)
                .into_iter()
                .map(Line::from)
                .collect(),
            Workspace::Findings => model
                .findings
                .lines(limit, 60, 0)
                .into_iter()
                .map(Line::from)
                .collect(),
        }
    } else {
        unavailable_lines(model)
    };
    lines.into_iter().take(limit).collect()
}

fn unavailable_lines(model: &Explorer) -> Vec<Line<'static>> {
    let text = match model.workspace() {
        Workspace::Findings => {
            "Stored findings: use sigy monitor finding MONITOR FINDING show. Listing is pending."
        }
        Workspace::Monitors => "Monitors: Enter inspects coverage and literal passages.",
        Workspace::Explore
        | Workspace::Live
        | Workspace::Recordings
        | Workspace::Globe
        | Workspace::System => "This workspace has no operation yet.",
    };
    vec![Line::from(paint(model, Tone::Muted, text))]
}

/// The station list fills its area. Columns align by terminal cell width, and the
/// selected row is reversed while the list has focus, so selection never relies on color.
fn explore_lines(model: &Explorer, width: usize, height: usize) -> Vec<Line<'static>> {
    if model.rows().is_empty() {
        return empty_explore(model);
    }
    let columns = Columns::for_width(width);
    let mut title = format!("Stations, {}", model.result_label());
    match (model.has_next_page(), model.page_number() > 1) {
        (true, true) => title.push_str(" (n, p)"),
        (true, false) => title.push_str(" (n more)"),
        (false, true) => title.push_str(" (p back)"),
        (false, false) => {}
    }
    if model.modes().linear {
        return linear_station_lines(model, &title, columns.name, height);
    }
    let mut header = format!(
        "  {} Favorite | {}",
        fit(&title, columns.name),
        fit("Directory health", 16)
    );
    if columns.languages {
        header.push_str(" | ");
        header.push_str(&fit("Directory languages", 19));
    }
    if columns.id {
        header.push_str(" | station ID");
    }
    let mut lines = vec![Line::from(paint(model, Tone::Muted, header))];
    let window = height.saturating_sub(1).max(1);
    let start = visible_start(model.selection(), model.rows().len(), window);
    let focused = model.focus() == Focus::Results && !model.modes().linear;
    for (offset, row) in model.rows().iter().enumerate().skip(start).take(window) {
        let selected = offset == model.selection();
        let mut spans = vec![
            paint(model, Tone::Plain, if selected { "> " } else { "  " }),
            paint(model, Tone::Plain, fit(&row.name, columns.name)),
            favorite_mark(model, row.favorite),
            paint(model, Tone::Plain, " | "),
            paint(
                model,
                tone_for_state(row.directory_health.label()),
                fit(row.directory_health.label(), 16),
            ),
        ];
        if columns.languages {
            spans.push(paint(model, Tone::Plain, " | "));
            spans.push(paint(
                model,
                Tone::Plain,
                fit(known(&row.directory_languages), 19),
            ));
        }
        if columns.id {
            spans.push(paint(model, Tone::Plain, " | "));
            spans.push(paint(model, Tone::Muted, sanitize(&row.id, 36)));
        }
        if selected && focused {
            // One uniform bar: reversing colored cells would turn tones into blocks.
            for span in &mut spans {
                span.style = Style::default().add_modifier(Modifier::REVERSED);
            }
        }
        lines.push(Line::from(spans));
    }
    if lines.len() < height && model.rows().len() <= window {
        let footer = if model.has_next_page() {
            "n shows the next page of cached stations."
        } else if model.page_number() > 1 {
            "Last cached page for this search. p goes back."
        } else {
            ""
        };
        if !footer.is_empty() {
            lines.push(Line::from(paint(model, Tone::Muted, footer)));
        }
    }
    lines
}

/// Linear order reads each row on its own, so every value keeps its label.
fn linear_station_lines(
    model: &Explorer,
    title: &str,
    name_cells: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![plain(title)];
    // Two lines per station keep every label whole at 80 columns.
    let window = (height.saturating_sub(1) / 2).max(1);
    let start = visible_start(model.selection(), model.rows().len(), window);
    for (offset, row) in model.rows().iter().enumerate().skip(start).take(window) {
        let mut name = vec![paint(
            model,
            Tone::Plain,
            format!(
                "{} {}",
                if offset == model.selection() {
                    ">"
                } else {
                    " "
                },
                clip(&row.name, name_cells.saturating_add(36))
            ),
        )];
        if row.favorite {
            name.push(paint(model, Tone::Favorite, " *"));
        }
        lines.push(Line::from(name));
        let health = row.directory_health.label();
        lines.push(Line::from(vec![
            paint(model, Tone::Plain, "    Directory health: "),
            paint(model, tone_for_state(health), health),
            paint(
                model,
                Tone::Plain,
                format!(
                    " | Directory languages: {}",
                    clip(known(&row.directory_languages), 30)
                ),
            ),
        ]));
    }
    lines
}

/// Station list columns for one terminal width. The name absorbs spare cells.
struct Columns {
    name: usize,
    languages: bool,
    id: bool,
}

impl Columns {
    fn for_width(width: usize) -> Self {
        if width >= LIST_FIXED + 24 + LIST_ID {
            Self {
                name: (width - LIST_FIXED - LIST_ID).min(48),
                languages: true,
                id: true,
            }
        } else if width >= LIST_FIXED + 20 {
            Self {
                name: (width - LIST_FIXED).min(40),
                languages: true,
                id: false,
            }
        } else {
            Self {
                name: width.saturating_sub(LIST_NARROW).clamp(8, 40),
                languages: false,
                id: false,
            }
        }
    }
}

fn empty_explore(model: &Explorer) -> Vec<Line<'static>> {
    if model
        .directory()
        .is_none_or(|directory| directory.maximum_stations == 0)
    {
        return vec![
            plain("Cached station coverage has not been observed."),
            plain("r reloads the local metadata read. No directory fetch is started."),
        ];
    }
    let cached = model
        .directory()
        .map_or(0, |directory| directory.cached_stations);
    if cached == 0 {
        return vec![
            plain(if model.favorites_only() {
                "No stations are cached. v shows all matches, then R and y fetch explicitly."
            } else {
                "No stations are cached yet. R then y explicitly fetches the applied filters."
            }),
            plain(&model.discovery_command()),
        ];
    }
    vec![
        plain("No cached station matches this search."),
        plain(&model.discovery_command()),
        plain(if model.favorites_only() {
            "F edits filters; x clears all; v toggles favorites. Cache matches only."
        } else {
            "R then y fetches these filters; F edits; C chooses a country; x clears."
        }),
    ]
}

fn line_width(text: &str) -> usize {
    Line::raw(text).width()
}

fn status_lines(text: &str, width: usize, max_rows: usize) -> Vec<Line<'static>> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || width == 0 || max_rows == 0 {
        return vec![plain("")];
    }
    if line_width(text) <= width {
        return vec![plain(text)];
    }
    if max_rows == 1 || words.iter().any(|word| line_width(word) > width) {
        return vec![plain(&middle_ellipsis(text, width))];
    }
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut on_second = false;
    for word in &words {
        let row = if on_second { &second } else { &first };
        if line_width(&join_words(row, word)) <= width {
            if on_second {
                second.push(*word);
            } else {
                first.push(*word);
            }
        } else if on_second {
            return vec![plain(&middle_ellipsis(text, width))];
        } else {
            on_second = true;
            second.push(*word);
        }
    }
    let mut rows = Vec::new();
    if !first.is_empty() {
        rows.push(plain(&first.join(" ")));
    }
    if !second.is_empty() {
        rows.push(plain(&second.join(" ")));
    }
    if rows.is_empty() {
        vec![plain("")]
    } else {
        rows
    }
}

fn join_words(words: &[&str], next: &str) -> String {
    if words.is_empty() {
        next.to_owned()
    } else {
        format!("{} {next}", words.join(" "))
    }
}

/// Keep the first and last whole words and mark the omitted middle.
fn middle_ellipsis(text: &str, width: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mark = "...";
    if width < mark.len() {
        return ".".repeat(width);
    }
    let Some(last) = words.last().copied() else {
        return String::new();
    };
    if line_width(last) + mark.len() >= width {
        return tail_words(last, width);
    }
    let mut tail = vec![last];
    let mut tail_used = line_width(last);
    for word in words.iter().rev().skip(1) {
        let extra = line_width(word) + 1;
        if tail_used + extra + mark.len() + 1 >= width {
            break;
        }
        tail_used += extra;
        tail.push(*word);
    }
    tail.reverse();
    let mut head = Vec::new();
    let mut head_used = 0usize;
    let room = width.saturating_sub(tail_used + mark.len() + 1);
    for word in &words {
        if tail.contains(word) && head.is_empty() && words.len() == tail.len() {
            break;
        }
        let extra = line_width(word) + usize::from(!head.is_empty());
        if head_used + extra > room {
            break;
        }
        if tail.contains(word) {
            break;
        }
        head.push(*word);
        head_used += extra;
    }
    if head.is_empty() {
        return tail.join(" ");
    }
    format!("{} {mark} {}", head.join(" "), tail.join(" "))
}

fn tail_words(word: &str, width: usize) -> String {
    let mut kept = String::new();
    for grapheme in word.chars().rev() {
        let next = format!("{grapheme}{kept}");
        if line_width(&next) > width {
            break;
        }
        kept = next;
    }
    kept
}

/// Truncates by terminal cells, then pads to exactly `cells` so columns align.
fn fit(text: &str, cells: usize) -> String {
    let clipped = clip(text, cells);
    let used = Line::raw(clipped.clone()).width();
    format!("{clipped}{}", " ".repeat(cells.saturating_sub(used)))
}

/// Truncates by terminal cells without splitting a grapheme. `~` marks a cut.
fn clip(text: &str, cells: usize) -> String {
    let clean = sanitize(text, 256);
    let line = Line::raw(clean.clone());
    if line.width() <= cells {
        return clean;
    }
    let mut out = String::new();
    let mut used = 0;
    for grapheme in line.styled_graphemes(Style::default()) {
        let next = Span::raw(grapheme.symbol).width();
        if used + next + 1 > cells {
            break;
        }
        out.push_str(grapheme.symbol);
        used += next;
    }
    if cells > 0 {
        out.push('~');
    }
    out
}

fn visible_start(selection: usize, len: usize, window: usize) -> usize {
    if len <= window {
        return 0;
    }
    selection.saturating_sub(window.saturating_sub(1))
}

fn live_lines(model: &Explorer) -> Vec<Line<'static>> {
    let mut lines = vec![
        plain(
            "The listen session is the receipt for a play, not directory health and not a recording.",
        ),
        plain("P then y plays a selected station; S stops."),
    ];
    if model.playback().is_none() {
        lines.push(plain("No listen receipt is loaded in this client."));
        lines.push(plain("Play a recording: sigy listen file RECORDING"));
        lines.push(plain("Hear a station: sigy play STATION_UUID"));
    }
    lines
}

fn recording_lines(model: &Explorer, width: usize, height: usize) -> Vec<Line<'static>> {
    if model.recordings().is_empty() {
        return vec![
            plain("No recordings yet. This view never starts one."),
            plain("Record from the command line: sigy record start ID --source REVISION."),
            plain("radio add registers a station as a source revision first."),
        ];
    }
    if height <= 5
        && let Some(recording) = model.selected_recording()
    {
        if recording.timeline.is_truncated() {
            return vec![
                plain(&recording.timeline.compact_axis()),
                plain(&recording.timeline.summary()),
            ];
        }
        let marks = timeline_marks(model);
        return vec![
            plain(&format!(
                "Recording {}: metadata",
                sanitize(&recording.id, 16)
            )),
            plain(&recording.timeline.compact_axis()),
            plain(&timeline_cells(&recording.timeline, width, marks)),
            plain(crate::explorer::timeline::Timeline::legend_for(
                marks, width,
            )),
        ];
    }
    let mut lines = vec![plain(
        "Recordings: arrows select; r reloads. Metadata snapshot. P then y plays; S stops.",
    )];
    let limit = height.saturating_sub(6).clamp(1, 8);
    let start = model
        .recording_selection()
        .saturating_sub(limit.saturating_sub(1));
    lines.extend(
        model
            .recordings()
            .iter()
            .enumerate()
            .skip(start)
            .take(limit)
            .map(|(index, recording)| {
                let format = recording.format.as_deref().unwrap_or("unknown");
                let state = sanitize(&recording.state, 16);
                let storage = sanitize(&recording.storage_state, 16);
                Line::from(vec![
                    paint(
                        model,
                        Tone::Plain,
                        if index == model.recording_selection() {
                            "> "
                        } else {
                            "  "
                        },
                    ),
                    paint(model, Tone::Muted, sanitize(&recording.id, 24)),
                    paint(model, Tone::Plain, " "),
                    paint(model, tone_for_state(&state), state),
                    paint(model, Tone::Plain, " "),
                    paint(model, tone_for_state(&storage), storage),
                    paint(model, Tone::Plain, format!(" format {format}")),
                ])
            }),
    );
    if let Some(recording) = model.selected_recording() {
        let marks = timeline_marks(model);
        lines.push(plain(&recording.timeline.summary()));
        lines.push(plain(&recording.timeline.axis()));
        if recording.timeline.is_truncated() {
            return lines;
        }
        lines.push(plain(&recording.timeline.detail()));
        lines.push(plain(&timeline_cells(&recording.timeline, width, marks)));
        lines.push(plain(crate::explorer::timeline::Timeline::legend_for(
            marks, width,
        )));
        lines.push(plain(
            "No waveform or live samples. The open tail is not yet published; r reloads.",
        ));
    }
    lines
}

fn timeline_marks(model: &Explorer) -> bool {
    !model.modes().linear && !model.modes().monochrome
}

fn timeline_cells(
    timeline: &crate::explorer::timeline::Timeline,
    width: usize,
    marks: bool,
) -> String {
    if marks {
        timeline.cells_marked(width, true)
    } else {
        timeline.cells(width)
    }
}

fn system_lines(model: &Explorer) -> Vec<Line<'static>> {
    let paid = crate::copy::paid_processing(model.provider_dispatch_available());
    let paid_tone = tone_for_state(if model.provider_dispatch_available() {
        "available"
    } else {
        "unavailable"
    });
    let dispatch = if model.dispatch_available() {
        "available"
    } else {
        "unavailable"
    };
    let mut lines = vec![
        Line::from(vec![
            paint(
                model,
                Tone::Plain,
                format!(
                    "Schema {} | SQLite {} | ",
                    model.schema_version(),
                    model.sqlite_version()
                ),
            ),
            paint(model, paid_tone, paid),
        ]),
        plain(&format!(
            "Modes: reduced motion {} | linear {} | mono {} | high contrast {}",
            on_off(model.modes().reduced_motion),
            on_off(model.modes().linear),
            on_off(model.modes().monochrome),
            on_off(model.modes().emphasis.high())
        )),
        quota_line(model),
        Line::from(paint(
            model,
            Tone::Plain,
            format!(
                "Captures: {} scheduled, {} active, {} interrupted, {} finished",
                model.captures_scheduled(),
                model.captures_active(),
                model.captures_interrupted(),
                model.captures_terminal()
            ),
        )),
        Line::from(vec![
            paint(model, Tone::Plain, "Recording dispatch: "),
            paint(model, tone_for_state(dispatch), dispatch),
            paint(
                model,
                Tone::Plain,
                if model.dispatch_available() {
                    ""
                } else {
                    " (needs the service and a configured decoder)"
                },
            ),
        ]),
    ];
    lines.extend(budget_lines(model));
    if let Some(refresh) = model
        .directory()
        .and_then(|directory| directory.refresh.as_ref())
    {
        let state = sanitize(&refresh.state, 16);
        let mut spans = vec![
            paint(
                model,
                Tone::Plain,
                format!("Fetch: {} ", sanitize(&refresh.id, 32)),
            ),
            paint(model, tone_for_state(&state), state),
            paint(
                model,
                Tone::Plain,
                format!(" accepted {} skipped {}", refresh.accepted, refresh.skipped),
            ),
        ];
        if let Some(detail) = refresh.failure.as_deref() {
            spans.push(paint(
                model,
                Tone::Fail,
                format!(" reason {}", sanitize(detail, 80)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn budget_lines(model: &Explorer) -> Vec<Line<'static>> {
    if model.budgets().is_empty() {
        return vec![plain("Budgets: none loaded")];
    }
    let mut lines = Vec::new();
    for budget in model.budgets().iter().take(4) {
        let mut spans = vec![paint(
            model,
            Tone::Plain,
            format!(
                "Budget {}: limit {}, available {}",
                sanitize(&budget.scope, 24),
                crate::copy::compact_usd(&budget.limit_usd),
                crate::copy::compact_usd(&budget.available_usd)
            ),
        )];
        if budget.frozen {
            spans.push(paint(model, Tone::Attention, " frozen"));
        }
        lines.push(Line::from(spans));
        lines.push(plain(&format!(
            "  settled {}, reserved {}",
            crate::copy::compact_usd(&budget.settled_usd),
            crate::copy::compact_usd(&budget.reserved_usd)
        )));
    }
    lines
}

#[cfg(test)]
mod tests {
    mod linked;
    mod raster;
    mod visual_qa;
    use super::{HELP_PRIMARY, HELP_SELECTION, render};
    use crate::explorer::state::{
        BudgetLine, Coordinates, Desk, DirectoryView, Explorer, Health, Key, Link, Modes,
        PlaybackView, QuotaView, RecordingLine, RefreshView, StationRow, Workspace,
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
                emphasis: crate::explorer::state::Emphasis::Standard,
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
                coordinates: None,
                metadata: None,
            }],
            next_after: None,
            catalog: None,
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
                timeline: crate::explorer::timeline::fixture(),
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
            let mut covered_until = 0;
            for x in 0..width {
                if x >= covered_until {
                    let symbol = view[(x, y)].symbol();
                    line.push_str(symbol);
                    let cells = ratatui::text::Span::raw(symbol).width();
                    covered_until = x.saturating_add(u16::try_from(cells).unwrap_or(1));
                }
            }
            lines.push(line.trim_end().to_owned());
        }
        (lines.join("\n"), colored)
    }

    fn globe_model(monochrome: bool) -> Explorer {
        let mut model = sample();
        let mut desk = sample_desk();
        desk.stations[0].coordinates = Some(Coordinates {
            latitude: 35.68,
            longitude: 139.69,
        });
        model.apply_desk(desk);
        if !monochrome {
            model = Explorer::new(
                Modes {
                    reduced_motion: true,
                    linear: false,
                    monochrome: false,
                    emphasis: crate::explorer::state::Emphasis::Standard,
                },
                10_000,
            );
            let mut desk = sample_desk();
            desk.stations[0].coordinates = Some(Coordinates {
                latitude: 35.68,
                longitude: 139.69,
            });
            model.apply_desk(desk);
        }
        model.handle(Key::Char('7'));
        model
    }

    fn braille(text: &str) -> usize {
        text.chars()
            .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
            .count()
    }

    #[test]
    fn globe_draws_coastline_station_and_an_explicit_instant() {
        let mut model = globe_model(true);
        assert_eq!(model.workspace(), Workspace::Globe);
        model.handle(Key::Char('c'));
        let (text, colored) = frame(&model, 160, 48);
        assert!(text.contains("Globe centered 140E 36N"), "{text}");
        assert!(
            text.contains("geometric night at 1970-01-01 00:00 UTC"),
            "{text}"
        );
        assert!(text.contains("1 of 1 stations on this page have directory coordinates"));
        assert!(braille(&text) > 200, "coastline and night were not drawn");
        assert!(text.contains('@'), "the selected station is not marked");
        assert!(text.contains("turn"), "{text}");
        assert!(!text.contains("rotate"), "{text}");
        assert!(!colored, "monochrome drew color");
    }

    #[test]
    fn rotating_away_hides_the_station_and_the_flat_map_shows_it() {
        let mut model = globe_model(true);
        model.handle(Key::Char('c'));
        for _ in 0..12 {
            model.handle(Key::Char('l'));
        }
        let (far, _) = frame(&model, 160, 48);
        assert!(far.contains("Globe centered 40W 36N"), "{far}");
        assert!(!far.contains('@'), "a far-side station was drawn");
        model.handle(Key::Char('m'));
        let (flat, _) = frame(&model, 160, 48);
        assert!(flat.contains("Flat map"), "{flat}");
        assert!(flat.contains('@'));
        assert!(braille(&flat) > 200);
    }

    #[test]
    fn globe_uses_color_only_when_monochrome_is_off_and_fits_small_terminals() {
        let model = globe_model(false);
        let (_, colored) = frame(&model, 120, 36);
        assert!(colored);
        let (small, _) = frame(&globe_model(true), 80, 24);
        assert!(small.contains("Globe centered"), "{small}");
        let (tiny, _) = frame(&globe_model(true), 40, 10);
        assert_ne!(tiny, "");
    }

    #[test]
    fn unmapped_stations_are_counted_not_placed() {
        let mut model = sample();
        model.handle(Key::Char('7'));
        let (text, _) = frame(&model, 160, 48);
        assert!(text.contains("0 of 1 stations on this page have directory coordinates"));
        assert!(!text.contains('@'));
    }

    #[test]
    fn eighty_by_twenty_four_keeps_search_identity_playback_and_help() {
        let model = sample();
        let (text, colored) = frame(&model, 80, 24);
        assert!(text.contains("Search:"), "{text}");
        assert!(text.contains("Selected station:"), "{text}");
        assert!(text.contains("Directory health: succeeded"), "{text}");
        assert!(text.contains("Directory languages: navajo"), "{text}");
        assert!(text.contains("Listen session:"), "{text}");
        assert!(text.contains("listen-1"), "{text}");
        assert!(text.contains("? more"), "{text}");
        assert!(!text.contains(HELP_SELECTION), "{text}");
        assert!(!text.contains("Quota:"), "{text}");
        assert!(text.contains("Cache: 1 of 10,000 stations"), "{text}");
        assert!(text.contains("Last fetch: completed"), "{text}");
        assert!(text.contains("refresh-demo"), "{text}");
        assert!(text.contains("observed 6s ago"), "{text}");
        assert!(text.contains("Favorites: 1"), "{text}");
        assert!(text.contains("Recording: rec-1 running"), "{text}");
        assert!(text.contains("service 42"), "{text}");
        assert!(text.contains("reduced motion on"), "{text}");
        assert!(
            text.contains("[1 Explore] 2 Live 3 Recordings 4 Monitors 5 Findings 6 System 7 Globe"),
            "{text}"
        );
        assert!(
            text.contains("Stations, page 1") && text.contains("| Directory health"),
            "{text}"
        );
        assert!(text.contains("> KTNN"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(!colored);
        let playback = text
            .lines()
            .find(|line| line.contains("Listen session:"))
            .unwrap_or("");
        assert!(!playback.contains("directory health"));
        assert!(!text.contains("Retained:"), "{text}");
        let row = text
            .lines()
            .find(|line| line.starts_with("> KTNN"))
            .unwrap_or("");
        assert!(
            row.contains(" *") && row.contains("| succeeded") && row.ends_with("| navajo"),
            "{row}"
        );
    }

    #[test]
    fn linear_order_prefixes_focus_and_still_has_no_color() {
        let mut linear = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: true,
                monochrome: true,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            10_000,
        );
        linear.apply_desk(sample_desk());
        linear.handle(crate::explorer::state::Key::Char('/'));
        let (text, colored) = frame(&linear, 80, 24);
        assert!(text.contains("Focus Search:"), "{text}");
        let search = text.find("Search:").unwrap_or(usize::MAX);
        let identity = text.find("Selected station:").unwrap_or(0);
        let playback = text.find("Listen session:").unwrap_or(usize::MAX);
        let help = text.find("Help:").unwrap_or(usize::MAX);
        assert!(help < search, "{text}");
        assert!(playback < search, "{text}");
        assert!(search < identity, "{text}");
        assert!(!colored);
        assert_eq!(linear.animation_frames(), 0);
        assert!(text.contains("> KTNN 東京 *\n"), "{text}");
        assert!(
            text.contains("    Directory health: succeeded | Directory languages: navajo"),
            "{text}"
        );
        assert!(text.contains("Stations, page 1"), "{text}");
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
            next_after: None,
            catalog: None,
            quota: model.quota(),
            recordings: model.recordings().to_vec(),
            playback: model.playback().cloned(),
        }
    }

    fn many_stations(monochrome: bool, count: usize) -> Explorer {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            10_000,
        );
        let mut desk = sample_desk();
        desk.stations = (0..count)
            .map(|index| StationRow {
                id: format!("00000000-0000-4000-8000-{index:012}"),
                name: if index == 1 {
                    "東京ラジオ放送局 long station name for alignment".into()
                } else {
                    format!("Station {index}")
                },
                favorite: index % 3 == 0,
                directory_health: Health::Succeeded,
                directory_languages: "navajo, english".into(),
                observed_ms: 4_000,
                hls: false,
                coordinates: None,
                metadata: None,
            })
            .collect();
        desk.next_after = Some("next".into());
        model.apply_desk(desk);
        model
    }

    fn row_cells(
        model: &Explorer,
        width: u16,
        height: u16,
        marker: &str,
    ) -> Vec<ratatui::buffer::Cell> {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).unwrap_or_else(|error| match error {});
        if terminal.draw(|ui| render(ui, model)).is_err() {
            return Vec::new();
        }
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .find(|&y| buffer[(0, y)].symbol() == marker)
            .map(|y| (0..width).map(|x| buffer[(x, y)].clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn the_station_list_fills_its_area_and_aligns_columns() {
        let model = many_stations(true, 16);
        let (small, _) = frame(&model, 80, 24);
        let (large, _) = frame(&model, 120, 36);
        let (huge, _) = frame(&model, 200, 60);
        let rows = |text: &str| {
            text.lines()
                .filter(|line| line.contains(" | succeeded"))
                .count()
        };
        assert_eq!(rows(&small), 13, "{small}");
        assert_eq!(rows(&large), 16, "{large}");
        assert_eq!(rows(&huge), 16, "{huge}");
        assert!(small.contains("Stations, page 1 (n more)"), "{small}");
        assert!(!small.contains("station ID"), "{small}");
        assert!(large.contains("| station ID"), "{large}");
        assert!(
            large.contains("00000000-0000-4000-8000-000000000015"),
            "{large}"
        );
        // Wide names are cut by cell width, so every health column starts in the same cell.
        let columns: Vec<usize> = large
            .lines()
            .filter(|line| line.contains(" | succeeded"))
            .map(|line| {
                let prefix = line.split(" | succeeded").next().unwrap_or("");
                ratatui::text::Line::raw(prefix.to_owned()).width()
            })
            .collect();
        assert!(
            columns.windows(2).all(|pair| pair[0] == pair[1]),
            "{columns:?}"
        );
        assert!(large.contains("東京ラジオ放送局"), "{large}");
        assert!(large.contains('~'), "{large}");
        let (compact, _) = frame(&model, 59, 20);
        assert!(
            compact.starts_with("Sigy [1 Explore] | service 42"),
            "{compact}"
        );
        assert!(rows(&compact) >= 9, "{compact}");
    }

    #[test]
    fn the_selected_row_is_reversed_only_while_the_list_has_focus() {
        for monochrome in [true, false] {
            let mut model = many_stations(monochrome, 4);
            model.handle(Key::Down);
            model.handle(Key::Down);
            let selected = row_cells(&model, 80, 24, ">");
            assert_ne!(selected, []);
            assert!(
                selected
                    .iter()
                    .take(40)
                    .all(|cell| cell.modifier.contains(ratatui::style::Modifier::REVERSED)),
                "{monochrome}"
            );
            // The bar is uniform in both modes, so reversed tones never read as blocks.
            assert!(
                selected
                    .iter()
                    .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset),
                "{monochrome}"
            );
            // Other rows are never reversed, so the selection does not depend on color.
            let (text, _) = frame(&model, 80, 24);
            assert!(text.contains("  Station 0"), "{text}");
            model.handle(Key::Char('/'));
            let unfocused = row_cells(&model, 80, 24, ">");
            assert!(
                unfocused
                    .iter()
                    .all(|cell| !cell.modifier.contains(ratatui::style::Modifier::REVERSED))
            );
        }
    }

    #[test]
    fn empty_views_say_what_to_run_next() {
        let mut model = sample();
        let mut desk = sample_desk();
        desk.stations.clear();
        desk.directory = Some(DirectoryView {
            cached_stations: 0,
            maximum_stations: 10_000,
            favorite_stations: 0,
            refresh: None,
        });
        desk.recordings.clear();
        if let Some(directory) = desk.directory.as_mut() {
            directory.cached_stations = 0;
        }
        model.apply_desk(desk.clone());
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("No stations are cached yet"), "{text}");
        assert!(text.contains("R then y"), "{text}");
        assert!(text.contains("sigy radio search --fetch"), "{text}");
        model.handle(Key::Char('3'));
        let (text, _) = frame(&model, 80, 24);
        assert!(
            text.contains("sigy record start ID --source REVISION"),
            "{text}"
        );
        model.handle(Key::Char('1'));
        if let Some(directory) = desk.directory.as_mut() {
            directory.cached_stations = 12;
        }
        model.apply_desk(desk);
        let (text, _) = frame(&model, 80, 24);
        assert!(
            text.contains("No cached station matches this search"),
            "{text}"
        );
        model.handle(Key::Char('2'));
        let mut desk = sample_desk();
        desk.playback = None;
        model.apply_desk(desk);
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("sigy listen file RECORDING"), "{text}");
    }

    #[test]
    fn retained_row_appears_only_after_an_explicit_listen() {
        let model = sample();
        let (absent, _) = frame(&model, 80, 24);
        assert!(!absent.contains("Retained:"), "{absent}");
        assert!(absent.contains("Listen session:"), "{absent}");
        let mut playing = sample();
        playing.begin_retained("reader-0123456789abcdef0123456789abcdef");
        let (text, _) = frame(&playing, 80, 24);
        assert!(text.contains("Retained: pending reader-"), "{text}");
        assert!(!text.contains("decoder "), "{text}");
        assert!(text.contains("Listen session:"), "{text}");
        assert!(playing.set_decoder_output("reader-0123456789abcdef0123456789abcdef", 1_500_000));
        let (timed, _) = frame(&playing, 100, 24);
        assert!(
            timed.contains("Retained: pending decoder 1500000 us reader-"),
            "{timed}"
        );
        assert!(!timed.contains("heard"), "{timed}");
        assert!(playing.show_paused("reader-0123456789abcdef0123456789abcdef", 1_500_000));
        let (paused, _) = frame(&playing, 100, 24);
        assert!(
            paused.contains("Retained: paused at 1500000 us reader-"),
            "{paused}"
        );
        assert!(!paused.contains("decoder "), "{paused}");
    }

    #[test]
    fn question_mark_lists_workspace_keys_and_escape_closes() {
        let mut model = sample();
        model.handle(Key::Char('?'));
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("Keys: Explore"), "{text}");
        assert!(text.contains("R then y fetch"), "{text}");
        model.handle(Key::Escape);
        let (closed, _) = frame(&model, 80, 24);
        assert!(!closed.contains("Keys: Explore"), "{closed}");
        model.handle(Key::Char('7'));
        model.handle(Key::Char('?'));
        let (globe, _) = frame(&model, 80, 24);
        assert!(globe.contains("Keys: Globe"), "{globe}");
        assert!(globe.contains("turn"), "{globe}");
        model.handle(Key::Escape);
        model.handle(Key::Char('3'));
        model.handle(Key::Char('?'));
        let (recordings, _) = frame(&model, 80, 40);
        assert!(recordings.contains("next H resumes"), "{recordings}");
        assert!(recordings.contains("seek 10 s"), "{recordings}");
        model.handle(Key::Escape);
        model.handle(Key::Char('5'));
        model.handle(Key::Char('?'));
        let (findings, _) = frame(&model, 80, 40);
        assert!(findings.contains("next H resumes"), "{findings}");
        assert!(findings.contains("seek 10 s"), "{findings}");
    }

    #[test]
    fn full_help_follows_the_workspace_and_fits_eighty_columns() {
        let mut seen = std::collections::BTreeSet::new();
        for workspace in Workspace::ALL {
            let mut model = sample();
            model.handle(Key::Char(workspace.digit()));
            let (text, _) = frame(&model, 80, 24);
            let help = text
                .lines()
                .find(|line| line.starts_with("Help:"))
                .unwrap_or("")
                .to_owned();
            assert!(help.ends_with("? more"), "{workspace:?}: {help}");
            assert!(ratatui::text::Line::raw(help.clone()).width() <= 80);
            seen.insert(help);
        }
        assert!(seen.len() >= 6, "{seen:?}");
        assert!(seen.iter().any(|help| help.contains("turn")));
        let mut system = sample();
        system.handle(Key::Char('6'));
        let (text, _) = frame(&system, 80, 24);
        assert!(text.contains("Captures: 0 scheduled, 1 active"), "{text}");
        assert!(
            text.contains("Quota: 5 B charged, 5 B reserved, 45 B available of 50 B"),
            "{text}"
        );
        assert!(text.contains("Modes: reduced motion on"), "{text}");
        assert!(
            text.contains("Budget global: limit $0.00, available $0.00"),
            "{text}"
        );
    }

    #[test]
    fn local_catalog_says_the_service_is_not_running() {
        let mut model = sample();
        model.note_link(Link::LocalCatalog);
        let (text, _) = frame(&model, 80, 24);
        assert!(
            text.starts_with("Sigy | no service, local catalog | reduced motion on"),
            "{text}"
        );
    }

    #[test]
    fn clipping_counts_cells_and_never_splits_wide_characters() {
        assert_eq!(super::fit("abc", 5), "abc  ");
        assert_eq!(super::clip("abcdef", 4), "abc~");
        assert_eq!(super::clip("東京ラジオ", 6), "東京~");
        assert_eq!(super::fit("東京ラジオ", 6), "東京~ ");
        assert_eq!(super::clip("a\u{1b}[2Jb", 8), "a[2Jb");
        assert_eq!(super::clip("abc", 0), "");
    }

    #[test]
    fn stale_catalog_restart_is_visible_at_minimum_size_and_during_editing()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut model = sample();
        let mut desk = sample_desk_from(&model);
        desk.catalog = Some(sigy_service::control::DirectoryCatalog {
            namespace: "a".repeat(32),
            revision: 5,
            comparison: sigy_service::discovery::ordered::COMPARISON.into(),
        });
        model.apply_desk(desk);
        for (width, height) in [(20, 8), (40, 12), (80, 24), (132, 40), (160, 50)] {
            requested_snapshot(
                &model,
                &format!("ordered-catalog-{width}x{height}.json"),
                width,
                height,
            )?;
        }
        model.note_cursor_changed();
        for (width, height) in [(20, 8), (40, 12), (80, 24), (132, 40), (160, 50)] {
            let (text, _) = frame(&model, width, height);
            assert!(
                text.contains("g first page") || text.contains("g opens the first cached page"),
                "{width}x{height}: {text}"
            );
            requested_snapshot(
                &model,
                &format!("ordered-stale-{width}x{height}.json"),
                width,
                height,
            )?;
        }
        model.handle(Key::Char('/'));
        for (width, height) in [(20, 8), (40, 12), (80, 24), (132, 40), (160, 50)] {
            assert!(
                frame(&model, width, height).0.contains("first page"),
                "{width}x{height}"
            );
            requested_snapshot(
                &model,
                &format!("ordered-stale-edit-{width}x{height}.json"),
                width,
                height,
            )?;
        }
        Ok(())
    }

    #[test]
    fn focused_context_keeps_back_scroll_and_identity_at_every_size()
    -> Result<(), Box<dyn std::error::Error>> {
        for (width, height) in [(20, 8), (40, 12), (80, 24), (132, 40), (160, 50)] {
            for linear in [false, true] {
                let mut model = sample();
                model.modes_for_context_fixture(linear);
                let before = model.current_search();
                let id = model.selected().map(|row| row.id.clone());
                assert_eq!(
                    model.handle(Key::Enter),
                    crate::explorer::state::Effect::None
                );
                let (text, colored) = frame(&model, width, height);
                assert!(
                    text.contains("Esc back") || text.contains("Esc/Backspace back"),
                    "{text}"
                );
                assert!(
                    text.contains("q quit") && text.contains("Up/Down"),
                    "{text}"
                );
                assert!(!colored);
                let Some(identity) = &id else {
                    panic!("selected");
                };
                // Compact UUID is wrapped rather than shortened.
                assert!(text.replace('\n', "").contains(identity), "{text}");
                assert_eq!(text.contains("Cached results"), !linear && width >= 112);
                requested_snapshot(
                    &model,
                    &format!("context-{width}x{height}-linear-{linear}.json"),
                    width,
                    height,
                )?;
                let mut seen = String::new();
                for _ in 0..500 {
                    frame(&model, width, height);
                    let Some(context) = &model.context else {
                        panic!("context");
                    };
                    seen.push_str(
                        &context
                            .visible(usize::from(width), 3, model.now_ms())
                            .iter()
                            .map(ToString::to_string)
                            .collect::<String>(),
                    );
                    assert_eq!(
                        model.handle(Key::Down),
                        crate::explorer::state::Effect::None
                    );
                }
                assert!(seen.contains("not inspected. i reads exact links"));
                model.note_cursor_changed();
                let (changed, _) = frame(&model, width, height);
                assert!(changed.contains("changed"), "{changed}");
                requested_snapshot(
                    &model,
                    &format!("context-changed-{width}x{height}-linear-{linear}.json"),
                    width,
                    height,
                )?;
                model.handle(Key::Escape);
                assert_eq!(model.current_search(), before);
                assert_eq!(model.selected().map(|row| row.id.clone()), id);
            }
        }
        Ok(())
    }

    #[test]
    fn compact_help_matches_active_controls() {
        for workspace in '1'..='7' {
            let mut model = sample();
            model.handle(Key::Char(workspace));
            for (width, height) in [(20, 8), (40, 10), (80, 24)] {
                let (text, _) = frame(&model, width, height);
                assert!(text.contains("? more"), "{workspace}: {text}");
            }
        }
        for workspace in ['1', '5'] {
            let mut model = sample();
            model.handle(Key::Char(workspace));
            model.handle(Key::Char('/'));
            for (width, height) in [(20, 8), (40, 10), (80, 24)] {
                let (text, _) = frame(&model, width, height);
                assert!(text.contains("Esc ends edit"), "{workspace}: {text}");
                assert!(!text.contains("q quit"), "{workspace}: {text}");
            }
            model.handle(Key::Escape);
            assert!(frame(&model, 40, 10).0.contains("? more"));
        }
    }

    #[test]
    fn eighty_by_twenty_four_chrome_is_four_rows_and_wide_detail_sits_beside_the_list() {
        let text = frame(&sample(), 80, 24).0;
        let lines: Vec<&str> = text.lines().collect();
        let content = lines
            .iter()
            .position(|line| line.contains("Cache:") || line.contains("Search:"))
            .unwrap_or(usize::MAX);
        assert!(content <= 4, "{text}");
        assert!(text.contains("Search:"), "{text}");
        assert!(text.contains("Selected station:"), "{text}");
        let wide = frame(&sample(), 132, 40).0;
        let wide_lines: Vec<&str> = wide.lines().collect();
        let stations = wide_lines
            .iter()
            .position(|line| line.contains("Stations, page"))
            .unwrap_or(usize::MAX);
        let selected = wide_lines
            .iter()
            .position(|line| line.contains("Selected station:"))
            .unwrap_or(0);
        assert!(
            stations.abs_diff(selected) <= 3,
            "stations {stations} selected {selected}\n{wide}"
        );
    }

    #[test]
    fn sixty_columns_show_every_workspace_and_status_keeps_its_tail() {
        let tabs = frame(&sample(), 60, 20).0;
        assert!(
            tabs.contains("[1 Exp] 2 Live 3 Rec 4 Mon 5 Find 6 Sys 7 Globe"),
            "{tabs}"
        );
        let mut model = sample();
        model.note_message(
            "Disconnected. Capture was not stopped. Last snapshot kept. r reconnects. Cause: upstream closed the socket",
        );
        let (text, _) = frame(&model, 40, 16);
        assert!(text.contains("upstream closed the socket"), "{text}");
        let words: Vec<&str> = model.status().split_whitespace().collect();
        for line in text.lines() {
            if line.contains("Disconnected") || line.contains("socket") || line.contains("Cause") {
                let trimmed = line.trim_end();
                let last = trimmed.split_whitespace().next_back().unwrap_or("");
                assert!(
                    last == "..." || words.contains(&last),
                    "status row ends mid-word: {trimmed}"
                );
            }
        }
    }

    #[test]
    fn low_space_quota_appears_in_the_header() {
        let mut model = sample();
        let mut desk = sample_desk();
        if let Some(quota) = desk.quota.as_mut() {
            quota.available = 1;
        }
        model.apply_desk(desk);
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("1 B free"), "{text}");
    }

    #[test]
    fn minimum_finding_view_keeps_details_and_linear_edit_tail_visible()
    -> Result<(), Box<dyn std::error::Error>> {
        for linear in [false, true] {
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
            model.handle(Key::Char('5'));
            let monitor = "m".repeat(128);
            let finding = format!("{}tail", "f".repeat(124));
            model.handle(Key::Char('/'));
            model.handle(Key::Paste(format!("{monitor} {finding}")));
            for width in [20, 40, 80] {
                let (text, _) = frame(&model, width, 10);
                assert!(text.contains("tail]"), "{linear}: {text}");
                assert!(text.contains("Esc ends edit"), "{linear}: {text}");
                requested_snapshot(
                    &model,
                    &format!("finding-edit-linear-{linear}-{width}.json"),
                    width,
                    10,
                )?;
            }
            model.handle(Key::Enter);
            model.findings = crate::explorer::finding::Browser::default();
            assert!(
                model
                    .findings
                    .load("world-news", "one", &crate::explorer::finding::fixture())
            );
            let (initial, _) = frame(&model, 20, 8);
            assert!(
                initial.lines().any(|line| line.starts_with("Finding:")),
                "{initial}"
            );
            let mut visited = String::new();
            for offset in 0..64 {
                let (text, _) = frame(&model, 20, 8);
                visited.push_str(&text);
                if matches!(offset, 0 | 11 | 16 | 18 | 25) {
                    requested_snapshot(
                        &model,
                        &format!("finding-20x8-linear-{linear}-{offset}.json"),
                        20,
                        8,
                    )?;
                }
                model.handle(Key::Down);
            }
            assert!(visited.contains("明日の会議です。"), "{visited}");
            assert!(visited.contains("The meeting is"), "{visited}");
            model.handle(Key::Char('/'));
            model.handle(Key::Enter);
            model.handle(Key::Escape);
            let mut error_visited = String::new();
            for _ in 0..64 {
                error_visited.push_str(&frame(&model, 20, 8).0);
                model.handle(Key::Down);
            }
            assert!(
                error_visited.contains("Enter exactly two"),
                "{error_visited}"
            );
            assert!(
                error_visited.contains("明日の会議です。"),
                "{error_visited}"
            );
        }
        Ok(())
    }

    #[test]
    fn filter_editor_renders_draft_applied_invalid_and_offline_states()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut model = sample();
        model.handle(Key::Char('F'));
        model.handle(Key::Tab);
        model.handle(Key::Paste("ca".into()));
        for (width, height) in [(20, 8), (59, 17), (80, 24), (132, 40)] {
            let (text, colored) = frame(&model, width, height);
            assert!(text.contains("[ca]"), "{text}");
            assert!(text.contains("Enter appl"), "{text}");
            assert!(text.contains("Esc cancel"), "{text}");
            assert!(!colored);
            assert!(!text.contains('\u{1b}'));
            requested_snapshot(
                &model,
                &format!("filters-draft-{width}x{height}.json"),
                width,
                height,
            )?;
        }
        let crate::explorer::state::Effect::Search(query) = model.handle(Key::Enter) else {
            panic!("valid draft reads cache");
        };
        let (pending, _) = frame(&model, 20, 8);
        assert!(pending.contains("Cache read pending"), "{pending}");
        assert!(model.apply_search(
            query.generation,
            model.rows().to_vec(),
            DirectoryView {
                cached_stations: 1,
                maximum_stations: 10_000,
                favorite_stations: 1,
                refresh: None,
            },
            None
        ));
        model.handle(Key::ClearInput);
        model.handle(Key::Paste("Congo".into()));
        model.handle(Key::Enter);
        let (invalid, _) = frame(&model, 20, 8);
        assert!(invalid.contains("Select country code"), "{invalid}");
        requested_snapshot(&model, "filters-invalid-20x8.json", 20, 8)?;
        let (wide, _) = frame(&model, 80, 24);
        assert!(wide.contains("Results: country=CA"), "{wide}");
        assert!(wide.contains("[Congo]"), "{wide}");
        model.note_disconnect(model.now_ms(), "fixture offline");
        let (offline, _) = frame(&model, 20, 8);
        assert!(offline.contains("Offline: read held"), "{offline}");
        requested_snapshot(&model, "filters-offline-20x8.json", 20, 8)?;
        model.handle(Key::Escape);
        let (results, _) = frame(&model, 80, 24);
        assert!(results.contains("Results: country=CA"), "{results}");
        assert!(!results.contains("France"), "{results}");
        requested_snapshot(&model, "filters-applied-offline-80x24.json", 80, 24)?;
        Ok(())
    }

    #[test]
    fn compact_filter_failure_keeps_the_applied_query_visible()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut model = sample();
        model.handle(Key::Char('F'));
        model.handle(Key::Tab);
        model.handle(Key::Paste("CA".into()));
        let crate::explorer::state::Effect::Search(query) = model.handle(Key::Enter) else {
            panic!("valid filters read cache");
        };
        assert!(model.fail_search(query.generation, "fixture cache failure"));
        let (text, _) = frame(&model, 20, 8);
        assert!(text.contains("Cache read failed"), "{text}");
        assert!(text.contains("Results:"), "{text}");
        assert!(!text.contains("Cache read pending"), "{text}");
        requested_snapshot(&model, "filters-failed-20x8.json", 20, 8)?;
        Ok(())
    }

    fn requested_snapshot(
        model: &Explorer,
        name: &str,
        width: u16,
        height: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(directory) = std::env::var_os("SIGY_TUI_SNAPSHOT_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory)?;
            write_snapshot(&directory.join(name), model, width, height)?;
        }
        Ok(())
    }

    #[test]
    fn offline_country_views_keep_selection_and_native_scripts_visible()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut model = sample();
        let mut desk = sample_desk();
        desk.stations.clear();
        model.apply_desk(desk);
        model.handle(Key::Char('C'));
        for (width, height) in [(20, 8), (80, 24), (132, 40)] {
            let (text, _) = frame(&model, width, height);
            assert!(text.contains("Countries"));
            assert!(text.contains(">AC"));
            assert!(text.contains("Enter pick") || text.contains("Enter sets draft"));
            requested_snapshot(
                &model,
                &format!("countries-world-{width}x{height}.json"),
                width,
                height,
            )?;
        }
        for _ in 0..15 {
            model.handle(Key::Down);
        }
        let code = model
            .search
            .picker
            .as_ref()
            .and_then(|picker| picker.page.as_ref().and_then(|page| page.entries.get(15)))
            .map(|entry| entry.code.as_str())
            .ok_or("selected")?;
        assert!(frame(&model, 20, 8).0.contains(&format!(">{code}")));
        model.handle(Key::Right);
        model.handle(Key::Tab);
        model.handle(Key::Tab);
        model.handle(Key::Tab);
        model.handle(Key::Paste("المغرب".into()));
        for (width, height) in [(20, 8), (80, 24), (132, 40)] {
            let text = frame(&model, width, height).0;
            assert!(text.contains("MA"));
            assert!(text.contains("المغرب"));
            requested_snapshot(
                &model,
                &format!("countries-arabic-{width}x{height}.json"),
                width,
                height,
            )?;
        }
        model.handle(Key::ClearInput);
        model.handle(Key::Paste("Re\u{301}union".into()));
        let text = frame(&model, 80, 24).0;
        assert!(text.contains("RE"));
        requested_snapshot(&model, "countries-combining-80x24.json", 80, 24)?;
        model.handle(Key::ClearInput);
        model.handle(Key::Paste("日本".into()));
        assert!(frame(&model, 20, 8).0.contains("JP"));
        requested_snapshot(&model, "countries-wide-script-20x8.json", 20, 8)?;
        Ok(())
    }

    #[test]
    fn monitor_workspace_is_visible_and_tiny_terminal_recovers() {
        let mut model = sample();
        model.handle(crate::explorer::state::Key::Char('4'));
        assert_eq!(model.workspace(), Workspace::Monitors);
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("Monitors: arrows select, Enter inspects"));
        let (small, _) = frame(&model, 10, 4);
        assert!(small.contains("Too small"), "{small}");
        assert!(small.contains("^C quits"), "{small}");
    }

    #[test]
    fn monitor_snapshots_render_coverage_and_keep_details_on_disconnect()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::explorer::state::Key;
        let mut model = sample();
        model.handle(Key::Char('4'));
        model.monitors = crate::explorer::monitor::fixture();
        for (width, height) in [(80, 24), (132, 40)] {
            for (label, scroll) in [("summary", 0), ("coverage", 5), ("passages", 4)] {
                for _ in 0..scroll {
                    model.handle(Key::Down);
                }
                let (text, _) = frame(&model, width, height);
                assert!(text.contains("Help:"));
                assert!(text.contains("Monitors"));
                assert!(!text.contains('\u{1b}'));
                if let Some(directory) = std::env::var_os("SIGY_TUI_SNAPSHOT_DIR") {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory)?;
                    let stem = format!("monitor-{width}x{height}-{label}");
                    std::fs::write(directory.join(format!("{stem}.txt")), &text)?;
                    write_snapshot(
                        &directory.join(format!("{stem}.json")),
                        &model,
                        width,
                        height,
                    )?;
                }
            }
            model.monitors = crate::explorer::monitor::fixture();
        }
        model.note_disconnect(model.now_ms(), "service stopped");
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("Monitor news v1"));
        assert!(text.contains("Last snapshot kept"));
        assert!(text.contains("Capture was not stopped"));
        Ok(())
    }

    #[test]
    fn finding_frames_preserve_scripts_and_revision_citations()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut model = sample();
        model.handle(Key::Char('5'));
        let page = crate::explorer::finding::fixture();
        for (width, height) in [(80, 24), (132, 40), (40, 10)] {
            model.findings = crate::explorer::finding::Browser::default();
            model.handle(Key::Char('/'));
            model.handle(Key::Paste("world-news one".into()));
            model.handle(Key::Enter);
            assert!(model.findings.load("world-news", "one", &page));
            let mut all = String::new();
            for offset in 0..32 {
                let (text, _) = frame(&model, width, height);
                assert!(!text.contains('\u{1b}'));
                all.push_str(&text);
                if matches!(offset, 0 | 3 | 9 | 13 | 18)
                    && let Some(directory) = std::env::var_os("SIGY_TUI_SNAPSHOT_DIR")
                {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory)?;
                    let stem = format!("finding-{width}x{height}-{offset}");
                    std::fs::write(directory.join(format!("{stem}.txt")), &text)?;
                    write_snapshot(
                        &directory.join(format!("{stem}.json")),
                        &model,
                        width,
                        height,
                    )?;
                }
                model.handle(Key::Down);
            }
            assert!(all.contains("world-news"), "{all}");
            assert!(all.contains("明日の会議です。"), "{all}");
            assert!(all.contains("The meeting is tomorrow."), "{all}");
            assert!(all.contains("revision 2"), "{all}");
        }
        model.note_disconnect(model.now_ms(), "service stopped");
        let text = model.findings.lines(20, 132, 0).join("\n");
        assert!(text.contains("work."));
        assert!(model.playback().is_some());
        Ok(())
    }

    fn write_snapshot(
        path: &std::path::Path,
        model: &Explorer,
        width: u16,
        height: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        terminal.draw(|frame| render(frame, model))?;
        let buffer = terminal.backend().buffer();
        let mut cells = Vec::new();
        for y in 0..height {
            let mut covered_until = 0;
            for x in 0..width {
                let cell = &buffer[(x, y)];
                let columns = ratatui::text::Span::raw(cell.symbol()).width();
                let continuation = x < covered_until;
                if !continuation {
                    covered_until = x.saturating_add(u16::try_from(columns).unwrap_or(1));
                }
                cells.push(serde_json::json!({"x": x, "y": y, "text": cell.symbol(), "width": columns, "continuation": continuation, "fg": format!("{:?}", cell.fg), "bg": format!("{:?}", cell.bg), "modifiers": format!("{:?}", cell.modifier)}));
            }
        }
        let frame = serde_json::json!({"cols": width, "rows": height, "method": "Ratatui TestBackend, synthetic fixture", "cells": cells});
        std::fs::write(path, serde_json::to_vec(&frame)?)?;
        Ok(())
    }

    #[test]
    fn recording_timeline_frames_keep_gaps_and_unpublished_tail_visible()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut model = sample();
        assert_eq!(
            model.handle(Key::Char('3')),
            crate::explorer::state::Effect::None
        );
        for (width, height) in [(80, 24), (132, 40), (40, 10)] {
            let (text, _) = frame(&model, width, height);
            if width >= 80 {
                assert!(text.contains("Recordings: arrows select"));
            } else {
                assert!(text.contains("Recording rec-1: metadata"));
                assert!(text.contains("# retained x unavail ! gap . open +mix"));
                assert!(text.contains("########xxxxxxxx!!!!!!!!"));
            }
            if width >= 80 {
                assert!(text.contains("Published intervals: 1 available, 1 unavailable; gaps 2"));
                assert!(
                    text.contains("# retained, x unavailable, ! gap, . not yet published, + mixed")
                );
                assert!(text.contains("[0,"));
                assert!(text.contains(")us"));
                assert!(text.contains("Selected recording: rec-1"));
                assert!(text.contains("Metadata snapshot"));
            }
            if let Some(directory) = std::env::var_os("SIGY_TUI_SNAPSHOT_DIR") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory)?;
                let stem = format!("recording-{width}x{height}");
                std::fs::write(directory.join(format!("{stem}.txt")), text)?;
                write_snapshot(
                    &directory.join(format!("{stem}.json")),
                    &model,
                    width,
                    height,
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn truncated_recording_metadata_never_renders_a_fabricated_clock_or_tail()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut desk = sample_desk();
        desk.recordings[0].timeline = crate::explorer::timeline::truncated_fixture();
        let mut model = sample();
        model.apply_desk(desk);
        model.handle(Key::Char('3'));
        for (width, height) in [(80, 24), (132, 40), (40, 10)] {
            let (text, _) = frame(&model, width, height);
            assert!(text.contains("Timeline unavailable: metadata"));
            assert!(text.contains("Partial counts:"));
            assert!(!text.contains("Clock:"));
            assert!(!text.contains("10000000us"));
            assert!(!text.contains("#:audio"));
            assert!(!text.contains("+ mixed"));
            assert!(!text.contains("!!!!"));
            if let Some(directory) = std::env::var_os("SIGY_TUI_SNAPSHOT_DIR") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory)?;
                let stem = format!("recording-limit-{width}x{height}");
                std::fs::write(directory.join(format!("{stem}.txt")), text)?;
                write_snapshot(
                    &directory.join(format!("{stem}.json")),
                    &model,
                    width,
                    height,
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn colored_mode_uses_color_only_when_monochrome_is_off() {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: false,
                linear: false,
                monochrome: false,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            10_000,
        );
        model.apply_desk(sample_desk());
        model.handle(crate::explorer::state::Key::Char('/'));
        let (text, colored) = frame(&model, 80, 24);
        assert!(colored, "{text}");
        assert_eq!(fg_of(&model, "succeeded"), Some(Color::Green));
        assert_eq!(fg_of_within(&model, "> KTNN", "★"), Some(Color::LightBlue));
        assert_eq!(fg_of(&model, "Sigy"), Some(Color::Cyan));
        assert_eq!(fg_of(&model, "Help:"), Some(Color::Reset));
        assert!(!text.contains('\u{1b}'));
        let disconnected = Explorer::new(
            Modes {
                reduced_motion: false,
                linear: false,
                monochrome: false,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            10_000,
        );
        assert_eq!(fg_of(&disconnected, "disconnected"), Some(Color::Red));
        assert_eq!(model.animation_frames(), 0);
    }

    #[test]
    fn linear_keeps_status_color_until_monochrome() {
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
        let (text, colored) = frame(&model, 80, 24);
        assert!(colored, "{text}");
        assert_eq!(fg_of(&model, "succeeded"), Some(Color::Green));
        assert!(text.contains("> KTNN 東京 *"), "{text}");
        assert!(!text.contains('★'), "{text}");
    }

    #[test]
    fn high_contrast_reverses_focus_and_keeps_failure_and_live_color() {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome: false,
                emphasis: crate::explorer::state::Emphasis::HighContrast,
            },
            10_000,
        );
        model.apply_desk(sample_desk());
        model.handle(Key::Char('/'));
        let (text, colored) = frame(&model, 80, 24);
        assert!(colored, "{text}");
        assert!(text.contains("high contrast on"), "{text}");
        assert!(text.contains('★'), "{text}");
        assert_eq!(fg_of(&model, "Sigy"), Some(Color::Reset));
        assert_eq!(fg_of(&model, "succeeded"), Some(Color::Reset));
        assert_eq!(
            fg_of_within(&model, "Listen session:", "running"),
            Some(Color::Green)
        );
        assert_eq!(
            fg_of_within(&model, "Recording:", "REC"),
            Some(Color::LightRed)
        );
        let search = line_cells(&model, "Search:");
        assert!(
            search
                .iter()
                .take(8)
                .all(|cell| cell.modifier.contains(ratatui::style::Modifier::REVERSED)),
            "focused search is reversed"
        );
    }

    fn line_cells(model: &Explorer, needle: &str) -> Vec<ratatui::buffer::Cell> {
        let mut terminal =
            Terminal::new(TestBackend::new(80, 24)).unwrap_or_else(|error| match error {});
        if terminal.draw(|ui| render(ui, model)).is_err() {
            return Vec::new();
        }
        let buffer = terminal.backend().buffer().clone();
        for y in 0..24 {
            let text: String = (0..80)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect();
            if text.contains(needle) {
                return (0..80).map(|x| buffer[(x, y)].clone()).collect();
            }
        }
        Vec::new()
    }

    fn fg_of(model: &Explorer, word: &str) -> Option<Color> {
        fg_of_within(model, "", word)
    }

    fn fg_of_within(model: &Explorer, line_has: &str, word: &str) -> Option<Color> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap_or_else(|error| match error {});
        let drawn = terminal.draw(|ui| render(ui, model));
        if drawn.is_err() {
            return None;
        }
        let view = terminal.backend().buffer().clone();
        for y in 0..24 {
            let mut text = String::new();
            let mut colors = Vec::new();
            for x in 0..80 {
                let cell = &view[(x, y)];
                text.push_str(cell.symbol());
                colors.push(cell.fg);
            }
            if !line_has.is_empty() && !text.contains(line_has) {
                continue;
            }
            if text.contains(word) {
                let mut acc = String::new();
                for (index, color) in colors.into_iter().enumerate() {
                    acc.push_str(view[(u16::try_from(index).unwrap_or(0), y)].symbol());
                    if acc.ends_with(word) {
                        return Some(color);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn help_and_docs_reject_retired_synonyms() {
        const RETIRED: &[&str] = &[
            "quick-play",
            "quick listen",
            "g restart",
            "Enter reads",
            "Enter read",
            "f save",
            "f saves",
        ];
        let mut blobs = vec![
            HELP_PRIMARY.to_owned(),
            HELP_SELECTION.to_owned(),
            include_str!("../../../../README.md").to_owned(),
            include_str!("../../../../docs/usage.md").to_owned(),
        ];
        let mut model = sample();
        for workspace in Workspace::ALL {
            model.handle(Key::Char(workspace.digit()));
            blobs.push(frame(&model, 80, 24).0);
            blobs.push(frame(&model, 40, 12).0);
        }
        model.note_cursor_changed();
        blobs.push(frame(&model, 80, 24).0);
        blobs.push(frame(&model, 30, 10).0);
        model.handle(Key::Char('/'));
        blobs.push(frame(&model, 80, 24).0);
        for blob in &blobs {
            for phrase in RETIRED {
                if let Some(index) = blob.find(phrase) {
                    let start = blob.floor_char_boundary(index.saturating_sub(48));
                    let end = blob.ceil_char_boundary(
                        index
                            .saturating_add(phrase.len().saturating_add(48))
                            .min(blob.len()),
                    );
                    panic!("retired synonym {phrase} near {}", &blob[start..end]);
                }
            }
        }
    }
}
