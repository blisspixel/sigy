//! Centered action menu and confirmation dialog.
//!
//! The screen underneath stays drawn. Below 40 by 12 the status line is the
//! whole prompt, so this module draws nothing there.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::explorer::actions::{self, MenuItem};
use crate::explorer::state::{DialogCopy, Explorer};

pub(super) fn render(frame: &mut Frame<'_>, area: Rect, model: &Explorer) {
    if area.width < 40 || area.height < 12 {
        return;
    }
    if model.menu_open() {
        render_menu(frame, area, model);
        return;
    }
    if let Some(dialog) = model.dialog_copy() {
        render_dialog(frame, area, model, &dialog);
    }
}

fn render_menu(frame: &mut Frame<'_>, area: Rect, model: &Explorer) {
    let items = actions::menu_items(model);
    if items.is_empty() {
        return;
    }
    let cursor = model.menu_cursor().unwrap_or(0).min(items.len() - 1);
    let bordered = !model.modes().linear;
    let host = above_status(area);
    let width = popup_width(host);
    let inner_width = width.saturating_sub(u16::from(bordered) * 2);
    let budget = line_budget(host, bordered, u16::from(!bordered));
    let start = super::visible_start(cursor, items.len(), usize::from(budget));
    let mut lines = Vec::new();
    if !bordered {
        lines.push(plain_line("Actions", usize::from(inner_width), false));
    }
    for (index, item) in items
        .iter()
        .enumerate()
        .skip(start)
        .take(usize::from(budget))
    {
        lines.push(plain_line(
            &row_text(model, item),
            usize::from(inner_width),
            index == cursor,
        ));
    }
    paint(frame, host, width, lines, bordered, " Actions ");
}

fn render_dialog(frame: &mut Frame<'_>, area: Rect, model: &Explorer, dialog: &DialogCopy) {
    let bordered = !model.modes().linear;
    let host = above_status(area);
    let width = popup_width(host);
    let inner_width = usize::from(width.saturating_sub(u16::from(bordered) * 2));
    let budget = usize::from(line_budget(host, bordered, u16::from(!bordered)));
    let lines = dialog_lines(dialog, inner_width, budget.max(1), !bordered);
    let title = format!(" {} ", dialog.title);
    paint(frame, host, width, lines, bordered, &title);
}

fn dialog_lines(
    dialog: &DialogCopy,
    width: usize,
    max_lines: usize,
    title: bool,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if title {
        rows.push(dialog.title.to_owned());
    }
    rows.push(super::clip(&dialog.target, width));
    let effect_budget = max_lines
        .saturating_sub(rows.len().saturating_add(1))
        .max(1);
    rows.extend(wrap_text(dialog.effect, width, effect_budget));
    rows.push(super::clip(dialog.buttons, width));
    rows.truncate(max_lines);
    rows.into_iter()
        .map(|text| plain_line(&text, width, false))
        .collect()
}

fn row_text(model: &Explorer, item: &MenuItem) -> String {
    let label = actions::menu_label(item);
    match model.menu_reason(item.slot) {
        Some(reason) if !reason.is_empty() => format!("{label}  {reason}"),
        _ => label,
    }
}

fn paint(
    frame: &mut Frame<'_>,
    host: Rect,
    width: u16,
    lines: Vec<Line<'static>>,
    bordered: bool,
    title: &str,
) {
    if lines.is_empty() || width == 0 {
        return;
    }
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(u16::from(bordered) * 2);
    let rect = centered(host, width, height.min(host.height));
    frame.render_widget(Clear, rect);
    let inner = if bordered {
        let block = Block::default()
            .title(title.to_owned())
            .borders(Borders::ALL);
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        inner
    } else {
        rect
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

fn plain_line(text: &str, cells: usize, selected: bool) -> Line<'static> {
    let clipped = super::clip(text, cells);
    let used = super::line_width(&clipped);
    let padded = format!("{clipped}{}", " ".repeat(cells.saturating_sub(used)));
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    Line::from(Span::styled(padded, style))
}

fn wrap_text(text: &str, cells: usize, max_lines: usize) -> Vec<String> {
    if cells == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        if word.is_empty() {
            continue;
        }
        if push_word(&mut lines, &mut current, word, cells, max_lines) {
            break;
        }
    }
    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }
    lines.truncate(max_lines);
    lines
}

fn push_word(
    lines: &mut Vec<String>,
    current: &mut String,
    word: &str,
    cells: usize,
    max_lines: usize,
) -> bool {
    let piece = if current.is_empty() {
        word.to_owned()
    } else {
        format!("{current} {word}")
    };
    if super::line_width(&piece) <= cells {
        *current = piece;
        return false;
    }
    if !current.is_empty() {
        lines.push(std::mem::take(current));
        if lines.len() == max_lines {
            return true;
        }
    }
    if super::line_width(word) <= cells {
        word.clone_into(current);
        false
    } else {
        lines.push(super::clip(word, cells));
        lines.len() == max_lines
    }
}

fn above_status(area: Rect) -> Rect {
    let keep = if area.height >= 16 { 2 } else { 1 };
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(keep),
    }
}

fn popup_width(host: Rect) -> u16 {
    // 80 columns still show a linked-history or coordinate refusal on one row.
    host.width.min(96)
}

fn line_budget(host: Rect, bordered: bool, title_rows: u16) -> u16 {
    host.height
        .saturating_sub(u16::from(bordered) * 2)
        .saturating_sub(title_rows)
        .max(1)
}

fn centered(host: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(host.width).max(1);
    let height = height.min(host.height).max(1);
    Rect {
        x: host.x + (host.width - width) / 2,
        y: host.y + (host.height - height) / 2,
        width,
        height,
    }
}
