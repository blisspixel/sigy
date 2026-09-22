//! Termina session. Quit restores the terminal and does not stop the service.

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TerminaBackend;
use termina::Terminal as TerminaControl;
use termina::escape::csi::{Csi, DecPrivateMode, DecPrivateModeCode, Mode};
use termina::event::{Event, KeyCode, KeyEventKind, Modifiers};
use termina::{EventReader, PlatformTerminal};

use crate::explorer::client;
use crate::explorer::screen::render;
use crate::explorer::state::{Effect, Explorer, Key};

pub fn drive(
    runtime: &tokio::runtime::Runtime,
    directory: &Path,
    model: &mut Explorer,
    inspect: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::open()?;
    let result = run_loop(&mut session, runtime, directory, model, inspect);
    session.restore()?;
    result
}

fn run_loop(
    session: &mut Session,
    runtime: &tokio::runtime::Runtime,
    directory: &Path,
    model: &mut Explorer,
    inspect: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = session.draw(model)?;
    if let Some(path) = inspect {
        write_inspect(path, session.size()?, model, &text)?;
        if session.size()?.0 >= 80 && session.size()?.1 >= 24 && !frame_has_labels(&text) {
            return Err("list frame is missing search, identity, playback, or help".into());
        }
        return Ok(());
    }
    loop {
        let Some(key) = session.poll(Duration::from_millis(250))? else {
            continue;
        };
        let effect = model.handle(key);
        if matches!(effect, Effect::Detach) {
            return Ok(());
        }
        if !client::operations_for(&effect, model).is_empty() {
            runtime.block_on(client::perform(directory, model, &effect))?;
        }
        if model.take_dirty() {
            session.draw(model)?;
        }
    }
}

fn frame_has_labels(text: &str) -> bool {
    text.contains("Search:")
        && text.contains("Selected source:")
        && text.contains("Playback:")
        && text.contains("Help:")
}

fn write_inspect(path: &Path, size: (u16, u16), model: &Explorer, text: &str) -> io::Result<()> {
    let report = serde_json::json!({
        "cols": size.0,
        "rows": size.1,
        "wt_session": std::env::var_os("WT_SESSION").is_some(),
        "backend": "termina",
        "reduced_motion": model.modes().reduced_motion,
        "linear": model.modes().linear,
        "monochrome": model.modes().monochrome,
        "has_search": text.contains("Search:"),
        "has_identity": text.contains("Selected source:"),
        "has_playback": text.contains("Playback:"),
        "has_help": text.contains("Help:"),
    });
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{report}\n"))
}

struct Session {
    terminal: Option<Terminal<TerminaBackend<PlatformTerminal>>>,
    reader: EventReader,
    restored: bool,
}

impl Session {
    fn open() -> io::Result<Self> {
        let mut output = PlatformTerminal::new()?;
        output.set_panic_hook(|handle| {
            let leave = csi_mode(false, DecPrivateModeCode::ClearAndEnableAlternateScreen);
            let unpaste = csi_mode(false, DecPrivateModeCode::BracketedPaste);
            let _ = write!(handle, "{leave}{unpaste}");
            let _ = handle.flush();
        });
        output.enter_raw_mode()?;
        let enter = csi_mode(true, DecPrivateModeCode::ClearAndEnableAlternateScreen);
        let paste = csi_mode(true, DecPrivateModeCode::BracketedPaste);
        write!(output, "{enter}{paste}")?;
        output.flush()?;
        let reader = output.event_reader();
        let backend = TerminaBackend::new(output);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal: Some(terminal),
            reader,
            restored: false,
        })
    }

    fn draw(&mut self, model: &Explorer) -> io::Result<String> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Err(io::Error::other("terminal already closed"));
        };
        let mut text = String::new();
        terminal.draw(|frame| {
            render(frame, model);
            text = plain(frame);
        })?;
        Ok(text)
    }

    fn poll(&mut self, timeout: Duration) -> io::Result<Option<Key>> {
        if !self.reader.poll(Some(timeout), |_| true)? {
            return Ok(None);
        }
        Ok(map_event(self.reader.read(|_| true)?))
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Err(io::Error::other("terminal already closed"));
        };
        let size = terminal.size()?;
        Ok((size.width, size.height))
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        if let Some(mut terminal) = self.terminal.take() {
            let backend = terminal.backend_mut();
            let leave = csi_mode(false, DecPrivateModeCode::ClearAndEnableAlternateScreen);
            let unpaste = csi_mode(false, DecPrivateModeCode::BracketedPaste);
            write!(backend, "{leave}{unpaste}")?;
            backend.flush()?;
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn plain(frame: &mut ratatui::Frame<'_>) -> String {
    let area = frame.area();
    let buffer = frame.buffer_mut();
    let mut lines = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer[(area.x.saturating_add(x), area.y.saturating_add(y))].symbol());
        }
        lines.push(line.trim_end().to_owned());
    }
    lines.join("\n")
}

fn map_event(event: Event) -> Option<Key> {
    match event {
        Event::Paste(text) => Some(Key::Paste(text)),
        Event::WindowResized(_) => Some(Key::Redraw),
        Event::Key(key) if key.kind == KeyEventKind::Release => None,
        Event::Key(key)
            if key.kind == KeyEventKind::Repeat
                && matches!(key.code, KeyCode::Char('f' | 'v' | 'r' | 'q')) =>
        {
            None
        }
        Event::Key(key) => map_key(key.code, key.modifiers),
        _ => None,
    }
}

fn map_key(code: KeyCode, modifiers: Modifiers) -> Option<Key> {
    if modifiers.contains(Modifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
        return Some(Key::Quit);
    }
    Some(match code {
        KeyCode::Char('\u{3}') => Key::Quit,
        KeyCode::Char(character) => Key::Char(character),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        _ => return None,
    })
}

fn csi_mode(enable: bool, code: DecPrivateModeCode) -> Csi {
    let mode = DecPrivateMode::Code(code);
    Csi::Mode(if enable {
        Mode::SetDecPrivateMode(mode)
    } else {
        Mode::ResetDecPrivateMode(mode)
    })
}
