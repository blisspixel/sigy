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
            Self::Monitors => "Monitors",
            Self::Findings => "Findings",
            Self::Globe => "Globe",
            Self::System => "System",
        }
    }

    #[must_use]
    pub const fn available(self) -> bool {
        matches!(
            self,
            Self::Explore
                | Self::Live
                | Self::Recordings
                | Self::Monitors
                | Self::Findings
                | Self::Globe
                | Self::System
        )
    }

    /// The number key that selects this workspace. The tab bar uses the same order.
    #[must_use]
    pub const fn digit(self) -> char {
        match self {
            Self::Explore => '1',
            Self::Live => '2',
            Self::Recordings => '3',
            Self::Monitors => '4',
            Self::Findings => '5',
            Self::System => '6',
            Self::Globe => '7',
        }
    }

    /// Every workspace in number-key order.
    pub const ALL: [Self; 7] = [
        Self::Explore,
        Self::Live,
        Self::Recordings,
        Self::Monitors,
        Self::Findings,
        Self::System,
        Self::Globe,
    ];

    const fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Explore, true) | (Self::Recordings, false) => Self::Live,
            (Self::Live, true) | (Self::Monitors, false) => Self::Recordings,
            (Self::Recordings, true) | (Self::Findings, false) => Self::Monitors,
            (Self::Monitors, true) | (Self::System, false) => Self::Findings,
            (Self::Findings, true) | (Self::Globe, false) => Self::System,
            (Self::System, true) | (Self::Explore, false) => Self::Globe,
            (Self::Globe, true) | (Self::Live, false) => Self::Explore,
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
    pub timeline: super::timeline::Timeline,
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
    /// Cursor for the station page after this one, when the cache has more matches.
    pub next_after: Option<String>,
    pub quota: Option<QuotaView>,
    pub recordings: Vec<RecordingLine>,
    pub playback: Option<PlaybackView>,
}

/// How a submitted station search moves through pages once its response applies.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PageMove {
    First,
    Next(String),
    Previous,
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
    MonitorList,
    MonitorDetail {
        id: String,
    },
    Finding {
        monitor: String,
        finding: String,
    },
    FindingOriginal {
        recording: String,
    },
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
    /// Cursor that produced the current station page; `None` is the first page.
    page_cursor: Option<String>,
    /// Cursors of earlier pages, so `p` can return without a second index.
    page_history: Vec<Option<String>>,
    next_after: Option<String>,
    pending_page: Option<PageMove>,
    link: Link,
    snapshot_ms: Option<i64>,
    now_ms: i64,
    directory: Option<DirectoryView>,
    recordings: Vec<RecordingLine>,
    recording_selection: usize,
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
    pub monitors: super::monitor::Browser,
    pub findings: super::finding::Browser,
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
            page_cursor: None,
            page_history: Vec::new(),
            next_after: None,
            pending_page: None,
            link: Link::Disconnected,
            snapshot_ms: None,
            now_ms,
            directory: None,
            recordings: Vec::new(),
            recording_selection: 0,
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
            monitors: super::monitor::Browser::default(),
            findings: super::finding::Browser::default(),
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

    /// One-based number of the station page on screen.
    #[must_use]
    pub const fn page_number(&self) -> usize {
        self.page_history.len().saturating_add(1)
    }

    #[must_use]
    pub const fn has_next_page(&self) -> bool {
        self.next_after.is_some()
    }

    /// Cursor that reproduces the current station page on reload.
    #[must_use]
    pub fn page_cursor(&self) -> Option<&str> {
        self.page_cursor.as_deref()
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
    pub fn selected_recording(&self) -> Option<&RecordingLine> {
        self.recordings.get(self.recording_selection)
    }

    #[must_use]
    pub const fn recording_selection(&self) -> usize {
        self.recording_selection
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
        let recording = self.selected_recording().map(|row| row.id.clone());
        self.search_generation = self.search_generation.saturating_add(1);
        self.applied_search = self.search_generation;
        self.pending_search = None;
        self.pending_page = None;
        self.pending_favorite = None;
        self.next_after = desk.next_after;
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
        self.recording_selection = recording
            .and_then(|id| self.recordings.iter().position(|record| record.id == id))
            .unwrap_or(0);
        self.playback = desk.playback;
        self.rows = desk.stations;
        self.restore_selection(selected);
        self.draw = Draw::Needed;
    }

    pub fn note_disconnect(&mut self, now_ms: i64, reason: &str) {
        self.link = Link::Disconnected;
        self.now_ms = now_ms;
        self.pending_search = None;
        self.pending_page = None;
        self.pending_favorite = None;
        self.status = sanitize(
            &format!(
                "Disconnected. Capture was not stopped. Last snapshot kept. r reconnects. Cause: {reason}."
            ),
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
        next_after: Option<String>,
    ) -> bool {
        if self.pending_search != Some(generation) || generation < self.applied_search {
            return false;
        }
        let selected = self.selected().map(|row| row.id.clone());
        self.pending_search = None;
        self.applied_search = generation;
        self.directory = Some(directory);
        self.rows = rows;
        self.next_after = next_after;
        match self.pending_page.take() {
            Some(PageMove::Next(after)) => {
                let previous = self.page_cursor.replace(after);
                self.page_history.push(previous);
                self.selection = 0;
            }
            Some(PageMove::Previous) => {
                self.page_cursor = self.page_history.pop().flatten();
                self.selection = 0;
            }
            Some(PageMove::First) | None => {
                self.page_cursor = None;
                self.page_history.clear();
                self.restore_selection(selected);
            }
        }
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
        if self.workspace == Workspace::Findings
            && let Some(effect) = self.findings.handle(&key)
        {
            return effect;
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
        if self.workspace == Workspace::Monitors
            && let Some(effect) = self.monitor_key(&key)
        {
            return effect;
        }
        if self.workspace == Workspace::Recordings && self.recording_key(&key) {
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
                self.workspace_effect()
            }
            Key::Right => {
                self.workspace = self.workspace.cycle(true);
                self.workspace_effect()
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
            Key::Char(direction @ ('n' | 'p')) => self.turn_page(direction == 'n'),
            Key::Char(character @ '1'..='7') => {
                self.workspace = workspace_from_digit(character);
                self.workspace_effect()
            }
            Key::Paste(_) | Key::Char(_) | Key::Backspace | Key::Escape | Key::Redraw => {
                Effect::None
            }
        }
    }

    fn workspace_effect(&self) -> Effect {
        if self.workspace == Workspace::Monitors {
            Effect::MonitorList
        } else {
            Effect::None
        }
    }

    pub fn open_finding_recording(&mut self, record: RecordingLine) {
        if let Some(index) = self.recordings.iter().position(|item| item.id == record.id) {
            self.recordings[index] = record;
            self.recording_selection = index;
        } else {
            self.recordings.insert(0, record);
            self.recordings
                .truncate(usize::try_from(PAGE_LIMIT).unwrap_or(16));
            self.recording_selection = 0;
        }
        self.workspace = Workspace::Recordings;
        self.focus = Focus::Results;
        self.note_message("Cited recording metadata selected. No playback or processing started.");
    }

    fn recording_key(&mut self, key: &Key) -> bool {
        match key {
            Key::Up => self.recording_selection = self.recording_selection.saturating_sub(1),
            Key::Down => {
                self.recording_selection = self
                    .recording_selection
                    .saturating_add(1)
                    .min(self.recordings.len().saturating_sub(1));
            }
            Key::Char('f' | 'v' | '/') => self
                .note_message("Recording selection is read-only. r reloads the metadata snapshot."),
            _ => return false,
        }
        true
    }

    fn monitor_key(&mut self, key: &Key) -> Option<Effect> {
        match key {
            Key::Up | Key::Down => self.monitors.move_selection(*key == Key::Down),
            Key::Escape => self.monitors.back(),
            Key::Enter => return Some(self.monitors.open()),
            Key::Char('r') => return Some(self.monitors.reload()),
            Key::Char('f' | 'v' | '/') => self.note_message(
                "Monitor navigation is read-only. Enter reads coverage and passages.",
            ),
            _ => return None,
        }
        Some(Effect::None)
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
            Key::Enter => self.submit_search(PageMove::First),
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
            self.status =
                "Disconnected; the favorites filter was not submitted. r reconnects.".into();
            return Effect::None;
        }
        self.favorites_only = !self.favorites_only;
        self.submit_search(PageMove::First)
    }

    fn toggle_favorite(&mut self) -> Effect {
        if matches!(self.link, Link::Disconnected) {
            self.status = "Disconnected; the favorite was not submitted. r reconnects.".into();
            return Effect::None;
        }
        let Some(row) = self.selected() else {
            self.status = "No station is selected.".into();
            return Effect::None;
        };
        let id = row.id.clone();
        let favorite = !row.favorite;
        self.favorite_generation = self.favorite_generation.saturating_add(1);
        let generation = self.favorite_generation;
        self.pending_favorite = Some(generation);
        self.status = "Favorite requested.".into();
        Effect::SetFavorite {
            generation,
            id,
            favorite,
        }
    }

    fn reload(&mut self) -> Effect {
        if matches!(self.link, Link::Disconnected) && self.snapshot_ms.is_none() {
            self.status = "Disconnected. Reload needs the library or a running service.".into();
        }
        Effect::Reload
    }

    /// `n` and `p` move through cached station pages with the existing search cursor.
    fn turn_page(&mut self, forward: bool) -> Effect {
        if !matches!(self.workspace, Workspace::Explore | Workspace::Globe) {
            self.status = "n and p turn station pages in Explore (1) and Globe (7).".into();
            return Effect::None;
        }
        if forward {
            let Some(after) = self.next_after.clone() else {
                self.status = "This is the last cached page for this search.".into();
                return Effect::None;
            };
            self.submit_search(PageMove::Next(after))
        } else if self.page_history.is_empty() {
            self.status = "This is the first page.".into();
            Effect::None
        } else {
            self.submit_search(PageMove::Previous)
        }
    }

    fn submit_search(&mut self, movement: PageMove) -> Effect {
        if matches!(self.link, Link::Disconnected) {
            self.status = "Disconnected; the search was not submitted. r reconnects.".into();
            return Effect::None;
        }
        let after = match &movement {
            PageMove::First => None,
            PageMove::Next(after) => Some(after.clone()),
            PageMove::Previous => self.page_history.last().cloned().flatten(),
        };
        self.search_generation = self.search_generation.saturating_add(1);
        let generation = self.search_generation;
        self.pending_search = Some(generation);
        self.pending_page = Some(movement);
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
                self.status = "The selected station is not on this page.".into();
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
            next_after: None,
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
                timeline: super::super::timeline::Timeline::default(),
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
            None,
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
            Some("station-a".into()),
        ));
        assert!(!model.has_next_page());
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

    fn page(model: &mut Explorer, effect: Effect, rows: &[&str], next: Option<&str>) -> bool {
        let Effect::Search(query) = effect else {
            return false;
        };
        model.apply_search(
            query.generation,
            rows.iter().map(|id| row(id, id)).collect(),
            DirectoryView {
                cached_stations: 40,
                maximum_stations: 10_000,
                favorite_stations: 0,
                refresh: None,
            },
            next.map(str::to_owned),
        )
    }

    #[test]
    fn pages_turn_with_the_search_cursor_and_return_without_new_state() {
        let mut model = loaded();
        assert_eq!(model.handle(Key::Char('p')), Effect::None);
        assert!(model.status().contains("first page"));
        assert_eq!(model.handle(Key::Char('n')), Effect::None);
        assert!(model.status().contains("last cached page"));
        let first = model.handle(Key::Char('v'));
        assert!(page(&mut model, first, &["a", "b"], Some("b")));
        assert_eq!(model.page_number(), 1);
        model.handle(Key::Down);
        let Effect::Search(query) = model.handle(Key::Char('n')) else {
            panic!("n requests the next page");
        };
        assert_eq!(query.after.as_deref(), Some("b"));
        assert!(query.favorites_only);
        assert!(page(
            &mut model,
            Effect::Search(query),
            &["c", "d"],
            Some("d")
        ));
        assert_eq!(model.page_number(), 2);
        assert_eq!(model.selection(), 0);
        assert_eq!(model.page_cursor(), Some("b"));
        let next = model.handle(Key::Char('n'));
        assert!(page(&mut model, next, &["e"], None));
        assert_eq!(model.page_number(), 3);
        assert!(!model.has_next_page());
        let Effect::Search(back) = model.handle(Key::Char('p')) else {
            panic!("p requests the previous page");
        };
        assert_eq!(back.after.as_deref(), Some("b"));
        assert!(page(
            &mut model,
            Effect::Search(back),
            &["c", "d"],
            Some("d")
        ));
        assert_eq!(model.page_number(), 2);
        let Effect::Search(start) = model.handle(Key::Char('p')) else {
            panic!("p returns to the first page");
        };
        assert_eq!(start.after, None);
        // A stale page response cannot move the cursor.
        let newer = model.handle(Key::Char('v'));
        assert!(!page(&mut model, Effect::Search(start), &["x"], None));
        assert_eq!(model.page_number(), 2);
        assert!(page(&mut model, newer, &["a"], None));
        assert_eq!(model.page_number(), 1);
        assert_eq!(model.page_cursor(), None);
        // Paging is a station read: it is refused outside Explore and Globe and starts no work.
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('n')), Effect::None);
        assert!(model.status().contains("Explore (1) and Globe (7)"));
        assert_eq!(model.captures_active(), 1);
        assert!(model.playback().is_none());
    }

    #[test]
    fn tab_order_and_number_keys_agree() {
        let mut model = loaded();
        for workspace in Workspace::ALL {
            model.handle(Key::Char(workspace.digit()));
            assert_eq!(model.workspace(), workspace);
        }
        model.handle(Key::Char('1'));
        for expected in Workspace::ALL.into_iter().skip(1) {
            model.handle(Key::Right);
            assert_eq!(model.workspace(), expected);
        }
        model.handle(Key::Right);
        assert_eq!(model.workspace(), Workspace::Explore);
        model.handle(Key::Left);
        assert_eq!(model.workspace(), Workspace::Globe);
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
    fn recording_selection_is_separate_and_never_starts_work() {
        let mut model = loaded();
        let mut second = model.recordings()[0].clone();
        second.id = "rec-2".into();
        model.recordings.push(second);
        assert_eq!(model.handle(Key::Char('3')), Effect::None);
        assert_eq!(model.handle(Key::Down), Effect::None);
        assert_eq!(
            model.selected_recording().map(|record| record.id.as_str()),
            Some("rec-2")
        );
        assert_eq!(
            model.selected().map(|station| station.id.as_str()),
            Some("station-a")
        );
        for key in [Key::Enter, Key::Char('f'), Key::Char('v'), Key::Char('/')] {
            assert_eq!(model.handle(key), Effect::None);
        }
        assert_eq!(model.handle(Key::Char('q')), Effect::Detach);
        assert_eq!(model.captures_active(), 1);
    }

    #[test]
    fn monitor_and_pending_findings_workspaces_can_be_selected() {
        let mut model = loaded();
        assert_eq!(model.handle(Key::Char('4')), Effect::MonitorList);
        assert_eq!(model.workspace(), Workspace::Monitors);
        assert!(model.workspace().available());
        model.handle(Key::Char('5'));
        assert_eq!(model.workspace(), Workspace::Findings);
        assert_eq!(model.handle(Key::Char('6')), Effect::None);
        assert_eq!(model.workspace(), Workspace::System);
    }

    #[test]
    fn finding_original_navigation_keeps_full_identity_and_bounds_the_loaded_page() {
        let mut model = loaded();
        let mut record = model.recordings()[0].clone();
        record.id = "exact-long-recording-id:".repeat(4);
        model.open_finding_recording(record.clone());
        assert_eq!(model.workspace(), Workspace::Recordings);
        assert_eq!(
            model.selected_recording().map(|item| &item.id),
            Some(&record.id)
        );
        assert_eq!(
            model.selected().map(|row| row.id.as_str()),
            Some("station-a")
        );
        record.state = "completed".into();
        model.open_finding_recording(record.clone());
        assert_eq!(model.recordings.len(), 2);
        assert_eq!(
            model.selected_recording().map(|item| item.state.as_str()),
            Some("completed")
        );
        for index in 0..50 {
            record.id = format!("recording-{index}");
            model.open_finding_recording(record.clone());
        }
        assert_eq!(model.recordings.len(), 16);
        assert_eq!(model.captures_active(), 1);
        assert!(model.playback().is_none());
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
