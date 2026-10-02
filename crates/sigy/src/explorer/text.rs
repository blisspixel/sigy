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

/// Decimal size for people, rounded down to one decimal place. The quota is set in GB.
#[must_use]
pub fn bytes_label(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["kB", "MB", "GB", "TB", "PB", "EB"];
    if bytes < 1_000 {
        return format!("{bytes} B");
    }
    let mut scale: u64 = 1_000;
    let mut unit = 0;
    while unit + 1 < UNITS.len() && bytes / scale >= 1_000 {
        scale = scale.saturating_mul(1_000);
        unit += 1;
    }
    let tenths = u128::from(bytes) * 10 / u128::from(scale);
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::{age_label, bytes_label, sanitize};

    #[test]
    fn byte_labels_round_down_in_decimal_units() {
        for (bytes, label) in [
            (0, "0 B"),
            (999, "999 B"),
            (1_000, "1.0 kB"),
            (268_435_456, "268.4 MB"),
            (49_999_999_999, "49.9 GB"),
            (50_000_000_000, "50.0 GB"),
            (9_007_199_254_740_993, "9.0 PB"),
            (u64::MAX, "18.4 EB"),
        ] {
            assert_eq!(bytes_label(bytes), label);
        }
    }

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
