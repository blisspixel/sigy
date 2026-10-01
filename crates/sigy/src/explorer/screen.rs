//! List layout. Reduced motion, linear order, and monochrome draw no animation.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::style::{Tone, tone_for_state};

use crate::explorer::state::{Explorer, Focus, Workspace};
use crate::explorer::text::{age_label, known, sanitize};

pub const HELP_PRIMARY: &str =
    "Help: / search, arrows, f favorite, 4 monitors, 5 findings, 7 globe, q quit";
pub const HELP_SELECTION: &str = "Selection does not start audio, capture, refresh, or a click.";
pub const RECOVERY: &str = "Too small\n^C quits\nService\nstays up";
const FOCUS_PREFIX: &str = "Focus ";

pub fn render(frame: &mut Frame<'_>, model: &Explorer) {
    let area = frame.area();
    if area.width < 20 || area.height < 8 {
        frame.render_widget(Paragraph::new(RECOVERY), area);
        return;
    }
    if area.height < 16 || area.width < 60 {
        let sections = compact_sections(model);
        render_sections(frame, area, &sections, model);
        return;
    }
    let sections = full_sections(model);
    render_sections(frame, area, &sections, model);
}

fn render_sections(frame: &mut Frame<'_>, area: Rect, sections: &[Section], model: &Explorer) {
    let constraints: Vec<Constraint> = sections
        .iter()
        .map(|section| match section.height {
            Height::Fixed(lines) => Constraint::Length(lines),
            Height::Fill => Constraint::Min(1),
        })
        .collect();
    let rects = Layout::vertical(constraints).split(area);
    for (section, rect) in sections.iter().zip(rects.iter()) {
        if section.globe {
            render_globe(frame, *rect, section, model);
            continue;
        }
        let mut lines = if model.workspace() == Workspace::Monitors
            && matches!(section.height, Height::Fill)
        {
            model
                .monitors
                .lines(usize::from(rect.height))
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
        frame.render_widget(Paragraph::new(lines).style(section.style), *rect);
    }
}

fn render_globe(frame: &mut Frame<'_>, area: Rect, section: &Section, model: &Explorer) {
    let header_height = if area.height >= 6 { 3 } else { 2 };
    let rows =
        Layout::vertical([Constraint::Length(header_height), Constraint::Min(1)]).split(area);
    let mut header = super::globe::header_lines(model);
    if section.linear_focus
        && let Some(first) = header.first_mut()
    {
        first.spans.insert(0, Span::raw(FOCUS_PREFIX));
    }
    frame.render_widget(Paragraph::new(header).style(section.style), rows[0]);
    frame.render_widget(
        super::globe::GlobeWidget::new(model, color_on(model)),
        rows[1],
    );
}

struct Section {
    height: Height,
    lines: Vec<Line<'static>>,
    style: Style,
    linear_focus: bool,
    globe: bool,
}

enum Height {
    Fixed(u16),
    Fill,
}

fn full_sections(model: &Explorer) -> Vec<Section> {
    vec![
        one(connection_line(model), false, model),
        one(
            workspace_line(model),
            model.focus() == Focus::Workspaces,
            model,
        ),
        one(cache_line(model), false, model),
        one(search_line(model), model.focus() == Focus::Search, model),
        fill(body(model, 8), model.focus() == Focus::Results, model),
        many(identity_lines(model), model.focus() == Focus::Detail, model),
        one(
            playback_line(model),
            model.focus() == Focus::Playback,
            model,
        ),
        one(recording_line(model), false, model),
        one(quota_line(model), false, model),
        many(help_lines(model), model.focus() == Focus::Help, model),
        one(plain(model.status()), false, model),
    ]
}

fn compact_sections(model: &Explorer) -> Vec<Section> {
    vec![
        one(connection_line(model), false, model),
        one(search_line(model), model.focus() == Focus::Search, model),
        one(
            identity_primary(model),
            model.focus() == Focus::Detail,
            model,
        ),
        one(
            playback_line(model),
            model.focus() == Focus::Playback,
            model,
        ),
        one(
            plain(compact_help(model)),
            model.focus() == Focus::Help,
            model,
        ),
        one(plain(model.status()), false, model),
        fill(body(model, 4), model.focus() == Focus::Results, model),
    ]
}

fn one(line: Line<'static>, focused: bool, model: &Explorer) -> Section {
    section(Height::Fixed(1), vec![line], focused, model)
}

fn compact_help(model: &Explorer) -> &'static str {
    if editing(model) {
        return "Help: Esc ends edit";
    }
    match model.workspace() {
        Workspace::Findings => "Help: q quit, / IDs, o metadata",
        Workspace::Recordings => "Help: q quit, arrows select, r reload",
        Workspace::Monitors => "Help: q quit, Enter read, r reload",
        Workspace::Globe => "Help: q quit, h/j/k/l turn, m map",
        Workspace::Explore | Workspace::Live | Workspace::System => {
            "Help: q quit, / search, 1-7 views"
        }
    }
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
    }
}

fn focus_style(model: &Explorer, focused: bool) -> Style {
    if !focused || model.modes().linear {
        return Style::default();
    }
    if model.modes().monochrome {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

fn color_on(model: &Explorer) -> bool {
    !model.modes().monochrome && !model.modes().linear
}

fn plain(text: &str) -> Line<'static> {
    Line::from(text.to_owned())
}

fn paint(color: bool, tone: Tone, text: impl Into<String>) -> Span<'static> {
    let text = text.into();
    if !color || tone == Tone::Plain {
        return Span::raw(text);
    }
    let mut style = Style::default().fg(tone_color(tone));
    if tone == Tone::Accent {
        style = style.add_modifier(Modifier::BOLD);
    }
    Span::styled(text, style)
}

fn tone_color(tone: Tone) -> Color {
    match tone {
        Tone::Plain => Color::Reset,
        Tone::Accent => Color::Cyan,
        Tone::Ok => Color::Green,
        Tone::Warn => Color::Yellow,
        Tone::Fail => Color::Red,
        Tone::Muted => Color::DarkGray,
    }
}

fn connection_line(model: &Explorer) -> Line<'static> {
    let color = color_on(model);
    let (link, tone) = match model.link() {
        crate::explorer::state::Link::LocalCatalog => ("local catalog".to_owned(), Tone::Accent),
        crate::explorer::state::Link::Service {
            process_id,
            stopping: false,
        } => (format!("service {process_id}"), Tone::Ok),
        crate::explorer::state::Link::Service {
            process_id,
            stopping: true,
        } => (format!("service {process_id} stopping"), Tone::Warn),
        crate::explorer::state::Link::Disconnected => (
            format!("disconnected, snapshot {}", model.snapshot_age()),
            Tone::Fail,
        ),
    };
    let motion = on_off(model.modes().reduced_motion);
    let linear = on_off(model.modes().linear);
    let mono = on_off(model.modes().monochrome);
    let frames = model.animation_frames();
    Line::from(vec![
        paint(color, Tone::Accent, "Sigy"),
        paint(color, Tone::Plain, " | "),
        paint(color, tone, link),
        paint(
            color,
            Tone::Muted,
            format!(" | motion {motion} | linear {linear} | mono {mono} | frames {frames}"),
        ),
    ])
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn workspace_line(model: &Explorer) -> Line<'static> {
    let color = color_on(model);
    let mut spans = Vec::new();
    for (index, workspace) in [
        Workspace::Explore,
        Workspace::Live,
        Workspace::Recordings,
        Workspace::Monitors,
        Workspace::Findings,
        Workspace::Globe,
        Workspace::System,
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            spans.push(paint(color, Tone::Plain, " "));
        }
        let current = workspace == model.workspace();
        let label = if current {
            format!("[{}]", workspace.label())
        } else {
            workspace.label().to_owned()
        };
        let tone = if current {
            Tone::Accent
        } else if workspace.available() {
            Tone::Plain
        } else {
            Tone::Muted
        };
        spans.push(paint(color, tone, label));
    }
    Line::from(spans)
}

fn cache_line(model: &Explorer) -> Line<'static> {
    let color = color_on(model);
    let Some(directory) = model.directory() else {
        return plain(
            "Partial cache unknown | refresh none | observed unknown | favorites unknown",
        );
    };
    let observed = model.selected().map_or_else(
        || "observed unknown".to_owned(),
        |row| format!("observed {}", age_label(row.observed_ms, model.now_ms())),
    );
    let mut spans = vec![paint(
        color,
        Tone::Plain,
        format!(
            "Partial cache {}/{} | ",
            directory.cached_stations, directory.maximum_stations
        ),
    )];
    match directory.refresh.as_ref() {
        Some(refresh) => {
            let state = sanitize(&refresh.state, 16);
            spans.push(paint(
                color,
                Tone::Plain,
                format!("{} ", sanitize(&refresh.id, 16)),
            ));
            spans.push(paint(color, tone_for_state(&state), state));
        }
        None => spans.push(paint(color, Tone::Plain, "refresh none")),
    }
    let favorites = directory.favorite_stations;
    let favorite_tone = if favorites > 0 {
        Tone::Warn
    } else {
        Tone::Plain
    };
    spans.push(paint(color, Tone::Plain, format!(" | {observed} | ")));
    spans.push(paint(
        color,
        favorite_tone,
        format!("favorites {favorites}"),
    ));
    Line::from(spans)
}

fn search_line(model: &Explorer) -> Line<'static> {
    if model.workspace() == Workspace::Findings {
        return plain(&format!("Finding IDs: [{}]", model.findings.query()));
    }
    let color = color_on(model);
    let mut spans = vec![
        paint(color, Tone::Accent, "Search: ["),
        paint(color, Tone::Plain, model.query().to_owned()),
        paint(color, Tone::Plain, "]"),
    ];
    if model.favorites_only() {
        spans.push(paint(color, Tone::Warn, " favorites"));
    }
    Line::from(spans)
}

fn identity_lines(model: &Explorer) -> Vec<Line<'static>> {
    let color = color_on(model);
    if model.workspace() == Workspace::Findings {
        return vec![
            plain("Stored citations: named lookup, / edits MONITOR FINDING."),
            plain("o reads original recording metadata. Arrows scroll."),
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
            plain("Selection starts no playback, capture or processing."),
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
                paint(color, Tone::Accent, "Selected source: "),
                paint(color, Tone::Plain, "none"),
            ]),
            Line::from(vec![
                paint(color, Tone::Plain, "directory health: "),
                paint(color, tone_for_state("unknown"), "unknown"),
            ]),
            Line::from(vec![
                paint(color, Tone::Plain, "directory languages: "),
                paint(color, Tone::Plain, "unknown"),
            ]),
        ];
    };
    let mut source = vec![
        paint(color, Tone::Accent, "Selected source: "),
        paint(color, Tone::Plain, sanitize(&row.name, 60)),
    ];
    if row.hls {
        source.push(paint(
            color,
            Tone::Plain,
            " Directory marks HLS. This list does not open it.",
        ));
    }
    vec![
        Line::from(source),
        Line::from(vec![
            paint(color, Tone::Plain, "directory health: "),
            paint(
                color,
                tone_for_state(row.directory_health.label()),
                row.directory_health.label(),
            ),
        ]),
        Line::from(vec![
            paint(color, Tone::Plain, "directory languages: "),
            paint(
                color,
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
        .unwrap_or_else(|| plain("Selected source: none"))
}

fn playback_line(model: &Explorer) -> Line<'static> {
    let color = color_on(model);
    let Some(session) = model.playback() else {
        return Line::from(vec![
            paint(color, Tone::Accent, "Playback: "),
            paint(color, Tone::Plain, "none"),
        ]);
    };
    let format = session.format.as_deref().unwrap_or("unknown");
    let state = sanitize(&session.state, 16);
    let mut spans = vec![
        paint(color, Tone::Accent, "Playback: listen "),
        paint(color, Tone::Muted, sanitize(&session.id, 24)),
        paint(color, Tone::Plain, " "),
        paint(color, tone_for_state(&state), state),
        paint(color, Tone::Plain, format!(" format {format} revision ")),
        paint(color, Tone::Muted, sanitize(&session.source_revision, 24)),
    ];
    if let Some(detail) = session.failure.as_deref() {
        spans.push(paint(
            color,
            Tone::Fail,
            format!(" failure {}", sanitize(detail, 40)),
        ));
    }
    Line::from(spans)
}

fn recording_line(model: &Explorer) -> Line<'static> {
    let color = color_on(model);
    let Some(recording) = model
        .recordings()
        .iter()
        .find(|recording| recording.active())
    else {
        return Line::from(vec![
            paint(color, Tone::Accent, "Recording: "),
            paint(color, Tone::Plain, "none"),
        ]);
    };
    let state = sanitize(&recording.state, 16);
    Line::from(vec![
        paint(color, Tone::Accent, "Recording: "),
        paint(color, Tone::Muted, sanitize(&recording.id, 24)),
        paint(color, Tone::Plain, " "),
        paint(color, tone_for_state(&state), state),
        paint(color, Tone::Plain, " continues in the service"),
    ])
}

fn quota_line(model: &Explorer) -> Line<'static> {
    let color = color_on(model);
    let Some(quota) = model.quota() else {
        return Line::from(vec![
            paint(color, Tone::Accent, "Quota: "),
            paint(color, Tone::Plain, "unknown"),
        ]);
    };
    Line::from(vec![
        paint(color, Tone::Accent, "Quota: "),
        paint(
            color,
            Tone::Plain,
            format!(
                "charged {} reserved {} available {} of {}",
                quota.charged, quota.reserved, quota.available, quota.quota
            ),
        ),
    ])
}

fn help_lines(model: &Explorer) -> Vec<Line<'static>> {
    let primary = if editing(model) {
        "Help: Esc ends edit; Enter reads; Ctrl-C quits"
    } else {
        HELP_PRIMARY
    };
    vec![plain(primary), plain(HELP_SELECTION)]
}

fn body(model: &Explorer, limit: usize) -> Vec<Line<'static>> {
    let lines = if model.workspace().available() {
        match model.workspace() {
            Workspace::Explore => explore_lines(model),
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
    let color = color_on(model);
    let text = match model.workspace() {
        Workspace::Findings => {
            "Stored findings: use sigy monitor finding MONITOR FINDING show. Listing is pending."
        }
        Workspace::Monitors => "Monitors: Enter reads coverage and literal passages.",
        Workspace::Explore
        | Workspace::Live
        | Workspace::Recordings
        | Workspace::Globe
        | Workspace::System => "This workspace has no operation yet.",
    };
    vec![Line::from(paint(color, Tone::Muted, text))]
}

fn explore_lines(model: &Explorer) -> Vec<Line<'static>> {
    let color = color_on(model);
    let mut lines = vec![Line::from(paint(
        color,
        Tone::Muted,
        "This view is the list. Press 7 for the globe and day/night map.",
    ))];
    if model.rows().is_empty() {
        lines.push(plain(
            "No cached stations. radio search reads this cache. radio refresh is separate.",
        ));
        return lines;
    }
    let start = visible_start(model.selection(), model.rows().len(), 8);
    for (offset, row) in model.rows().iter().enumerate().skip(start).take(8) {
        let marker = if offset == model.selection() {
            ">"
        } else {
            " "
        };
        let marker_tone = if offset == model.selection() {
            Tone::Accent
        } else {
            Tone::Plain
        };
        let mut spans = vec![
            paint(color, marker_tone, marker),
            paint(color, Tone::Plain, " "),
            paint(color, Tone::Plain, sanitize(&row.name, 24)),
        ];
        if row.favorite {
            spans.push(paint(color, Tone::Warn, " favorite"));
        }
        spans.push(paint(color, Tone::Plain, " | directory health "));
        spans.push(paint(
            color,
            tone_for_state(row.directory_health.label()),
            row.directory_health.label(),
        ));
        spans.push(paint(color, Tone::Plain, " | "));
        spans.push(paint(color, Tone::Muted, sanitize(&row.id, 36)));
        lines.push(Line::from(spans));
    }
    lines
}

fn visible_start(selection: usize, len: usize, window: usize) -> usize {
    if len <= window {
        return 0;
    }
    selection.saturating_sub(window.saturating_sub(1))
}

fn live_lines(model: &Explorer) -> Vec<Line<'static>> {
    let mut lines = vec![
        plain("Playback is the listen receipt, not directory health and not a recording."),
        plain("listen file and listen source are CLI operations. Selection does not start them."),
    ];
    if model.playback().is_none() {
        lines.push(plain("No listen receipt is loaded in this client."));
    }
    lines
}

fn recording_lines(model: &Explorer, width: usize, height: usize) -> Vec<Line<'static>> {
    let color = color_on(model);
    if model.recordings().is_empty() {
        return vec![plain(
            "No recordings. record start is a separate CLI operation.",
        )];
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
        return vec![
            plain(&format!(
                "Recording {}: metadata",
                sanitize(&recording.id, 16)
            )),
            plain(&recording.timeline.compact_axis()),
            plain(&recording.timeline.cells(width)),
            plain("#:audio x:gone !:gap .:open +:mixed"),
        ];
    }
    let mut lines = vec![plain(
        "Recordings: arrows select; r reloads. Metadata snapshot only.",
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
                        color,
                        Tone::Accent,
                        if index == model.recording_selection() {
                            "> "
                        } else {
                            "  "
                        },
                    ),
                    paint(color, Tone::Muted, sanitize(&recording.id, 24)),
                    paint(color, Tone::Plain, " "),
                    paint(color, tone_for_state(&state), state),
                    paint(color, Tone::Plain, " "),
                    paint(color, tone_for_state(&storage), storage),
                    paint(color, Tone::Plain, format!(" format {format}")),
                ])
            }),
    );
    if let Some(recording) = model.selected_recording() {
        lines.push(plain(&recording.timeline.summary()));
        lines.push(plain(&recording.timeline.axis()));
        if recording.timeline.is_truncated() {
            return lines;
        }
        lines.push(plain(&recording.timeline.cells(width)));
        lines.push(plain(
            "# available, x unavailable, ! gap, . unpublished, + mixed",
        ));
        lines.push(plain(
            "No waveform or live samples. The open tail is unpublished; r reloads.",
        ));
    }
    lines
}

fn system_lines(model: &Explorer) -> Vec<Line<'static>> {
    let color = color_on(model);
    let provider = if model.provider_dispatch_available() {
        "available"
    } else {
        "unavailable"
    };
    let dispatch = if model.dispatch_available() {
        "available"
    } else {
        "unavailable"
    };
    let mut lines = vec![
        Line::from(vec![
            paint(
                color,
                Tone::Plain,
                format!(
                    "Schema {} | SQLite {} | provider dispatch ",
                    model.schema_version(),
                    model.sqlite_version()
                ),
            ),
            paint(color, tone_for_state(provider), provider),
        ]),
        Line::from(vec![
            paint(
                color,
                Tone::Plain,
                format!(
                    "Captures scheduled {} active {} interrupted {} terminal {} | recording dispatch ",
                    model.captures_scheduled(),
                    model.captures_active(),
                    model.captures_interrupted(),
                    model.captures_terminal()
                ),
            ),
            paint(color, tone_for_state(dispatch), dispatch),
        ]),
    ];
    if model.budgets().is_empty() {
        lines.push(plain("Budgets: none loaded"));
    }
    for budget in model.budgets().iter().take(4) {
        let mut spans = vec![paint(
            color,
            Tone::Plain,
            format!(
                "Budget {} limit {} settled {} reserved {} available {}",
                sanitize(&budget.scope, 24),
                budget.limit_usd,
                budget.settled_usd,
                budget.reserved_usd,
                budget.available_usd
            ),
        )];
        if budget.frozen {
            spans.push(paint(color, Tone::Warn, " frozen"));
        }
        lines.push(Line::from(spans));
    }
    if let Some(refresh) = model
        .directory()
        .and_then(|directory| directory.refresh.as_ref())
    {
        let state = sanitize(&refresh.state, 16);
        let mut spans = vec![
            paint(
                color,
                Tone::Plain,
                format!("Refresh {} ", sanitize(&refresh.id, 32)),
            ),
            paint(color, tone_for_state(&state), state),
            paint(
                color,
                Tone::Plain,
                format!(" accepted {} skipped {}", refresh.accepted, refresh.skipped),
            ),
        ];
        if let Some(detail) = refresh.failure.as_deref() {
            spans.push(paint(
                color,
                Tone::Fail,
                format!(" reason {}", sanitize(detail, 80)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

#[cfg(test)]
mod tests {
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
        assert!(!tiny.is_empty());
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
        assert!(text.contains("Monitors"), "{text}");
        assert!(text.contains("Findings"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(!colored);
        let playback = text
            .lines()
            .find(|line| line.contains("Playback:"))
            .unwrap_or("");
        assert!(!playback.contains("directory health"));
        assert!(text.contains("Press 7 for the globe"));
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
    fn compact_help_matches_active_controls() {
        for workspace in '1'..='7' {
            let mut model = sample();
            model.handle(Key::Char(workspace));
            for (width, height) in [(20, 8), (40, 10), (80, 24)] {
                let (text, _) = frame(&model, width, height);
                assert!(text.contains("q quit"), "{workspace}: {text}");
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
            assert!(frame(&model, 40, 10).0.contains("q quit"));
        }
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
    fn monitor_workspace_is_visible_and_tiny_terminal_recovers() {
        let mut model = sample();
        model.handle(crate::explorer::state::Key::Char('4'));
        assert_eq!(model.workspace(), Workspace::Monitors);
        let (text, _) = frame(&model, 80, 24);
        assert!(text.contains("Monitors: arrows select, Enter reads"));
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
                assert!(text.contains("#:audio x:gone !:gap .:open +:mixed"));
                assert!(text.contains("########xxxxxxxx!!!!!!!!"));
            }
            if width >= 80 {
                assert!(text.contains("Published intervals: 1 available, 1 unavailable; gaps 2"));
                assert!(text.contains("# available, x unavailable, ! gap, . unpublished, + mixed"));
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
            },
            10_000,
        );
        model.apply_desk(sample_desk());
        model.handle(crate::explorer::state::Key::Char('/'));
        let (text, colored) = frame(&model, 80, 24);
        assert!(colored, "{text}");
        assert_eq!(fg_of(&model, "succeeded"), Some(Color::Green));
        assert_eq!(fg_of(&model, "favorite"), Some(Color::Yellow));
        assert_eq!(fg_of(&model, "Sigy"), Some(Color::Cyan));
        assert_eq!(fg_of(&model, "Help:"), Some(Color::Reset));
        assert!(!text.contains('\u{1b}'));
        let disconnected = Explorer::new(
            Modes {
                reduced_motion: false,
                linear: false,
                monochrome: false,
            },
            10_000,
        );
        assert_eq!(fg_of(&disconnected, "disconnected"), Some(Color::Red));
        assert_eq!(model.animation_frames(), 0);
    }

    fn fg_of(model: &Explorer, word: &str) -> Option<Color> {
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
}
