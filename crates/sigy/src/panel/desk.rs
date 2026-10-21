//! Read-only keyboard desk. It shares the panel projection and starts no work.

use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TerminaBackend;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use termina::Terminal as TerminaControl;
use termina::event::{Event, KeyCode, KeyEventKind, Modifiers};
use termina::{EventReader, PlatformTerminal};

use super::present::{Freshness, Projection, ServicePresence, revision_label, station_label};
use super::project;
use crate::explorer::Modes;
use crate::explorer::text::sanitize;
use crate::explorer::{
    await_stop, enable_session_modes, require_window, restored_modes, synchronized_output,
};
use crate::style::{self, Tone};

/// Plain refusal for a pipe, a redirect, or `TERM=dumb`.
#[derive(Debug)]
pub(crate) struct DeskRefusal {
    next: String,
}

impl DeskRefusal {
    pub(crate) const MESSAGE: &'static str = "The desk needs an interactive terminal.";

    #[must_use]
    pub(crate) fn new(explicit_directory: Option<&str>) -> Self {
        let directory = explicit_directory
            .map(|value| sanitize(value, 4096))
            .filter(|value| !value.is_empty());
        Self {
            next: sigy_service::guidance::with_data_dir("sigy panel status", directory.as_deref()),
        }
    }

    #[must_use]
    pub(crate) fn next(&self) -> &str {
        &self.next
    }
}

impl std::fmt::Display for DeskRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\nNext: {}", Self::MESSAGE, self.next)
    }
}

impl std::error::Error for DeskRefusal {}

/// Command printed after Enter on Open full Sigy. This desk does not spawn it.
#[must_use]
pub(crate) fn explorer_command(explicit: bool, directory: &Path) -> String {
    let shown = explicit
        .then(|| sanitize(&directory.display().to_string(), 4096))
        .filter(|value| !value.is_empty());
    sigy_service::guidance::with_data_dir("sigy tui", shown.as_deref())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Favorite {
        id: String,
        name: String,
        country: String,
        codec: String,
        hls: bool,
    },
    More,
    Open,
}

impl Row {
    fn key(&self) -> &str {
        match self {
            Self::Favorite { id, .. } => id,
            Self::More => "more",
            Self::Open => "open",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Up,
    Down,
    Enter,
    Quit,
    Reload,
    Redraw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effect {
    Stay,
    Quit,
    Open,
    Reload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leave {
    Quit,
    Open,
}

pub(crate) struct Desk {
    view: Projection,
    rows: Vec<Row>,
    selection: usize,
    modes: Modes,
    note: String,
}

impl Desk {
    #[must_use]
    pub(crate) fn new(view: Projection, modes: Modes) -> Self {
        let rows = rows_from(&view);
        Self {
            view,
            rows,
            selection: 0,
            modes,
            note: String::new(),
        }
    }

    fn reload(&mut self, view: Projection) {
        let selected = self
            .rows
            .get(self.selection)
            .map(Row::key)
            .map(str::to_owned);
        self.view = view;
        self.rows = rows_from(&self.view);
        self.selection = selected
            .and_then(|key| self.rows.iter().position(|row| row.key() == key))
            .unwrap_or(0);
        self.note.clear();
    }

    fn handle(&mut self, key: Key) -> Effect {
        match key {
            Key::Redraw => Effect::Stay,
            Key::Up => {
                self.move_by(false);
                Effect::Stay
            }
            Key::Down => {
                self.move_by(true);
                Effect::Stay
            }
            Key::Quit => Effect::Quit,
            Key::Reload => Effect::Reload,
            Key::Enter => self.activate(),
        }
    }

    fn move_by(&mut self, down: bool) {
        let last = self.rows.len().saturating_sub(1);
        if down {
            self.selection = self.selection.saturating_add(1).min(last);
        } else {
            self.selection = self.selection.saturating_sub(1);
        }
        self.note.clear();
    }

    fn activate(&mut self) -> Effect {
        match self.rows.get(self.selection) {
            Some(Row::Open) => Effect::Open,
            Some(Row::More) => {
                "Favorites page is partial. `sigy radio search --favorites` lists more."
                    .clone_into(&mut self.note);
                Effect::Stay
            }
            Some(Row::Favorite { .. }) => {
                "Selected. This does not play.".clone_into(&mut self.note);
                Effect::Stay
            }
            None => Effect::Stay,
        }
    }
}

fn rows_from(view: &Projection) -> Vec<Row> {
    let mut rows = view
        .favorites
        .iter()
        .map(|favorite| Row::Favorite {
            id: sanitize(&favorite.id, 256),
            name: sanitize(&favorite.name, 256),
            country: sanitize(&favorite.country, 256),
            codec: sanitize(&favorite.codec, 256),
            hls: favorite.hls,
        })
        .collect::<Vec<_>>();
    if view.favorites_truncated {
        rows.push(Row::More);
    }
    rows.push(Row::Open);
    rows
}

/// # Errors
/// Returns an error when the terminal cannot be opened, drawn, or restored.
pub(crate) async fn run(
    directory: &Path,
    explicit_data_dir: bool,
    modes: Modes,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut desk = Desk::new(project(directory).await, modes);
    let stop = watch_stop();
    let mut session = DeskSession::open()?;
    let outcome = drive(&mut session, &mut desk, directory, &stop).await;
    session.restore()?;
    let leave = outcome?;
    if leave == Leave::Open {
        let command = explorer_command(explicit_data_dir, directory);
        let mut stdout = io::stdout().lock();
        if json {
            let mut object = serde_json::Map::new();
            object.insert("next".to_owned(), serde_json::Value::String(command));
            object.insert("read_only".to_owned(), serde_json::Value::Bool(true));
            writeln!(stdout, "{}", serde_json::Value::Object(object))?;
        } else {
            writeln!(stdout, "Next: {command}")?;
        }
    }
    Ok(())
}

async fn drive(
    session: &mut DeskSession,
    model: &mut Desk,
    directory: &Path,
    stop: &AtomicBool,
) -> Result<Leave, Box<dyn std::error::Error>> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Err(crate::explorer::Interrupted.into());
        }
        session.draw(model)?;
        let Some(key) = session.poll(Duration::from_millis(250))? else {
            continue;
        };
        match model.handle(key) {
            Effect::Stay => {}
            Effect::Quit => return Ok(Leave::Quit),
            Effect::Open => return Ok(Leave::Open),
            Effect::Reload => model.reload(project(directory).await),
        }
    }
}

fn watch_stop() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    tokio::spawn(async move {
        await_stop().await;
        flag.store(true, Ordering::SeqCst);
    });
    stop
}

fn render(frame: &mut ratatui::Frame<'_>, desk: &Desk) {
    let area = frame.area();
    let lines = visible(desk, usize::from(area.width), usize::from(area.height));
    frame.render_widget(Paragraph::new(lines), area);
}

fn visible(desk: &Desk, width: usize, height: usize) -> Vec<Line<'static>> {
    if height == 0 || width == 0 {
        return Vec::new();
    }
    let mut lines = identity(desk, width);
    let help_at = height.saturating_sub(1);
    let row_room = help_at.saturating_sub(lines.len());
    lines.extend(row_window(desk, width, row_room));
    if lines.len() < height {
        lines.push(plain(clip(&help(desk), width)));
    }
    lines.truncate(height);
    lines
}

fn identity(desk: &Desk, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        plain(clip(
            "Sigy desk. Read only. This is not a qualified installation.",
            width,
        )),
        plain(clip(&service_line(&desk.view), width)),
        plain(clip(&directory_line(&desk.view), width)),
    ];
    if desk.view.freshness == Freshness::Empty {
        lines.push(plain(clip("Opening this desk does not fetch.", width)));
    }
    lines.push(listening_line(desk, width));
    if let Some(receipt) = receipt_line(desk, width) {
        lines.push(receipt);
    }
    lines.push(plain(clip(&capture_line(&desk.view), width)));
    lines.push(plain(clip(&selected_line(desk), width)));
    lines.push(plain(clip(
        &format!(
            "Session hint: {}. It does not name the station.",
            desk.view.session_hint
        ),
        width,
    )));
    lines.push(plain(clip(&modes_line(desk.modes), width)));
    if let Some(reason) = failure_line(&desk.view) {
        lines.push(plain(clip(&reason, width)));
    }
    if !desk.note.is_empty() {
        lines.push(plain(clip(&desk.note, width)));
    }
    lines
}

fn service_line(view: &Projection) -> String {
    if view.service == ServicePresence::Running {
        format!("Service: {}.", view.service.as_str())
    } else {
        format!(
            "Service: {}. This desk does not start it.",
            view.service.as_str()
        )
    }
}

fn directory_line(view: &Projection) -> String {
    format!(
        "Directory: {}. Cached {}. Favorites: {}.",
        view.freshness.as_str(),
        crate::copy::station_count(u64::from(view.cached_stations)),
        view.favorite_stations
    )
}

fn listening_line(desk: &Desk, width: usize) -> Line<'static> {
    let Some(listen) = desk.view.listen.as_ref() else {
        return plain(clip("Listening: none.", width));
    };
    let state = sanitize(&listen.state, 64);
    let station = station_label(listen);
    let prefix = "Listening: ".to_owned();
    let suffix = format!(". Station: {station}.");
    let full = format!("{prefix}{state}{suffix}");
    if desk.modes.linear || Line::raw(full.clone()).width() > width {
        return plain(clip(&full, width));
    }
    Line::from(vec![
        Span::raw(prefix),
        state_span(desk.modes, &state),
        Span::raw(suffix),
    ])
}

fn receipt_line(desk: &Desk, width: usize) -> Option<Line<'static>> {
    desk.view.listen.as_ref().map(|listen| {
        plain(clip(
            &format!(
                "Listen session: {}. Revision: {}.",
                sanitize(&listen.id, 256),
                revision_label(&listen.source_revision)
            ),
            width,
        ))
    })
}

fn state_span(modes: Modes, state: &str) -> Span<'static> {
    let tone = listen_tone(state);
    let owned = state.to_owned();
    if modes.monochrome {
        if modes.emphasis.high() && !matches!(tone, Tone::Plain | Tone::Muted) {
            return Span::styled(owned, Style::default().add_modifier(Modifier::BOLD));
        }
        return Span::raw(owned);
    }
    if tone == Tone::Plain {
        return Span::raw(owned);
    }
    if modes.emphasis.high() {
        let mut style = Style::default().add_modifier(Modifier::BOLD);
        if matches!(tone, Tone::Fail | Tone::Live | Tone::Record) {
            style = style.fg(style::role_color(tone));
        }
        return Span::styled(owned, style);
    }
    Span::styled(owned, Style::default().fg(style::role_color(tone)))
}

fn capture_line(view: &Projection) -> String {
    format!(
        "Capture: {} active. Stopping a listen leaves capture running.",
        view.active_captures
    )
}

fn selected_line(desk: &Desk) -> String {
    match desk.rows.get(desk.selection) {
        Some(Row::Favorite { name, id, .. }) => {
            format!("Selected station: {name}. ID {id}.")
        }
        Some(Row::More | Row::Open) | None => "Selected station: none.".to_owned(),
    }
}

fn failure_line(view: &Projection) -> Option<String> {
    view.listen.as_ref().and_then(|listen| {
        listen
            .failure
            .as_deref()
            .map(|reason| format!("Reason: {}.", sanitize(reason, 256)))
    })
}

fn modes_line(modes: Modes) -> String {
    format!(
        "Modes: reduced motion {} | linear {} | monochrome {}.",
        on_off(modes.reduced_motion),
        on_off(modes.linear),
        on_off(modes.monochrome)
    )
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn help(desk: &Desk) -> String {
    if desk.modes.reduced_motion {
        "Arrows select. Enter opens full Sigy. r reloads. q quits. Still.".to_owned()
    } else {
        "Arrows select. Enter opens full Sigy. r reloads. q quits.".to_owned()
    }
}

fn row_window(desk: &Desk, width: usize, room: usize) -> Vec<Line<'static>> {
    if room == 0 || desk.rows.is_empty() {
        return Vec::new();
    }
    let start = if desk.rows.len() <= room {
        0
    } else {
        desk.selection
            .saturating_add(1)
            .saturating_sub(room)
            .min(desk.rows.len().saturating_sub(room))
    };
    desk.rows
        .iter()
        .enumerate()
        .skip(start)
        .take(room)
        .map(|(index, row)| row_line(desk, row, index == desk.selection, width))
        .collect()
}

fn row_line(desk: &Desk, row: &Row, selected: bool, width: usize) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let body = match row {
        Row::Favorite {
            name,
            country,
            codec,
            hls,
            ..
        } => format!(
            "{name} | {country} | {codec} | hls {}",
            if *hls { "yes" } else { "no" }
        ),
        Row::More => "More in Sigy".to_owned(),
        Row::Open => "Open full Sigy".to_owned(),
    };
    let text = clip(&format!("{marker}{body}"), width);
    if selected && !desk.modes.linear {
        Line::from(Span::styled(
            text,
            Style::default().add_modifier(Modifier::REVERSED),
        ))
    } else {
        plain(text)
    }
}

fn listen_tone(state: &str) -> Tone {
    match state {
        "running" | "starting" => Tone::Live,
        "failed" | "interrupted" => Tone::Fail,
        "stopping" => Tone::Attention,
        _ => Tone::Plain,
    }
}

fn plain(text: String) -> Line<'static> {
    Line::from(text)
}

fn clip(text: &str, cells: usize) -> String {
    let clean = sanitize(text, 256);
    if cells == 0 {
        return String::new();
    }
    let line = Line::raw(clean.clone());
    if line.width() <= cells {
        return clean;
    }
    let mut out = String::new();
    let mut used = 0usize;
    for grapheme in line.styled_graphemes(Style::default()) {
        let next = Span::raw(grapheme.symbol).width();
        if used.saturating_add(next).saturating_add(1) > cells {
            break;
        }
        out.push_str(grapheme.symbol);
        used = used.saturating_add(next);
    }
    out.push('~');
    out
}

struct DeskSession {
    terminal: Option<Terminal<TerminaBackend<PlatformTerminal>>>,
    reader: EventReader,
    restored: bool,
}

impl DeskSession {
    fn open() -> io::Result<Self> {
        let mut output = PlatformTerminal::new()?;
        let size = output.get_dimensions()?;
        require_window(size.cols, size.rows)?;
        output.set_panic_hook(|handle| {
            let _ = write!(handle, "{}", restored_modes());
            let _ = handle.flush();
        });
        output.enter_raw_mode()?;
        let reader = output.event_reader();
        let backend = TerminaBackend::new(output);
        let mut terminal = Terminal::new(backend)?;
        if let Err(error) = enable_session_modes(&mut terminal) {
            let _ = write!(terminal.backend_mut(), "{}", restored_modes());
            let _ = terminal.backend_mut().flush();
            return Err(error);
        }
        Ok(Self {
            terminal: Some(terminal),
            reader,
            restored: false,
        })
    }

    fn draw(&mut self, model: &Desk) -> io::Result<()> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Err(io::Error::other("terminal already closed"));
        };
        let begin = synchronized_output(true);
        let end = synchronized_output(false);
        write!(terminal.backend_mut(), "{begin}")?;
        let draw_error = terminal.draw(|frame| render(frame, model)).err();
        let end_error = write!(terminal.backend_mut(), "{end}").err();
        let flush_error = terminal.backend_mut().flush().err();
        draw_error.or(end_error).or(flush_error).map_or(Ok(()), Err)
    }

    fn poll(&mut self, timeout: Duration) -> io::Result<Option<Key>> {
        if !self.reader.poll(Some(timeout), |_| true)? {
            return Ok(None);
        }
        Ok(map_event(&self.reader.read(|_| true)?))
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        if let Some(mut terminal) = self.terminal.take() {
            let backend = terminal.backend_mut();
            write!(backend, "{}", restored_modes())?;
            backend.flush()?;
        }
        Ok(())
    }
}

impl Drop for DeskSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn map_event(event: &Event) -> Option<Key> {
    match event {
        Event::WindowResized(_) => Some(Key::Redraw),
        Event::Key(key) if key.kind == KeyEventKind::Release => None,
        Event::Key(key)
            if key.kind == KeyEventKind::Repeat
                && matches!(key.code, KeyCode::Char('r' | 'q' | 'R' | 'Q')) =>
        {
            None
        }
        Event::Key(key) => map_key(key.code, key.modifiers),
        _ => None,
    }
}

fn map_key(code: KeyCode, modifiers: Modifiers) -> Option<Key> {
    if modifiers.contains(Modifiers::CONTROL) && matches!(code, KeyCode::Char('c' | 'C')) {
        return Some(Key::Quit);
    }
    Some(match code {
        KeyCode::Up | KeyCode::BackTab => Key::Up,
        KeyCode::Down | KeyCode::Tab => Key::Down,
        KeyCode::Enter => Key::Enter,
        KeyCode::Char('\u{3}' | 'q' | 'Q') | KeyCode::Escape => Key::Quit,
        KeyCode::Char('r' | 'R') => Key::Reload,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};

    use super::{Desk, Effect, Key, Row, clip, explorer_command};
    use crate::explorer::{Emphasis, Modes};
    use crate::panel::present::{FavoriteRow, Freshness, ListenRow, Projection, ServicePresence};

    fn modes(linear: bool, monochrome: bool) -> Modes {
        Modes {
            reduced_motion: false,
            linear,
            monochrome,
            emphasis: Emphasis::Standard,
        }
    }

    fn favorite(id: &str, name: &str) -> FavoriteRow {
        FavoriteRow {
            id: id.to_owned(),
            name: name.to_owned(),
            country: "CA".to_owned(),
            hls: false,
            codec: "MP3".to_owned(),
        }
    }

    fn projection() -> Projection {
        Projection {
            service: ServicePresence::Running,
            freshness: Freshness::Current,
            cached_stations: 2,
            favorite_stations: 1,
            stale_stations: 0,
            newest_age: Some("1h".into()),
            favorites: vec![favorite("station-1", "North Coast")],
            favorites_truncated: false,
            active_captures: 0,
            recordings: Vec::new(),
            recordings_truncated: false,
            listen: None,
            session_hint: "absent",
        }
    }

    fn draw(desk: &Desk, width: u16, height: u16) -> Result<String, Box<dyn std::error::Error>> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| super::render(frame, desk))?;
        let view = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            let mut covered_until = 0u16;
            for x in 0..width {
                if x >= covered_until {
                    let symbol = view[(x, y)].symbol();
                    line.push_str(symbol);
                    let cells = ratatui::text::Span::raw(symbol).width();
                    covered_until = x.saturating_add(u16::try_from(cells).unwrap_or(1));
                }
            }
            lines.push(line.trim_end().to_owned());
        }
        Ok(lines.join("\n"))
    }

    fn cell_fg(
        desk: &Desk,
        width: u16,
        height: u16,
        line_prefix: &str,
        word: &str,
    ) -> Result<Option<Color>, Box<dyn std::error::Error>> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| super::render(frame, desk))?;
        let view = terminal.backend().buffer().clone();
        for y in 0..height {
            let mut text = String::new();
            for x in 0..width {
                text.push_str(view[(x, y)].symbol());
            }
            if text.contains(line_prefix)
                && let Some(start) = text.find(word)
            {
                return Ok(Some(view[(u16::try_from(start).unwrap_or(0), y)].fg));
            }
        }
        Ok(None)
    }

    fn reversed(desk: &Desk, width: u16, height: u16) -> Result<bool, Box<dyn std::error::Error>> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| super::render(frame, desk))?;
        let view = terminal.backend().buffer().clone();
        for y in 0..height {
            if view[(0, y)].symbol() == ">" {
                return Ok(view[(0, y)].modifier.contains(Modifier::REVERSED));
            }
        }
        Ok(false)
    }

    #[test]
    fn refusal_names_status_and_strips_controls() {
        let refusal = super::DeskRefusal::new(Some("lib\u{1b}[31m"));
        let text = refusal.to_string();
        assert_eq!(
            text,
            "The desk needs an interactive terminal.\nNext: sigy --data-dir 'lib[31m' panel status"
        );
        assert!(!text.contains('\u{1b}'), "{text}");
        assert_eq!(super::DeskRefusal::new(None).next(), "sigy panel status");
    }

    #[test]
    fn explorer_command_keeps_an_explicit_library() {
        assert_eq!(explorer_command(false, Path::new("library")), "sigy tui");
        assert_eq!(
            explorer_command(true, Path::new("library")),
            "sigy --data-dir 'library' tui"
        );
    }

    #[test]
    fn empty_offline_and_partial_states_share_the_same_words()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut empty = projection();
        empty.service = ServicePresence::Absent;
        empty.freshness = Freshness::Empty;
        empty.cached_stations = 0;
        empty.favorite_stations = 0;
        empty.favorites.clear();
        let desk = Desk::new(empty, modes(false, true));
        let text = draw(&desk, 80, 24)?;
        assert!(
            text.contains("Service: absent. This desk does not start it."),
            "{text}"
        );
        assert!(text.contains("Directory: empty."), "{text}");
        assert!(text.contains("Opening this desk does not fetch."), "{text}");
        assert!(text.contains("Favorites: 0."), "{text}");
        assert!(text.contains("Listening: none."), "{text}");
        assert!(
            text.contains("Capture: 0 active. Stopping a listen leaves capture running."),
            "{text}"
        );
        assert!(text.contains("Selected station: none."), "{text}");
        assert!(text.contains("Open full Sigy"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");

        let mut partial = projection();
        partial.favorites_truncated = true;
        partial.favorite_stations = 5;
        partial.favorites.push(favorite("station-2", "Ribeira"));
        let mut desk = Desk::new(partial, modes(false, true));
        let text = draw(&desk, 80, 24)?;
        assert!(text.contains("More in Sigy"), "{text}");
        assert!(text.contains("North Coast"), "{text}");
        while !matches!(desk.rows.get(desk.selection), Some(Row::More)) {
            desk.handle(Key::Down);
        }
        assert_eq!(desk.handle(Key::Enter), Effect::Stay);
        assert!(
            desk.note.contains("sigy radio search --favorites"),
            "{}",
            desk.note
        );
        while !matches!(desk.rows.get(desk.selection), Some(Row::Open)) {
            desk.handle(Key::Down);
        }
        assert_eq!(desk.handle(Key::Enter), Effect::Open);
        Ok(())
    }

    #[test]
    fn listen_states_keep_capture_separate_and_station_unknown()
    -> Result<(), Box<dyn std::error::Error>> {
        for (state, failure) in [
            ("starting", None),
            ("running", None),
            ("stopping", None),
            ("failed", Some("decoder closed\u{1b}[31m".to_owned())),
            ("interrupted", Some("cancelled".to_owned())),
        ] {
            let mut view = projection();
            view.active_captures = 2;
            view.session_hint = "present";
            view.listen = Some(ListenRow {
                id: "listen-1".to_owned(),
                state: state.to_owned(),
                failure,
                source_revision: "rev-1".to_owned(),
                station: None,
            });
            let desk = Desk::new(view, modes(false, true));
            let text = draw(&desk, 80, 24)?;
            assert!(text.contains(&format!("Listening: {state}.")), "{text}");
            assert!(text.contains("Station: unknown."), "{text}");
            assert!(text.contains("Listen session: listen-1."), "{text}");
            assert!(text.contains("Revision: rev-1."), "{text}");
            assert!(text.contains("Capture: 2 active."), "{text}");
            assert!(
                text.contains("Session hint: present. It does not name the station."),
                "{text}"
            );
            assert!(!text.contains('\u{1b}'), "{text}");
        }
        let mut failed = projection();
        failed.listen = Some(ListenRow {
            id: "listen-1".to_owned(),
            state: "failed".to_owned(),
            failure: Some("decoder closed".to_owned()),
            source_revision: "rev-1".to_owned(),
            station: None,
        });
        let text = draw(&Desk::new(failed, modes(false, true)), 80, 24)?;
        assert!(text.contains("Reason: decoder closed."), "{text}");
        Ok(())
    }

    #[test]
    fn selection_does_not_play_and_reload_keeps_the_row() {
        let mut view = projection();
        view.favorites.push(favorite("station-2", "Kyst"));
        view.favorite_stations = 2;
        let mut desk = Desk::new(view, modes(false, true));
        assert_eq!(desk.handle(Key::Enter), Effect::Stay);
        assert_eq!(desk.note, "Selected. This does not play.");
        assert_eq!(desk.handle(Key::Enter), Effect::Stay);
        assert_eq!(desk.note, "Selected. This does not play.");
        desk.handle(Key::Down);
        let mut next = projection();
        next.favorites.push(favorite("station-2", "Kyst"));
        next.favorite_stations = 2;
        next.listen = Some(ListenRow {
            id: "listen-2".to_owned(),
            state: "running".to_owned(),
            failure: None,
            source_revision: "rev-2".to_owned(),
            station: None,
        });
        next.session_hint = "present";
        desk.reload(next);
        assert!(
            matches!(desk.rows.get(desk.selection), Some(Row::Favorite { id, .. }) if id == "station-2")
        );
        let mut gone = projection();
        gone.favorites.clear();
        gone.favorite_stations = 0;
        desk.reload(gone);
        assert!(matches!(desk.rows.get(desk.selection), Some(Row::Open)));
    }

    #[test]
    fn narrow_unicode_names_clip_and_selection_is_a_marker()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut view = projection();
        view.favorites[0] = favorite(
            "station-1",
            "東京放送局ラジオステーション名前がとても長い例です",
        );
        view.favorites[0].name.push_str("\u{1b}[2J\u{202e}");
        let desk = Desk::new(view, modes(true, true));
        let narrow = draw(&desk, 40, 12)?;
        let wide = draw(&desk, 80, 24)?;
        for text in [&narrow, &wide] {
            assert!(text.contains("Selected station:"), "{text}");
            assert!(text.contains("Listening:"), "{text}");
            assert!(text.contains("Capture:"), "{text}");
            assert!(text.contains("Open full Sigy"), "{text}");
            assert!(!text.contains('\u{1b}'), "{text}");
            assert!(!text.contains('\u{202e}'), "{text}");
        }
        assert!(narrow.contains('~'), "{narrow}");
        assert!(!reversed(&desk, 80, 24)?);
        let colored = Desk::new(projection(), modes(false, false));
        assert!(reversed(&colored, 80, 24)?);
        let running = {
            let mut view = projection();
            view.listen = Some(ListenRow {
                id: "listen-1".to_owned(),
                state: "running".to_owned(),
                failure: None,
                source_revision: "rev-1".to_owned(),
                station: Some("station-1".to_owned()),
            });
            Desk::new(view, modes(false, false))
        };
        assert_eq!(
            cell_fg(&running, 80, 24, "Listening:", "running")?,
            Some(Color::Green)
        );
        let plain = Desk::new(running.view.clone(), modes(false, true));
        assert_eq!(
            cell_fg(&plain, 80, 24, "Listening:", "running")?,
            Some(Color::Reset)
        );
        assert!(draw(&plain, 80, 24)?.contains("Station: station-1."));
        let mut still = projection();
        still.listen = None;
        let mut reduced = modes(false, true);
        reduced.reduced_motion = true;
        let text = draw(&Desk::new(still, reduced), 80, 24)?;
        assert!(text.contains("reduced motion on"), "{text}");
        assert!(text.contains("Still."), "{text}");
        assert_eq!(clip("abcdef", 4), "abc~");
        Ok(())
    }
}
