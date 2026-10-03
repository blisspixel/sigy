//! Draft and applied cache queries. Editing never changes the scope of displayed rows.

use sigy_service::discovery::StationFilter;

use super::{state::Key, text::sanitize};

const TEXT_BYTES: usize = 128;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Scope {
    pub filter: StationFilter,
    pub favorites_only: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Field {
    #[default]
    Name,
    Country,
    Language,
    Tag,
    Health,
    Favorites,
}

impl Field {
    pub const ALL: [Self; 6] = [
        Self::Name,
        Self::Country,
        Self::Language,
        Self::Tag,
        Self::Health,
        Self::Favorites,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Station name",
            Self::Country => "Country code",
            Self::Language => "Directory language",
            Self::Tag => "Directory tag",
            Self::Health => "Upstream check succeeded",
            Self::Favorites => "Favorites only",
        }
    }

    fn next(self, forward: bool) -> Self {
        let index = Self::ALL
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Self::ALL[(index + if forward { 1 } else { Self::ALL.len() - 1 }) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Editor {
    #[default]
    Closed,
    Name,
    Filters(Field),
}

pub(super) enum Edit {
    Continue,
    Apply,
    End,
    Back,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Invalid {
    CountryCode,
    TextBounds,
}

impl Invalid {
    pub const fn message(self) -> &'static str {
        match self {
            Self::CountryCode => {
                "Country currently needs a two-letter code, such as CA or FR; blank means all."
            }
            Self::TextBounds => "The filters exceed the service's text bounds.",
        }
    }

    pub const fn compact(self) -> &'static str {
        match self {
            Self::CountryCode => "Need 2-letter code",
            Self::TextBounds => "Text exceeds bounds",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Search {
    pub draft: Scope,
    pub applied: Scope,
    pending: Option<Scope>,
    editor: Editor,
    pub validation: Option<Invalid>,
    pub failed: bool,
}

impl Search {
    pub fn begin(&mut self, filters: bool) {
        self.validation = None;
        self.failed = false;
        self.draft = self.pending.as_ref().unwrap_or(&self.applied).clone();
        self.editor = if filters {
            Editor::Filters(Field::Name)
        } else {
            Editor::Name
        };
    }

    pub const fn editing_filters(&self) -> bool {
        matches!(self.editor, Editor::Filters(_))
    }

    pub const fn field(&self) -> Field {
        match self.editor {
            Editor::Filters(field) => field,
            Editor::Name | Editor::Closed => Field::Name,
        }
    }

    pub fn value(&self, field: Field) -> String {
        match field {
            Field::Name => self.draft.filter.name.clone(),
            Field::Country => self.draft.filter.country.clone(),
            Field::Language => self.draft.filter.language.clone(),
            Field::Tag => self.draft.filter.tag.clone(),
            Field::Health => toggle_label(self.draft.filter.healthy_only).into(),
            Field::Favorites => toggle_label(self.draft.favorites_only).into(),
        }
    }

    pub fn normalized_draft(&self) -> Result<Scope, Invalid> {
        let mut scope = self.draft.clone();
        scope.filter.country = scope.filter.country.to_ascii_uppercase();
        if !scope.filter.country.is_empty()
            && (scope.filter.country.len() != 2
                || !scope
                    .filter
                    .country
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase()))
        {
            return Err(Invalid::CountryCode);
        }
        scope.filter.validate().map_err(|_| Invalid::TextBounds)?;
        Ok(scope)
    }

    pub fn request(&mut self, scope: Scope) {
        self.validation = None;
        self.failed = false;
        self.pending = Some(scope);
    }

    pub fn accept(&mut self) -> bool {
        let Some(scope) = self.pending.take() else {
            return false;
        };
        self.applied = scope;
        if self.editor == Editor::Closed {
            self.draft = self.applied.clone();
        }
        true
    }

    pub fn interrupt(&mut self) {
        self.pending = None;
    }

    pub fn fail(&mut self) {
        self.interrupt();
        self.failed = true;
    }

    pub const fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn toggle_favorites(&mut self) {
        self.draft = self.pending.as_ref().unwrap_or(&self.applied).clone();
        self.draft.favorites_only = !self.draft.favorites_only;
        self.editor = Editor::Closed;
    }

    pub fn reset(&mut self) {
        self.validation = None;
        self.failed = false;
        self.draft = Scope::default();
        self.editor = Editor::Closed;
    }

    pub fn handle(&mut self, key: Key) -> Edit {
        match key {
            Key::Tab | Key::Down if self.editing_filters() => self.move_field(true),
            Key::BackTab | Key::Up if self.editing_filters() => self.move_field(false),
            Key::Escape | Key::Tab => self.end(Edit::End),
            Key::BackTab => self.end(Edit::Back),
            Key::Enter => Edit::Apply,
            Key::Quit => Edit::Quit,
            Key::Backspace => {
                if let Some(text) = self.text_mut() {
                    remove_grapheme(text);
                }
                Edit::Continue
            }
            Key::ClearInput => {
                match self.field() {
                    Field::Health => self.draft.filter.healthy_only = false,
                    Field::Favorites => self.draft.favorites_only = false,
                    _ => {
                        if let Some(text) = self.text_mut() {
                            text.clear();
                        }
                    }
                }
                Edit::Continue
            }
            Key::Char(' ') if self.field() == Field::Health => {
                self.draft.filter.healthy_only = !self.draft.filter.healthy_only;
                Edit::Continue
            }
            Key::Char(' ') if self.field() == Field::Favorites => {
                self.draft.favorites_only = !self.draft.favorites_only;
                Edit::Continue
            }
            Key::Char(character) => {
                let mut encoded = [0; 4];
                self.push(character.encode_utf8(&mut encoded));
                Edit::Continue
            }
            Key::Paste(text) => {
                self.push(&text);
                Edit::Continue
            }
            Key::Up | Key::Down | Key::Left | Key::Right | Key::Redraw => Edit::Continue,
        }
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        match self.field() {
            Field::Name => Some(&mut self.draft.filter.name),
            Field::Country => Some(&mut self.draft.filter.country),
            Field::Language => Some(&mut self.draft.filter.language),
            Field::Tag => Some(&mut self.draft.filter.tag),
            Field::Health | Field::Favorites => None,
        }
    }

    fn push(&mut self, text: &str) {
        let Some(value) = self.text_mut() else {
            return;
        };
        for character in sanitize(text, TEXT_BYTES).chars() {
            if value.len() + character.len_utf8() > TEXT_BYTES {
                break;
            }
            value.push(character);
        }
    }

    fn move_field(&mut self, forward: bool) -> Edit {
        self.editor = Editor::Filters(self.field().next(forward));
        Edit::Continue
    }

    fn end(&mut self, action: Edit) -> Edit {
        self.validation = None;
        self.editor = Editor::Closed;
        self.draft = self.applied.clone();
        action
    }
}

fn toggle_label(enabled: bool) -> &'static str {
    if enabled { "yes" } else { "no" }
}

fn remove_grapheme(text: &mut String) {
    let line = ratatui::text::Line::raw(text.as_str());
    let mut previous = 0;
    let mut end = 0;
    for grapheme in line.styled_graphemes(ratatui::style::Style::default()) {
        previous = end;
        end += grapheme.symbol.len();
    }
    text.truncate(previous);
}
