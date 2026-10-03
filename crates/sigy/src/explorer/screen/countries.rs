//! Reference names are independent of station observations and cache coverage.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::Paragraph,
};

use crate::explorer::{country::Picker, text::sanitize};

pub(super) fn render(frame: &mut Frame<'_>, area: Rect, picker: &Picker) {
    let compact = area.width < 60 || area.height < 18;
    let panes = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(if compact { 3 } else { 4 }),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(if compact {
                "Countries: offline"
            } else {
                "Countries and territories: offline worldwide reference"
            }),
            Line::raw(super::clip(
                &format!("Find: {}", sanitize(&picker.query, 128)),
                usize::from(area.width),
            )),
        ]),
        panes[0],
    );
    let mut lines = Vec::new();
    if let Some(page) = &picker.page {
        let visible = usize::from(panes[1].height);
        let start = picker.selected.saturating_sub(visible.saturating_sub(1));
        for (index, entry) in page.entries.iter().enumerate().skip(start).take(visible) {
            let label = if compact || page.display_locale == "en" {
                format!(
                    "{}{} {}",
                    if index == picker.selected { ">" } else { " " },
                    entry.code,
                    entry.name
                )
            } else {
                format!(
                    "{}{} {} | {}",
                    if index == picker.selected { "> " } else { "  " },
                    entry.code,
                    entry.name,
                    entry.english_name
                )
            };
            lines.push(Line::raw(super::clip(
                &sanitize(&label, 512),
                usize::from(area.width),
            )));
        }
        if page.entries.is_empty() {
            lines.push(Line::raw("No reference matches"));
        }
    } else {
        lines.push(Line::raw(super::clip(
            picker.issue.as_deref().unwrap_or("Reference unavailable"),
            usize::from(area.width),
        )));
    }
    frame.render_widget(Paragraph::new(lines), panes[1]);
    let page_status = picker.page.as_ref().map_or_else(
        || "Reference unavailable".to_owned(),
        |page| {
            format!(
                "{} candidates | {} | {}",
                page.total_candidates,
                page.display_locale,
                if page.next_after.is_some() {
                    "more >"
                } else {
                    "last page"
                }
            )
        },
    );
    let mut footer = vec![Line::raw(super::clip(
        &page_status,
        usize::from(area.width),
    ))];
    if compact {
        footer.push(Line::raw("Enter pick Esc back"));
        footer.push(Line::raw("<> page Tab locale"));
    } else {
        footer.push(Line::raw(
            "Up/Down select; Left/Right page; Tab/Shift-Tab locale; Ctrl-U clears query.",
        ));
        footer.push(Line::raw(
            "Enter sets draft; Enter in filters applies. Esc returns to the draft unchanged.",
        ));
        footer.push(Line::raw(
            "Station availability: unknown. Reference names do not measure coverage.",
        ));
    }
    frame.render_widget(Paragraph::new(footer), panes[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::state::Key;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn country_picker_fits_wide_and_compact_without_station_cache()
    -> Result<(), Box<dyn std::error::Error>> {
        for (width, height) in [(20, 8), (80, 24), (132, 40)] {
            let mut picker = Picker::new(String::new());
            let mut terminal = Terminal::new(TestBackend::new(width, height))?;
            terminal.draw(|frame| render(frame, frame.area(), &picker))?;
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>();
            assert!(text.contains("Countries"));
            assert!(text.contains("AC"));
            assert!(text.contains("Enter"));
            for _ in 0..15 {
                picker.handle(Key::Down);
            }
            terminal.draw(|frame| render(frame, frame.area(), &picker))?;
            assert_eq!(picker.selected, 15);
            picker.handle(Key::Right);
            assert_eq!(picker.selected, 0);
            picker.handle(Key::Tab);
            picker.handle(Key::Tab);
            picker.handle(Key::Tab);
            terminal.draw(|frame| render(frame, frame.area(), &picker))?;
            picker.handle(Key::Paste("\u{1b}[2J\u{202e}中国".into()));
            terminal.draw(|frame| render(frame, frame.area(), &picker))?;
            assert!(!picker.query.contains('\u{1b}'));
            assert!(!picker.query.contains('\u{202e}'));
        }
        Ok(())
    }
}
