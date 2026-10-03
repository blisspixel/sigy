//! An explicit filter workbench over the cached directory query.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::Paragraph,
};

use crate::explorer::{
    search::{Field, Scope},
    state::{Explorer, Link},
};
use crate::style::Tone;

pub(super) fn render(frame: &mut Frame<'_>, area: Rect, model: &Explorer) {
    let compact = area.width < 60 || area.height < 18;
    let panes = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(if compact { 3 } else { 4 }),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(if compact {
            "Filters: local cache"
        } else {
            "Filters: cached stations"
        }),
        panes[0],
    );
    let fields = if compact {
        vec![model.search.field()]
    } else {
        Field::ALL.to_vec()
    };
    let mut lines = Vec::new();
    for field in fields {
        let active = field == model.search.field();
        let label = if compact {
            field.label().to_owned()
        } else {
            format!("{}{:<26}", if active { "> " } else { "  " }, field.label())
        };
        if compact {
            lines.push(Line::raw(super::clip(&label, usize::from(area.width))));
            lines.push(Line::raw(format!(
                "[{}]",
                tail(
                    &model.search.value(field),
                    usize::from(area.width).saturating_sub(2)
                )
            )));
        } else {
            let room = usize::from(area.width).saturating_sub(30);
            lines.push(Line::from(super::paint(
                super::color_on(model),
                if active { Tone::Accent } else { Tone::Plain },
                format!("{label}[{}]", tail(&model.search.value(field), room)),
            )));
        }
    }
    if compact {
        let status = if matches!(model.link(), Link::Disconnected) {
            "Offline: read held"
        } else if let Some(issue) = model.search.validation {
            issue.compact()
        } else if model.search.failed {
            "Cache read failed"
        } else if model.search.pending() {
            "Cache read pending"
        } else {
            "Editing draft only"
        };
        lines.push(Line::raw(status));
        lines.push(Line::raw(super::clip(
            &summary(&model.search.applied),
            usize::from(area.width),
        )));
    }
    if !compact {
        lines.push(Line::raw(""));
        lines.push(Line::raw(
            "Country: name or code; Right on that field opens the offline picker.",
        ));
        lines.push(Line::raw(
            "Language/tag match whole directory labels, not observed speech.",
        ));
        lines.push(Line::raw(
            "Health is the directory's check, not local playback validation.",
        ));
        lines.push(Line::raw(
            "Esc returns to results; x there clears every filter.",
        ));
        lines.push(super::playback_line(model));
        lines.push(super::recording_line(model));
    }
    frame.render_widget(Paragraph::new(lines), panes[1]);
    let footer = if compact {
        vec![
            Line::raw("Enter apply"),
            Line::raw("Esc cancel Tab field"),
            Line::raw("Space toggle Ctrl-U"),
        ]
    } else {
        vec![
            Line::raw("Enter applies; Esc cancels. Tab/Shift-Tab or arrows select fields."),
            Line::raw("Space toggles a yes/no field. Ctrl-U clears the selected field."),
            Line::raw(summary(&model.search.applied)),
            Line::raw(model.status().to_owned()),
        ]
    };
    frame.render_widget(Paragraph::new(footer), panes[2]);
}

pub(super) fn summary(scope: &Scope) -> String {
    let filter = &scope.filter;
    let mut parts = Vec::new();
    if !filter.country.is_empty() {
        parts.push(format!("country={}", filter.country));
    }
    if !filter.language.is_empty() {
        parts.push(format!("language={}", filter.language));
    }
    if !filter.tag.is_empty() {
        parts.push(format!("tag={}", filter.tag));
    }
    if filter.healthy_only {
        parts.push("upstream=ok".into());
    }
    if scope.favorites_only {
        parts.push("favorites".into());
    }
    if !filter.name.is_empty() {
        parts.push(format!("name={}", filter.name));
    }
    if parts.is_empty() {
        "Results: all cached stations".into()
    } else {
        format!("Results: {}", parts.join(" | "))
    }
}

pub(super) fn tail(value: &str, cells: usize) -> String {
    if cells == 0 {
        return String::new();
    }
    let line = Line::raw(value);
    if line.width() <= cells {
        return value.to_owned();
    }
    let graphemes = line
        .styled_graphemes(ratatui::style::Style::default())
        .collect::<Vec<_>>();
    let mut used = 0;
    let mut first = graphemes.len();
    for (index, grapheme) in graphemes.iter().enumerate().rev() {
        let width = ratatui::text::Span::raw(grapheme.symbol).width();
        if used + width > cells.saturating_sub(1) {
            break;
        }
        used += width;
        first = index;
    }
    let mut output = String::from("~");
    for grapheme in &graphemes[first..] {
        output.push_str(grapheme.symbol);
    }
    output
}
