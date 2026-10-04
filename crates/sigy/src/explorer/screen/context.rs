//! Focused station layout, with a stable list beside it when space permits.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders, Paragraph},
};

use super::{color_on, connection_line, explore_lines, paint};
use crate::{
    explorer::{state::Explorer, text::sanitize},
    style::Tone,
};

pub(super) fn render(frame: &mut Frame<'_>, area: Rect, model: &Explorer) {
    let Some(context) = &model.context else {
        return;
    };
    let narrow = area.width < 60;
    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if area.width < 39 { 2 } else { 1 }),
        Constraint::Length(1),
        Constraint::Length(u16::from(!narrow)),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    let title = if narrow {
        ratatui::text::Line::from(paint(color_on(model), Tone::Accent, "Sigy | Inspect"))
    } else {
        connection_line(model, true)
    };
    frame.render_widget(Paragraph::new(title), sections[0]);
    frame.render_widget(
        Paragraph::new(
            crate::explorer::context::wrap(
                vec![format!("ID {}", sanitize(&context.row.id, 64))],
                usize::from(area.width),
            )
            .into_iter()
            .map(ratatui::text::Line::raw)
            .collect::<Vec<_>>(),
        ),
        sections[1],
    );
    let revision = context
        .catalog
        .as_ref()
        .map_or_else(|| "?".into(), |catalog| catalog.revision.to_string());
    let state = if model.restart_required() {
        "changed"
    } else {
        "snapshot"
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{state} | cache {revision} | page {}",
            model.page_number()
        )),
        sections[2],
    );
    if !narrow {
        frame.render_widget(
            Paragraph::new(super::filters::summary(&model.search.applied)),
            sections[3],
        );
    }
    let mut details = sections[4];
    if !model.modes().linear && area.width >= 112 && area.height >= 20 {
        let columns = Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(details);
        let block = Block::default()
            .title(" Cached results ")
            .borders(Borders::RIGHT);
        let inner = block.inner(columns[0]);
        frame.render_widget(block, columns[0]);
        frame.render_widget(
            Paragraph::new(explore_lines(
                model,
                usize::from(inner.width),
                usize::from(inner.height),
            )),
            inner,
        );
        details = columns[1];
    }
    frame.render_widget(
        Paragraph::new(context.visible(
            usize::from(details.width),
            usize::from(details.height),
            model.now_ms(),
        )),
        details,
    );
    footer(frame, sections[5], model);
}

fn footer(frame: &mut Frame<'_>, area: Rect, model: &Explorer) {
    let Some(context) = &model.context else {
        return;
    };
    let help = if area.width < 60 {
        format!("Esc back q quit i links\nUp/Down {}", context.position())
    } else if model.restart_required() {
        format!(
            "Esc back, then g restarts cached page 1 | q quit\nUp/Down scroll | {}",
            context.position()
        )
    } else {
        format!(
            "Esc/Backspace back | q quit | i linked history\nUp/Down scroll | {}",
            context.position()
        )
    };
    frame.render_widget(
        Paragraph::new(
            help.lines()
                .map(|line| {
                    ratatui::text::Line::from(paint(
                        color_on(model),
                        Tone::Accent,
                        sanitize(line, 160),
                    ))
                })
                .collect::<Vec<_>>(),
        ),
        area,
    );
}
