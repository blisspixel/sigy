//! Termina session. Quit restores the terminal and does not stop the service.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TerminaBackend;
use termina::Terminal as TerminaControl;
use termina::escape::csi::{Csi, DecPrivateMode, DecPrivateModeCode, Mode};
use termina::event::{Event, KeyCode, KeyEventKind, Modifiers};
use termina::{EventReader, PlatformTerminal};

use crate::explorer::screen::render;
use crate::explorer::state::{Effect, Explorer, Key};

pub fn drive(
    runtime: &tokio::runtime::Runtime,
    directory: &Path,
    model: &mut Explorer,
    inspect: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stop = arm_stop(runtime);
    model.configure_motion(
        session_may_animate(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            std::env::var_os("CI").is_some(),
        ),
        inspect.is_some(),
        std::time::Instant::now(),
    );
    let mut session = Session::open()?;
    let mut playback_unresolved = false;
    let result = run_loop(
        &mut session,
        runtime,
        directory,
        model,
        inspect,
        &stop,
        &mut playback_unresolved,
    );
    let restored = session.restore();
    if playback_unresolved {
        eprintln!(
            "Playback stop requested. Closure is unresolved until that client exits. Capture keeps running."
        );
    }
    restored?;
    result
}

fn run_loop(
    session: &mut Session,
    runtime: &tokio::runtime::Runtime,
    directory: &Path,
    model: &mut Explorer,
    inspect: Option<&Path>,
    stop: &AtomicBool,
    playback_unresolved: &mut bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if stopped(stop) {
        return Err(Interrupted.into());
    }
    note_canvas(session, model)?;
    let text = session.draw(model)?;
    if let Some(path) = inspect {
        write_inspect(path, session.size()?, model, &text)?;
        if session.size()?.0 >= 80 && session.size()?.1 >= 24 && !frame_has_labels(&text) {
            return Err("list frame is missing search, identity, playback, or help".into());
        }
        if stopped(stop) {
            return Err(Interrupted.into());
        }
        return Ok(());
    }
    let mut clock_tick = std::time::Instant::now();
    let mut audio = super::audio::Audio::default();
    let mut fetcher = super::fetch::Fetcher::default();
    let mut metadata = super::metadata::Reads::default();
    loop {
        if stopped(stop) {
            *playback_unresolved = audio.quit(runtime, directory, model);
            return Err(Interrupted.into());
        }
        fetcher.poll(model);
        audio.poll(directory, model);
        metadata.poll(runtime, directory, model);
        note_canvas(session, model)?;
        let visible = if model.spin_requested() {
            let (width, height) = session.size()?;
            super::screen::globe_visible(model, ratatui::layout::Rect::new(0, 0, width, height))
        } else {
            false
        };
        let timeout = model.advance_spin(std::time::Instant::now(), visible);
        if clock_tick.elapsed() >= Duration::from_secs(1) {
            model.tick_clock(super::now_ms()?);
            audio.request_status(runtime, directory);
            clock_tick = std::time::Instant::now();
        }
        if model.take_dirty() {
            session.draw(model)?;
        }
        let Some(key) = session.poll(poll_wait(timeout))? else {
            continue;
        };
        if !matches!(key, Key::Redraw) {
            model.motion_input(std::time::Instant::now());
        }
        let effect = model.handle(key);
        audio.apply(runtime, directory, model, &effect);
        if matches!(effect, Effect::Detach) {
            *playback_unresolved = audio.quit(runtime, directory, model);
            return Ok(());
        }
        if let Effect::Fetch(query) = &effect {
            fetcher.start(runtime, directory, model, query);
        }
        dispatch_metadata(model, &effect, &mut metadata, runtime, directory, |model| {
            session.draw(model).map(|_| ()).map_err(Into::into)
        })?;
    }
}

/// Paint the key's intent, queue its read, and return before that read finishes.
pub(super) fn dispatch_metadata(
    model: &mut Explorer,
    effect: &Effect,
    metadata: &mut super::metadata::Reads,
    runtime: &tokio::runtime::Runtime,
    directory: &Path,
    mut draw: impl FnMut(&Explorer) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if model.take_dirty() {
        draw(model)?;
    }
    metadata.submit(runtime, directory, model, effect);
    if model.take_dirty() {
        draw(model)?;
    }
    Ok(())
}

fn frame_has_labels(text: &str) -> bool {
    text.contains("Search:")
        && text.contains("Selected station:")
        && text.contains("Listen session:")
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
        "animation_frames": model.animation_frames(),
        "has_search": text.contains("Search:"),
        "has_identity": text.contains("Selected station:"),
        "has_playback": text.contains("Listen session:"),
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
            let _ = clear_session_modes(&mut terminal);
            return Err(error);
        }
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
        let begin = csi_mode(true, DecPrivateModeCode::SynchronizedOutput);
        let end = csi_mode(false, DecPrivateModeCode::SynchronizedOutput);
        write!(terminal.backend_mut(), "{begin}")?;
        let mut text = String::new();
        let draw_error = terminal
            .draw(|frame| {
                render(frame, model);
                text = plain(frame);
            })
            .err();
        let end_error = write!(terminal.backend_mut(), "{end}").err();
        let flush_error = terminal.backend_mut().flush().err();
        if let Some(error) = draw_error.or(end_error).or(flush_error) {
            return Err(error);
        }
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
            write!(backend, "{}", restored_modes())?;
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
    if modifiers.contains(Modifiers::CONTROL) && matches!(code, KeyCode::Char('u')) {
        return Some(Key::ClearInput);
    }
    Some(match code {
        KeyCode::Char('\u{3}') => Key::Quit,
        KeyCode::Char('\u{15}') => Key::ClearInput,
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

fn session_may_animate(interactive: bool, ci: bool) -> bool {
    interactive && !ci
}

/// Both streams must be terminals, and `TERM=dumb` never draws.
#[must_use]
pub(crate) fn interactive_terminal(stdin_tty: bool, stdout_tty: bool, term_dumb: bool) -> bool {
    stdin_tty && stdout_tty && !term_dumb
}

pub(crate) fn restored_modes() -> String {
    let show = csi_mode(true, DecPrivateModeCode::ShowCursor);
    let leave = csi_mode(false, DecPrivateModeCode::ClearAndEnableAlternateScreen);
    let unpaste = csi_mode(false, DecPrivateModeCode::BracketedPaste);
    let unsync = csi_mode(false, DecPrivateModeCode::SynchronizedOutput);
    format!("{show}{leave}{unpaste}{unsync}")
}

pub(crate) fn enable_session_modes(
    terminal: &mut Terminal<TerminaBackend<PlatformTerminal>>,
) -> io::Result<()> {
    let unsync = csi_mode(false, DecPrivateModeCode::SynchronizedOutput);
    let enter = csi_mode(true, DecPrivateModeCode::ClearAndEnableAlternateScreen);
    let paste = csi_mode(true, DecPrivateModeCode::BracketedPaste);
    write!(terminal.backend_mut(), "{unsync}{enter}{paste}")?;
    terminal.backend_mut().flush()
}

fn clear_session_modes(
    terminal: &mut Terminal<TerminaBackend<PlatformTerminal>>,
) -> io::Result<()> {
    write!(terminal.backend_mut(), "{}", restored_modes())?;
    terminal.backend_mut().flush()
}

pub(crate) fn require_window(cols: u16, rows: u16) -> io::Result<()> {
    if cols == 0 || rows == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal size is zero",
        ));
    }
    Ok(())
}

const STOP_POLL: Duration = Duration::from_millis(250);

fn poll_wait(animation: Duration) -> Duration {
    animation.min(STOP_POLL)
}

fn stopped(stop: &AtomicBool) -> bool {
    stop.load(Ordering::SeqCst)
}

fn arm_stop(runtime: &tokio::runtime::Runtime) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    runtime.spawn(async move {
        await_stop().await;
        flag.store(true, Ordering::SeqCst);
    });
    stop
}

pub(crate) async fn await_stop() {
    #[cfg(unix)]
    unix_stop().await;
    #[cfg(windows)]
    windows_stop().await;
}

#[cfg(unix)]
async fn unix_stop() {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
        return;
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if result.is_err() {
                terminate.recv().await;
            }
        }
        _ = terminate.recv() => {}
    }
}

#[cfg(windows)]
async fn windows_stop() {
    let Ok(mut close) = tokio::signal::windows::ctrl_close() else {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
        return;
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if result.is_err() {
                close.recv().await;
            }
        }
        _ = close.recv() => {}
    }
}

/// External stop. The session restores modes before the process exits 130.
#[derive(Debug)]
pub(crate) struct Interrupted;

impl std::fmt::Display for Interrupted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("interrupted")
    }
}

impl std::error::Error for Interrupted {}

fn note_canvas(session: &mut Session, model: &mut Explorer) -> io::Result<()> {
    let (width, height) = session.size()?;
    if let Some((canvas_width, canvas_height)) =
        super::screen::globe_flourish_canvas(model, ratatui::layout::Rect::new(0, 0, width, height))
    {
        model.note_globe_canvas(canvas_width, canvas_height);
    }
    Ok(())
}

pub(crate) fn synchronized_output(enable: bool) -> String {
    csi_mode(enable, DecPrivateModeCode::SynchronizedOutput).to_string()
}

fn csi_mode(enable: bool, code: DecPrivateModeCode) -> Csi {
    let mode = DecPrivateMode::Code(code);
    Csi::Mode(if enable {
        Mode::SetDecPrivateMode(mode)
    } else {
        Mode::ResetDecPrivateMode(mode)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::state::{Link, Modes};

    #[test]
    fn production_dispatch_draws_intent_before_read_and_outcome_after()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("missing");
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome: true,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            0,
        );
        model.note_link(Link::LocalCatalog);
        model.handle(Key::Char('/'));
        model.handle(Key::Paste("Tokyo".into()));
        let effect = model.handle(Key::Enter);
        let (release_send, release_recv) = tokio::sync::oneshot::channel();
        let mut metadata = super::super::metadata::Reads::default();
        metadata.withhold_next(release_recv);
        let trace = std::cell::RefCell::new(Vec::new());
        dispatch_metadata(&mut model, &effect, &mut metadata, &runtime, &path, |_| {
            trace.borrow_mut().push("draw");
            Ok(())
        })?;
        assert_eq!(*trace.borrow(), ["draw"]);
        assert!(metadata.read_busy());
        assert!(model.status().contains("Reading cached station matches"));
        let _ = release_send.send(());
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    metadata.poll(&runtime, &path, &mut model);
                    if !metadata.read_busy() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
        })?;
        assert!(model.take_dirty());
        trace.borrow_mut().push("draw");
        assert_eq!(*trace.borrow(), ["draw", "draw"]);
        assert!(model.status().contains("Disconnected"));
        Ok(())
    }

    #[test]
    fn synchronized_output_is_private_mode_2026() {
        let begin = csi_mode(true, DecPrivateModeCode::SynchronizedOutput).to_string();
        let end = csi_mode(false, DecPrivateModeCode::SynchronizedOutput).to_string();
        assert_eq!(begin, "\u{1b}[?2026h");
        assert_eq!(end, "\u{1b}[?2026l");
        let restored = restored_modes();
        assert!(restored.contains("\u{1b}[?25h"), "{restored}");
        assert!(restored.contains("\u{1b}[?1049l"), "{restored}");
        assert!(restored.contains("\u{1b}[?2004l"), "{restored}");
        assert!(restored.contains("\u{1b}[?2026l"), "{restored}");
    }

    #[test]
    fn a_pipe_or_dumb_terminal_does_not_open_the_explorer() {
        assert!(interactive_terminal(true, true, false));
        assert!(!interactive_terminal(false, true, false));
        assert!(!interactive_terminal(true, false, false));
        assert!(!interactive_terminal(false, false, false));
        assert!(!interactive_terminal(true, true, true));
    }

    #[test]
    fn stop_poll_stays_inside_the_animation_and_idle_bounds() {
        assert_eq!(
            poll_wait(Duration::from_millis(100)),
            Duration::from_millis(100)
        );
        assert_eq!(
            poll_wait(Duration::from_millis(250)),
            Duration::from_millis(250)
        );
        assert_eq!(
            poll_wait(Duration::from_secs(300)),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn a_zero_window_is_refused_before_modes_change() {
        assert!(require_window(0, 24).is_err());
        assert!(require_window(80, 0).is_err());
        assert!(require_window(80, 24).is_ok());
    }

    #[test]
    fn an_external_stop_is_a_distinct_error() {
        let error: Box<dyn std::error::Error> = Box::new(Interrupted);
        assert!(error.is::<Interrupted>());
        assert_eq!(error.to_string(), "interrupted");
    }

    #[test]
    fn continuous_integration_does_not_animate() {
        assert!(session_may_animate(true, false));
        assert!(!session_may_animate(true, true));
        assert!(!session_may_animate(false, false));
        assert!(!session_may_animate(false, true));
    }
}
