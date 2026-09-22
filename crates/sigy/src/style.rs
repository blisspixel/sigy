//! Semantic terminal color. The words stay authoritative. Color only reinforces them.
//!
//! ANSI names follow the terminal theme, so the same roles work on light and dark screens.
//! A pipe, `NO_COLOR`, and `TERM=dumb` stay plain. `FORCE_COLOR` can request color anyway,
//! except `NO_COLOR` still wins.

use std::env;
use std::io::{self, IsTerminal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Accent,
    Ok,
    Warn,
    Fail,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink {
    enabled: bool,
}

impl Ink {
    #[must_use]
    pub fn stdout(json: bool) -> Self {
        Self {
            enabled: !json && automatic(&io::stdout()),
        }
    }

    #[must_use]
    pub fn stderr() -> Self {
        Self {
            enabled: automatic(&io::stderr()),
        }
    }

    #[must_use]
    pub fn tint(self, tone: Tone, text: &str) -> String {
        if !self.enabled || tone == Tone::Plain {
            return text.to_owned();
        }
        format!("{}{text}\u{1b}[0m", tone.code())
    }
}

impl Tone {
    const fn code(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Accent => "\u{1b}[36m",
            Self::Ok => "\u{1b}[32m",
            Self::Warn => "\u{1b}[33m",
            Self::Fail => "\u{1b}[31m",
            Self::Muted => "\u{1b}[90m",
        }
    }
}

#[must_use]
pub fn tone_for_state(state: &str) -> Tone {
    match state.trim().to_ascii_lowercase().as_str() {
        "ok" | "succeeded" | "completed" | "complete" | "current" | "retained" | "configured"
        | "available" => Tone::Ok,
        "failed" | "blocked" | "disconnected" | "error" | "interrupted" => Tone::Fail,
        "running" | "stopping" | "paused" | "attention" | "unknown" | "reserved" | "frozen"
        | "stale" | "scheduled" => Tone::Warn,
        "unavailable" => Tone::Muted,
        _ => Tone::Plain,
    }
}

#[must_use]
pub fn plain_requested() -> bool {
    no_color() || dumb_terminal()
}

fn automatic(stream: &impl IsTerminal) -> bool {
    if no_color() {
        return false;
    }
    if force_color() {
        return true;
    }
    if dumb_terminal() {
        return false;
    }
    stream.is_terminal()
}

fn no_color() -> bool {
    env_set("NO_COLOR")
}

fn force_color() -> bool {
    match env::var("FORCE_COLOR") {
        Ok(value) => !value.is_empty() && value != "0",
        Err(_) => false,
    }
}

fn dumb_terminal() -> bool {
    env::var("TERM").ok().as_deref() == Some("dumb")
}

fn env_set(name: &str) -> bool {
    env::var(name).is_ok_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{Ink, Tone, tone_for_state};

    #[test]
    fn plain_ink_keeps_the_words() {
        let ink = Ink { enabled: false };
        assert_eq!(ink.tint(Tone::Fail, "blocked"), "blocked");
        assert!(!ink.tint(Tone::Ok, "succeeded").contains('\u{1b}'));
    }

    #[test]
    fn color_marks_a_state_word_and_leaves_plain_text_alone() {
        let ink = Ink { enabled: true };
        let blocked = ink.tint(Tone::Fail, "blocked");
        assert!(blocked.contains("blocked"));
        assert!(blocked.starts_with("\u{1b}[31m"));
        assert!(blocked.ends_with("\u{1b}[0m"));
        assert_eq!(ink.tint(Tone::Plain, "Help: / search"), "Help: / search");
        assert_eq!(tone_for_state("Succeeded"), Tone::Ok);
        assert_eq!(tone_for_state("failed"), Tone::Fail);
        assert_eq!(tone_for_state("running"), Tone::Warn);
        assert_eq!(tone_for_state("favorite"), Tone::Plain);
    }
}
