//! Read-only claim list and page. Nothing here creates a claim or starts playback.

use sigy_service::{claim::ClaimView, control::ClaimPage};

use super::{
    state::{Effect, Key},
    text::sanitize,
};

/// Stored claims opened from Findings. The list and one page are reads.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Browser {
    open: bool,
    ids: Vec<String>,
    selection: usize,
    page: Option<ClaimView>,
    offset: usize,
    message: Option<String>,
}

impl Browser {
    pub const fn open(&self) -> bool {
        self.open
    }

    pub(super) fn open_list(&mut self) {
        self.open = true;
        self.page = None;
        self.offset = 0;
        self.message = None;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.page = None;
        self.offset = 0;
        self.message = None;
    }

    pub(super) fn load_ids(&mut self, ids: Vec<String>) {
        self.ids = ids;
        self.selection = self.selection.min(self.ids.len().saturating_sub(1));
        self.page = None;
        self.offset = 0;
    }

    pub(super) fn load_claim(&mut self, id: &str, claim: &ClaimView) -> bool {
        if claim.id != id {
            return false;
        }
        self.page = Some(claim.clone());
        self.offset = 0;
        self.message = None;
        true
    }

    pub(super) fn handle(&mut self, key: &Key) -> Effect {
        match key {
            Key::Escape => {
                if self.page.take().is_some() {
                    self.offset = 0;
                    return Effect::None;
                }
                self.open = false;
                Effect::None
            }
            Key::Up => {
                self.move_by(false);
                Effect::None
            }
            Key::Down => {
                self.move_by(true);
                Effect::None
            }
            Key::Enter => self.open_selected(),
            Key::Char('r') => self.reload(),
            Key::Quit => Effect::Detach,
            _ => {
                self.message = Some(
                    "Claims are read-only. This page does not create a claim or start playback."
                        .into(),
                );
                Effect::None
            }
        }
    }

    pub(super) fn lines(&self, limit: usize) -> Vec<String> {
        let body = if let Some(claim) = &self.page {
            rendered(claim)
        } else if self.ids.is_empty() {
            vec!["No claims. Create one with sigy claim create ID --text TEXT.".into()]
        } else {
            let mut rows = vec!["Stored claims. Enter reads one. This page writes nothing.".into()];
            for (index, id) in self.ids.iter().enumerate() {
                let marker = if index == self.selection { ">" } else { " " };
                rows.push(format!("{marker} {}", sanitize(id, 128)));
            }
            rows
        };
        let start = if self.page.is_some() {
            self.offset.min(body.len().saturating_sub(1))
        } else {
            0
        };
        let room = limit.saturating_sub(usize::from(self.message.is_some()));
        let mut lines: Vec<String> = body.into_iter().skip(start).take(room).collect();
        if let Some(message) = &self.message
            && lines.len() < limit
        {
            lines.push(sanitize(message, 160));
        }
        lines
    }

    fn move_by(&mut self, down: bool) {
        if let Some(claim) = &self.page {
            let last = rendered(claim).len().saturating_sub(1);
            if down {
                self.offset = self.offset.saturating_add(1).min(last);
            } else {
                self.offset = self.offset.saturating_sub(1);
            }
            return;
        }
        let last = self.ids.len().saturating_sub(1);
        if down {
            self.selection = self.selection.saturating_add(1).min(last);
        } else {
            self.selection = self.selection.saturating_sub(1);
        }
    }

    fn open_selected(&mut self) -> Effect {
        if self.page.is_some() {
            return Effect::None;
        }
        let Some(id) = self.ids.get(self.selection) else {
            self.message = Some("No claim is selected.".into());
            return Effect::None;
        };
        Effect::ClaimShow { id: id.clone() }
    }

    fn reload(&mut self) -> Effect {
        self.message = None;
        if let Some(claim) = &self.page {
            return Effect::ClaimShow {
                id: claim.id.clone(),
            };
        }
        Effect::ClaimList
    }
}

fn rendered(claim: &ClaimView) -> Vec<String> {
    let mut output = Vec::new();
    if crate::claim::render(
        &mut output,
        &ClaimPage::Claim {
            claim: Box::new(claim.clone()),
        },
    )
    .is_err()
    {
        return vec!["Claim page could not be rendered.".into()];
    }
    let Ok(text) = String::from_utf8(output) else {
        return vec!["Claim page could not be rendered.".into()];
    };
    text.lines().map(str::to_owned).collect()
}
