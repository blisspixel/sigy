//! One action table per workspace, and one for Home.
//!
//! The footer and the `?` overlay read this table. Dispatch match arms stay in
//! `state`. Home is the parent of every workspace. This module does not change
//! a service.

use super::state::{Explorer, Workspace};

#[cfg(test)]
use super::state::Key;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScreenId {
    Home,
    Explore,
    Live,
    Recordings,
    Monitors,
    Findings,
    System,
    Globe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Move,
    Find,
    Listen,
    Keep,
    View,
}

impl Group {
    const ALL: [Self; 5] = [Self::Move, Self::Find, Self::Listen, Self::Keep, Self::View];

    const fn label(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::Find => "Find",
            Self::Listen => "Listen",
            Self::Keep => "Keep",
            Self::View => "View",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Show {
    Always,
    /// Footer pair only after the catalog identity changes.
    Restart,
    /// Footer pair only when slow turning is available.
    Spin,
    /// Footer pair only while a retained listen is active.
    Retained,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cli {
    None,
    Fixed(&'static str),
    Fetch,
}

/// One selection-menu row. Empty means the action stays off that menu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    None,
    Play,
    Fetch,
    Inspect,
    Favorite,
    Linked,
    Center,
    Latest,
    Hold,
    Interval,
    Metadata,
    Copy,
}

impl Slot {
    const fn label(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Play => "Play",
            Self::Fetch => "Fetch",
            Self::Inspect => "Inspect",
            Self::Favorite => "Favorite",
            Self::Linked => "Linked history",
            Self::Center => "Center on globe",
            Self::Latest => "Latest",
            Self::Hold => "Hold",
            Self::Interval => "Interval",
            Self::Metadata => "Metadata",
            Self::Copy => "Copy CLI command",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Char(char),
    Tab,
    BackTab,
    Enter,
    Escape,
    Backspace,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy)]
struct Action {
    keys: &'static [Act],
    pair: &'static str,
    group: Group,
    line: &'static str,
    show: Show,
    cli: Cli,
    reason: &'static str,
    slot: Slot,
}

const G: &[Act] = &[Act::Char('g')];
const SLASH: &[Act] = &[Act::Char('/')];
const FILTERS: &[Act] = &[Act::Char('F')];
const COUNTRY: &[Act] = &[Act::Char('C')];
const EXPLORE_KEY: &[Act] = &[Act::Char('e')];
const FETCH: &[Act] = &[Act::Char('R')];
const FAVORITE: &[Act] = &[Act::Char('f')];
const PAGE: &[Act] = &[Act::Char('n'), Act::Char('p')];
const QUIT: &[Act] = &[Act::Char('q')];
const PLAY: &[Act] = &[Act::Char('P')];
const STOP: &[Act] = &[Act::Char('S')];
const RELOAD: &[Act] = &[Act::Char('r')];
const FAVORITES: &[Act] = &[Act::Char('v')];
const CLEAR: &[Act] = &[Act::Char('x')];
const ENTER: &[Act] = &[Act::Enter];
const TAB: &[Act] = &[Act::Tab];
const SPIN: &[Act] = &[Act::Char('s')];
const MAP: &[Act] = &[Act::Char('m')];
const TURN: &[Act] = &[
    Act::Char('h'),
    Act::Char('l'),
    Act::Char('j'),
    Act::Char('k'),
];
const CENTER: &[Act] = &[Act::Char('c')];
const ARROWS: &[Act] = &[Act::Left, Act::Right, Act::Up, Act::Down];
const CLAIMS: &[Act] = &[Act::Char('c')];
const ORIGINAL: &[Act] = &[Act::Char('o')];
const HOLD: &[Act] = &[Act::Char('H')];
const SEEK: &[Act] = &[Act::Char('['), Act::Char(']')];
const INTERVAL: &[Act] = &[Act::Char('I')];
const PROTECT: &[Act] = &[Act::Char('K')];
const LATEST: &[Act] = &[Act::Char('L')];
const STEP: &[Act] = &[Act::Char(','), Act::Char('.')];
const BACK: &[Act] = &[Act::Escape, Act::Backspace];
const HOME_KEY: &[Act] = &[Act::Char('0')];
const SELECT: &[Act] = &[Act::Up, Act::Down];
const DIGITS: &[Act] = &[
    Act::Char('1'),
    Act::Char('2'),
    Act::Char('3'),
    Act::Char('4'),
    Act::Char('5'),
    Act::Char('6'),
    Act::Char('7'),
];
const ACROSS: &[Act] = &[Act::Left, Act::Right];
const BACK_TAB: &[Act] = &[Act::BackTab];
const QUESTION: &[Act] = &[Act::Char('?')];
const LINKED: &[Act] = &[Act::Char('i')];
const SPACE: &[Act] = &[Act::Char(' ')];
const COPY_KEYS: &[Act] = &[];

const PLAY_CLI: &str = "sigy play STATION_UUID";
const STOP_CLI: &str = "sigy stop";
const FILE_CLI: &str = "sigy listen file RECORDING";
const HOLD_CLI: &str = "sigy record hold RECORDING";
const SPIN_OFF: &str = "Globe turning is off in this presentation mode.";

/// Parent screen for one workspace. Home has no parent.
#[cfg(test)]
#[must_use]
pub(crate) const fn screen_for(workspace: Workspace) -> ScreenId {
    match workspace {
        Workspace::Explore => ScreenId::Explore,
        Workspace::Live => ScreenId::Live,
        Workspace::Recordings => ScreenId::Recordings,
        Workspace::Monitors => ScreenId::Monitors,
        Workspace::Findings => ScreenId::Findings,
        Workspace::System => ScreenId::System,
        Workspace::Globe => ScreenId::Globe,
    }
}

/// `None` for Home. Every current workspace names Home.
#[cfg(test)]
#[must_use]
pub(crate) const fn parent(screen: ScreenId) -> Option<ScreenId> {
    match screen {
        ScreenId::Home => None,
        ScreenId::Explore
        | ScreenId::Live
        | ScreenId::Recordings
        | ScreenId::Monitors
        | ScreenId::Findings
        | ScreenId::System
        | ScreenId::Globe => Some(ScreenId::Home),
    }
}

/// Footer pairs in priority order. The hint fitter keeps the prefix that fits.
#[must_use]
pub(crate) fn footer_pairs(model: &Explorer) -> Vec<&'static str> {
    let actions = if model.at_home() {
        home()
    } else {
        workspace_actions(model.workspace())
    };
    let mut pairs = visible_pairs(&actions, model);
    // Keep the workspace's first pair at 20 columns, and show Back as soon as a second pair fits.
    if let Some(index) = pairs.iter().position(|pair| *pair == "Esc back") {
        let pair = pairs.remove(index);
        let at = usize::from(!pairs.is_empty());
        pairs.insert(at, pair);
    }
    pairs
}

fn visible_pairs(actions: &[Action], model: &Explorer) -> Vec<&'static str> {
    actions
        .iter()
        .filter(|action| !action.pair.is_empty() && shown(model, action))
        .map(|action| action.pair)
        .collect()
}

/// Grouped overlay text, including a disabled reason and a copyable command.
#[must_use]
pub(crate) fn overlay_lines(model: &Explorer) -> Vec<String> {
    let title = if model.at_home() {
        "Home"
    } else {
        model.workspace().task()
    };
    let mut lines = vec![format!("Keys: {title}"), String::new()];
    let actions = if model.at_home() {
        home()
    } else {
        workspace_actions(model.workspace())
    };
    for group in Group::ALL {
        let mut started = false;
        for action in &actions {
            if action.group != group || action.line.is_empty() {
                continue;
            }
            if action.show == Show::Retained && !shown(model, action) {
                continue;
            }
            if !started {
                lines.push(group.label().to_owned());
                started = true;
            }
            lines.push(render_line(model, action));
        }
        if started {
            lines.push(String::new());
        }
    }
    lines.push("Close".to_owned());
    lines.push("  ? or Esc closes this list    q quits this client".to_owned());
    lines
}

/// Whether this workspace table lists the key.
#[cfg(test)]
#[must_use]
pub(crate) fn covers(workspace: Workspace, key: &Key) -> bool {
    let Some(act) = act_of(key) else {
        return false;
    };
    workspace_actions(workspace)
        .iter()
        .any(|action| action.keys.contains(&act))
}

/// Every key the workspace table lists, in table order.
#[cfg(test)]
#[must_use]
pub(crate) fn keys_for(workspace: Workspace) -> Vec<Key> {
    listed_keys(&workspace_actions(workspace))
}

/// Whether the Home table lists the key.
#[cfg(test)]
#[must_use]
pub(crate) fn covers_home(key: &Key) -> bool {
    let Some(act) = act_of(key) else {
        return false;
    };
    home().iter().any(|action| action.keys.contains(&act))
}

/// Every key the Home table lists, in table order.
#[cfg(test)]
#[must_use]
pub(crate) fn home_keys() -> Vec<Key> {
    listed_keys(&home())
}

#[cfg(test)]
fn listed_keys(actions: &[Action]) -> Vec<Key> {
    let mut keys = Vec::new();
    for action in actions {
        for act in action.keys {
            keys.push(key_of(*act));
        }
    }
    keys
}

fn shown(model: &Explorer, action: &Action) -> bool {
    match action.show {
        Show::Always => true,
        Show::Restart => model.restart_required(),
        Show::Spin => model.spin_label().is_some(),
        Show::Retained => model.retained_active(),
    }
}

fn render_line(model: &Explorer, action: &Action) -> String {
    let mut line = action.line.to_owned();
    if !shown(model, action) && !action.reason.is_empty() {
        line.push_str("  ");
        line.push_str(action.reason);
    }
    if let Some(command) = cli_text(model, action.cli) {
        line.push_str("    ");
        line.push_str(&command);
    }
    line
}

fn cli_text(model: &Explorer, cli: Cli) -> Option<String> {
    match cli {
        Cli::None => None,
        Cli::Fixed(text) => Some(text.to_owned()),
        Cli::Fetch => Some(model.discovery_command()),
    }
}

fn workspace_actions(workspace: Workspace) -> Vec<Action> {
    let head = match workspace {
        Workspace::Explore => explore(),
        Workspace::Live => live(),
        Workspace::Recordings => recordings(),
        Workspace::Monitors => monitors(),
        Workspace::Findings => findings(),
        Workspace::System => system(),
        Workspace::Globe => globe(),
    };
    finish(head, common())
}

fn finish(head: Vec<Action>, tail: &[Action]) -> Vec<Action> {
    let mut actions = head;
    for action in tail {
        let overlap = actions.iter().any(|have| keys_overlap(have, action));
        if !overlap {
            actions.push(*action);
        }
    }
    actions
}

fn keys_overlap(left: &Action, right: &Action) -> bool {
    left.keys.iter().any(|key| right.keys.contains(key))
}

fn row(
    keys: &'static [Act],
    pair: &'static str,
    group: Group,
    line: &'static str,
    show: Show,
) -> Action {
    Action {
        keys,
        pair,
        group,
        line,
        show,
        cli: Cli::None,
        reason: "",
        slot: Slot::None,
    }
}

fn slot(mut action: Action, slot: Slot) -> Action {
    action.slot = slot;
    action
}

fn fixed(mut action: Action, command: &'static str) -> Action {
    action.cli = Cli::Fixed(command);
    action
}

fn fetch(mut action: Action) -> Action {
    action.cli = Cli::Fetch;
    action
}

fn because(mut action: Action, reason: &'static str) -> Action {
    action.reason = reason;
    action
}

fn space_row() -> Action {
    row(
        SPACE,
        "",
        Group::View,
        "  Space opens actions for the selection",
        Show::Always,
    )
}

fn copy_row() -> Action {
    slot(
        row(
            COPY_KEYS,
            "",
            Group::View,
            "  Copy CLI command writes it on the status line",
            Show::Always,
        ),
        Slot::Copy,
    )
}

/// How a menu row is activated from the keyboard.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shortcut {
    Char(char),
    Enter,
    None,
}

/// One row of the selection menu, in action-table order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct MenuItem {
    pub slot: Slot,
    pub shortcut: Shortcut,
    pub label: &'static str,
}

/// Selection-menu rows for the workspace on screen. Home has none.
#[must_use]
pub(crate) fn menu_items(model: &Explorer) -> Vec<MenuItem> {
    if model.at_home() {
        return Vec::new();
    }
    workspace_actions(model.workspace())
        .into_iter()
        .filter(|action| action.slot != Slot::None)
        .map(|action| MenuItem {
            slot: action.slot,
            shortcut: shortcut_of(action.keys),
            label: action.slot.label(),
        })
        .collect()
}

fn shortcut_of(keys: &[Act]) -> Shortcut {
    match keys {
        [Act::Char(character)] => Shortcut::Char(*character),
        [Act::Enter] => Shortcut::Enter,
        _ => Shortcut::None,
    }
}

/// `sigy play` for one station, or the placeholder when no id is known.
#[must_use]
pub(crate) fn play_command(station: Option<&str>) -> String {
    substitute(PLAY_CLI, "STATION_UUID", station)
}

/// `sigy listen file` for one recording, or the placeholder when no id is known.
#[must_use]
pub(crate) fn file_command(recording: Option<&str>) -> String {
    substitute(FILE_CLI, "RECORDING", recording)
}

/// Visible menu text, including the direct key when the row has one.
#[must_use]
pub(crate) fn menu_label(item: &MenuItem) -> String {
    match item.shortcut {
        Shortcut::Char(character) => format!("{character} {}", item.label),
        Shortcut::Enter => format!("Enter {}", item.label),
        Shortcut::None => item.label.to_owned(),
    }
}

fn substitute(template: &str, token: &str, value: Option<&str>) -> String {
    let Some(value) = value else {
        return template.to_owned();
    };
    let clean = super::text::sanitize(value, 128);
    if clean.is_empty() {
        template.to_owned()
    } else {
        template.replace(token, &clean)
    }
}

fn explore() -> Vec<Action> {
    vec![
        row(
            G,
            "g first page",
            Group::Move,
            "  g first page",
            Show::Restart,
        ),
        row(SLASH, "/ search", Group::Find, "  / search", Show::Always),
        row(
            FILTERS,
            "F filters",
            Group::Find,
            "  F filters",
            Show::Always,
        ),
        row(
            COUNTRY,
            "C country",
            Group::Find,
            "  C country",
            Show::Always,
        ),
        row(
            EXPLORE_KEY,
            "e explore",
            Group::Find,
            "  e explore",
            Show::Always,
        ),
        slot(
            fetch(row(
                FETCH,
                "R fetch",
                Group::Find,
                "  R then y fetch    Enter inspect",
                Show::Always,
            )),
            Slot::Fetch,
        ),
        slot(
            row(
                FAVORITE,
                "f favorite",
                Group::Keep,
                "  f favorite",
                Show::Always,
            ),
            Slot::Favorite,
        ),
        row(PAGE, "n/p page", Group::Move, "  n/p page", Show::Always),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
        slot(
            fixed(
                row(
                    PLAY,
                    "",
                    Group::Listen,
                    "  P then y plays    S stops",
                    Show::Always,
                ),
                PLAY_CLI,
            ),
            Slot::Play,
        ),
        fixed(
            row(STOP, "", Group::Listen, "  S stops", Show::Always),
            STOP_CLI,
        ),
        row(RELOAD, "", Group::View, "  r reload", Show::Always),
        row(
            FAVORITES,
            "",
            Group::Find,
            "  v favorites filter",
            Show::Always,
        ),
        row(CLEAR, "", Group::Find, "  x clears filters", Show::Always),
        slot(
            row(ENTER, "", Group::View, "  Enter inspect", Show::Always),
            Slot::Inspect,
        ),
        slot(
            row(LINKED, "", Group::View, "  i linked history", Show::Always),
            Slot::Linked,
        ),
        space_row(),
        copy_row(),
    ]
}

fn live() -> Vec<Action> {
    vec![
        fixed(
            row(PLAY, "P play", Group::Listen, "  P play", Show::Always),
            PLAY_CLI,
        ),
        fixed(
            row(STOP, "S stop", Group::Listen, "  S stop", Show::Always),
            STOP_CLI,
        ),
        row(
            FAVORITE,
            "f favorite",
            Group::Keep,
            "  f favorite",
            Show::Always,
        ),
        row(
            EXPLORE_KEY,
            "",
            Group::Find,
            "  e surprise me",
            Show::Always,
        ),
        row(
            FAVORITES,
            "",
            Group::Find,
            "  v refreshes this favorites list",
            Show::Always,
        ),
        row(
            ENTER,
            "",
            Group::Listen,
            "  Enter confirms play",
            Show::Always,
        ),
        row(HOLD, "H pause", Group::Listen, "  H pause", Show::Retained),
        row(SEEK, "[/] 10s", Group::Listen, "  [ ] seek", Show::Retained),
        row(RELOAD, "r reload", Group::View, "  r reload", Show::Always),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
    ]
}

fn system() -> Vec<Action> {
    vec![
        row(RELOAD, "r reload", Group::View, "  r reload", Show::Always),
        row(TAB, "Tab focus", Group::Move, "  Tab focus", Show::Always),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
    ]
}

fn recordings() -> Vec<Action> {
    vec![
        row(
            SELECT,
            "select row",
            Group::Move,
            "  arrows select a recording",
            Show::Always,
        ),
        row(
            RELOAD,
            "r reload",
            Group::View,
            "  r reloads its metadata",
            Show::Always,
        ),
        slot(
            fixed(
                row(
                    PLAY,
                    "P play",
                    Group::Listen,
                    "  P then y plays a retained recording    S stops",
                    Show::Always,
                ),
                FILE_CLI,
            ),
            Slot::Play,
        ),
        slot(
            fixed(
                row(
                    LATEST,
                    "L latest",
                    Group::Listen,
                    "  L then y plays the latest sealed interval",
                    Show::Always,
                ),
                FILE_CLI,
            ),
            Slot::Latest,
        ),
        slot(
            fixed(
                row(
                    PROTECT,
                    "K hold",
                    Group::Keep,
                    "  K then y records one hold on that interval",
                    Show::Always,
                ),
                HOLD_CLI,
            ),
            Slot::Hold,
        ),
        row(STOP, "S stop", Group::Listen, "  S stops", Show::Always),
        row(
            HOLD,
            "H pause",
            Group::Listen,
            "  H pauses at the decoder time and the next H resumes",
            Show::Always,
        ),
        row(
            SEEK,
            "[/] 10s",
            Group::Listen,
            "  [ and ] seek 10 s inside this admitted listen",
            Show::Always,
        ),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
        row(
            STEP,
            "",
            Group::Move,
            "  the next sealed segment starts once sealed; an open tail waits",
            Show::Always,
        ),
        slot(
            row(
                INTERVAL,
                "",
                Group::Listen,
                "  , and . choose a sealed interval    I then y plays that interval",
                Show::Always,
            ),
            Slot::Interval,
        ),
        space_row(),
        copy_row(),
    ]
}

fn monitors() -> Vec<Action> {
    vec![
        row(
            ENTER,
            "inspect row",
            Group::View,
            "  Enter inspects",
            Show::Always,
        ),
        row(RELOAD, "r reload", Group::View, "  r reload", Show::Always),
        row(
            BACK,
            "Esc back",
            Group::Move,
            "  Esc or Backspace goes up one level",
            Show::Always,
        ),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
        row(
            SELECT,
            "",
            Group::Move,
            "  arrows select or scroll",
            Show::Always,
        ),
    ]
}

fn findings() -> Vec<Action> {
    vec![
        row(
            SLASH,
            "/ edit IDs",
            Group::Find,
            "  / edit IDs",
            Show::Always,
        ),
        slot(
            row(
                ENTER,
                "Enter inspect",
                Group::View,
                "  Enter inspects",
                Show::Always,
            ),
            Slot::Inspect,
        ),
        row(
            CLAIMS,
            "c claims",
            Group::View,
            "  c opens stored claims, read-only",
            Show::Always,
        ),
        slot(
            row(
                ORIGINAL,
                "o metadata",
                Group::View,
                "  o recording metadata",
                Show::Always,
            ),
            Slot::Metadata,
        ),
        slot(
            fixed(
                row(
                    PLAY,
                    "P play",
                    Group::Listen,
                    "  P then y plays a cited interval",
                    Show::Always,
                ),
                FILE_CLI,
            ),
            Slot::Play,
        ),
        row(STOP, "S stop", Group::Listen, "  S stops", Show::Always),
        row(
            HOLD,
            "H pause",
            Group::Listen,
            "  H pauses at the decoder time and the next H resumes",
            Show::Always,
        ),
        row(
            SEEK,
            "[/] 10s",
            Group::Listen,
            "  [ and ] seek 10 s inside this admitted listen",
            Show::Always,
        ),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
        row(
            &[],
            "",
            Group::Listen,
            "  a cited listen does not continue into the next segment",
            Show::Always,
        ),
        space_row(),
        copy_row(),
    ]
}

fn globe() -> Vec<Action> {
    vec![
        row(
            G,
            "g first page",
            Group::Move,
            "  g first page",
            Show::Restart,
        ),
        slot(
            fixed(
                row(
                    PLAY,
                    "P play",
                    Group::Listen,
                    "  P then y plays",
                    Show::Always,
                ),
                PLAY_CLI,
            ),
            Slot::Play,
        ),
        slot(
            row(
                ENTER,
                "Enter inspect",
                Group::View,
                "  Enter inspect",
                Show::Always,
            ),
            Slot::Inspect,
        ),
        row(
            ARROWS,
            "arrows turn",
            Group::Move,
            "  arrows or h/l/j/k turn",
            Show::Always,
        ),
        because(
            row(SPIN, "s spin", Group::View, "  s spin", Show::Spin),
            SPIN_OFF,
        ),
        row(MAP, "m map", Group::View, "  m map", Show::Always),
        row(RELOAD, "r reload", Group::View, "  r reload", Show::Always),
        slot(
            fetch(row(
                FETCH,
                "R fetch",
                Group::Find,
                "  R then y fetch",
                Show::Always,
            )),
            Slot::Fetch,
        ),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
        row(TURN, "", Group::Move, "  h l j k turn", Show::Always),
        slot(
            row(CENTER, "", Group::Move, "  c center", Show::Always),
            Slot::Center,
        ),
        row(SLASH, "", Group::Find, "  / search", Show::Always),
        row(FILTERS, "", Group::Find, "  F filters", Show::Always),
        row(COUNTRY, "", Group::Find, "  C country", Show::Always),
        row(EXPLORE_KEY, "", Group::Find, "  e explore", Show::Always),
        row(PAGE, "", Group::Move, "  n/p page", Show::Always),
        slot(
            row(FAVORITE, "", Group::Keep, "  f favorite", Show::Always),
            Slot::Favorite,
        ),
        fixed(
            row(STOP, "", Group::Listen, "  S stops", Show::Always),
            STOP_CLI,
        ),
        row(
            FAVORITES,
            "",
            Group::Find,
            "  v favorites filter",
            Show::Always,
        ),
        row(CLEAR, "", Group::Find, "  x clears filters", Show::Always),
        slot(
            row(LINKED, "", Group::View, "  i linked history", Show::Always),
            Slot::Linked,
        ),
        space_row(),
        copy_row(),
    ]
}

fn home() -> Vec<Action> {
    vec![
        row(
            SELECT,
            "",
            Group::Move,
            "  Up and Down select",
            Show::Always,
        ),
        row(DIGITS, "", Group::Move, "  1-7 open a task", Show::Always),
        row(
            ENTER,
            "Enter open",
            Group::View,
            "  Enter opens the selected task",
            Show::Always,
        ),
        row(
            EXPLORE_KEY,
            "e explore",
            Group::Find,
            "  e explore",
            Show::Always,
        ),
        row(RELOAD, "r reload", Group::View, "  r reload", Show::Always),
        row(
            QUIT,
            "q quit",
            Group::View,
            "  q quits this client",
            Show::Always,
        ),
        row(
            QUESTION,
            "",
            Group::View,
            "  ? opens this list",
            Show::Always,
        ),
        row(
            HOLD,
            "",
            Group::Listen,
            "  H pauses at the decoder time and the next H resumes",
            Show::Always,
        ),
        row(
            SEEK,
            "",
            Group::Listen,
            "  [ and ] seek 10 s inside this admitted listen",
            Show::Always,
        ),
    ]
}

fn common() -> &'static [Action] {
    &COMMON
}

const COMMON: [Action; 17] = [
    row_const(HOME_KEY, "", Group::Move, "  0 returns home", Show::Always),
    row_const(
        BACK,
        "Esc back",
        Group::Move,
        "  Esc or Backspace goes up one level",
        Show::Always,
    ),
    row_const(DIGITS, "", Group::Move, "  1-7 workspace", Show::Always),
    row_const(
        ACROSS,
        "",
        Group::Move,
        "  Left and Right change workspace",
        Show::Always,
    ),
    row_const(
        SELECT,
        "",
        Group::Move,
        "  Up and Down select",
        Show::Always,
    ),
    row_const(TAB, "", Group::Move, "  Tab focus", Show::Always),
    row_const(BACK_TAB, "", Group::Move, "  Shift-Tab focus", Show::Always),
    row_const(
        QUESTION,
        "",
        Group::View,
        "  ? opens this list",
        Show::Always,
    ),
    row_const(QUIT, "", Group::View, "  q quits this client", Show::Always),
    row_const(
        HOLD,
        "",
        Group::Listen,
        "  H pauses at the decoder time and the next H resumes",
        Show::Always,
    ),
    row_const(
        SEEK,
        "",
        Group::Listen,
        "  [ and ] seek 10 s inside this admitted listen",
        Show::Always,
    ),
    row_const(PAGE, "", Group::Move, "  n/p page", Show::Always),
    row_const(SLASH, "", Group::Find, "  / search", Show::Always),
    row_const(FAVORITE, "", Group::Keep, "  f favorite", Show::Always),
    row_const(
        FAVORITES,
        "",
        Group::Find,
        "  v favorites filter",
        Show::Always,
    ),
    row_const(ENTER, "", Group::View, "  Enter inspect", Show::Always),
    row_const(RELOAD, "", Group::View, "  r reload", Show::Always),
];

const fn row_const(
    keys: &'static [Act],
    pair: &'static str,
    group: Group,
    line: &'static str,
    show: Show,
) -> Action {
    Action {
        keys,
        pair,
        group,
        line,
        show,
        cli: Cli::None,
        reason: "",
        slot: Slot::None,
    }
}

#[cfg(test)]
fn act_of(key: &Key) -> Option<Act> {
    Some(match key {
        Key::Char(character) => Act::Char(*character),
        Key::Tab => Act::Tab,
        Key::BackTab => Act::BackTab,
        Key::Enter => Act::Enter,
        Key::Escape => Act::Escape,
        Key::Backspace => Act::Backspace,
        Key::Up => Act::Up,
        Key::Down => Act::Down,
        Key::Left => Act::Left,
        Key::Right => Act::Right,
        Key::ClearInput | Key::Paste(_) | Key::Quit | Key::Redraw => return None,
    })
}

#[cfg(test)]
fn key_of(act: Act) -> Key {
    match act {
        Act::Char(character) => Key::Char(character),
        Act::Tab => Key::Tab,
        Act::BackTab => Key::BackTab,
        Act::Enter => Key::Enter,
        Act::Escape => Key::Escape,
        Act::Backspace => Key::Backspace,
        Act::Up => Key::Up,
        Act::Down => Key::Down,
        Act::Left => Key::Left,
        Act::Right => Key::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::{ScreenId, parent, screen_for};
    use crate::explorer::state::Workspace;

    #[test]
    fn every_workspace_names_home_and_home_has_no_parent() {
        assert_eq!(parent(ScreenId::Home), None);
        for workspace in Workspace::ALL {
            assert_eq!(parent(screen_for(workspace)), Some(ScreenId::Home));
        }
    }
}
