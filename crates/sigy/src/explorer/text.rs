//! Presentation text. Station names and errors are untrusted.

const CONTROL_REPLACEMENTS: &[char] = &[
    '\u{2028}', '\u{2029}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}',
    '\u{2067}', '\u{2068}', '\u{2069}',
];

#[must_use]
pub fn sanitize(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for character in value.chars() {
        if count >= max_chars {
            break;
        }
        if character.is_control() || CONTROL_REPLACEMENTS.contains(&character) {
            continue;
        }
        out.push(character);
        count = count.saturating_add(1);
    }
    out
}

#[must_use]
pub fn age_label(observed_ms: i64, now_ms: i64) -> String {
    if observed_ms < 0 || now_ms < observed_ms {
        return "unknown".into();
    }
    let seconds = (now_ms - observed_ms) / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

#[must_use]
pub fn known(value: &str) -> &str {
    if value.is_empty() { "unknown" } else { value }
}

#[cfg(test)]
mod tests {
    use super::{age_label, sanitize};

    #[test]
    fn sanitize_drops_terminal_controls_and_limits_length() {
        let cleaned = sanitize("a\u{1b}[31m\nb\u{202e}c", 8);
        assert_eq!(cleaned, "a[31mbc");
        assert!(!cleaned.chars().any(char::is_control));
        assert_eq!(sanitize("navajo station", 6), "navajo");
    }

    #[test]
    fn age_uses_the_supplied_clock() {
        assert_eq!(age_label(1_000, 6_000), "5s");
        assert_eq!(age_label(1_000, 61_000), "1m");
        assert_eq!(age_label(5_000, 1_000), "unknown");
    }
}
