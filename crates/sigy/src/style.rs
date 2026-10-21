//! Semantic terminal color. The words stay authoritative. Color only reinforces them.
//!
//! One role table maps to ANSI slots. The terminal theme supplies the actual hues, so the
//! same roles read on a light or dark palette. Sigy has no theme of its own. High contrast
//! keeps failure and live color, and draws every other role in the default foreground.

use std::env;
use std::io::{self, IsTerminal};

use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Accent,
    Ok,
    Attention,
    Fail,
    /// Playing or running. ANSI 2, the same slot as [`Tone::Ok`], with its own word.
    Live,
    /// The recording marker. ANSI 9, distinct from failure on ANSI 1.
    Record,
    Favorite,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorEnv {
    pub no_color: bool,
    pub force_color: bool,
    pub dumb: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink {
    enabled: bool,
    high_contrast: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapPalette {
    pub coast: Color,
    pub night: Color,
    pub rim: Color,
    pub station: Color,
    pub selected: Color,
    pub muted: Color,
    pub live: Color,
}

impl Ink {
    #[must_use]
    pub fn stdout(json: bool) -> Self {
        Self::choose(json, ColorChoice::Auto, false)
    }

    #[must_use]
    pub fn stderr() -> Self {
        Self {
            enabled: color_enabled(ColorChoice::Auto, io::stderr().is_terminal(), current_env()),
            high_contrast: env_set("SIGY_HIGH_CONTRAST"),
        }
    }

    #[must_use]
    pub fn choose(json: bool, color: ColorChoice, high_contrast: bool) -> Self {
        Self {
            enabled: !json && color_enabled(color, io::stdout().is_terminal(), current_env()),
            high_contrast: !json && (high_contrast || env_set("SIGY_HIGH_CONTRAST")),
        }
    }

    #[must_use]
    pub fn tint(self, tone: Tone, text: &str) -> String {
        let Some(code) = self.code(tone) else {
            return text.to_owned();
        };
        format!("{code}{text}\u{1b}[0m")
    }

    fn code(self, tone: Tone) -> Option<&'static str> {
        if tone == Tone::Plain {
            return None;
        }
        if self.high_contrast {
            return high_contrast_code(tone, self.enabled);
        }
        if !self.enabled {
            return None;
        }
        Some(tone.code())
    }
}

impl Tone {
    const fn code(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Accent => "\u{1b}[1;36m",
            Self::Ok | Self::Live => "\u{1b}[32m",
            Self::Attention => "\u{1b}[1;33m",
            Self::Fail => "\u{1b}[31m",
            Self::Record => "\u{1b}[91m",
            Self::Favorite => "\u{1b}[94m",
            Self::Muted => "\u{1b}[90m",
        }
    }

    /// ANSI slot, when the role uses one. `None` is the terminal's default foreground.
    #[must_use]
    pub const fn ansi_slot(self) -> Option<u8> {
        match self {
            Self::Plain => None,
            Self::Fail => Some(1),
            Self::Ok | Self::Live => Some(2),
            Self::Attention => Some(3),
            Self::Accent => Some(6),
            Self::Muted => Some(8),
            Self::Record => Some(9),
            Self::Favorite => Some(12),
        }
    }
}

/// Ratatui color for one role. Product code outside this module does not name a `Color`.
#[must_use]
pub const fn role_color(tone: Tone) -> Color {
    match tone.ansi_slot() {
        Some(1) => Color::Red,
        Some(2) => Color::Green,
        Some(3) => Color::Yellow,
        Some(6) => Color::Cyan,
        Some(8) => Color::DarkGray,
        Some(9) => Color::LightRed,
        Some(12) => Color::LightBlue,
        None | Some(0 | 4 | 5 | 7 | 10 | 11 | 13..=u8::MAX) => Color::Reset,
    }
}

/// Globe canvas only. The rim is muted, not ANSI 7. Stations are not yellow.
#[must_use]
pub const fn map_palette(color: bool) -> MapPalette {
    if color {
        MapPalette {
            coast: Color::Green,
            night: Color::DarkGray,
            rim: Color::DarkGray,
            station: Color::Reset,
            selected: Color::Cyan,
            muted: role_color(Tone::Muted),
            live: role_color(Tone::Live),
        }
    } else {
        MapPalette {
            coast: Color::Reset,
            night: Color::Reset,
            rim: Color::Reset,
            station: Color::Reset,
            selected: Color::Reset,
            muted: Color::Reset,
            live: Color::Reset,
        }
    }
}

#[must_use]
pub fn tone_for_state(state: &str) -> Tone {
    match state.trim().to_ascii_lowercase().as_str() {
        "ok" | "succeeded" | "completed" | "complete" | "current" | "retained" | "configured"
        | "available" => Tone::Ok,
        "failed" | "blocked" | "disconnected" | "error" | "interrupted" => Tone::Fail,
        "running" | "playing" | "recording" => Tone::Live,
        "paused" | "attention" | "stale" | "frozen" | "stopping" | "reserved" => Tone::Attention,
        "unknown" | "unavailable" => Tone::Muted,
        // `scheduled` and `queued` stay plain text. They are not attention.
        _ => Tone::Plain,
    }
}

#[must_use]
pub fn plain_requested() -> bool {
    let env = current_env();
    env.no_color || env.dumb
}

#[must_use]
pub fn color_enabled(choice: ColorChoice, terminal: bool, env: ColorEnv) -> bool {
    match choice {
        ColorChoice::Never => false,
        ColorChoice::Always => true,
        ColorChoice::Auto => {
            if env.no_color {
                return false;
            }
            if env.force_color {
                return true;
            }
            !env.dumb && terminal
        }
    }
}

fn high_contrast_code(tone: Tone, color: bool) -> Option<&'static str> {
    if !color {
        return match tone {
            Tone::Plain | Tone::Muted => None,
            _ => Some("\u{1b}[1m"),
        };
    }
    Some(match tone {
        Tone::Fail => "\u{1b}[1;31m",
        Tone::Live => "\u{1b}[1;32m",
        Tone::Record => "\u{1b}[1;91m",
        Tone::Plain | Tone::Muted => return None,
        Tone::Accent | Tone::Attention | Tone::Ok | Tone::Favorite => "\u{1b}[1m",
    })
}

fn current_env() -> ColorEnv {
    ColorEnv {
        no_color: env_set("NO_COLOR"),
        force_color: match env::var("FORCE_COLOR") {
            Ok(value) => !value.is_empty() && value != "0",
            Err(_) => false,
        },
        dumb: env::var("TERM").ok().as_deref() == Some("dumb"),
    }
}

pub(crate) fn env_set(name: &str) -> bool {
    env::var(name).is_ok_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{ColorChoice, ColorEnv, Ink, Tone, color_enabled, tone_for_state};

    #[test]
    fn plain_ink_keeps_the_words() {
        let ink = Ink {
            enabled: false,
            high_contrast: false,
        };
        assert_eq!(ink.tint(Tone::Fail, "blocked"), "blocked");
        assert!(!ink.tint(Tone::Ok, "succeeded").contains('\u{1b}'));
    }

    #[test]
    fn color_marks_a_state_word_and_leaves_plain_text_alone() {
        let ink = Ink {
            enabled: true,
            high_contrast: false,
        };
        let blocked = ink.tint(Tone::Fail, "blocked");
        assert!(blocked.contains("blocked"));
        assert!(blocked.starts_with("\u{1b}[31m"));
        assert!(blocked.ends_with("\u{1b}[0m"));
        assert!(
            ink.tint(Tone::Attention, "stale")
                .starts_with("\u{1b}[1;33m")
        );
        assert_eq!(ink.tint(Tone::Plain, "Help: / search"), "Help: / search");
    }

    #[test]
    fn state_words_use_one_role_each() {
        for (word, tone) in [
            ("ok", Tone::Ok),
            ("succeeded", Tone::Ok),
            ("completed", Tone::Ok),
            ("complete", Tone::Ok),
            ("current", Tone::Ok),
            ("retained", Tone::Ok),
            ("configured", Tone::Ok),
            ("available", Tone::Ok),
            ("failed", Tone::Fail),
            ("blocked", Tone::Fail),
            ("disconnected", Tone::Fail),
            ("error", Tone::Fail),
            ("interrupted", Tone::Fail),
            ("running", Tone::Live),
            ("playing", Tone::Live),
            ("recording", Tone::Live),
            ("paused", Tone::Attention),
            ("stale", Tone::Attention),
            ("frozen", Tone::Attention),
            ("stopping", Tone::Attention),
            ("reserved", Tone::Attention),
            ("unknown", Tone::Muted),
            ("unavailable", Tone::Muted),
            ("scheduled", Tone::Plain),
            ("queued", Tone::Plain),
            ("favorite", Tone::Plain),
        ] {
            assert_eq!(tone_for_state(word), tone, "{word}");
            assert_eq!(tone_for_state(&word.to_ascii_uppercase()), tone, "{word}");
        }
    }

    #[test]
    fn roles_avoid_black_blue_gray_white_and_magenta() {
        for tone in [
            Tone::Plain,
            Tone::Accent,
            Tone::Ok,
            Tone::Attention,
            Tone::Fail,
            Tone::Live,
            Tone::Record,
            Tone::Favorite,
            Tone::Muted,
        ] {
            if let Some(slot) = tone.ansi_slot() {
                assert!(
                    !matches!(slot, 0 | 4 | 5 | 7 | 13 | 15),
                    "{tone:?} uses ANSI {slot}"
                );
            }
            let code = tone.code();
            assert!(!code.contains("[30m") && !code.contains(";30m"), "{tone:?}");
            assert!(!code.contains("[35m") && !code.contains("[95m"), "{tone:?}");
            assert!(!code.contains("[37m") && !code.contains("[97m"), "{tone:?}");
        }
    }

    #[test]
    fn color_choice_overrides_are_explicit() {
        let plain = ColorEnv {
            no_color: false,
            force_color: false,
            dumb: false,
        };
        let no_color = ColorEnv {
            no_color: true,
            ..plain
        };
        let forced = ColorEnv {
            force_color: true,
            ..plain
        };
        assert!(!color_enabled(ColorChoice::Never, true, forced));
        assert!(color_enabled(ColorChoice::Always, false, no_color));
        assert!(!color_enabled(ColorChoice::Auto, true, no_color));
        assert!(!color_enabled(ColorChoice::Auto, false, plain));
        assert!(color_enabled(ColorChoice::Auto, false, forced));
        assert!(!color_enabled(
            ColorChoice::Auto,
            true,
            ColorEnv {
                dumb: true,
                ..plain
            }
        ));
        assert!(color_enabled(
            ColorChoice::Auto,
            false,
            ColorEnv {
                force_color: true,
                dumb: true,
                no_color: false
            }
        ));
        assert!(!color_enabled(
            ColorChoice::Auto,
            true,
            ColorEnv {
                no_color: true,
                force_color: true,
                dumb: false
            }
        ));
    }

    #[test]
    fn high_contrast_keeps_failure_and_live_color_and_bolds_the_rest() {
        let ink = Ink {
            enabled: true,
            high_contrast: true,
        };
        assert!(ink.tint(Tone::Fail, "blocked").starts_with("\u{1b}[1;31m"));
        assert!(ink.tint(Tone::Live, "playing").starts_with("\u{1b}[1;32m"));
        assert!(ink.tint(Tone::Record, "REC").starts_with("\u{1b}[1;91m"));
        assert!(ink.tint(Tone::Favorite, "★").starts_with("\u{1b}[1m"));
        assert!(!ink.tint(Tone::Favorite, "★").contains("[94m"));
        assert_eq!(ink.tint(Tone::Plain, "Cache"), "Cache");
        let mono = Ink {
            enabled: false,
            high_contrast: true,
        };
        assert_eq!(
            mono.tint(Tone::Fail, "blocked"),
            "\u{1b}[1mblocked\u{1b}[0m"
        );
    }

    #[test]
    fn never_emits_no_sgr_and_always_colors_without_a_terminal() {
        let blocked = ColorEnv {
            no_color: true,
            force_color: true,
            dumb: true,
        };
        let never = Ink {
            enabled: color_enabled(ColorChoice::Never, false, blocked),
            high_contrast: false,
        };
        let always = Ink {
            enabled: color_enabled(ColorChoice::Always, false, blocked),
            high_contrast: false,
        };
        assert!(!never.tint(Tone::Fail, "blocked").contains('\u{1b}'));
        assert!(always.tint(Tone::Fail, "blocked").starts_with("\u{1b}[31m"));
        assert!(always.tint(Tone::Live, "playing").starts_with("\u{1b}[32m"));
    }

    #[test]
    fn product_sources_name_colors_only_in_the_role_table() -> Result<(), std::io::Error> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut pending = vec![root];
        let mut offenders = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(&directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                let in_tests = name == "tests.rs"
                    || name.ends_with("_tests.rs")
                    || path
                        .components()
                        .any(|component| component.as_os_str() == "tests");
                if name == "style.rs" || in_tests {
                    continue;
                }
                let source = std::fs::read_to_string(&path)?;
                let product = source
                    .split("\n#[cfg(test)]")
                    .next()
                    .unwrap_or(source.as_str());
                if product.contains("Color::") || product.contains("\\u{1b}[") {
                    offenders.push(path.display().to_string());
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "product sources name a color outside the role table: {offenders:?}"
        );
        Ok(())
    }
}
