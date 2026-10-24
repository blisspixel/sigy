//! Explorer view state. Mutations are explicit effects, never selection.

use super::search::{Edit, Search};
use super::timeline::MAX_INTERVALS;
#[cfg(test)]
mod filter_tests;
use crate::explorer::text::{age_label, sanitize};
use sigy_service::storage::dvr::{Recording, RecordingGap, RecordingHold, RecordingInterval};
use sigy_service::{control::DirectoryCatalog, discovery::StationFilter};
mod catalog;
mod context;
#[cfg(test)]
mod ordered_tests;
mod random;
mod spin;
const PAGE_LIMIT: u32 = 16;
/// Newest holds kept on one recording row. The catalog can store more.
pub(super) const MAX_SHOWN_HOLDS: usize = 8;
pub(super) const PROTECT_BUSY: &str =
    "A hold is still settling. It was not repeated. Capture keeps running.";
pub(super) const PROTECT_UNRESOLVED: &str =
    "Hold request ended without a result. It was not repeated. Capture keeps running.";

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

/// High contrast is an accessibility mode. It is not a second palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Standard,
    HighContrast,
}

impl Emphasis {
    #[must_use]
    pub const fn high(self) -> bool {
        matches!(self, Self::HighContrast)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    pub reduced_motion: bool,
    pub linear: bool,
    pub monochrome: bool,
    pub emphasis: Emphasis,
}

impl Modes {
    #[must_use]
    pub fn resolve(reduced_motion: bool, linear: bool, monochrome: bool) -> Self {
        Self::resolve_with(
            reduced_motion,
            linear,
            monochrome,
            Emphasis::Standard,
            crate::style::ColorChoice::Auto,
        )
    }

    #[must_use]
    pub fn resolve_with(
        reduced_motion: bool,
        linear: bool,
        monochrome: bool,
        emphasis: Emphasis,
        color: crate::style::ColorChoice,
    ) -> Self {
        let plain = match color {
            crate::style::ColorChoice::Never => true,
            crate::style::ColorChoice::Always => false,
            crate::style::ColorChoice::Auto => crate::style::plain_requested(),
        };
        Self {
            reduced_motion: reduced_motion || env_flag("SIGY_REDUCED_MOTION"),
            linear: linear || env_flag("SIGY_LINEAR"),
            monochrome: monochrome || env_flag("SIGY_MONOCHROME") || plain,
            emphasis: if emphasis.high() || env_flag("SIGY_HIGH_CONTRAST") {
                Emphasis::HighContrast
            } else {
                Emphasis::Standard
            },
        }
    }
}

fn env_flag(name: &str) -> bool {
    crate::style::env_set(name)
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
    /// Full metadata from the same bounded ordered page, before preview clipping.
    pub metadata: Option<sigy_service::discovery::Station>,
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

/// Sealed intervals and gaps used to choose the next listen. The glyph timeline is not this map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SealMap {
    pub state: String,
    pub open_ceiling: u64,
    pub open_object_key: Option<String>,
    pub intervals: Vec<RecordingInterval>,
    pub gaps: Vec<RecordingGap>,
    pub truncated: bool,
}

impl SealMap {
    /// Copy at most [`MAX_INTERVALS`] rows. A longer recording is marked truncated and must not start audio.
    #[must_use]
    pub(super) fn from_recording(record: &Recording) -> Self {
        let truncated = record.intervals.len() > MAX_INTERVALS || record.gaps.len() > MAX_INTERVALS;
        Self {
            state: record.state.chars().take(32).collect(),
            open_ceiling: record.open_ceiling,
            open_object_key: record.open_object_key.clone(),
            intervals: record
                .intervals
                .iter()
                .take(MAX_INTERVALS)
                .cloned()
                .collect(),
            gaps: record.gaps.iter().take(MAX_INTERVALS).cloned().collect(),
            truncated,
        }
    }
}

/// Bounds of one `record hold` row. No object key or path is stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldSpan {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingLine {
    pub id: String,
    pub state: String,
    pub storage_state: String,
    pub format: Option<String>,
    pub timeline: super::timeline::Timeline,
    pub seal: SealMap,
    pub hold_total: u32,
    pub shown_holds: Vec<HoldSpan>,
}

/// Keep the newest [`MAX_SHOWN_HOLDS`] ranges and the full count.
#[must_use]
pub(super) fn shown_holds(holds: &[RecordingHold]) -> (u32, Vec<HoldSpan>) {
    let hold_total = u32::try_from(holds.len()).unwrap_or(u32::MAX);
    let skip = holds.len().saturating_sub(MAX_SHOWN_HOLDS);
    let shown_holds = holds
        .iter()
        .skip(skip)
        .map(|hold| HoldSpan {
            ordinal: hold.ordinal,
            start_us: hold.start_us,
            end_us: hold.end_us,
        })
        .collect();
    (hold_total, shown_holds)
}

impl RecordingLine {
    #[must_use]
    pub fn active(&self) -> bool {
        matches!(
            self.state.as_str(),
            "scheduled" | "starting" | "running" | "retrying" | "stopping"
        )
    }

    /// One line for holds already on this row. Empty when nothing is recorded.
    #[must_use]
    pub fn hold_caption(&self) -> Option<String> {
        let latest = self.shown_holds.last()?;
        if u32::try_from(self.shown_holds.len()).ok() == Some(self.hold_total) {
            Some(format!(
                "Hold {}: [{}, {}). {} on this recording.",
                latest.ordinal,
                latest.start_us,
                latest.end_us,
                hold_word(self.hold_total)
            ))
        } else {
            Some(format!(
                "Hold {}: [{}, {}). Showing {} of {} holds.",
                latest.ordinal,
                latest.start_us,
                latest.end_us,
                self.shown_holds.len(),
                self.hold_total
            ))
        }
    }
}

fn hold_word(count: u32) -> String {
    if count == 1 {
        "1 hold".to_owned()
    } else {
        format!("{count} holds")
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
    pub catalog: Option<DirectoryCatalog>,
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
    pub filter: StationFilter,
    pub favorites_only: bool,
    pub after: Option<String>,
}

/// A confirmed play that is waiting for `y` or `n`. Selection itself is not this value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingPlay {
    Station(String),
    Recording(String),
    Finding {
        monitor: String,
        finding: String,
        recording: String,
        start_us: u64,
        end_us: u64,
    },
    Interval {
        id: String,
        start_us: u64,
        end_us: u64,
    },
    Latest {
        id: String,
        start_us: u64,
    },
}

/// A confirmed interval hold that is waiting for `y` or `n`. It starts no audio.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingProtect {
    id: String,
    start_us: u64,
    end_us: u64,
}

/// One explicit retained listen. It is independent of the live session receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RetainedView {
    request: String,
    phase: String,
    receipt_state: Option<String>,
    decoder_output_us: Option<u64>,
    paused_at_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Detach,
    Play {
        station: String,
    },
    PlayRecording {
        id: String,
        request: String,
    },
    PlayFinding {
        monitor: String,
        finding: String,
        request: String,
        recording: String,
        start_us: u64,
        end_us: u64,
    },
    PlayInterval {
        id: String,
        request: String,
        start_us: u64,
        end_us: u64,
    },
    /// Play the newest sealed interval on the loaded map. No end is stored.
    PlayLatest {
        id: String,
        request: String,
        start_us: u64,
    },
    /// Record one hold on a sealed interval. This starts no decoder.
    ProtectInterval {
        generation: u64,
        id: String,
        start_us: u64,
        end_us: u64,
    },
    /// Stop the retained decoder at its latest sample, or resume that position.
    HoldRetained,
    /// Move one retained listen by ten seconds inside its admitted range.
    SeekRetained {
        forward: bool,
    },
    StopListen,
    StopRetained,
    LinkedContext {
        generation: u64,
        id: String,
        catalog: DirectoryCatalog,
    },
    Search(SearchQuery),
    ExploreRandom(SearchQuery),
    Fetch(SearchQuery),
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
    /// Read claim ids. This writes nothing.
    ClaimList,
    /// Read one stored claim. This writes nothing.
    ClaimShow {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    ClearInput,
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
    fetch_confirmation: Option<super::search::Scope>,
    keys: KeyList,
    explicit_directory: Option<String>,
    play_confirmation: Option<PendingPlay>,
    protect_confirmation: Option<PendingProtect>,
    pub(super) context: Option<super::context::StationContext>,
    context_generation: u64,
    focus: Focus,
    workspace: Workspace,
    modes: Modes,
    pub(super) search: Search,
    rows: Vec<StationRow>,
    selection: usize,
    /// Cursor that produced the current station page; `None` is the first page.
    page_cursor: Option<String>,
    /// Cursors of earlier pages, so `p` can return without a second index.
    page_history: Vec<Option<String>>,
    next_after: Option<String>,
    catalog: Option<DirectoryCatalog>,
    page_catalog: Option<DirectoryCatalog>,
    retired_catalogs: Vec<String>,
    restart_required: bool,
    pending_page: Option<PageMove>,
    link: Link,
    snapshot_ms: Option<i64>,
    now_ms: i64,
    directory: Option<DirectoryView>,
    recordings: Vec<RecordingLine>,
    recording_selection: usize,
    /// Index into the selected recording's loaded sealed intervals.
    interval_index: usize,
    captures_active: u64,
    quota: Option<QuotaView>,
    playback: Option<PlaybackView>,
    /// Explicit retained listen. Browsing does not replace it.
    retained: Option<RetainedView>,
    /// Station whose play was confirmed. Cleared when that listen is not running.
    live_station: Option<String>,
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
    pending_random: Option<u64>,
    random_count: Option<u32>,
    favorite_generation: u64,
    pending_favorite: Option<u64>,
    protect_generation: u64,
    pending_protect: Option<u64>,
    /// Fences discardable metadata reads. A late success or failure is ignored.
    read_epoch: u64,
    status: String,
    draw: Draw,
    globe: GlobeView,
    pub monitors: super::monitor::Browser,
    pub findings: super::finding::Browser,
    pub claims: super::claims::Browser,
}

/// What the Globe workspace shows. Changing it never contacts a station.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GlobeView {
    center: (f64, f64),
    flat: bool,
    spin: spin::Spin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Draw {
    Needed,
    Clean,
}

/// Whether the focused-workspace key list is covering the explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyList {
    Closed,
    Open,
}

impl Explorer {
    #[must_use]
    pub fn new(modes: Modes, now_ms: i64) -> Self {
        Self {
            fetch_confirmation: None,
            keys: KeyList::Closed,
            explicit_directory: None,
            play_confirmation: None,
            protect_confirmation: None,
            context: None,
            context_generation: 0,
            focus: Focus::Results,
            workspace: Workspace::Explore,
            modes,
            search: Search::default(),
            rows: Vec::new(),
            selection: 0,
            page_cursor: None,
            page_history: Vec::new(),
            next_after: None,
            catalog: None,
            page_catalog: None,
            retired_catalogs: Vec::new(),
            restart_required: false,
            pending_page: None,
            link: Link::Disconnected,
            snapshot_ms: None,
            now_ms,
            directory: None,
            recordings: Vec::new(),
            recording_selection: 0,
            interval_index: 0,
            captures_active: 0,
            quota: None,
            playback: None,
            retained: None,
            live_station: None,
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
            pending_random: None,
            random_count: None,
            favorite_generation: 0,
            pending_favorite: None,
            protect_generation: 0,
            pending_protect: None,
            read_epoch: 0,
            status: "Quit detaches this client. It does not stop the service or a recording."
                .into(),
            draw: Draw::Needed,
            globe: GlobeView {
                center: (0.0, 20.0),
                flat: false,
                spin: spin::Spin::default(),
            },
            monitors: super::monitor::Browser::default(),
            findings: super::finding::Browser::default(),
            claims: super::claims::Browser::default(),
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
        match key {
            's' => self.toggle_spin(),
            'h' => self.turn_by(-15.0, 0.0),
            'l' => self.turn_by(15.0, 0.0),
            'k' => self.turn_by(0.0, 15.0),
            'j' => self.turn_by(0.0, -15.0),
            'm' => self.globe.flat = !self.globe.flat,
            'c' => {
                self.stop_spin();
                match self.selected().and_then(|row| row.coordinates) {
                    Some(place) => {
                        self.glide_toward(place.longitude, place.latitude);
                        self.status =
                            "Centered on the selected station's directory coordinates.".into();
                    }
                    None => {
                        self.status = "The selected station has no directory coordinates.".into();
                    }
                }
            }
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

    /// The key overlay is open. It lists the focused workspace and starts no work.
    #[must_use]
    pub const fn keys_open(&self) -> bool {
        matches!(self.keys, KeyList::Open)
    }

    #[must_use]
    pub const fn modes(&self) -> Modes {
        self.modes
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.search.draft.filter.name
    }

    #[must_use]
    pub const fn favorites_only(&self) -> bool {
        self.search.applied.favorites_only
    }

    pub fn current_search(&self) -> SearchQuery {
        SearchQuery {
            generation: self.applied_search,
            filter: self.search.applied.filter.clone(),
            favorites_only: self.search.applied.favorites_only,
            after: self.page_cursor().map(str::to_owned),
        }
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
        self.next_after.is_some() && !self.restart_required
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

    pub(crate) fn observe_playback(&mut self, playback: PlaybackView) {
        if self.playback.as_ref() != Some(&playback) {
            self.playback = Some(playback);
            self.draw = Draw::Needed;
        }
        self.retain_live_station();
    }

    fn retain_live_station(&mut self) {
        let running = self
            .playback
            .as_ref()
            .is_some_and(|view| matches!(view.state.as_str(), "running" | "playing"));
        if !running {
            self.live_station = None;
        }
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
    pub const fn page_limit() -> u32 {
        PAGE_LIMIT
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.draw == Draw::Needed;
        self.draw = Draw::Clean;
        dirty
    }

    /// Advance presentation time without changing any stored observation.
    pub fn tick_clock(&mut self, now_ms: i64) {
        if self.search.picker.is_some() || self.search.editing_filters() {
            self.now_ms = now_ms;
            return;
        }
        let observed = self
            .context
            .as_ref()
            .map(|context| context.row.observed_ms)
            .or_else(|| self.selected().map(|row| row.observed_ms));
        let changed = self
            .snapshot_ms
            .is_some_and(|time| age_label(time, self.now_ms) != age_label(time, now_ms))
            || observed.is_some_and(|time| age_label(time, self.now_ms) != age_label(time, now_ms))
            || (self.workspace == Workspace::Globe
                && self.now_ms.div_euclid(60_000) != now_ms.div_euclid(60_000));
        self.now_ms = now_ms;
        if changed {
            self.draw = Draw::Needed;
        }
    }

    pub fn apply_desk(&mut self, desk: Desk) {
        if self.context.is_some() {
            return;
        }
        if let Some(catalog) = &desk.catalog
            && !self.catalog_fresh(catalog, self.page_cursor.is_none())
        {
            self.note_message("An older catalog reply was ignored. g opens the first cached page.");
            return;
        }
        let selected = self.selected().map(|row| row.id.clone());
        let recording = self.selected_recording().map(|row| row.id.clone());
        self.search_generation = self.search_generation.saturating_add(1);
        self.applied_search = self.search_generation;
        self.pending_search = None;
        self.search.interrupt();
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
        self.playback = desk.playback;
        self.retain_live_station();
        self.recordings = desk.recordings;
        self.recording_selection = recording
            .as_deref()
            .and_then(|id| self.recordings.iter().position(|record| record.id == id))
            .unwrap_or(0);
        let same_recording = self
            .selected_recording()
            .is_some_and(|record| Some(record.id.as_str()) == recording.as_deref());
        if same_recording {
            let len = self
                .selected_recording()
                .map_or(0, |record| record.seal.intervals.len());
            if self.interval_index >= len {
                self.interval_index = 0;
            }
        } else {
            self.interval_index = 0;
        }
        self.rows = desk.stations;
        self.random_count = None;
        if let Some(catalog) = desk.catalog {
            self.install_catalog(&catalog);
            self.page_catalog = Some(catalog);
            self.restart_required = false;
        }
        self.restore_selection(selected);
        self.draw = Draw::Needed;
    }

    pub fn note_disconnect(&mut self, now_ms: i64, reason: &str) {
        self.invalidate_reads();
        self.link = Link::Disconnected;
        self.now_ms = now_ms;
        self.pending_search = None;
        self.search.interrupt();
        self.pending_page = None;
        self.pending_favorite = None;
        self.pending_protect = None;
        self.protect_confirmation = None;
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
        if !self.search.accept() {
            return false;
        }
        let selected = self.selected().map(|row| row.id.clone());
        self.pending_search = None;
        self.applied_search = generation;
        self.directory = Some(directory);
        self.rows = rows;
        self.random_count = None;
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
        if let Some(issue) = self.search.validation {
            self.status = issue.message().into();
        } else {
            self.status = format!(
                "Cached page read: {}. F edits filters; x resets.",
                crate::copy::station_count(u64::try_from(self.rows.len()).unwrap_or(u64::MAX))
            );
        }
        self.draw = Draw::Needed;
        true
    }

    /// A failed request cannot change the applied scope or strand its pending page.
    #[must_use]
    pub fn fail_search(&mut self, generation: u64, message: &str) -> bool {
        if self.pending_search != Some(generation) {
            return false;
        }
        self.pending_search = None;
        self.pending_page = None;
        self.search.fail();
        self.note_message(message);
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

    pub(crate) fn initialize_search(
        &mut self,
        filter: StationFilter,
        explicit_directory: Option<&str>,
    ) -> sigy_service::Result<()> {
        filter.validate()?;
        self.search.applied.filter = filter;
        self.search.draft = self.search.applied.clone();
        self.explicit_directory = explicit_directory.map(str::to_owned);
        Ok(())
    }

    pub(crate) fn discovery_command(&self) -> String {
        let hint = sigy_service::guidance::DirectoryHint {
            filter: self.search.applied.filter.clone(),
            favorites: self.favorites_only(),
            order: None,
            limit: None,
        };
        sigy_service::guidance::with_data_dir(&hint.command(), self.explicit_directory.as_deref())
    }

    fn keys_key(&mut self, key: &Key) -> Option<Effect> {
        if self.keys_open() {
            return Some(match key {
                Key::Escape | Key::Char('?') => {
                    self.keys = KeyList::Closed;
                    Effect::None
                }
                Key::Char('q') | Key::Quit => Effect::Detach,
                _ => Effect::None,
            });
        }
        if !matches!(key, Key::Char('?')) || self.text_entry() {
            return None;
        }
        self.keys = KeyList::Open;
        Some(Effect::None)
    }

    fn text_entry(&self) -> bool {
        self.focus == Focus::Search
            || self.search.editing_filters()
            || self.search.picker.is_some()
            || self.findings.editing()
    }

    pub(super) fn search_is_current(&self, generation: u64) -> bool {
        self.pending_search == Some(generation)
    }

    /// Epoch of the newest discardable metadata read.
    #[must_use]
    pub(super) const fn read_epoch(&self) -> u64 {
        self.read_epoch
    }

    /// Starts one discardable read generation. `None` means the counter is exhausted.
    pub(super) fn begin_read(&mut self) -> Option<u64> {
        let next = self.read_epoch.checked_add(1)?;
        self.read_epoch = next;
        Some(next)
    }

    /// Drops pending discardable reads. An in-flight favorite is left alone.
    pub(super) fn invalidate_reads(&mut self) {
        self.read_epoch = self.read_epoch.saturating_add(1);
        self.pending_search = None;
        self.pending_page = None;
        self.pending_random = None;
        self.search.interrupt();
    }

    /// Restores the favorite generation that is already in flight.
    pub(super) fn restore_pending_favorite(&mut self, generation: u64) {
        self.pending_favorite = Some(generation);
    }

    #[must_use]
    pub(super) fn playback_snapshot(&self) -> Option<PlaybackView> {
        self.playback.clone()
    }

    pub(super) fn restore_playback(&mut self, playback: Option<PlaybackView>) {
        if self.playback != playback {
            self.playback = playback;
            self.draw = Draw::Needed;
        }
        self.retain_live_station();
    }

    pub fn handle(&mut self, key: Key) -> Effect {
        self.draw = Draw::Needed;
        if matches!(key, Key::Redraw) {
            return Effect::None;
        }
        self.settle_motion();
        if matches!(key, Key::Quit) {
            return Effect::Detach;
        }
        if self.fetch_confirmation.is_some() {
            return self.confirm_fetch(&key);
        }
        if self.play_confirmation.is_some() {
            return self.confirm_play(&key);
        }
        if self.protect_confirmation.is_some() {
            return self.confirm_protect(&key);
        }
        if let Some(effect) = self.keys_key(&key) {
            return effect;
        }
        if !self.text_entry()
            && let Some(effect) = Self::transport_key(&key)
        {
            return effect;
        }
        if let Some(effect) = self.station_play_key(&key) {
            return effect;
        }
        if self.context.is_some() {
            return self.context_key(&key);
        }
        if self.workspace == Workspace::Findings {
            if self.claims.open() {
                if leaves_claim_page(&key) {
                    self.claims.close();
                } else {
                    let effect = self.claims.handle(&key);
                    if matches!(key, Key::Escape) {
                        self.invalidate_reads();
                    }
                    return effect;
                }
            } else if matches!(key, Key::Char('c')) {
                self.invalidate_reads();
                self.claims.open_list();
                return Effect::ClaimList;
            }
            let previous_query = self.findings.query().to_owned();
            if let Some(effect) = self.findings.handle(&key) {
                if self.findings.query() != previous_query {
                    self.invalidate_reads();
                }
                return effect;
            }
            if let Some(effect) = self.finding_play_key(&key) {
                return effect;
            }
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
        if self.workspace == Workspace::Recordings {
            if let Some(effect) = self.recording_play_key(&key) {
                return effect;
            }
            if self.recording_key(&key) {
                return Effect::None;
            }
        }
        self.handle_browser(&key)
    }

    fn transport_key(key: &Key) -> Option<Effect> {
        match key {
            Key::Char('H') => Some(Effect::HoldRetained),
            Key::Char('[') => Some(Effect::SeekRetained { forward: false }),
            Key::Char(']') => Some(Effect::SeekRetained { forward: true }),
            _ => None,
        }
    }

    fn confirm_play(&mut self, key: &Key) -> Effect {
        match key {
            Key::Char('y' | 'Y') => self.accept_play(),
            Key::Char('n' | 'N') | Key::Escape => {
                self.play_confirmation = None;
                self.note_message("Playback request discarded. No source was contacted.");
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn accept_play(&mut self) -> Effect {
        let Some(pending) = self.play_confirmation.take() else {
            return Effect::None;
        };
        match pending {
            PendingPlay::Station(station) => {
                self.live_station = Some(station.clone());
                self.note_message(
                    "Starting the explicitly selected station. Browsing stays independent.",
                );
                Effect::Play { station }
            }
            PendingPlay::Recording(id) => {
                let Some(request) = self.allocate_retained() else {
                    return Effect::None;
                };
                self.note_message(&format!(
                    "Starting retained listen {request}. Browsing stays independent."
                ));
                Effect::PlayRecording { id, request }
            }
            PendingPlay::Finding {
                monitor,
                finding,
                recording,
                start_us,
                end_us,
            } => {
                let Some(request) = self.allocate_retained() else {
                    return Effect::None;
                };
                self.note_message(&format!(
                    "Starting retained listen {request}. Browsing stays independent."
                ));
                Effect::PlayFinding {
                    monitor,
                    finding,
                    request,
                    recording,
                    start_us,
                    end_us,
                }
            }
            PendingPlay::Interval {
                id,
                start_us,
                end_us,
            } => {
                let Some(request) = self.allocate_retained() else {
                    return Effect::None;
                };
                self.note_message(&format!(
                    "Starting retained listen {request}. Browsing stays independent."
                ));
                Effect::PlayInterval {
                    id,
                    request,
                    start_us,
                    end_us,
                }
            }
            PendingPlay::Latest { id, start_us } => {
                let Some(request) = self.allocate_retained() else {
                    return Effect::None;
                };
                self.note_message(&format!(
                    "Starting retained listen {request}. Browsing stays independent."
                ));
                Effect::PlayLatest {
                    id,
                    request,
                    start_us,
                }
            }
        }
    }

    pub(super) fn allocate_retained(&mut self) -> Option<String> {
        if let Ok(request) = sigy_service::recordings::new_retained_request_id() {
            Some(request)
        } else {
            self.note_message(
                "Could not allocate a retained request id. No playback was requested.",
            );
            None
        }
    }

    fn station_play_key(&mut self, key: &Key) -> Option<Effect> {
        if self.focus == Focus::Search
            || !matches!(
                self.workspace,
                Workspace::Explore | Workspace::Globe | Workspace::Live
            )
        {
            return None;
        }
        match key {
            Key::Char('S') => {
                self.live_station = None;
                Some(Effect::StopListen)
            }
            Key::Char('P') => {
                let row = self
                    .context
                    .as_ref()
                    .map(|context| &context.row)
                    .or_else(|| self.selected());
                if let Some(row) = row {
                    let station = row.id.clone();
                    let name = sanitize(&row.name, 32);
                    self.play_confirmation = Some(PendingPlay::Station(station));
                    self.note_message(&format!("Play {name}? y confirms; n/Esc cancels. Contacts the station; may register its first revision."));
                } else {
                    self.note_message("No selected station to play.");
                }
                Some(Effect::None)
            }
            _ => None,
        }
    }

    fn finding_play_key(&mut self, key: &Key) -> Option<Effect> {
        match key {
            Key::Char('P') => Some(self.confirm_finding()),
            Key::Char('S') => Some(Effect::StopRetained),
            _ => None,
        }
    }

    fn recording_play_key(&mut self, key: &Key) -> Option<Effect> {
        match key {
            Key::Char('P') => Some(self.confirm_recording()),
            Key::Char('I') => Some(self.confirm_interval()),
            Key::Char('L') => Some(self.confirm_latest()),
            Key::Char('K') => Some(self.ask_protect()),
            Key::Char('S') => Some(Effect::StopRetained),
            _ => None,
        }
    }

    fn confirm_interval(&mut self) -> Effect {
        let stored = self.selected_recording().map(|recording| {
            (
                recording.id.clone(),
                recording.storage_state.clone(),
                recording.seal.clone(),
            )
        });
        let Some((id, storage, seal)) = stored else {
            self.note_message(
                "No selected recording. No interval was started. Capture keeps running.",
            );
            return Effect::None;
        };
        if !matches!(storage.as_str(), "reserved" | "retained") {
            self.note_message(super::interval::UNSTORED);
            return Effect::None;
        }
        match super::interval::select(&seal, self.interval_index) {
            Ok(bounds) => {
                self.play_confirmation = Some(PendingPlay::Interval {
                    id,
                    start_us: bounds.start_us,
                    end_us: bounds.end_us,
                });
                self.note_message(&format!(
                    "Play sealed interval [{}, {})? y confirms; n/Esc cancels. Capture keeps running.",
                    bounds.start_us, bounds.end_us
                ));
            }
            Err(refusal) => self.note_message(super::interval::sentence(refusal)),
        }
        Effect::None
    }

    fn confirm_latest(&mut self) -> Effect {
        let stored = self.selected_recording().map(|recording| {
            (
                recording.id.clone(),
                recording.storage_state.clone(),
                recording.seal.clone(),
            )
        });
        let Some((id, storage, seal)) = stored else {
            self.note_message(
                "No selected recording. No interval was started. Capture keeps running.",
            );
            return Effect::None;
        };
        if !matches!(storage.as_str(), "reserved" | "retained") {
            self.note_message(super::interval::UNSTORED);
            return Effect::None;
        }
        match super::interval::latest_playable(&seal) {
            Ok(bounds) => {
                self.play_confirmation = Some(PendingPlay::Latest {
                    id,
                    start_us: bounds.start_us,
                });
                self.note_message(&format!(
                    "Play latest sealed interval [{}, {})? y confirms; n/Esc cancels. Capture keeps running.",
                    bounds.start_us, bounds.end_us
                ));
            }
            Err(refusal) => self.note_message(super::interval::sentence(refusal)),
        }
        Effect::None
    }

    fn confirm_protect(&mut self, key: &Key) -> Effect {
        match key {
            Key::Char('y' | 'Y') => self.accept_protect(),
            Key::Char('n' | 'N') | Key::Escape => {
                self.protect_confirmation = None;
                self.note_message(
                    "Hold request discarded. No retention change was recorded. Capture keeps running.",
                );
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn accept_protect(&mut self) -> Effect {
        let Some(pending) = self.protect_confirmation.take() else {
            return Effect::None;
        };
        if self.pending_protect.is_some() {
            self.note_message(PROTECT_BUSY);
            return Effect::None;
        }
        self.protect_generation = self.protect_generation.saturating_add(1);
        let generation = self.protect_generation;
        self.pending_protect = Some(generation);
        self.note_message("Recording one hold on that sealed interval. Capture keeps running.");
        Effect::ProtectInterval {
            generation,
            id: pending.id,
            start_us: pending.start_us,
            end_us: pending.end_us,
        }
    }

    fn ask_protect(&mut self) -> Effect {
        if matches!(self.link, Link::Disconnected) {
            self.note_message(
                "Disconnected. The hold was not submitted. r reconnects. Capture keeps running.",
            );
            return Effect::None;
        }
        if self.pending_protect.is_some() {
            self.note_message(PROTECT_BUSY);
            return Effect::None;
        }
        let stored = self.selected_recording().map(|recording| {
            (
                recording.id.clone(),
                recording.storage_state.clone(),
                recording.seal.clone(),
            )
        });
        let Some((id, storage, seal)) = stored else {
            self.note_message(
                "No selected recording. No hold was recorded. Capture keeps running.",
            );
            return Effect::None;
        };
        if !matches!(storage.as_str(), "reserved" | "retained") {
            self.note_message(super::interval::UNSTORED);
            return Effect::None;
        }
        match super::interval::select(&seal, self.interval_index) {
            Ok(bounds) => {
                self.protect_confirmation = Some(PendingProtect {
                    id,
                    start_us: bounds.start_us,
                    end_us: bounds.end_us,
                });
                self.note_message(&format!(
                    "Hold sealed interval [{}, {})? y confirms; n/Esc cancels. Capture keeps running.",
                    bounds.start_us, bounds.end_us
                ));
            }
            Err(refusal) => self.note_message(super::interval::sentence(refusal)),
        }
        Effect::None
    }

    /// Drops the in-flight hold fence when this generation is still current.
    pub(super) fn take_protect(&mut self, generation: u64) -> bool {
        if self.pending_protect != Some(generation) {
            return false;
        }
        self.pending_protect = None;
        true
    }

    /// Restores the generation that is already in flight.
    pub(super) fn restore_pending_protect(&mut self, generation: u64) {
        self.pending_protect = Some(generation);
    }

    pub(super) fn fail_protect(&mut self, generation: u64, message: &str) -> bool {
        if !self.take_protect(generation) {
            return false;
        }
        self.note_message(message);
        true
    }

    /// Replaces one loaded recording after a hold reply. The cursor stays when it still fits.
    pub(super) fn install_protected_recording(&mut self, line: RecordingLine) -> bool {
        let Some(index) = self.recordings.iter().position(|item| item.id == line.id) else {
            self.note_message(
                "Hold reply did not match a loaded recording. The previous view was kept. Capture keeps running.",
            );
            return false;
        };
        let same = self.recording_selection == index;
        self.recordings[index] = line;
        if same {
            let len = self.recordings[index].seal.intervals.len();
            if self.interval_index >= len {
                self.interval_index = 0;
            }
        }
        self.draw = Draw::Needed;
        true
    }

    fn confirm_recording(&mut self) -> Effect {
        let Some(recording) = self.selected_recording() else {
            self.note_message("No selected recording to play.");
            return Effect::None;
        };
        if recording.storage_state != "retained" || recording.active() {
            self.note_message("Only a retained recording that is not capturing can play. Selection does not start audio.");
            return Effect::None;
        }
        let id = recording.id.clone();
        self.play_confirmation = Some(PendingPlay::Recording(id.clone()));
        self.note_message(&format!(
            "Play retained recording {id}? y confirms; n/Esc cancels. Capture keeps running."
        ));
        Effect::None
    }

    fn confirm_finding(&mut self) -> Effect {
        let Some(span) = self.findings.cited_span() else {
            self.note_message("This finding has no cited interval. Playback was not requested.");
            return Effect::None;
        };
        self.play_confirmation = Some(PendingPlay::Finding {
            monitor: span.monitor,
            finding: span.finding,
            recording: span.recording,
            start_us: span.start_us,
            end_us: span.end_us,
        });
        self.note_message(
            "Play this cited finding? y confirms; n/Esc cancels. Enter still only inspects.",
        );
        Effect::None
    }

    pub(super) fn begin_retained(&mut self, request: &str) {
        let next = RetainedView {
            request: request.to_owned(),
            phase: "pending".into(),
            receipt_state: None,
            decoder_output_us: None,
            paused_at_us: None,
        };
        if self.retained.as_ref() != Some(&next) {
            self.retained = Some(next);
            self.draw = Draw::Needed;
        }
    }

    pub(super) fn set_retained_phase(
        &mut self,
        request: &str,
        phase: &str,
        receipt_state: Option<&str>,
    ) {
        let Some(view) = &mut self.retained else {
            return;
        };
        if view.request != request {
            return;
        }
        let next_receipt = receipt_state.map(str::to_owned);
        if view.phase == "paused" {
            if view.receipt_state != next_receipt {
                view.receipt_state = next_receipt;
                self.draw = Draw::Needed;
            }
            return;
        }
        if view.phase == phase && view.receipt_state == next_receipt {
            return;
        }
        phase.clone_into(&mut view.phase);
        view.receipt_state = next_receipt;
        self.draw = Draw::Needed;
    }

    /// Show the frozen recording-timeline position. A later receipt cannot erase it.
    pub(super) fn show_paused(&mut self, request: &str, at_us: u64) -> bool {
        let Some(view) = &mut self.retained else {
            return false;
        };
        if view.request != request {
            return false;
        }
        if view.phase == "paused" && view.paused_at_us == Some(at_us) {
            return false;
        }
        view.phase = "paused".into();
        view.paused_at_us = Some(at_us);
        self.draw = Draw::Needed;
        true
    }

    pub(super) fn release_pause(&mut self) {
        let Some(view) = &mut self.retained else {
            return;
        };
        if view.phase != "paused" {
            return;
        }
        view.phase = "closed".into();
        view.paused_at_us = None;
        self.draw = Draw::Needed;
    }

    #[must_use]
    pub(super) fn paused_at_us(&self) -> Option<u64> {
        self.retained.as_ref().and_then(|view| view.paused_at_us)
    }

    #[must_use]
    pub(super) fn retained_label(&self) -> Option<(&str, &str)> {
        self.retained
            .as_ref()
            .map(|view| (view.phase.as_str(), view.request.as_str()))
    }

    #[must_use]
    pub(super) fn decoder_output_us(&self) -> Option<u64> {
        self.retained
            .as_ref()
            .and_then(|view| view.decoder_output_us)
    }

    pub(super) fn set_decoder_output(&mut self, request: &str, time: u64) -> bool {
        let changed = self
            .retained
            .as_ref()
            .is_some_and(|view| view.request == request && view.decoder_output_us != Some(time));
        if !changed {
            return false;
        }
        if let Some(view) = &mut self.retained {
            view.decoder_output_us = Some(time);
            self.draw = Draw::Needed;
        }
        true
    }

    fn confirm_fetch(&mut self, key: &Key) -> Effect {
        match key {
            Key::Char('y' | 'Y') => {
                let Some(scope) = self.fetch_confirmation.take() else {
                    return Effect::None;
                };
                self.search.draft = scope;
                match self.submit_search(PageMove::First) {
                    Effect::Search(query) => {
                        self.note_message("Requesting one filtered directory page. Browsing and playback stay available.");
                        Effect::Fetch(query)
                    }
                    effect => effect,
                }
            }
            Key::Char('n' | 'N') | Key::Escape => {
                self.fetch_confirmation = None;
                self.note_message("Directory request discarded. No network work was requested.");
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn globe_results_arrow(&mut self, key: &Key) -> Option<Effect> {
        if self.workspace != Workspace::Globe || self.focus != Focus::Results {
            return None;
        }
        let (longitude, latitude) = match key {
            Key::Left => (-15.0, 0.0),
            Key::Right => (15.0, 0.0),
            Key::Up => (0.0, 15.0),
            Key::Down => (0.0, -15.0),
            _ => return None,
        };
        self.turn_by(longitude, latitude);
        Some(Effect::None)
    }

    fn handle_browser(&mut self, key: &Key) -> Effect {
        if let Some(effect) = self.globe_results_arrow(key) {
            return effect;
        }
        match key {
            Key::Char('e') if matches!(self.workspace, Workspace::Explore | Workspace::Globe) => {
                self.explore_random()
            }
            Key::Char('R') if matches!(self.workspace, Workspace::Explore | Workspace::Globe) => {
                if self.favorites_only() {
                    self.note_message("Favorites are local choices. v shows all matches before R fetches; f favorites a selected station.");
                } else if self.search.pending() {
                    self.note_message("A search is pending. Wait for its result before requesting a directory page.");
                } else {
                    self.fetch_confirmation = Some(self.search.applied.clone());
                    self.note_message(&format!("Fetch up to 100 directory rows for the applied filters? y confirms; n/Esc cancels. {}", self.discovery_command()));
                }
                Effect::None
            }
            Key::Char('q') | Key::Quit => Effect::Detach,
            Key::Char('/') => {
                self.focus = Focus::Search;
                self.search.begin(false);
                Effect::None
            }
            Key::Char('F') if matches!(self.workspace, Workspace::Explore | Workspace::Globe) => {
                self.focus = Focus::Search;
                self.search.begin(true);
                self.status =
                    "Editing cache filters. Enter applies; Esc discards the draft.".into();
                Effect::None
            }
            Key::Char('C') if matches!(self.workspace, Workspace::Explore | Workspace::Globe) => {
                self.focus = Focus::Search;
                self.search.begin_countries();
                self.status = "Choosing a country from the offline reference. Select, then Enter applies the draft.".into();
                Effect::None
            }
            Key::Char('x') if matches!(self.workspace, Workspace::Explore | Workspace::Globe) => {
                self.search.reset();
                self.submit_search(PageMove::First)
            }
            Key::Tab => {
                self.cycle_focus(true);
                if self.focus == Focus::Search {
                    self.search.begin(false);
                }
                Effect::None
            }
            Key::BackTab => {
                self.cycle_focus(false);
                if self.focus == Focus::Search {
                    self.search.begin(false);
                }
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
                if matches!(self.workspace, Workspace::Explore | Workspace::Globe) {
                    return self.open_context();
                }
                self.focus = Focus::Detail;
                Effect::None
            }
            Key::Char('v') => self.toggle_favorite_filter(),
            Key::Char('f') => self.toggle_favorite(),
            Key::Char('r') => self.reload(),
            Key::Char('g') if matches!(self.workspace, Workspace::Explore | Workspace::Globe) => {
                self.search.draft = self.search.applied.clone();
                self.submit_search(PageMove::First)
            }
            Key::Char(direction @ ('n' | 'p')) => self.turn_page(*direction == 'n'),
            Key::Char(character @ '1'..='7') => {
                self.workspace = workspace_from_digit(*character);
                self.workspace_effect()
            }
            Key::Paste(_)
            | Key::Char(_)
            | Key::Backspace
            | Key::ClearInput
            | Key::Escape
            | Key::Redraw => Effect::None,
        }
    }

    fn workspace_effect(&mut self) -> Effect {
        if self.workspace != Workspace::Findings {
            self.claims.close();
        }
        self.invalidate_reads();
        if self.workspace == Workspace::Monitors {
            Effect::MonitorList
        } else {
            Effect::None
        }
    }

    fn cycle_focus(&mut self, forward: bool) {
        self.focus = self.focus.cycle(forward);
        if self.focus == Focus::Search
            && matches!(
                self.workspace,
                Workspace::Monitors
                    | Workspace::Recordings
                    | Workspace::Findings
                    | Workspace::System
            )
        {
            self.focus = self.focus.cycle(forward);
        }
    }

    pub fn open_finding_recording(&mut self, record: RecordingLine) {
        self.claims.close();
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
            Key::Up => self.move_recording(false),
            Key::Down => self.move_recording(true),
            Key::Char(',') => self.shift_interval(false),
            Key::Char('.') => self.shift_interval(true),
            Key::Char('f' | 'v' | '/') => self
                .note_message("Recording selection is read-only. r reloads the metadata snapshot."),
            _ => return false,
        }
        true
    }

    fn move_recording(&mut self, forward: bool) {
        if forward {
            self.recording_selection = self
                .recording_selection
                .saturating_add(1)
                .min(self.recordings.len().saturating_sub(1));
        } else {
            self.recording_selection = self.recording_selection.saturating_sub(1);
        }
        self.interval_index = 0;
    }

    fn shift_interval(&mut self, forward: bool) {
        let facts = self
            .selected_recording()
            .map(|recording| (recording.seal.truncated, recording.seal.intervals.len()));
        let Some((truncated, len)) = facts else {
            self.note_message(
                "No selected recording. No interval was chosen. Capture keeps running.",
            );
            return;
        };
        if truncated {
            self.note_message(super::interval::TRUNCATED);
            return;
        }
        if len == 0 {
            self.note_message(super::interval::MISSING);
            return;
        }
        if forward {
            self.interval_index = self.interval_index.saturating_add(1).min(len - 1);
        } else {
            self.interval_index = self.interval_index.saturating_sub(1);
        }
        let caption = self
            .selected_recording()
            .map(|recording| super::interval::caption(&recording.seal, self.interval_index));
        if let Some(caption) = caption {
            self.note_message(&caption);
        }
    }

    #[must_use]
    pub(super) fn interval_index(&self) -> usize {
        self.interval_index
    }

    fn monitor_key(&mut self, key: &Key) -> Option<Effect> {
        match key {
            Key::Up | Key::Down => self.monitors.move_selection(*key == Key::Down),
            Key::Escape => {
                self.monitors.back();
                self.invalidate_reads();
            }
            Key::Enter => return Some(self.monitors.open()),
            Key::Char('r') => return Some(self.monitors.reload()),
            Key::Char('f' | 'v' | '/') => self.note_message(
                "Monitor navigation is read-only. Enter inspects coverage and passages.",
            ),
            _ => return None,
        }
        Some(Effect::None)
    }

    fn edit_search(&mut self, key: Key) -> Effect {
        match self.search.handle(key) {
            Edit::End => {
                self.focus = Focus::Results;
                self.status =
                    "Draft discarded. Displayed results keep their applied filters.".into();
                Effect::None
            }
            Edit::Back => {
                self.focus = Focus::Workspaces;
                Effect::None
            }
            Edit::Apply => self.submit_search(PageMove::First),
            Edit::Continue => Effect::None,
            Edit::Quit => Effect::Detach,
        }
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
        self.search.toggle_favorites();
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
        if self.restart_required {
            self.note_cursor_changed();
            return Effect::None;
        }
        if forward {
            if self.page_history.len() >= 625 {
                self.note_message("Page history is full. g opens the first cached page.");
                return Effect::None;
            }
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
        self.pending_random = None;
        if matches!(self.link, Link::Disconnected) {
            self.status = "Disconnected; the search was not submitted. r reconnects.".into();
            return Effect::None;
        }
        let scope = if movement == PageMove::First {
            match self.search.normalized_draft() {
                Ok(scope) => scope,
                Err(issue) => {
                    self.search.validation = Some(issue);
                    self.status = issue.message().into();
                    return Effect::None;
                }
            }
        } else {
            self.search.applied.clone()
        };
        let after = match &movement {
            PageMove::First => None,
            PageMove::Next(after) => Some(after.clone()),
            PageMove::Previous => self.page_history.last().cloned().flatten(),
        };
        self.search_generation = self.search_generation.saturating_add(1);
        let generation = self.search_generation;
        self.pending_search = Some(generation);
        self.pending_page = Some(movement);
        self.search.request(scope.clone());
        self.status = "Reading cached station matches. No directory refresh was requested.".into();
        Effect::Search(SearchQuery {
            generation,
            filter: scope.filter,
            favorites_only: scope.favorites_only,
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

    /// Puts the explorer on one workspace and focus without pressing a key.
    #[cfg(test)]
    pub(crate) fn place_for_actions(&mut self, workspace: Workspace, focus: Focus) {
        self.workspace = workspace;
        self.focus = focus;
        self.keys = KeyList::Closed;
        self.context = None;
        self.fetch_confirmation = None;
        self.play_confirmation = None;
        self.protect_confirmation = None;
        self.claims.close();
        self.draw = Draw::Needed;
    }

    /// Adds two copies of the first station and recording so Up and Down both move.
    #[cfg(test)]
    pub(crate) fn triplicate_lists_for_test(&mut self) {
        if let Some(first) = self.rows.first().cloned() {
            let mut second = first.clone();
            second.id = format!("{}-b", first.id);
            let mut third = first.clone();
            third.id = format!("{}-c", first.id);
            self.rows.push(second);
            self.rows.push(third);
            self.selection = 1;
        }
        if let Some(first) = self.recordings.first().cloned() {
            let mut second = first.clone();
            second.id = format!("{}-b", first.id);
            let mut third = first.clone();
            third.id = format!("{}-c", first.id);
            self.recordings.push(second);
            self.recordings.push(third);
            self.recording_selection = 1;
        }
    }
}

fn leaves_claim_page(key: &Key) -> bool {
    matches!(key, Key::Left | Key::Right | Key::Char('1'..='7'))
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
            emphasis: crate::explorer::state::Emphasis::Standard,
        }
    }

    pub(super) fn row(id: &str, name: &str) -> StationRow {
        StationRow {
            id: id.into(),
            name: name.into(),
            favorite: false,
            directory_health: Health::Succeeded,
            directory_languages: "navajo".into(),
            observed_ms: 1_000,
            hls: false,
            coordinates: None,
            metadata: None,
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
            catalog: None,
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

    pub(super) fn loaded() -> Explorer {
        let mut model = Explorer::new(modes(), 10_000);
        model.apply_desk(desk(
            vec![row("station-a", "Alpha"), row("station-b", "Beta")],
            vec![RecordingLine {
                id: "rec-1".into(),
                state: "running".into(),
                storage_state: "reserved".into(),
                format: None,
                timeline: super::super::timeline::Timeline::default(),
                seal: super::SealMap::default(),
                hold_total: 0,
                shown_holds: Vec::new(),
            }],
        ));
        model
    }

    #[test]
    fn quick_play_requires_confirmation_and_keeps_the_selected_identity() {
        let mut model = loaded();
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert!(model.status().contains("y confirms"));
        assert_eq!(model.handle(Key::Down), Effect::None);
        assert_eq!(
            model.handle(Key::Char('y')),
            Effect::Play {
                station: "station-a".into()
            }
        );
        assert_eq!(
            model.selected().map(|row| row.id.as_str()),
            Some("station-a")
        );
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert_eq!(model.handle(Key::Escape), Effect::None);
        assert_eq!(model.handle(Key::Char('y')), Effect::None);
        assert_eq!(model.handle(Key::Char('S')), Effect::StopListen);
        model.handle(Key::Char('2'));
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert_eq!(
            model.handle(Key::Char('y')),
            Effect::Play {
                station: "station-a".into()
            }
        );
        assert_eq!(model.handle(Key::Char('/')), Effect::None);
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert_eq!(model.handle(Key::Char('S')), Effect::None);
        assert_eq!(model.query(), "PS");
    }

    #[test]
    fn focused_play_confirmation_and_receipt_observation_preserve_navigation() {
        let mut model = loaded();
        model.handle(Key::Enter);
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert_eq!(model.handle(Key::Char('n')), Effect::None);
        assert!(model.context.is_some());
        model.observe_playback(PlaybackView {
            id: "listen-real".into(),
            state: "running".into(),
            format: Some("mp3".into()),
            source_revision: "station:v1".into(),
            failure: None,
        });
        assert_eq!(
            model.playback().map(|view| view.id.as_str()),
            Some("listen-real")
        );
        assert!(model.context.is_some());
        assert_eq!(model.handle(Key::Escape), Effect::None);
        assert_eq!(
            model.selected().map(|row| row.id.as_str()),
            Some("station-a")
        );
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
        assert_eq!(model.workspace(), Workspace::Globe);
        assert_eq!(model.focus(), Focus::Results);
        model.handle(Key::Tab);
        assert_eq!(model.focus(), Focus::Detail);
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

    fn kept_recording(id: &str, state: &str, storage: &str) -> RecordingLine {
        RecordingLine {
            id: id.into(),
            state: state.into(),
            storage_state: storage.into(),
            format: None,
            timeline: super::super::timeline::Timeline::default(),
            seal: super::SealMap::default(),
            hold_total: 0,
            shown_holds: Vec::new(),
        }
    }

    fn finding_page(start: Option<u64>, end: Option<u64>) -> sigy_service::monitor::FindingPage {
        sigy_service::monitor::FindingPage {
            monitor_id: "world-news".into(),
            id: "one".into(),
            transcript_id: "original".into(),
            transcript_revision: 1,
            translation_revision: 1,
            cue_ordinal: 0,
            original: sigy_service::monitor::FindingOriginal::Retained,
            recording_id: "recording-one".into(),
            start_us: start,
            end_us: end,
            original_script: "hello".into(),
            english: None,
            untranslated_reason: Some("not requested".into()),
            stale_transcript: None,
            stale_translation: None,
        }
    }

    #[test]
    fn retained_recording_play_confirms_one_request_and_leaves_navigation() {
        let mut model = loaded();
        model
            .recordings
            .push(kept_recording("rec-kept", "completed", "retained"));
        assert_eq!(model.handle(Key::Char('3')), Effect::None);
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert!(model.status().contains("not capturing"));
        assert_eq!(model.handle(Key::Char('y')), Effect::None);
        assert_eq!(model.handle(Key::Down), Effect::None);
        assert_eq!(
            model.selected_recording().map(|record| record.id.as_str()),
            Some("rec-kept")
        );
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert!(model.status().contains("y confirms"));
        for key in [
            Key::Down,
            Key::Char('1'),
            Key::Enter,
            Key::Char('o'),
            Key::Char('r'),
            Key::Left,
            Key::Right,
        ] {
            assert_eq!(model.handle(key), Effect::None);
        }
        assert_eq!(model.workspace(), Workspace::Recordings);
        assert_eq!(
            model.selected_recording().map(|record| record.id.as_str()),
            Some("rec-kept")
        );
        assert_eq!(model.handle(Key::Char('n')), Effect::None);
        assert!(model.status().contains("discarded"));
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        let effect = model.handle(Key::Char('y'));
        let Effect::PlayRecording { id, request } = effect else {
            panic!("expected retained recording play, got {effect:?}");
        };
        assert_eq!(id, "rec-kept");
        assert!(request.starts_with("reader-"));
        assert_eq!(request.len(), 39);
        assert_eq!(model.handle(Key::Char('S')), Effect::StopRetained);
        assert_eq!(model.handle(Key::Enter), Effect::None);
        assert_eq!(model.handle(Key::Up), Effect::None);
    }

    #[test]
    fn cited_finding_play_confirms_and_a_missing_interval_does_not() {
        let mut model = loaded();
        assert!(
            model
                .findings
                .load("world-news", "one", &finding_page(Some(100), Some(900)))
        );
        assert_eq!(model.handle(Key::Char('5')), Effect::None);
        assert!(!matches!(
            model.handle(Key::Enter),
            Effect::PlayFinding { .. }
        ));
        assert!(matches!(
            model.handle(Key::Char('o')),
            Effect::FindingOriginal { .. }
        ));
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert!(model.status().contains("Enter still only inspects"));
        let effect = model.handle(Key::Char('y'));
        let Effect::PlayFinding {
            monitor,
            finding,
            request,
            recording,
            start_us,
            end_us,
        } = effect
        else {
            panic!("expected cited finding play, got {effect:?}");
        };
        assert_eq!(monitor, "world-news");
        assert_eq!(finding, "one");
        assert_eq!(recording, "recording-one");
        assert_eq!(start_us, 100);
        assert_eq!(end_us, 900);
        assert!(request.starts_with("reader-"));
        assert_eq!(request.len(), 39);
        assert_eq!(model.handle(Key::Char('S')), Effect::StopRetained);

        let mut missing = loaded();
        assert!(
            missing
                .findings
                .load("world-news", "one", &finding_page(None, Some(900)))
        );
        assert_eq!(missing.handle(Key::Char('5')), Effect::None);
        assert_eq!(missing.handle(Key::Char('/')), Effect::None);
        assert_eq!(missing.handle(Key::Char('P')), Effect::None);
        assert_eq!(missing.findings.query(), "P");
        missing.handle(Key::Escape);
        assert_eq!(missing.handle(Key::Char('P')), Effect::None);
        assert!(missing.status().contains("no cited interval"));
        assert_eq!(missing.handle(Key::Char('y')), Effect::None);
    }

    #[test]
    fn desk_reload_keeps_the_explicit_retained_request() {
        let mut model = loaded();
        model.begin_retained("reader-kept");
        model.apply_desk(desk(
            vec![row("station-a", "Alpha")],
            vec![kept_recording("rec-kept", "completed", "retained")],
        ));
        assert_eq!(model.retained_label(), Some(("pending", "reader-kept")));
        model.set_retained_phase("reader-other", "closed", None);
        assert_eq!(model.retained_label(), Some(("pending", "reader-kept")));
        model.set_retained_phase("reader-kept", "decoding", Some("running"));
        assert_eq!(model.retained_label(), Some(("decoding", "reader-kept")));
        model.set_retained_phase("reader-kept", "decoding", Some("running"));
        assert_eq!(model.retained_label(), Some(("decoding", "reader-kept")));
        assert!(model.decoder_output_us().is_none());
        assert!(!model.set_decoder_output("reader-other", 1_000_000));
        assert!(model.decoder_output_us().is_none());
        assert!(model.set_decoder_output("reader-kept", 1_000_000));
        assert_eq!(model.decoder_output_us(), Some(1_000_000));
        assert!(!model.set_decoder_output("reader-kept", 1_000_000));
        model.set_retained_phase("reader-kept", "closed", Some("completed"));
        assert_eq!(model.decoder_output_us(), Some(1_000_000));
    }

    #[test]
    fn heard_pause_key_does_not_touch_capture_and_lowercase_h_stays_on_the_globe() {
        let mut model = loaded();
        let captures = model.captures_active();
        let recordings = model.recordings().len();
        assert_eq!(model.handle(Key::Char('H')), Effect::HoldRetained);
        assert_eq!(model.captures_active(), captures);
        assert_eq!(model.recordings().len(), recordings);
        assert!(model.playback().is_none());
        assert_eq!(model.handle(Key::Char('/')), Effect::None);
        assert_eq!(model.handle(Key::Char('H')), Effect::None);
        assert_eq!(model.query(), "H");
        assert_eq!(model.handle(Key::Escape), Effect::None);
        model.handle(Key::Char('7'));
        assert_eq!(model.handle(Key::Char('h')), Effect::None);
        assert_eq!(model.handle(Key::Char('H')), Effect::HoldRetained);
        assert_eq!(model.captures_active(), captures);
        model
            .recordings
            .push(kept_recording("rec-kept", "completed", "retained"));
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Down), Effect::None);
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert!(model.status().contains("y confirms"));
        assert_eq!(model.handle(Key::Char('H')), Effect::None);
        assert!(model.playback().is_none());
        assert_eq!(model.captures_active(), captures);
    }

    #[test]
    fn heard_seek_keys_do_not_touch_capture_or_confirm_playback() {
        let mut model = loaded();
        let captures = model.captures_active();
        let recordings = model.recordings().len();
        assert_eq!(
            model.handle(Key::Char('[')),
            Effect::SeekRetained { forward: false }
        );
        assert_eq!(
            model.handle(Key::Char(']')),
            Effect::SeekRetained { forward: true }
        );
        assert_eq!(model.captures_active(), captures);
        assert_eq!(model.recordings().len(), recordings);
        assert!(model.playback().is_none());
        assert_eq!(model.handle(Key::Char('/')), Effect::None);
        assert_eq!(model.handle(Key::Char('[')), Effect::None);
        assert_eq!(model.query(), "[");
        assert_eq!(model.handle(Key::Escape), Effect::None);
        model
            .recordings
            .push(kept_recording("rec-kept", "completed", "retained"));
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Down), Effect::None);
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert!(model.status().contains("y confirms"));
        assert_eq!(model.handle(Key::Char(']')), Effect::None);
        assert!(model.playback().is_none());
        assert_eq!(model.captures_active(), captures);
    }

    #[test]
    fn paused_row_keeps_its_time_when_a_later_receipt_arrives() {
        let mut model = loaded();
        model.begin_retained("reader-kept");
        assert!(model.set_decoder_output("reader-kept", 1_500_000));
        assert!(model.show_paused("reader-kept", 1_500_000));
        assert!(!model.show_paused("reader-kept", 1_500_000));
        model.set_retained_phase("reader-kept", "closed", Some("completed"));
        assert_eq!(model.retained_label(), Some(("paused", "reader-kept")));
        assert_eq!(model.paused_at_us(), Some(1_500_000));
        assert_eq!(model.decoder_output_us(), Some(1_500_000));
        assert!(!model.show_paused("reader-other", 1));
        assert_eq!(model.retained_label(), Some(("paused", "reader-kept")));
        model.release_pause();
        assert_eq!(model.retained_label(), Some(("closed", "reader-kept")));
        assert_eq!(model.paused_at_us(), None);
        model.set_retained_phase("reader-kept", "closed", Some("completed"));
        assert_eq!(model.retained_label(), Some(("closed", "reader-kept")));
    }

    #[test]
    fn read_only_workspace_tab_navigation_never_enters_station_search() {
        for workspace in ['3', '4', '5', '6'] {
            for key in [Key::Tab, Key::BackTab] {
                let mut model = loaded();
                model.handle(Key::Char(workspace));
                let before = model.current_search();
                for _ in 0..12 {
                    assert_eq!(model.handle(key.clone()), Effect::None);
                    assert_ne!(model.focus(), Focus::Search);
                    assert_eq!(
                        model.handle(Key::Paste("unrelated station".into())),
                        Effect::None
                    );
                }
                assert!(!matches!(model.handle(Key::Enter), Effect::Search(_)));
                assert_eq!(model.current_search(), before);
            }
        }
        let mut model = loaded();
        for _ in 0..5 {
            model.handle(Key::Tab);
        }
        assert_eq!(model.focus(), Focus::Search);
        model.handle(Key::Paste("Alpha".into()));
        assert!(matches!(model.handle(Key::Enter), Effect::Search(_)));
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
    fn findings_claim_page_is_read_only_and_closes_when_the_workspace_changes() {
        let mut model = loaded();
        assert_eq!(model.handle(Key::Char('5')), Effect::None);
        assert_eq!(model.handle(Key::Char('c')), Effect::ClaimList);
        assert!(model.claims.open());
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        assert_eq!(model.handle(Key::Char('S')), Effect::None);
        assert!(model.playback().is_none());
        assert!(model.claims.open());
        assert_eq!(model.handle(Key::Char('H')), Effect::HoldRetained);
        assert!(model.claims.open());
        assert_eq!(model.handle(Key::Char('3')), Effect::None);
        assert_eq!(model.workspace(), Workspace::Recordings);
        assert!(!model.claims.open());
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
                emphasis: crate::explorer::state::Emphasis::Standard,
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

    fn sealed_line(
        storage: &str,
        intervals: Vec<sigy_service::storage::dvr::RecordingInterval>,
        open: Option<&str>,
    ) -> RecordingLine {
        RecordingLine {
            id: "rec-sealed".into(),
            state: "completed".into(),
            storage_state: storage.into(),
            format: Some("wav".into()),
            timeline: super::super::timeline::Timeline::default(),
            seal: super::SealMap {
                state: "completed".into(),
                open_ceiling: u64::from(open.is_some()),
                open_object_key: open.map(|key| key.repeat(32)),
                intervals,
                gaps: Vec::new(),
                truncated: false,
            },
            hold_total: 0,
            shown_holds: Vec::new(),
        }
    }

    fn one_interval(
        start: u64,
        end: u64,
        key: &str,
    ) -> sigy_service::storage::dvr::RecordingInterval {
        sigy_service::storage::dvr::RecordingInterval {
            ordinal: 0,
            decoded_start_us: start,
            decoded_end_us: end,
            byte_start: 0,
            byte_end: 100,
            object_key: key.repeat(32),
            sha256: "ab".repeat(32),
            format: "wav".into(),
            ceiling_bytes: 100,
            released: false,
        }
    }

    #[test]
    fn a_confirmed_sealed_interval_requests_one_bounded_listen() {
        let mut model = loaded();
        model.recordings[0] = sealed_line(
            "retained",
            vec![
                one_interval(0, 5_000_000, "a"),
                one_interval(5_000_000, 9_000_000, "b"),
            ],
            None,
        );
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('.')), Effect::None);
        assert!(model.status().contains("[5000000, 9000000)"));
        assert_eq!(model.handle(Key::Char('I')), Effect::None);
        assert!(model.status().contains("y confirms"));
        let effect = model.handle(Key::Char('y'));
        let Effect::PlayInterval {
            id,
            start_us,
            end_us,
            request,
        } = effect
        else {
            panic!("expected one interval listen");
        };
        assert_eq!(id, "rec-sealed");
        assert_eq!(start_us, 5_000_000);
        assert_eq!(end_us, 9_000_000);
        assert!(request.starts_with("reader-"));
        assert!(model.playback().is_none());
    }

    #[test]
    fn an_open_tail_and_a_whole_recording_play_stay_distinct() {
        let mut model = loaded();
        model.recordings[0] =
            sealed_line("retained", vec![one_interval(0, 5_000_000, "a")], Some("a"));
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('I')), Effect::None);
        assert!(model.status().contains("open tail"));
        assert_eq!(model.handle(Key::Char('y')), Effect::None);
        model.recordings[0].seal.open_object_key = None;
        model.recordings[0].state = "completed".into();
        assert_eq!(model.handle(Key::Char('P')), Effect::None);
        let effect = model.handle(Key::Char('y'));
        assert!(matches!(effect, Effect::PlayRecording { .. }));
    }

    #[test]
    fn a_confirmed_sealed_interval_records_one_hold_and_starts_nothing() {
        let mut model = loaded();
        model.recordings[0] = sealed_line(
            "reserved",
            vec![
                one_interval(0, 5_000_000, "a"),
                one_interval(5_000_000, 9_000_000, "b"),
            ],
            Some("c"),
        );
        model.recordings[0].state = "running".into();
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('.')), Effect::None);
        assert_eq!(model.interval_index(), 1);
        assert_eq!(model.handle(Key::Char('H')), Effect::HoldRetained);
        assert_eq!(model.handle(Key::Char('K')), Effect::None);
        assert!(
            model
                .status()
                .contains("Hold sealed interval [5000000, 9000000)")
        );
        assert_eq!(model.handle(Key::Char('H')), Effect::None);
        assert_eq!(model.handle(Key::Char('.')), Effect::None);
        assert_eq!(model.interval_index(), 1);
        assert_eq!(model.handle(Key::Char('n')), Effect::None);
        assert!(model.status().contains("No retention change"));
        assert_eq!(model.handle(Key::Char('H')), Effect::HoldRetained);
        assert_eq!(model.handle(Key::Char('K')), Effect::None);
        let effect = model.handle(Key::Char('y'));
        let Effect::ProtectInterval {
            generation,
            id,
            start_us,
            end_us,
        } = effect
        else {
            panic!("expected one interval hold");
        };
        assert_eq!(generation, 1);
        assert_eq!(id, "rec-sealed");
        assert_eq!(start_us, 5_000_000);
        assert_eq!(end_us, 9_000_000);
        assert!(model.playback().is_none());
        assert!(model.retained_label().is_none());
        assert_eq!(model.handle(Key::Char('K')), Effect::None);
        assert!(model.status().contains("still settling"));
        assert!(!model.take_protect(2));
        assert!(model.take_protect(1));
        let mut line = model.recordings[0].clone();
        line.hold_total = 1;
        line.shown_holds = vec![super::HoldSpan {
            ordinal: 0,
            start_us: 5_000_000,
            end_us: 9_000_000,
        }];
        assert!(model.install_protected_recording(line));
        assert_eq!(model.interval_index(), 1);
        assert_eq!(
            model
                .selected_recording()
                .and_then(super::RecordingLine::hold_caption)
                .as_deref(),
            Some("Hold 0: [5000000, 9000000). 1 hold on this recording.")
        );
    }

    #[test]
    fn an_open_tail_a_gap_and_a_disconnect_record_no_hold() {
        let mut model = loaded();
        model.recordings[0] =
            sealed_line("retained", vec![one_interval(0, 5_000_000, "a")], Some("a"));
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('K')), Effect::None);
        assert!(model.status().contains("open tail"));
        assert_eq!(model.handle(Key::Char('y')), Effect::None);
        model.recordings[0].seal.open_object_key = None;
        model.recordings[0]
            .seal
            .gaps
            .push(sigy_service::storage::dvr::RecordingGap {
                ordinal: 0,
                cause: sigy_service::storage::dvr::GapCause::Disconnect,
                start_us: 1,
                end_us: 2,
            });
        assert_eq!(model.handle(Key::Char('K')), Effect::None);
        assert!(model.status().contains("overlaps a gap"));
        model.recordings[0].seal.gaps.clear();
        model.note_disconnect(20_000, "fixture");
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('K')), Effect::None);
        assert!(model.status().contains("not submitted"));
    }

    #[test]
    fn shown_holds_keep_the_newest_eight_and_sentences_fit() {
        let holds: Vec<sigy_service::storage::dvr::RecordingHold> = (0..9)
            .map(|ordinal| sigy_service::storage::dvr::RecordingHold {
                ordinal,
                start_us: u64::from(ordinal),
                end_us: u64::from(ordinal) + 1,
                segments: vec![ordinal],
                gaps: Vec::new(),
            })
            .collect();
        let (total, shown) = super::shown_holds(&holds);
        assert_eq!(total, 9);
        assert_eq!(shown.len(), 8);
        assert_eq!(shown[0].ordinal, 1);
        assert_eq!(shown[7].ordinal, 8);
        let confirmation = format!(
            "Hold sealed interval [{}, {})? y confirms; n/Esc cancels. Capture keeps running.",
            u64::MAX,
            u64::MAX
        );
        let recorded = format!(
            "Hold {ordinal} recorded on [{start}, {end}). Capture keeps running. Another confirmation records another hold.",
            ordinal = u32::MAX,
            start = u64::MAX,
            end = u64::MAX
        );
        assert!(confirmation.len() <= 160, "{}", confirmation.len());
        assert!(recorded.len() <= 160, "{}", recorded.len());
        assert!(super::PROTECT_BUSY.len() <= 160);
        assert!(super::PROTECT_UNRESOLVED.len() <= 160);
        let latest = format!(
            "Play latest sealed interval [{}, {})? y confirms; n/Esc cancels. Capture keeps running.",
            u64::MAX,
            u64::MAX
        );
        assert!(latest.len() <= 160, "{}", latest.len());
    }

    #[test]
    fn l_then_y_plays_the_latest_sealed_interval_and_stores_no_end() {
        let mut model = loaded();
        model.recordings[0] = sealed_line(
            "reserved",
            vec![
                one_interval(0, 5_000_000, "a"),
                one_interval(5_000_000, 9_000_000, "b"),
                one_interval(9_000_000, 12_000_000, "c"),
            ],
            Some("c"),
        );
        model.recordings[0].state = "running".into();
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('l')), Effect::None);
        assert_eq!(model.handle(Key::Char('L')), Effect::None);
        assert!(
            model
                .status()
                .contains("Play latest sealed interval [5000000, 9000000)")
        );
        assert_eq!(model.handle(Key::Char('H')), Effect::None);
        assert_eq!(model.handle(Key::Char('.')), Effect::None);
        let effect = model.handle(Key::Char('y'));
        let Effect::PlayLatest {
            id,
            start_us,
            request,
        } = effect
        else {
            panic!("expected the latest sealed interval");
        };
        assert_eq!(id, "rec-sealed");
        assert_eq!(start_us, 5_000_000);
        assert!(request.starts_with("reader-"));
        assert!(model.playback().is_none());
        assert!(model.retained_label().is_none());
    }

    #[test]
    fn an_open_tail_and_an_unstored_recording_do_not_start_the_latest_interval() {
        let mut model = loaded();
        model.recordings[0] =
            sealed_line("retained", vec![one_interval(0, 5_000_000, "a")], Some("a"));
        model.handle(Key::Char('3'));
        assert_eq!(model.handle(Key::Char('L')), Effect::None);
        assert!(model.status().contains("open tail"));
        assert_eq!(model.handle(Key::Char('y')), Effect::None);
        model.recordings[0].storage_state = "deleting".into();
        model.recordings[0].seal.open_object_key = None;
        assert_eq!(model.handle(Key::Char('L')), Effect::None);
        assert!(model.status().contains("not reserved or retained"));
        model.recordings[0].storage_state = "retained".into();
        model.recordings[0].seal.truncated = true;
        assert_eq!(model.handle(Key::Char('L')), Effect::None);
        assert!(model.status().contains("partial"));
    }
}
