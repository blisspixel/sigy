//! Named finding lookup and explicit navigation to original recording metadata.

use sigy_service::{monitor::FindingPage, storage::dvr::Recording};
use std::cell::Cell;

#[cfg(test)]
pub(super) use tests::fixture;

use super::{
    state::{Effect, Key},
    text::sanitize,
};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Browser {
    query: String,
    editing: bool,
    page: Option<FindingPage>,
    offset: usize,
    current_media: Option<String>,
    message: Option<String>,
    viewport_width: Cell<usize>,
}

impl Browser {
    /// The lookup text. The screen draws it through `lines`, which keeps its tail visible.
    #[cfg(test)]
    pub fn query(&self) -> &str {
        &self.query
    }

    pub const fn editing(&self) -> bool {
        self.editing
    }

    pub fn handle(&mut self, key: &Key) -> Option<Effect> {
        if self.editing {
            return Some(self.edit(key));
        }
        match key {
            Key::Char('/') => self.editing = true,
            Key::Enter | Key::Char('r') => return Some(self.lookup()),
            Key::Char('o') => {
                return Some(self.page.as_ref().map_or(Effect::None, |page| {
                    Effect::FindingOriginal {
                        recording: page.recording_id.clone(),
                    }
                }));
            }
            Key::Up => {
                self.offset = self
                    .offset
                    .min(self.wrapped_detail().len().saturating_sub(1))
                    .saturating_sub(1);
            }
            Key::Down => {
                self.offset = self
                    .offset
                    .saturating_add(1)
                    .min(self.wrapped_detail().len().saturating_sub(1));
            }
            Key::Char('f' | 'v') => {
                self.message = Some("Finding navigation is read-only.".into());
                self.offset = 0;
            }
            _ => return None,
        }
        Some(Effect::None)
    }

    fn edit(&mut self, key: &Key) -> Effect {
        match key {
            Key::Escape | Key::Tab | Key::BackTab => self.editing = false,
            Key::Enter => return self.lookup(),
            Key::Backspace => {
                self.query.pop();
                self.message = None;
            }
            Key::Char(character) => self.push(&character.to_string()),
            Key::Paste(text) => self.push(text),
            Key::Quit => return Effect::Detach,
            _ => {}
        }
        Effect::None
    }

    fn push(&mut self, text: &str) {
        self.message = None;
        for character in text.chars() {
            if self.query.len() == 257 {
                break;
            }
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':' | '.' | ' ')
            {
                self.query.push(character);
            }
        }
    }

    fn lookup(&mut self) -> Effect {
        let fields: Vec<_> = self.query.split_whitespace().collect();
        if fields.len() != 2 || fields.iter().any(|id| id.len() > 128) {
            self.message =
                Some("Enter exactly two IDs: MONITOR FINDING (each at most 128 bytes).".into());
            self.offset = 0;
            return Effect::None;
        }
        self.editing = false;
        self.message = None;
        Effect::Finding {
            monitor: fields[0].into(),
            finding: fields[1].into(),
        }
    }

    pub fn load(&mut self, monitor: &str, finding: &str, page: &FindingPage) -> bool {
        if page.monitor_id != monitor || page.id != finding {
            return false;
        }
        self.page = Some(page.clone());
        self.offset = 0;
        self.current_media = None;
        self.message = None;
        true
    }

    pub fn load_original(&mut self, recording: &Recording) -> bool {
        let Some(page) = &self.page else {
            return false;
        };
        if page.recording_id != recording.id {
            return false;
        }
        let available = page.start_us.zip(page.end_us).is_some_and(|(start, end)| {
            start < end
                && matches!(recording.storage_state.as_str(), "reserved" | "retained")
                && recording.intervals.iter().any(|interval| {
                    !interval.released
                        && interval.decoded_start_us <= start
                        && interval.decoded_end_us >= end
                })
                && !recording
                    .gaps
                    .iter()
                    .any(|gap| gap.start_us < end && gap.end_us > start)
        });
        self.current_media = Some(if available {
            "Cited interval available in catalog metadata; file not verified.".into()
        } else {
            "Cited interval unavailable in catalog metadata; citation preserved.".into()
        });
        true
    }

    pub fn lines(&self, limit: usize, width: usize, first_prefix: usize) -> Vec<String> {
        self.viewport_width.set(width.max(2));
        let mut lines = vec![self.query_line(width.saturating_sub(first_prefix))];
        if limit >= 3 {
            lines.push("/ edits MONITOR FINDING; Enter reads; o opens recording metadata.".into());
        }
        let detail = self.wrapped_detail();
        let offset = self.offset.min(detail.len().saturating_sub(1));
        lines.extend(
            detail
                .into_iter()
                .skip(offset)
                .take(limit.saturating_sub(lines.len())),
        );
        lines.truncate(limit);
        lines
    }

    fn query_line(&self, width: usize) -> String {
        let prefix = if width < 28 {
            "IDs: ["
        } else if self.editing {
            "Lookup editing: ["
        } else {
            "Lookup: ["
        };
        let available = width.saturating_sub(prefix.len() + 1);
        let marker = if self.query.len() > available && available >= 3 {
            "..."
        } else {
            ""
        };
        let start = self
            .query
            .len()
            .saturating_sub(available.saturating_sub(marker.len()));
        let visible = self.query.get(start..).unwrap_or_default();
        format!("{prefix}{marker}{visible}]")
    }

    fn wrapped_detail(&self) -> Vec<String> {
        let width = self.viewport_width.get().max(2);
        let mut rows = Vec::new();
        for text in self.detail() {
            let line = ratatui::text::Line::raw(text);
            let mut row = String::new();
            let mut cells = 0;
            for grapheme in line.styled_graphemes(ratatui::style::Style::default()) {
                let next = ratatui::text::Span::raw(grapheme.symbol).width();
                if cells + next > width && !row.is_empty() {
                    rows.push(std::mem::take(&mut row));
                    cells = 0;
                }
                row.push_str(grapheme.symbol);
                cells += next;
            }
            rows.push(row);
        }
        rows
    }

    fn detail(&self) -> Vec<String> {
        let mut lines: Vec<_> = self.message.iter().cloned().collect();
        let Some(page) = &self.page else {
            lines.push("Named lookup only. No finding enumeration. Classification off.".into());
            return lines;
        };
        lines.extend([
            format!(
                "Finding: {} / {}",
                sanitize(&page.monitor_id, 128),
                sanitize(&page.id, 128)
            ),
            format!(
                "Transcript: {} revision {}; translation {}; cue {}",
                sanitize(&page.transcript_id, 128),
                page.transcript_revision,
                page.translation_revision,
                page.cue_ordinal
            ),
            format!(
                "Stale transcript: {}; translation: {}",
                stale(page.stale_transcript),
                stale(page.stale_translation)
            ),
            format!(
                "Original statement at publication: {:?}; recording {}",
                page.original,
                sanitize(&page.recording_id, 128)
            ),
            format!(
                "Cited interval: {}",
                page.start_us.zip(page.end_us).map_or_else(
                    || "none".into(),
                    |(start, end)| format!("[{start}, {end})us")
                )
            ),
            self.current_media
                .clone()
                .unwrap_or_else(|| "Current media unread. o reads recording metadata.".into()),
            "Original script (display limited to 4096 characters):".into(),
            sanitize(&page.original_script, 4096),
            "English (display limited to 4096 characters; wording uncertain):".into(),
            page.english.as_deref().map_or_else(
                || {
                    format!(
                        "untranslated: {}",
                        sanitize(
                            page.untranslated_reason.as_deref().unwrap_or("unknown"),
                            128
                        )
                    )
                },
                |text| sanitize(text, 4096),
            ),
            "Reads are separate snapshots. Selection admits no work.".into(),
        ]);
        lines
    }
}

fn stale(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) | None => "no",
    }
}

#[cfg(test)]
mod tests;
