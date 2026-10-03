//! Offline reference selection changes only the filter draft.

use sigy_service::discovery::countries::{self, Page};

use super::{state::Key, text::sanitize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Picker {
    pub query: String,
    pub locale: usize,
    pub page: Option<Page>,
    pub selected: usize,
    pub issue: Option<String>,
    cursors: Vec<Option<String>>,
}

pub(super) enum Selection {
    Continue,
    Cancel,
    Code(String),
    Quit,
}

impl Picker {
    pub fn new(query: String) -> Self {
        let mut picker = Self {
            query,
            locale: 0,
            page: None,
            selected: 0,
            issue: None,
            cursors: vec![None],
        };
        picker.load();
        picker
    }

    pub fn handle(&mut self, key: Key) -> Selection {
        match key {
            Key::Escape => return Selection::Cancel,
            Key::Quit => return Selection::Quit,
            Key::Enter => {
                if let Some(entry) = self
                    .page
                    .as_ref()
                    .and_then(|page| page.entries.get(self.selected))
                {
                    return Selection::Code(entry.code.clone());
                }
            }
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Down => {
                self.selected = self.selected.saturating_add(1).min(
                    self.page
                        .as_ref()
                        .map_or(0, |page| page.entries.len().saturating_sub(1)),
                );
            }
            Key::Left => {
                if self.cursors.len() > 1 {
                    self.cursors.pop();
                    self.load();
                }
            }
            Key::Right => {
                if let Some(cursor) = self.page.as_ref().and_then(|page| page.next_after.clone()) {
                    self.cursors.push(Some(cursor));
                    self.load();
                }
            }
            Key::Tab | Key::BackTab => {
                self.locale = (self.locale
                    + if key == Key::Tab {
                        1
                    } else {
                        countries::LOCALES.len() - 1
                    })
                    % countries::LOCALES.len();
                self.restart();
            }
            Key::ClearInput => {
                self.query.clear();
                self.restart();
            }
            Key::Backspace => {
                super::search::remove_grapheme(&mut self.query);
                self.restart();
            }
            Key::Paste(text) => self.push(&text),
            Key::Char(character) => {
                let mut encoded = [0; 4];
                self.push(character.encode_utf8(&mut encoded));
            }
            Key::Redraw => {}
        }
        Selection::Continue
    }

    fn push(&mut self, text: &str) {
        for character in sanitize(text, 128).chars() {
            if self.query.len() + character.len_utf8() > 128 {
                break;
            }
            self.query.push(character);
        }
        self.restart();
    }

    fn restart(&mut self) {
        self.cursors = vec![None];
        self.load();
    }

    fn load(&mut self) {
        self.selected = 0;
        match countries::page(
            &self.query,
            countries::LOCALES[self.locale],
            self.cursors.last().and_then(Option::as_deref),
        ) {
            Ok(page) => {
                self.page = Some(page);
                self.issue = None;
            }
            Err(error) => {
                self.page = None;
                self.issue = Some(sanitize(&error.to_string(), 256));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reference_entry_can_be_selected_and_pages_are_finite() {
        let mut picker = Picker::new(String::new());
        let mut codes = std::collections::BTreeSet::new();
        loop {
            let Some(page) = picker.page.clone() else {
                panic!("reference page");
            };
            for entry in &page.entries {
                let Selection::Code(code) = picker.handle(Key::Enter) else {
                    panic!("explicit country selection");
                };
                assert_eq!(code, entry.code);
                assert!(codes.insert(code));
                picker.handle(Key::Down);
            }
            if page.next_after.is_none() {
                assert_eq!(codes.len(), page.total_candidates);
                break;
            }
            picker.handle(Key::Right);
        }
        assert!(picker.cursors.len() <= 19);
        picker.handle(Key::Left);
        assert_eq!(picker.selected, 0);
        picker.handle(Key::Tab);
        assert_eq!(picker.cursors.len(), 1);
        assert_eq!(
            picker
                .page
                .as_ref()
                .map(|page| page.display_locale.as_str()),
            Some("fr")
        );
        picker.handle(Key::Paste("Congo".into()));
        assert_eq!(picker.cursors.len(), 1);
        assert!(picker.page.as_ref().is_some_and(|page| page.ambiguous));
        assert!(matches!(picker.handle(Key::Escape), Selection::Cancel));
        assert!(matches!(picker.handle(Key::Quit), Selection::Quit));
    }

    #[test]
    fn pasted_native_text_is_byte_bounded_and_backspace_preserves_graphemes() {
        let mut picker = Picker::new(String::new());
        picker.handle(Key::Paste(format!("\u{1b}\u{202e}{}", "東".repeat(128))));
        assert_eq!(picker.query.len(), 126);
        assert!(!picker.query.contains('\u{1b}'));
        picker.handle(Key::ClearInput);
        picker.handle(Key::Paste("Re\u{301}".into()));
        picker.handle(Key::Backspace);
        assert_eq!(picker.query, "R");
        picker.handle(Key::ClearInput);
        picker.handle(Key::Paste("भारत".into()));
        assert!(
            picker
                .page
                .as_ref()
                .is_some_and(|page| page.entries.iter().any(|entry| entry.code == "IN"))
        );
    }
}
