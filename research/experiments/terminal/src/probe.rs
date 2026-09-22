//! List and search probe. One process uses one backend.

use std::env;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::backend::{CrosstermBackend, TerminaBackend};
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event as CrossEvent, KeyCode, KeyEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::termina::escape::csi::{Csi, DecPrivateMode, DecPrivateModeCode, Mode};
use ratatui::termina::event::{Event as TermEvent, KeyCode as TermKey, KeyEventKind as TermKind};
use ratatui::termina::{EventReader, PlatformTerminal, Terminal as TerminaIo};
use ratatui::widgets::{List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};

use crate::modes::{self, ConsoleModes};
use crate::textutil::{self, TextFacts, json_string};

type MeasureResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind {
    Crossterm,
    Termina,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Workload,
    Modes,
    Panic,
    Hosted,
    Contrast,
    Color,
}

struct Model {
    stations: Vec<String>,
    query: String,
    reduced_motion: bool,
    no_color: bool,
    pulse: u32,
    pasted: String,
}

enum InputEvent {
    Char(char),
    Backspace,
    Paste(String),
    Resize(u16, u16),
    Quit,
    Ignore,
}

enum Step {
    Continue,
    Quit,
}

struct Report {
    path: PathBuf,
}

impl Report {
    fn create(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::File::create(path)?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    fn emit(&self, line: &str) -> io::Result<()> {
        append_line(&self.path, line)
    }
}

fn append_line(path: &Path, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    file.flush()
}

fn modes_line(tag: &str, modes: ConsoleModes) -> String {
    format!(
        "{{\"t\":{},\"input\":{},\"output\":{},\"input_cp\":{},\"output_cp\":{}}}",
        json_string(tag),
        modes.input,
        modes.output,
        modes.input_cp,
        modes.output_cp
    )
}

pub fn run(args: impl Iterator<Item = String>) -> MeasureResult<()> {
    let mut backend = None;
    let mut phase = None;
    let mut report_path = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--backend" => {
                backend = Some(parse_backend(
                    &args.next().ok_or("missing --backend value")?,
                )?);
            }
            "--phase" => {
                phase = Some(parse_phase(&args.next().ok_or("missing --phase value")?)?);
            }
            "--report" => {
                report_path = Some(PathBuf::from(args.next().ok_or("missing --report value")?));
            }
            other => return Err(format!("unknown probe argument {other}").into()),
        }
    }
    let backend = backend.ok_or("probe requires --backend")?;
    let phase = phase.ok_or("probe requires --phase")?;
    let report_path = report_path.ok_or("probe requires --report")?;
    let report = Report::create(&report_path)?;
    let before = modes::current()?;
    report.emit(&modes_line("mode_before", before))?;
    if phase == Phase::Hosted && env::var_os("WT_SESSION").is_none() {
        return Err("hosted phase is not running inside Windows Terminal".into());
    }
    let model = Model {
        stations: textutil::stations(),
        query: String::new(),
        reduced_motion: env::var("SIGY_REDUCED_MOTION").ok().as_deref() == Some("1"),
        no_color: env::var("NO_COLOR")
            .ok()
            .is_some_and(|value| !value.is_empty()),
        pulse: 0,
        pasted: String::new(),
    };
    match backend {
        BackendKind::Crossterm => run_crossterm(phase, report, model),
        BackendKind::Termina => run_termina(phase, report, model),
    }
}

fn parse_backend(value: &str) -> MeasureResult<BackendKind> {
    match value {
        "crossterm" => Ok(BackendKind::Crossterm),
        "termina" => Ok(BackendKind::Termina),
        other => Err(format!("unknown backend {other}").into()),
    }
}

fn parse_phase(value: &str) -> MeasureResult<Phase> {
    match value {
        "workload" => Ok(Phase::Workload),
        "modes" => Ok(Phase::Modes),
        "panic" => Ok(Phase::Panic),
        "hosted" => Ok(Phase::Hosted),
        "contrast" => Ok(Phase::Contrast),
        "color" => Ok(Phase::Color),
        other => Err(format!("unknown phase {other}").into()),
    }
}

fn run_crossterm(phase: Phase, report: Report, model: Model) -> MeasureResult<()> {
    install_crossterm_panic_hook(report.path.clone());
    enable_raw_mode()?;
    let mut output = io::stdout();
    execute!(output, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(output);
    let terminal = Terminal::new(backend)?;
    let mut session = CrosstermSession {
        terminal: Some(terminal),
    };
    let result = event_loop(&mut session, phase, &report, model);
    session.cleanup()?;
    if result.is_ok() {
        finish_report(&report)?;
    }
    result
}

fn run_termina(phase: Phase, report: Report, model: Model) -> MeasureResult<()> {
    install_record_hook(report.path.clone(), "termina");
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
    let mut session = TerminaSession {
        terminal: Some(terminal),
        reader,
    };
    let result = event_loop(&mut session, phase, &report, model);
    session.cleanup()?;
    if result.is_ok() {
        finish_report(&report)?;
    }
    result
}

fn finish_report(report: &Report) -> MeasureResult<()> {
    let after = modes::current()?;
    report.emit(&modes_line("mode_after", after))?;
    report.emit("{\"t\":\"done\"}")?;
    Ok(())
}

fn install_crossterm_panic_hook(report: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let mut output = io::stdout();
        let _ = execute!(output, LeaveAlternateScreen, DisableBracketedPaste, Show);
        if let Ok(modes) = modes::current() {
            let _ = append_line(&report, &modes_line("mode_after", modes));
        }
        let _ = append_line(&report, "{\"t\":\"panic\",\"backend\":\"crossterm\"}");
        previous(info);
    }));
}

fn install_record_hook(report: PathBuf, backend: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(modes) = modes::current() {
            let _ = append_line(&report, &modes_line("mode_after", modes));
        }
        let line = format!("{{\"t\":\"panic\",\"backend\":{}}}", json_string(backend));
        let _ = append_line(&report, &line);
        previous(info);
    }));
}

fn csi_mode(enable: bool, code: DecPrivateModeCode) -> Csi {
    let mode = DecPrivateMode::Code(code);
    Csi::Mode(if enable {
        Mode::SetDecPrivateMode(mode)
    } else {
        Mode::ResetDecPrivateMode(mode)
    })
}

trait Session {
    fn draw(&mut self, model: &Model) -> io::Result<TextFacts>;
    fn poll(&mut self, timeout: Duration) -> io::Result<Option<InputEvent>>;
    fn size(&mut self) -> io::Result<(u16, u16)>;
    fn cleanup(&mut self) -> io::Result<()>;
}

struct CrosstermSession {
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
}

struct TerminaSession {
    terminal: Option<Terminal<TerminaBackend<PlatformTerminal>>>,
    reader: EventReader,
}

impl Session for CrosstermSession {
    fn draw(&mut self, model: &Model) -> io::Result<TextFacts> {
        draw_terminal(self.terminal.as_mut(), model)
    }

    fn poll(&mut self, timeout: Duration) -> io::Result<Option<InputEvent>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        Ok(Some(map_crossterm(event::read()?)))
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Err(io::Error::other("terminal already closed"));
        };
        let size = terminal.size()?;
        Ok((size.width, size.height))
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.terminal.take();
        disable_raw_mode()?;
        execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            Show
        )?;
        Ok(())
    }
}

impl Session for TerminaSession {
    fn draw(&mut self, model: &Model) -> io::Result<TextFacts> {
        draw_terminal(self.terminal.as_mut(), model)
    }

    fn poll(&mut self, timeout: Duration) -> io::Result<Option<InputEvent>> {
        if !self.reader.poll(Some(timeout), |_| true)? {
            return Ok(None);
        }
        Ok(Some(map_termina(self.reader.read(|_| true)?)))
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Err(io::Error::other("terminal already closed"));
        };
        let size = terminal.size()?;
        Ok((size.width, size.height))
    }

    fn cleanup(&mut self) -> io::Result<()> {
        if let Some(mut terminal) = self.terminal.take() {
            {
                let backend = terminal.backend_mut();
                let leave = csi_mode(false, DecPrivateModeCode::ClearAndEnableAlternateScreen);
                let unpaste = csi_mode(false, DecPrivateModeCode::BracketedPaste);
                write!(backend, "{leave}{unpaste}")?;
                backend.flush()?;
            }
            drop(terminal);
        }
        Ok(())
    }
}

fn draw_terminal<B>(terminal: Option<&mut Terminal<B>>, model: &Model) -> io::Result<TextFacts>
where
    B: ratatui::backend::Backend<Error = io::Error>,
{
    let Some(terminal) = terminal else {
        return Err(io::Error::other("terminal already closed"));
    };
    let mut facts = TextFacts {
        combining_symbol: String::new(),
        combining_split: false,
        wide_symbol: String::new(),
        wide_next: String::new(),
        wide_after: String::new(),
    };
    terminal.draw(|frame| {
        render(frame, model);
        facts = scan_frame(frame);
    })?;
    Ok(facts)
}

fn scan_frame(frame: &mut Frame<'_>) -> TextFacts {
    let area = frame.area();
    let buffer = frame.buffer_mut();
    textutil::scan_cells(area.width, area.height, |x, y| {
        buffer[(area.x.saturating_add(x), area.y.saturating_add(y))].symbol()
    })
}

fn render(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    let [search_area, list_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let search_style = if model.no_color {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    };
    frame.render_widget(
        Paragraph::new(format!("search: {}", model.query)).style(search_style),
        search_area,
    );
    let items: Vec<ListItem<'_>> = textutil::visible(&model.stations, &model.query)
        .into_iter()
        .map(ListItem::new)
        .collect();
    frame.render_widget(List::new(items), list_area);
    let motion = if model.reduced_motion { "on" } else { "off" };
    let color = if model.no_color { "off" } else { "on" };
    frame.render_widget(
        Paragraph::new(format!(
            "shown {} motion {motion} color {color} pulse {}",
            textutil::visible(&model.stations, &model.query).len(),
            model.pulse
        )),
        status_area,
    );
}

fn map_crossterm(event: CrossEvent) -> InputEvent {
    match event {
        CrossEvent::Paste(text) => InputEvent::Paste(text),
        CrossEvent::Resize(cols, rows) => InputEvent::Resize(cols, rows),
        CrossEvent::Key(key) if key.kind == KeyEventKind::Release => InputEvent::Ignore,
        CrossEvent::Key(key) => match key.code {
            KeyCode::Char('q') => InputEvent::Quit,
            KeyCode::Char(character) => InputEvent::Char(character),
            KeyCode::Backspace => InputEvent::Backspace,
            _ => InputEvent::Ignore,
        },
        _ => InputEvent::Ignore,
    }
}

fn map_termina(event: TermEvent) -> InputEvent {
    match event {
        TermEvent::Paste(text) => InputEvent::Paste(text),
        TermEvent::WindowResized(size) => InputEvent::Resize(size.cols, size.rows),
        TermEvent::Key(key) if key.kind == TermKind::Release => InputEvent::Ignore,
        TermEvent::Key(key) => match key.code {
            TermKey::Char('q') => InputEvent::Quit,
            TermKey::Char(character) => InputEvent::Char(character),
            TermKey::Backspace => InputEvent::Backspace,
            _ => InputEvent::Ignore,
        },
        _ => InputEvent::Ignore,
    }
}

fn event_loop(
    session: &mut dyn Session,
    phase: Phase,
    report: &Report,
    mut model: Model,
) -> MeasureResult<()> {
    let facts = session.draw(&model)?;
    report.emit(&facts_line(&facts)?)?;
    absorb_startup(session, &mut model)?;
    let size = session.size()?;
    if phase == Phase::Panic {
        panic!("sigy terminal measure panic check");
    }
    if matches!(phase, Phase::Modes | Phase::Color) {
        report.emit(&format!(
            "{{\"t\":\"frame\",\"cols\":{},\"rows\":{}}}",
            size.0, size.1
        ))?;
        return Ok(());
    }
    if phase == Phase::Contrast {
        return run_contrast(session, report, &mut model);
    }

    report.emit(&format!(
        "{{\"t\":\"ready\",\"cols\":{},\"rows\":{},\"wt\":{},\"font\":{},\"reduced_motion\":{},\"no_color\":{}}}",
        size.0,
        size.1,
        env::var_os("WT_SESSION").is_some(),
        json_string(&modes::font_face().unwrap_or_default()),
        model.reduced_motion,
        model.no_color
    ))?;

    let idle_draws = idle_window(session, &mut model)?;
    report.emit(&format!(
        "{{\"t\":\"idle\",\"draws\":{idle_draws},\"animation\":{}}}",
        model.pulse
    ))?;

    if phase == Phase::Hosted {
        if let Err(error) = modes::inject_chars("navajo") {
            report.emit(&format!(
                "{{\"t\":\"inject_error\",\"op\":\"keys\",\"error\":{}}}",
                json_string(&error.to_string())
            ))?;
        }
        if let Err(error) = modes::inject_chars("\u{1b}[200~q\u{6771}\u{4eac}\u{1b}[201~") {
            report.emit(&format!(
                "{{\"t\":\"inject_error\",\"op\":\"paste\",\"error\":{}}}",
                json_string(&error.to_string())
            ))?;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last_size = session.size()?;
    loop {
        if phase == Phase::Hosted && Instant::now() >= deadline {
            report.emit("{\"t\":\"hosted_timeout\"}")?;
            break;
        }
        let Some(event) = session.poll(Duration::from_millis(100))? else {
            let observed = session.size()?;
            if observed != last_size {
                note_resize(session, report, &mut model, observed)?;
                last_size = observed;
            }
            continue;
        };
        if let InputEvent::Resize(cols, rows) = &event {
            last_size = (*cols, *rows);
        }
        match apply_event(session, report, &mut model, event)? {
            Step::Quit => break,
            Step::Continue => {}
        }
        if phase == Phase::Hosted && hosted_script_done(&model) {
            break;
        }
    }
    Ok(())
}

fn absorb_startup(session: &mut dyn Session, model: &mut Model) -> MeasureResult<()> {
    let until = Instant::now() + Duration::from_millis(80);
    while Instant::now() < until {
        if let Some(event) = session.poll(Duration::from_millis(20))? {
            let _ = apply_event_quiet(session, model, event)?;
        }
    }
    Ok(())
}

fn idle_window(session: &mut dyn Session, model: &mut Model) -> MeasureResult<u32> {
    let until = Instant::now() + Duration::from_millis(500);
    let mut draws = 0u32;
    while Instant::now() < until {
        let remaining = until.saturating_duration_since(Instant::now());
        let timeout = remaining.min(Duration::from_millis(50));
        if timeout.is_zero() {
            break;
        }
        if let Some(event) = session.poll(timeout)? {
            if matches!(event, InputEvent::Resize(_, _)) {
                let _ = apply_event_quiet(session, model, event)?;
            }
        } else if !model.reduced_motion {
            model.pulse = model.pulse.wrapping_add(1);
            session.draw(model)?;
            draws = draws.saturating_add(1);
        }
    }
    Ok(draws)
}

fn run_contrast(
    session: &mut dyn Session,
    report: &Report,
    model: &mut Model,
) -> MeasureResult<()> {
    let until = Instant::now() + Duration::from_millis(400);
    let mut last = Instant::now() - Duration::from_millis(50);
    let mut draws = 0u32;
    while Instant::now() < until {
        if !model.reduced_motion && last.elapsed() >= Duration::from_millis(50) {
            model.pulse = model.pulse.wrapping_add(1);
            session.draw(model)?;
            draws = draws.saturating_add(1);
            last = Instant::now();
        } else {
            let _ = session.poll(Duration::from_millis(20))?;
        }
    }
    report.emit(&format!(
        "{{\"t\":\"contrast\",\"animation\":{draws},\"reduced_motion\":{}}}",
        model.reduced_motion
    ))?;
    Ok(())
}

fn apply_event(
    session: &mut dyn Session,
    report: &Report,
    model: &mut Model,
    event: InputEvent,
) -> MeasureResult<Step> {
    let started = Instant::now();
    match event {
        InputEvent::Ignore => Ok(Step::Continue),
        InputEvent::Quit => Ok(Step::Quit),
        InputEvent::Char(character) => {
            model.query.push(character);
            session.draw(model)?;
            let latency_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
            report.emit(&format!(
                "{{\"t\":\"key\",\"ch\":{},\"latency_us\":{latency_us},\"query\":{},\"shown\":{}}}",
                json_string(&character.to_string()),
                json_string(&model.query),
                textutil::visible(&model.stations, &model.query).len()
            ))?;
            Ok(Step::Continue)
        }
        InputEvent::Backspace => {
            model.query.pop();
            session.draw(model)?;
            let latency_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
            report.emit(&format!(
                "{{\"t\":\"key\",\"ch\":\"backspace\",\"latency_us\":{latency_us},\"query\":{},\"shown\":{}}}",
                json_string(&model.query),
                textutil::visible(&model.stations, &model.query).len()
            ))?;
            Ok(Step::Continue)
        }
        InputEvent::Paste(text) => {
            model.query.push_str(&text);
            model.pasted.push_str(&text);
            session.draw(model)?;
            report.emit(&format!(
                "{{\"t\":\"paste\",\"text\":{},\"query\":{},\"quit\":false,\"shown\":{}}}",
                json_string(&text),
                json_string(&model.query),
                textutil::visible(&model.stations, &model.query).len()
            ))?;
            Ok(Step::Continue)
        }
        InputEvent::Resize(cols, rows) => {
            session.draw(model)?;
            report.emit(&format!(
                "{{\"t\":\"resize\",\"cols\":{cols},\"rows\":{rows}}}"
            ))?;
            Ok(Step::Continue)
        }
    }
}

fn apply_event_quiet(
    session: &mut dyn Session,
    model: &mut Model,
    event: InputEvent,
) -> MeasureResult<()> {
    match event {
        InputEvent::Char(character) => model.query.push(character),
        InputEvent::Backspace => {
            model.query.pop();
        }
        InputEvent::Paste(ref text) => {
            model.query.push_str(text);
            model.pasted.push_str(text);
        }
        InputEvent::Resize(_, _) | InputEvent::Ignore | InputEvent::Quit => {}
    }
    if !matches!(event, InputEvent::Ignore | InputEvent::Quit) {
        session.draw(model)?;
    }
    Ok(())
}

fn note_resize(
    session: &mut dyn Session,
    report: &Report,
    model: &mut Model,
    observed: (u16, u16),
) -> MeasureResult<()> {
    session.draw(model)?;
    report.emit(&format!(
        "{{\"t\":\"resize\",\"cols\":{},\"rows\":{}}}",
        observed.0, observed.1
    ))?;
    Ok(())
}

fn hosted_script_done(model: &Model) -> bool {
    model.query.contains("navajo") && model.pasted.contains('q') && model.pasted.contains('東')
}

fn facts_line(facts: &TextFacts) -> MeasureResult<String> {
    Ok(format!(
        "{{\"t\":\"text\",\"combining\":{},\"combining_split\":{},\"wide\":{},\"wide_next\":{},\"wide_after\":{},\"combining_ok\":{},\"wide_ok\":{}}}",
        json_string(&facts.combining_symbol),
        facts.combining_split,
        json_string(&facts.wide_symbol),
        json_string(&facts.wide_next),
        json_string(&facts.wide_after),
        facts.combining_ok(),
        facts.wide_ok()
    ))
}
