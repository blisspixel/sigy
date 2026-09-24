//! Explorer view state. Mutations are explicit effects, never selection.

use crate::explorer::text::{age_label, sanitize};

const QUERY_LIMIT: usize = 128;
const PAGE_LIMIT: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Explore,
    Live,
    Recordings,
    Monitors,
    Findings,
    Globe,
    System,
}

impl Workspace {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Explore => "Explore",
            Self::Live => "Live",
            Self::Recordings => "Recordings",
            Self::Monitors => "Monitors unavailable",
            Self::Findings => "Findings unavailable",
            Self::Globe => "Globe",
            Self::System => "System",
        }
    }

    #[must_use]
    pub const fn available(self) -> bool {
        matches!(
            self,
            Self::Explore | Self::Live | Self::Recordings | Self::Globe | Self::System
        )
    }

    const fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Explore, true) | (Self::Recordings, false) => Self::Live,
            (Self::Live, true) | (Self::Monitors, false) => Self::Recordings,
            (Self::Recordings, true) | (Self::Findings, false) => Self::Monitors,
            (Self::Monitors, true) | (Self::Globe, false) => Self::Findings,
            (Self::Findings, true) | (Self::System, false) => Self::Globe,
            (Self::Globe, true) | (Self::Explore, false) => Self::System,
            (Self::System, true) | (Self::Live, false) => Self::Explore,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    Results,
    Detail,
    Playback,
    Help,
    Workspaces,
}

impl Focus {
    const fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Search, true) | (Self::Detail, false) => Self::Results,
            (Self::Results, true) | (Self::Playback, false) => Self::Detail,
            (Self::Detail, true) | (Self::Help, false) => Self::Playback,
            (Self::Playback, true) | (Self::Workspaces, false) => Self::Help,
            (Self::Help, true) | (Self::Search, false) => Self::Workspaces,
            (Self::Workspaces, true) | (Self::Results, false) => Self::Search,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    pub reduced_motion: bool,
    pub linear: bool,
    pub monochrome: bool,
}

impl Modes {
    #[must_use]
    pub fn resolve(reduced_motion: bool, linear: bool, monochrome: bool) -> Self {
        Self {
            reduced_motion: reduced_motion || env_flag("SIGY_REDUCED_MOTION"),
            linear: linear || env_flag("SIGY_LINEAR"),
            monochrome: monochrome || crate::style::plain_requested(),
        }
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some("1")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Unknown,
    Succeeded,
    Failed,
}

impl Health {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

/// Directory coordinates: where a directory says a stream is located.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StationRow {
    pub id: String,
    pub name: String,
    pub favorite: bool,
    pub directory_health: Health,
    pub directory_languages: String,
    pub observed_ms: i64,
    pub hls: bool,
    pub coordinates: Option<Coordinates>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshView {
    pub id: String,
    pub state: String,
    pub accepted: u32,
    pub skipped: u32,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryView {
    pub cached_stations: u32,
    pub maximum_stations: u32,
    pub favorite_stations: u32,
    pub refresh: Option<RefreshView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingLine {
    pub id: String,
    pub state: String,
    pub storage_state: String,
    pub format: Option<String>,
}

impl RecordingLine {
    #[must_use]
    pub fn active(&self) -> bool {
        matches!(
            self.state.as_str(),
            "scheduled" | "starting" | "running" | "retrying" | "stopping"
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaView {
    pub quota: u64,
    pub charged: u64,
    pub reserved: u64,
    pub available: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackView {
    pub id: String,
    pub state: String,
    pub format: Option<String>,
    pub source_revision: String,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetLine {
    pub scope: String,
    pub limit_usd: String,
    pub settled_usd: String,
    pub reserved_usd: String,
    pub available_usd: String,
    pub frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    LocalCatalog,
    Service { process_id: u32, stopping: bool },
    Disconnected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Desk {
    pub link: Link,
    pub observed_ms: i64,
    pub schema_version: u32,
    pub sqlite_version: String,
    pub provider_dispatch_available: bool,
    pub budgets: Vec<BudgetLine>,
    pub captures_active: u64,
    pub captures_scheduled: u64,
    pub captures_interrupted: u64,
    pub captures_terminal: u64,
    pub dispatch_available: bool,
    pub directory: Option<DirectoryView>,
    pub stations: Vec<StationRow>,
    pub quota: Option<QuotaView>,
    pub recordings: Vec<RecordingLine>,
    pub playback: Option<PlaybackView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub generation: u64,
    pub name: String,
    pub favorites_only: bool,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Detach,
    Search(SearchQuery),
    SetFavorite {
        generation: u64,
        id: String,
        favorite: bool,
    },
    Reload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Escape,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Paste(String),
    Quit,
    Redraw,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Explorer {
    focus: Focus,
    workspace: Workspace,
    modes: Modes,
    query: String,
    favorites_only: bool,
    rows: Vec<StationRow>,
    selection: usize,
    link: Link,
    snapshot_ms: Option<i64>,
    now_ms: i64,
    directory: Option<DirectoryView>,
    recordings: Vec<RecordingLine>,
    captures_active: u64,
    quota: Option<QuotaView>,
    playback: Option<PlaybackView>,
    schema_version: u32,
    sqlite_version: String,
    provider_dispatch_available: bool,
    budgets: Vec<BudgetLine>,
    captures_scheduled: u64,
    captures_interrupted: u64,
    captures_terminal: u64,
    dispatch_available: bool,
    search_generation: u64,
    applied_search: u64,
    pending_search: Option<u64>,
    favorite_generation: u64,
    pending_favorite: Option<u64>,
    status: String,
    draw: Draw,
    globe: GlobeView,
}

/// What the Globe workspace shows. Changing it never contacts a station.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GlobeView {
    center: (f64, f64),
    flat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Draw {
    Needed,
    Clean,
}

impl Explorer {
    #[must_use]
    pub fn new(modes: Modes, now_ms: i64) -> Self {
        Self {
            focus: Focus::Results,
            workspace: Workspace::Explore,
            modes,
            query: String::new(),
            favorites_only: false,
            rows: Vec::new(),
            selection: 0,
            link: Link::Disconnected,
            snapshot_ms: None,
            now_ms,
            directory: None,
            recordings: Vec::new(),
            captures_active: 0,
            quota: None,
            playback: None,
            schema_version: 0,
            sqlite_version: String::new(),
            provider_dispatch_available: false,
            budgets: Vec::new(),
            captures_scheduled: 0,
            captures_interrupted: 0,
            captures_terminal: 0,
            dispatch_available: false,
            search_generation: 0,
            applied_search: 0,
            pending_search: None,
            favorite_generation: 0,
            pending_favorite: None,
            status: "Quit detaches this client. It does not stop the service or a recording."
                .into(),
            draw: Draw::Needed,
            globe: GlobeView {
                center: (0.0, 20.0),
                flat: false,
            },
        }
    }

    /// Longitude and latitude of the globe's center, in degrees.
    #[must_use]
    pub const fn globe_center(&self) -> (f64, f64) {
        self.globe.center
    }

    #[must_use]
    pub const fn flat_map(&self) -> bool {
        self.globe.flat
    }

    fn globe_key(&mut self, key: char) -> bool {
        let (lon, lat) = self.globe.center;
        match key {
            'h' => self.globe.center = (wrap_longitude(lon - 15.0), lat),
            'l' => self.globe.center = (wrap_longitude(lon + 15.0), lat),
            'k' => self.globe.center = (lon, (lat + 15.0).min(90.0)),
            'j' => self.globe.center = (lon, (lat - 15.0).max(-90.0)),
            'm' => self.globe.flat = !self.globe.flat,
            'c' => match self.selected().and_then(|row| row.coordinates) {
                Some(place) => {
                    self.globe.center = (place.longitude, place.latitude);
                    self.status =
                        "Centered on the selected station's directory coordinates.".into();
                }
                None => self.status = "The selected station has no directory coordinates.".into(),
            },
            _ => return false,
        }
        true
    }

    #[must_use]
    pub const fn focus(&self) -> Focus {
        self.focus
    }

    #[must_use]
    pub const fn workspace(&self) -> Workspace {
        self.workspace
    }

    #[must_use]
    pub const fn modes(&self) -> Modes {
        self.modes
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub const fn favorites_only(&self) -> bool {
        self.favorites_only
    }

    #[must_use]
    pub fn rows(&self) -> &[StationRow] {
        &self.rows
    }

    #[must_use]
    pub const fn selection(&self) -> usize {
        self.selection
    }

    #[must_use]
    pub fn selected(&self) -> Option<&StationRow> {
        self.rows.get(self.selection)
    }

    #[must_use]
    pub const fn link(&self) -> &Link {
        &self.link
    }

    #[must_use]
    pub const fn directory(&self) -> Option<&DirectoryView> {
        self.directory.as_ref()
    }

    #[must_use]
    pub fn recordings(&self) -> &[RecordingLine] {
        &self.recordings
    }

    #[must_use]
    pub const fn captures_active(&self) -> u64 {
        self.captures_active
    }

    #[must_use]
    pub const fn quota(&self) -> Option<QuotaView> {
        self.quota
    }

    #[must_use]
    pub const fn playback(&self) -> Option<&PlaybackView> {
        self.playback.as_ref()
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub fn sqlite_version(&self) -> &str {
        &self.sqlite_version
    }

    #[must_use]
    pub const fn provider_dispatch_available(&self) -> bool {
        self.provider_dispatch_available
    }

    #[must_use]
    pub fn budgets(&self) -> &[BudgetLine] {
        &self.budgets
    }

    #[must_use]
    pub const fn captures_scheduled(&self) -> u64 {
        self.captures_scheduled
    }

    #[must_use]
    pub const fn captures_interrupted(&self) -> u64 {
        self.captures_interrupted
    }

    #[must_use]
    pub const fn captures_terminal(&self) -> u64 {
        self.captures_terminal
    }

    #[must_use]
    pub const fn dispatch_available(&self) -> bool {
        self.dispatch_available
    }

    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    #[must_use]
    pub const fn now_ms(&self) -> i64 {
        self.now_ms
    }

    #[must_use]
    pub const fn animation_frames(&self) -> u32 {
        match self.draw {
            Draw::Needed | Draw::Clean => 0,
        }
    }

    #[must_use]
    pub const fn page_limit() -> u32 {
        PAGE_LIMIT
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.draw == Draw::Needed;
        self.draw = Draw::Clean;
        dirty
    }

    pub fn apply_desk(&mut self, desk: Desk) {
        let selected = self.selected().map(|row| row.id.clone());
        self.search_generation = self.search_generation.saturating_add(1);
        self.applied_search = self.search_generation;
        self.pending_search = None;
        self.pending_favorite = None;
        self.link = desk.link;
        self.snapshot_ms = Some(desk.observed_ms);
        self.now_ms = desk.observed_ms;
        self.schema_version = desk.schema_version;
        self.sqlite_version = desk.sqlite_version;
        self.provider_dispatch_available = desk.provider_dispatch_available;
        self.budgets = desk.budgets;
        self.captures_active = desk.captures_active;
        self.captures_scheduled = desk.captures_scheduled;
        self.captures_interrupted = desk.captures_interrupted;
        self.captures_terminal = desk.captures_terminal;
        self.dispatch_available = desk.dispatch_available;
        self.directory = desk.directory;
        self.quota = desk.quota;
        self.recordings = desk.recordings;
        self.playback = desk.playback;
        self.rows = desk.stations;
        self.restore_selection(selected);
        self.draw = Draw::Needed;
    }

    pub fn note_disconnect(&mut self, now_ms: i64, reason: &str) {
        self.link = Link::Disconnected;
        self.now_ms = now_ms;
        self.pending_search = None;
        self.pending_favorite = None;
        self.status = sanitize(
            &format!("disconnected: {reason}. Last snapshot kept. Capture was not stopped."),
            160,
        );
        self.draw = Draw::Needed;
    }

    pub fn note_link(&mut self, link: Link) {
        self.link = link;
        self.draw = Draw::Needed;
    }

    pub fn note_message(&mut self, message: &str) {
        self.status = sanitize(message, 160);
        self.draw = Draw::Needed;
    }

    #[must_use]
    pub fn apply_search(
        &mut self,
        generation: u64,
        rows: Vec<StationRow>,
        directory: DirectoryView,
    ) -> bool {
        if self.pending_search != Some(generation) || generation < self.applied_search {
            return false;
        }
        let selected = self.selected().map(|row| row.id.clone());
        self.pending_search = None;
        self.applied_search = generation;
        self.directory = Some(directory);
        self.rows = rows;
        self.restore_selection(selected);
        self.snapshot_ms = Some(self.now_ms);
        self.draw = Draw::Needed;
        true
    }

    #[must_use]
    pub fn apply_favorite(
        &mut self,
        generation: u64,
        id: &str,
        favorite: bool,
        directory: Option<DirectoryView>,
    ) -> bool {
        if self.pending_favorite != Some(generation) {
            return false;
        }
        self.pending_favorite = None;
        if let Some(directory) = directory {
            self.directory = Some(directory);
        }
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.favorite = favorite;
        }
        self.status = if favorite {
            "Saved favorite. The station was not tuned.".into()
        } else {
            "Removed favorite. Recordings and the cache row were kept.".into()
        };
        self.draw = Draw::Needed;
        true
    }

    #[must_use]
    pub fn snapshot_age(&self) -> String {
        self.snapshot_ms.map_or_else(
            || "unknown".into(),
            |observed| age_label(observed, self.now_ms),
        )
    }

    pub fn handle(&mut self, key: Key) -> Effect {
        self.draw = Draw::Needed;
        if matches!(key, Key::Quit) {
            return Effect::Detach;
        }
        if matches!(key, Key::Redraw) {
            return Effect::None;
        }
        if self.focus == Focus::Search {
            return self.edit_search(key);
        }
        if self.workspace == Workspace::Globe
            && let Key::Char(character) = key
            && self.globe_key(character)
        {
            return Effect::None;
        }
        match key {
            Key::Char('q') | Key::Quit => Effect::Detach,
            Key::Char('/') => {
                self.focus = Focus::Search;
                Effect::None
            }
            Key::Tab => {
                self.focus = self.focus.cycle(true);
                Effect::None
            }
            Key::BackTab => {
                self.focus = self.focus.cycle(false);
                Effect::None
            }
            Key::Left => {
                self.workspace = self.workspace.cycle(false);
                Effect::None
            }
            Key::Right => {
                self.workspace = self.workspace.cycle(true);
                Effect::None
            }
            Key::Up => self.move_selection(-1),
            Key::Down => self.move_selection(1),
            Key::Enter => {
                self.focus = Focus::Detail;
                Effect::None
            }
            Key::Char('v') => self.toggle_favorite_filter(),
            Key::Char('f') => self.toggle_favorite(),
            Key::Char('r') => self.reload(),
            Key::Char(character @ '1'..='7') => {
                self.workspace = workspace_from_digit(character);
                Effect::None
            }
            Key::Paste(_) | Key::Char(_) | Key::Backspace | Key::Escape | Key::Redraw => {
                Effect::None
            }
        }
    }

    fn edit_search(&mut self, key: Key) -> Effect {
        match key {
            Key::Escape | Key::Tab => {
                self.focus = Focus::Results;
                Effect::None
            }
            Key::BackTab => {
                self.focus = Focus::Workspaces;
                Effect::None
            }
            Key::Enter => self.submit_search(None),
            Key::Backspace => {
                self.query.pop();
                Effect::None
            }
            Key::Char(character) => {
                let mut encoded = [0; 4];
                self.push_query(character.encode_utf8(&mut encoded));
                Effect::None
            }
            Key::Paste(text) => {
                self.push_query(&text);
                Effect::None
            }
            Key::Up | Key::Down | Key::Left | Key::Right | Key::Redraw => Effect::None,
            Key::Quit => Effect::Detach,
        }
    }

    fn push_query(&mut self, text: &str) {
        let mut combined = self.query.clone();
        combined.push_str(&sanitize(text, QUERY_LIMIT));
        self.query = sanitize(&combined, QUERY_LIMIT);
    }

    fn move_selection(&mut self, delta: isize) -> Effect {
        if self.rows.is_empty() {
            return Effect::None;
        }
        let next = isize::try_from(self.selection)
            .unwrap_or(0)
            .saturating_add(delta);
        let last = isize::try_from(self.rows.len().saturating_sub(1)).unwrap_or(0);
        self.selection = usize::try_from(next.clamp(0, last)).unwrap_or(0);
        Effect::None
    }

    fn toggle_favorite_filter(&mut self) -> Effect {
        if matches!(self.link, Link::Disconnected) {
            self.status = "disconnected; favorites filter was not submitted".into();
            return Effect::None;
        }
        self.favorites_only = !self.favorites_only;
        self.submit_search(None)
    }

    fn toggle_favorite(&mut self) -> Effect {
        if matches!(self.link, Link::Disconnected) {
            self.status = "disconnected; favorite was not submitted".into();
            return Effect::None;
        }
        let Some(row) = self.selected() else {
            self.status = "no station selected".into();
            return Effect::None;
        };
        let id = row.id.clone();
        let favorite = !row.favorite;
        self.favorite_generation = self.favorite_generation.saturating_add(1);
        let generation = self.favorite_generation;
        self.pending_favorite = Some(generation);
        self.status = "favorite requested".into();
        Effect::SetFavorite {
            generation,
            id,
            favorite,
        }
    }

    fn reload(&mut self) -> Effect {
        if matches!(self.link, Link::Disconnected) && self.snapshot_ms.is_none() {
            self.status = "disconnected; reload needs a catalog or a running service".into();
        }
        Effect::Reload
    }

    fn submit_search(&mut self, after: Option<String>) -> Effect {
        if matches!(self.link, Link::Disconnected) {
            self.status = "disconnected; search was not submitted".into();
            return Effect::None;
        }
        self.search_generation = self.search_generation.saturating_add(1);
        let generation = self.search_generation;
        self.pending_search = Some(generation);
        Effect::Search(SearchQuery {
            generation,
            name: self.query.clone(),
            favorites_only: self.favorites_only,
            after,
        })
    }

    fn restore_selection(&mut self, selected: Option<String>) {
        if self.rows.is_empty() {
            self.selection = 0;
            return;
        }
        if let Some(id) = selected {
            if let Some(index) = self.rows.iter().position(|row| row.id == id) {
                self.selection = index;
            } else {
                self.selection = self.selection.min(self.rows.len() - 1);
                self.status = "selected station left this page".into();
            }
        }
    }
}

fn workspace_from_digit(character: char) -> Workspace {
    match character {
        '2' => Workspace::Live,
        '3' => Workspace::Recordings,
        '4' => Workspace::Monitors,
        '5' => Workspace::Findings,
        '6' => Workspace::System,
        '7' => Workspace::Globe,
        _ => Workspace::Explore,
    }
}

/// Longitude in [-180, 180).
fn wrap_longitude(value: f64) -> f64 {
    (value + 180.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
mod tests {
    use super::{
        Desk, DirectoryView, Effect, Explorer, Focus, Health, Key, Link, Modes, PlaybackView,
        QuotaView, RecordingLine, StationRow, Workspace,
    };

    fn modes() -> Modes {
        Modes {
            reduced_motion: true,
            linear: true,
            monochrome: true,
        }
    }

    fn row(id: &str, name: &str) -> StationRow {
        StationRow {
            id: id.into(),
            name: name.into(),
            favorite: false,
            directory_health: Health::Succeeded,
            directory_languages: "navajo".into(),
            observed_ms: 1_000,
            hls: false,
            coordinates: None,
        }
    }

    fn desk(stations: Vec<StationRow>, recordings: Vec<RecordingLine>) -> Desk {
        Desk {
            link: Link::Service {
                process_id: 42,
                stopping: false,
            },
            observed_ms: 10_000,
            schema_version: 11,
            sqlite_version: "3.53.4".into(),
            provider_dispatch_available: false,
            budgets: Vec::new(),
            captures_active: u64::from(!recordings.is_empty()),
            captures_scheduled: 0,
            captures_interrupted: 0,
            captures_terminal: 0,
            dispatch_available: true,
            directory: Some(DirectoryView {
                cached_stations: 1,
                maximum_stations: 10_000,
                favorite_stations: 0,
                refresh: None,
            }),
            stations,
            quota: Some(QuotaView {
                quota: 50,
                charged: 5,
                reserved: 5,
                available: 45,
            }),
            recordings,
            playback: None,
        }
    }

    fn loaded() -> Explorer {
        let mut model = Explorer::new(modes(), 10_000);
        model.apply_desk(desk(
            vec![row("station-a", "Alpha"), row("station-b", "Beta")],
            vec![RecordingLine {
                id: "rec-1".into(),
                state: "running".into(),
                storage_state: "reserved".into(),
                format: None,
            }],
        ));
        model
    }

    #[test]
    fn focus_moves_and_search_edits_without_quitting_on_q() {
        let mut model = loaded();
        assert_eq!(model.focus(), Focus::Results);
        assert!(matches!(model.handle(Key::Char('/')), Effect::None));
        assert_eq!(model.focus(), Focus::Search);
        assert!(matches!(model.handle(Key::Char('q')), Effect::None));
        assert!(matches!(
            model.handle(Key::Paste("\u{1b}東京".into())),
            Effect::None
        ));
        assert_eq!(model.query(), "q東京");
        assert!(matches!(model.handle(Key::Escape), Effect::None));
        assert_eq!(model.focus(), Focus::Results);
        assert!(matches!(model.handle(Key::Tab), Effect::None));
        assert_eq!(model.focus(), Focus::Detail);
        assert!(matches!(model.handle(Key::BackTab), Effect::None));
        assert_eq!(model.focus(), Focus::Results);
    }

    #[test]
    fn selection_does_not_start_audio_capture_refresh_or_click() {
        let mut model = loaded();
        for key in [
            Key::Up,
            Key::Down,
            Key::Enter,
            Key::Left,
            Key::Right,
            Key::Paste("click refresh listen record".into()),
        ] {
            assert_eq!(model.handle(key), Effect::None);
        }
        assert_eq!(model.captures_active(), 1);
        assert!(model.playback().is_none());
        assert_eq!(model.workspace(), Workspace::Explore);
    }

    #[test]
    fn stale_search_and_favorite_do_not_replace_current_state() {
        let mut model = loaded();
        assert_eq!(model.handle(Key::Char('/')), Effect::None);
        let Effect::Search(older) = model.handle(Key::Enter) else {
            panic!("enter submits search");
        };
        let Effect::Search(newer) = model.handle(Key::Enter) else {
            panic!("second enter submits a newer search");
        };
        assert!(model.apply_search(
            newer.generation,
            vec![row("station-b", "Beta")],
            DirectoryView {
                cached_stations: 2,
                maximum_stations: 10_000,
                favorite_stations: 1,
                refresh: None,
            },
        ));
        assert!(!model.apply_search(
            older.generation,
            vec![row("station-a", "Stale")],
            DirectoryView {
                cached_stations: 9,
                maximum_stations: 10_000,
                favorite_stations: 9,
                refresh: None,
            },
        ));
        assert_eq!(model.rows()[0].name, "Beta");
        assert_eq!(model.directory().map(|item| item.cached_stations), Some(2));

        assert_eq!(model.handle(Key::Escape), Effect::None);
        let Effect::SetFavorite {
            generation,
            id,
            favorite,
        } = model.handle(Key::Char('f'))
        else {
            panic!("favorite is explicit");
        };
        assert!(favorite);
        assert!(!model.apply_favorite(generation.saturating_sub(1), &id, false, None));
        assert!(!model.rows().iter().any(|item| item.favorite));
        assert!(model.apply_favorite(generation, &id, true, None));
        assert!(model.selected().is_some_and(|item| item.favorite));
    }

    #[test]
    fn disconnect_keeps_the_snapshot_and_blocks_favorite() {
        let mut model = loaded();
        model.note_disconnect(20_000, "service stopped");
        assert_eq!(model.link(), &Link::Disconnected);
        assert_eq!(model.snapshot_age(), "10s");
        assert_eq!(model.rows().len(), 2);
        assert_eq!(model.recordings()[0].state, "running");
        assert_eq!(model.handle(Key::Char('f')), Effect::None);
        assert!(model.status().contains("not submitted"));
        assert_eq!(model.handle(Key::Char('v')), Effect::None);
    }

    #[test]
    fn quit_during_a_recording_detaches_and_leaves_it() {
        let mut model = loaded();
        assert_eq!(model.handle(Key::Char('q')), Effect::Detach);
        assert_eq!(model.recordings()[0].state, "running");
        assert_eq!(model.captures_active(), 1);
        assert!(model.playback().is_none());
    }

    #[test]
    fn unavailable_workspaces_can_be_selected() {
        let mut model = loaded();
        model.handle(Key::Char('4'));
        assert_eq!(model.workspace(), Workspace::Monitors);
        assert!(!model.workspace().available());
        model.handle(Key::Char('5'));
        assert_eq!(model.workspace(), Workspace::Findings);
        assert_eq!(model.handle(Key::Char('6')), Effect::None);
        assert_eq!(model.workspace(), Workspace::System);
    }

    #[test]
    fn globe_keys_rotate_clamp_wrap_and_center_without_effects() {
        let mut model = Explorer::new(modes(), 0);
        assert!(matches!(model.handle(Key::Char('7')), Effect::None));
        assert_eq!(model.workspace(), Workspace::Globe);
        assert_eq!(model.globe_center(), (0.0, 20.0));
        for _ in 0..13 {
            assert!(matches!(model.handle(Key::Char('h')), Effect::None));
        }
        assert_eq!(model.globe_center(), (165.0, 20.0));
        for _ in 0..10 {
            model.handle(Key::Char('k'));
        }
        assert_eq!(model.globe_center(), (165.0, 90.0));
        for _ in 0..20 {
            model.handle(Key::Char('j'));
        }
        assert_eq!(model.globe_center(), (165.0, -90.0));
        assert!(!model.flat_map());
        model.handle(Key::Char('m'));
        assert!(model.flat_map());
        model.handle(Key::Char('c'));
        assert_eq!(model.globe_center(), (165.0, -90.0));
        assert!(model.status().contains("no directory coordinates"));
        // Globe keys only act in the Globe workspace.
        model.handle(Key::Char('1'));
        model.handle(Key::Char('h'));
        assert_eq!(model.globe_center(), (165.0, -90.0));
        assert!(matches!(model.handle(Key::Char('f')), Effect::None));
    }

    #[test]
    fn modes_exist_and_animation_stays_off() {
        let model = Explorer::new(modes(), 0);
        assert!(model.modes().reduced_motion);
        assert!(model.modes().linear);
        assert!(model.modes().monochrome);
        assert_eq!(model.animation_frames(), 0);
        let plain = Explorer::new(
            Modes {
                reduced_motion: false,
                linear: false,
                monochrome: false,
            },
            0,
        );
        assert_eq!(plain.animation_frames(), 0);
    }

    #[test]
    fn playback_receipt_is_separate_from_directory_health() {
        let mut model = loaded();
        model.apply_desk(Desk {
            playback: Some(PlaybackView {
                id: "listen-1".into(),
                state: "running".into(),
                format: Some("wav".into()),
                source_revision: "rev-1".into(),
                failure: None,
            }),
            ..desk(vec![row("station-a", "Alpha")], Vec::new())
        });
        assert_eq!(
            model.playback().map(|item| item.id.as_str()),
            Some("listen-1")
        );
        assert_eq!(
            model.selected().map(|item| item.directory_health),
            Some(Health::Succeeded)
        );
    }
}
