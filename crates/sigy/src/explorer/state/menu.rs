//! Selection menu and the confirmation dialog copy.
//!
//! Rows come from the workspace action table. A row that cannot run stays
//! listed with the same refusal the direct key would show. Confirmations still
//! accept only `y` and cancel on `n` or Esc.

use super::{Effect, Explorer, Key, Link, Menu, PendingPlay, Workspace};
use crate::explorer::actions::{self, Shortcut, Slot};
use crate::explorer::text::sanitize;

/// Centered confirmation text. The status sentence stays the authority for `y`.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DialogCopy {
    pub title: &'static str,
    pub target: String,
    pub effect: &'static str,
    pub buttons: &'static str,
}

const PLAY_EFFECT: &str =
    "Contacts the station. May register its first source revision. Nothing is recorded.";
const FETCH_EFFECT: &str = "Requests one filtered directory page. No station is contacted.";
const HOLD_EFFECT: &str = "Records one hold on that sealed interval. Capture keeps running.";
const INTERVAL_EFFECT: &str = "Plays that sealed interval. Capture keeps running.";
const LATEST_EFFECT: &str =
    "Plays the latest sealed interval from that start. Capture keeps running.";
const RECORDING_EFFECT: &str = "Plays that retained recording. Capture keeps running.";
const FINDING_EFFECT: &str = "Plays the cited interval. Enter still only inspects.";
const PLAY_BUTTONS: &str = "y Play   n Cancel";
const NO_SELECTION: &str =
    "No selection. Space opens actions for a station, recording, or finding.";

impl Explorer {
    pub(crate) const fn menu_open(&self) -> bool {
        matches!(self.menu, Menu::Open(_))
    }

    pub(crate) const fn confirming(&self) -> bool {
        self.play_confirmation.is_some()
            || self.fetch_confirmation.is_some()
            || self.protect_confirmation.is_some()
    }

    /// Selected menu row while the menu is open.
    pub(crate) const fn menu_cursor(&self) -> Option<usize> {
        match self.menu {
            Menu::Open(index) => Some(index),
            Menu::Closed | Menu::Collapsed => None,
        }
    }

    /// Remember the terminal size. A menu that no longer fits becomes a status line.
    pub(crate) fn note_window(&mut self, width: u16, height: u16) {
        self.window = (width, height);
        self.collapse_if_tight();
        if matches!(self.menu, Menu::Collapsed) && self.window_fits() {
            self.menu = Menu::Open(0);
        }
    }

    pub(crate) fn dialog_copy(&self) -> Option<DialogCopy> {
        if let Some(pending) = &self.play_confirmation {
            return Some(self.play_dialog(pending));
        }
        if self.fetch_confirmation.is_some() {
            return Some(DialogCopy {
                title: "Fetch",
                target: self.fetch_target(),
                effect: FETCH_EFFECT,
                buttons: "y Fetch   n Cancel",
            });
        }
        let pending = self.protect_confirmation.as_ref()?;
        Some(DialogCopy {
            title: "Hold",
            target: format!(
                "{} [{}, {})",
                sanitize(&pending.id, 64),
                pending.start_us,
                pending.end_us
            ),
            effect: HOLD_EFFECT,
            buttons: "y Hold   n Cancel",
        })
    }

    /// Why this menu row will not run. `None` means Enter dispatches the direct key.
    pub(crate) fn menu_reason(&self, slot: Slot) -> Option<String> {
        self.menu_block(slot)
    }

    pub(super) fn menu_key(&mut self, key: &Key) -> Option<Effect> {
        if matches!(self.menu, Menu::Closed) {
            if self.text_entry() || self.at_home() || self.claims.open() {
                return None;
            }
            return self.open_menu_key(key);
        }
        self.collapse_if_tight();
        match self.menu {
            Menu::Collapsed => self.collapsed_key(key),
            Menu::Open(_) => Some(self.drive_menu(key)),
            Menu::Closed => None,
        }
    }

    fn collapsed_key(&mut self, key: &Key) -> Option<Effect> {
        self.menu = Menu::Closed;
        if matches!(key, Key::Escape | Key::Backspace) {
            return Some(Effect::None);
        }
        self.open_menu_key(key)
    }

    fn open_menu_key(&mut self, key: &Key) -> Option<Effect> {
        if !matches!(key, Key::Char(' ')) || !self.menu_surface() {
            return None;
        }
        if actions::menu_items(self).is_empty() {
            self.note_message("No actions for this selection.");
            return Some(Effect::None);
        }
        if !self.window_fits() {
            let line = self.tight_status();
            self.note_message(&line);
            return Some(Effect::None);
        }
        self.menu = Menu::Open(0);
        Some(Effect::None)
    }

    fn drive_menu(&mut self, key: &Key) -> Effect {
        let items = actions::menu_items(self);
        if items.is_empty() {
            self.menu = Menu::Closed;
            return Effect::None;
        }
        let index = self.menu_cursor().unwrap_or(0).min(items.len() - 1);
        match key {
            Key::Escape | Key::Backspace => {
                self.menu = Menu::Closed;
                Effect::None
            }
            Key::Up => {
                self.menu = Menu::Open(index.saturating_sub(1));
                Effect::None
            }
            Key::Down => {
                self.menu = Menu::Open(index.saturating_add(1).min(items.len() - 1));
                Effect::None
            }
            Key::Char('q') | Key::Quit => Effect::Detach,
            Key::Enter => self.activate(index),
            Key::Char(character) => self.shortcut(*character),
            _ => Effect::None,
        }
    }

    fn shortcut(&mut self, character: char) -> Effect {
        let found = actions::menu_items(self).iter().position(
            |item| matches!(item.shortcut, Shortcut::Char(shortcut) if shortcut == character),
        );
        let Some(found) = found else {
            return Effect::None;
        };
        self.menu = Menu::Open(found);
        self.activate(found)
    }

    fn activate(&mut self, index: usize) -> Effect {
        let Some(item) = actions::menu_items(self).get(index).copied() else {
            return Effect::None;
        };
        if let Some(reason) = self.menu_block(item.slot) {
            self.note_message(&reason);
            return Effect::None;
        }
        if item.slot == Slot::Copy {
            let command = self.copy_line();
            self.menu = Menu::Closed;
            self.note_message(&command);
            return Effect::None;
        }
        let Some(replay) = replay_key(item.shortcut) else {
            return Effect::None;
        };
        self.menu = Menu::Closed;
        self.handle(replay)
    }

    fn menu_surface(&self) -> bool {
        matches!(
            self.workspace,
            Workspace::Explore | Workspace::Globe | Workspace::Recordings | Workspace::Findings
        )
    }

    const fn window_fits(&self) -> bool {
        self.window.0 >= 40 && self.window.1 >= 12
    }

    fn collapse_if_tight(&mut self) {
        if matches!(self.menu, Menu::Open(_)) && !self.window_fits() {
            self.menu = Menu::Collapsed;
            let line = self.tight_status();
            self.note_message(&line);
        }
    }

    fn tight_status(&self) -> String {
        if !self.menu_has_target() {
            return NO_SELECTION.to_owned();
        }
        let parts: Vec<String> = actions::menu_items(self)
            .iter()
            .map(actions::menu_label)
            .collect();
        if parts.is_empty() {
            "No actions for this selection.".to_owned()
        } else {
            format!("Actions: {}", parts.join(", "))
        }
    }

    fn menu_has_target(&self) -> bool {
        match self.workspace {
            Workspace::Explore | Workspace::Globe => {
                self.context.is_some() || self.selected().is_some()
            }
            Workspace::Recordings => self.selected_recording().is_some(),
            Workspace::Findings => self.findings.has_page() || !self.findings.query().is_empty(),
            Workspace::Live | Workspace::Monitors | Workspace::System => false,
        }
    }

    fn copy_line(&self) -> String {
        match self.workspace {
            Workspace::Recordings => {
                actions::file_command(self.selected_recording().map(|row| row.id.as_str()))
            }
            Workspace::Findings => actions::file_command(self.findings.cited_recording()),
            Workspace::Explore | Workspace::Globe | Workspace::Live => {
                actions::play_command(self.station_id())
            }
            Workspace::Monitors | Workspace::System => actions::play_command(None),
        }
    }

    fn station_id(&self) -> Option<&str> {
        self.context
            .as_ref()
            .map(|context| context.row.id.as_str())
            .or_else(|| self.selected().map(|row| row.id.as_str()))
    }

    fn menu_block(&self, slot: Slot) -> Option<String> {
        match slot {
            Slot::None | Slot::Copy => None,
            Slot::Play => self.play_block(),
            Slot::Fetch => self.fetch_block(),
            Slot::Inspect => self.inspect_block(),
            Slot::Favorite => self.favorite_block(),
            Slot::Linked => self.linked_block(),
            Slot::Center => self.center_block(),
            Slot::Latest => self.latest_block(),
            Slot::Hold => self.hold_block(),
            Slot::Interval => self.interval_block(),
            Slot::Metadata => self.metadata_block(),
        }
    }

    fn play_block(&self) -> Option<String> {
        match self.workspace {
            Workspace::Recordings => self.recording_play_block(),
            Workspace::Findings => self.finding_play_block(),
            Workspace::Explore | Workspace::Globe | Workspace::Live => self.station_play_block(),
            Workspace::Monitors | Workspace::System => None,
        }
    }

    fn station_play_block(&self) -> Option<String> {
        if self.station_id().is_some() {
            None
        } else {
            Some("No selected station to play.".to_owned())
        }
    }

    fn recording_play_block(&self) -> Option<String> {
        let Some(recording) = self.selected_recording() else {
            return Some("No selected recording to play.".to_owned());
        };
        if recording.storage_state != "retained" || recording.active() {
            Some(
                "Only a retained recording that is not capturing can play. Selection does not start audio."
                    .to_owned(),
            )
        } else {
            None
        }
    }

    fn finding_play_block(&self) -> Option<String> {
        if self.findings.cited_span().is_some() {
            None
        } else {
            Some("This finding has no cited interval. Playback was not requested.".to_owned())
        }
    }

    fn fetch_block(&self) -> Option<String> {
        if self.context.is_some() {
            return Some("Esc returns to the station list before a fetch.".to_owned());
        }
        if self.favorites_only() {
            return Some(
                "Favorites are local choices. v shows all matches before R fetches; f favorites a selected station."
                    .to_owned(),
            );
        }
        if self.search.pending() {
            return Some(
                "A search is pending. Wait for its result before requesting a directory page."
                    .to_owned(),
            );
        }
        None
    }

    fn inspect_block(&self) -> Option<String> {
        if self.workspace == Workspace::Findings {
            return finding_query_block(self.findings.query());
        }
        if self.context.is_some() {
            return Some("This station is already open.".to_owned());
        }
        if self.pending_search.is_some() || self.pending_favorite.is_some() {
            return Some("The cached request is pending. Inspect after it finishes.".to_owned());
        }
        if self.selected().is_none() {
            return Some(
                "No station on this page. C chooses a country; / searches the cache.".to_owned(),
            );
        }
        None
    }

    fn favorite_block(&self) -> Option<String> {
        if self.context.is_some() {
            return Some("Esc returns to the station list before a favorite.".to_owned());
        }
        if matches!(self.link, Link::Disconnected) {
            return Some("Disconnected; the favorite was not submitted. r reconnects.".to_owned());
        }
        if self.selected().is_none() {
            Some("No station is selected.".to_owned())
        } else {
            None
        }
    }

    fn linked_block(&self) -> Option<String> {
        let Some(context) = &self.context else {
            return Some("Enter inspects this station before linked history.".to_owned());
        };
        if context.catalog.is_none() {
            return Some("Linked history unavailable: frozen catalog identity missing.".to_owned());
        }
        if context.linked_generation.is_some() {
            Some("Linked history is already pending.".to_owned())
        } else {
            None
        }
    }

    fn center_block(&self) -> Option<String> {
        if self.context.is_some() {
            return Some("Esc returns to the globe before centering.".to_owned());
        }
        if self.selected().and_then(|row| row.coordinates).is_some() {
            None
        } else {
            Some("The selected station has no directory coordinates.".to_owned())
        }
    }

    fn latest_block(&self) -> Option<String> {
        self.sealed_block(Slot::Latest)
    }

    fn interval_block(&self) -> Option<String> {
        self.sealed_block(Slot::Interval)
    }

    fn hold_block(&self) -> Option<String> {
        if matches!(self.link, Link::Disconnected) {
            return Some(
                "Disconnected. The hold was not submitted. r reconnects. Capture keeps running."
                    .to_owned(),
            );
        }
        if self.pending_protect.is_some() {
            return Some(super::PROTECT_BUSY.to_owned());
        }
        self.sealed_block(Slot::Hold)
    }

    fn sealed_block(&self, slot: Slot) -> Option<String> {
        let Some(recording) = self.selected_recording() else {
            return Some(missing_recording(slot));
        };
        if !matches!(recording.storage_state.as_str(), "reserved" | "retained") {
            return Some(crate::explorer::interval::UNSTORED.to_owned());
        }
        let choice = if slot == Slot::Latest {
            crate::explorer::interval::latest_playable(&recording.seal)
        } else {
            crate::explorer::interval::select(&recording.seal, self.interval_index)
        };
        match choice {
            Ok(_) => None,
            Err(refusal) => Some(crate::explorer::interval::sentence(refusal).to_owned()),
        }
    }

    fn metadata_block(&self) -> Option<String> {
        if self.findings.has_page() {
            None
        } else {
            Some("This finding has no loaded citation.".to_owned())
        }
    }

    fn play_dialog(&self, pending: &PendingPlay) -> DialogCopy {
        match pending {
            PendingPlay::Station(id) => DialogCopy {
                title: "Play",
                target: sanitize(self.station_name(id), 32),
                effect: PLAY_EFFECT,
                buttons: PLAY_BUTTONS,
            },
            PendingPlay::Recording(id) => DialogCopy {
                title: "Play",
                target: sanitize(id, 64),
                effect: RECORDING_EFFECT,
                buttons: PLAY_BUTTONS,
            },
            PendingPlay::Interval {
                id,
                start_us,
                end_us,
            } => DialogCopy {
                title: "Play",
                target: format!("{} [{start_us}, {end_us})", sanitize(id, 64)),
                effect: INTERVAL_EFFECT,
                buttons: PLAY_BUTTONS,
            },
            PendingPlay::Latest { id, start_us } => DialogCopy {
                title: "Play",
                target: format!("{} from {start_us}", sanitize(id, 64)),
                effect: LATEST_EFFECT,
                buttons: PLAY_BUTTONS,
            },
            PendingPlay::Finding {
                finding,
                start_us,
                end_us,
                ..
            } => DialogCopy {
                title: "Play",
                target: format!("{} [{start_us}, {end_us})", sanitize(finding, 64)),
                effect: FINDING_EFFECT,
                buttons: PLAY_BUTTONS,
            },
        }
    }

    fn station_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.context
            .as_ref()
            .filter(|context| context.row.id == id)
            .map(|context| context.row.name.as_str())
            .or_else(|| {
                self.rows
                    .iter()
                    .find(|row| row.id == id)
                    .map(|row| row.name.as_str())
            })
            .or_else(|| self.listen_name(id))
            .unwrap_or(id)
    }

    fn fetch_target(&self) -> String {
        let Some(scope) = &self.fetch_confirmation else {
            return "Applied directory page".to_owned();
        };
        let mut parts = Vec::new();
        push_filter(&mut parts, &scope.filter.name);
        push_filter(&mut parts, &scope.filter.country);
        push_filter(&mut parts, &scope.filter.language);
        push_filter(&mut parts, &scope.filter.tag);
        if scope.filter.healthy_only {
            parts.push("healthy".to_owned());
        }
        if scope.favorites_only {
            parts.push("favorites".to_owned());
        }
        if parts.is_empty() {
            "Applied directory page".to_owned()
        } else {
            parts.join(" ")
        }
    }
}

fn missing_recording(slot: Slot) -> String {
    if slot == Slot::Hold {
        "No selected recording. No hold was recorded. Capture keeps running.".to_owned()
    } else {
        "No selected recording. No interval was started. Capture keeps running.".to_owned()
    }
}

fn finding_query_block(query: &str) -> Option<String> {
    let fields: Vec<&str> = query.split_whitespace().collect();
    if fields.len() == 2 && fields.iter().all(|id| id.len() <= 128) {
        None
    } else {
        Some("Enter exactly two IDs: MONITOR FINDING (each at most 128 bytes).".to_owned())
    }
}

fn push_filter(parts: &mut Vec<String>, value: &str) {
    let clean = sanitize(value, 32);
    if !clean.is_empty() {
        parts.push(clean);
    }
}

const fn replay_key(shortcut: Shortcut) -> Option<Key> {
    match shortcut {
        Shortcut::Char(character) => Some(Key::Char(character)),
        Shortcut::Enter => Some(Key::Enter),
        Shortcut::None => None,
    }
}
