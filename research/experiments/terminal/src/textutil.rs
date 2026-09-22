//! List fixture, frame scan, and escape checks for the terminal measurement.
//!
//! The fixture is local text. It does not open a catalog or a network connection.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFacts {
    pub combining_symbol: String,
    pub combining_split: bool,
    pub wide_symbol: String,
    pub wide_next: String,
    pub wide_after: String,
}

impl TextFacts {
    pub fn combining_ok(&self) -> bool {
        self.combining_symbol.contains('\u{0301}') && !self.combining_split
    }

    pub fn wide_ok(&self) -> bool {
        // A reset wide-character spacer has no symbol. `Cell::symbol` reports that as a space.
        let spacer = self.wide_next.is_empty() || self.wide_next == " ";
        self.wide_symbol.contains('東') && spacer && self.wide_after.contains('京')
    }
}

pub const COMBINING_NAME: &str = "Cafe\u{0301} Norte";
pub const WIDE_NAME: &str = "\u{6771}\u{4eac} FM";

pub fn stations() -> Vec<String> {
    let mut names = vec![
        COMBINING_NAME.to_string(),
        WIDE_NAME.to_string(),
        "Navajo Voice".to_string(),
        "Canyon Navigation".to_string(),
        "Klingon Opera".to_string(),
    ];
    for index in 1..=20 {
        names.push(format!("Station {index:02}"));
    }
    names
}

pub fn visible<'a>(stations: &'a [String], query: &str) -> Vec<&'a str> {
    let needle = query.to_lowercase();
    stations
        .iter()
        .filter(|name| name.to_lowercase().contains(&needle))
        .map(String::as_str)
        .collect()
}

pub fn scan_cells<'a, F>(width: u16, height: u16, mut cell_at: F) -> TextFacts
where
    F: FnMut(u16, u16) -> &'a str,
{
    let mut facts = TextFacts {
        combining_symbol: String::new(),
        combining_split: false,
        wide_symbol: String::new(),
        wide_next: String::new(),
        wide_after: String::new(),
    };
    for y in 0..height {
        for x in 0..width {
            let symbol = cell_at(x, y);
            if symbol == "\u{0301}" {
                facts.combining_split = true;
            }
            if symbol.contains('\u{0301}') && facts.combining_symbol.is_empty() {
                facts.combining_symbol = symbol.to_string();
            }
            if symbol.contains('東') && facts.wide_symbol.is_empty() {
                facts.wide_symbol = symbol.to_string();
                facts.wide_next = if x + 1 < width {
                    cell_at(x + 1, y).to_string()
                } else {
                    String::from("edge")
                };
                facts.wide_after = if x + 2 < width {
                    cell_at(x + 2, y).to_string()
                } else {
                    String::from("edge")
                };
            }
        }
    }
    facts
}

pub fn has_color_sgr(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == 0x1b && bytes[index + 1] == b'[' {
            let start = index + 2;
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_digit() || bytes[end] == b';' || bytes[end] == b':')
            {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'm' && sgr_sets_color(&bytes[start..end]) {
                return true;
            }
            index = end.saturating_add(1);
        } else {
            index += 1;
        }
    }
    false
}

fn sgr_sets_color(params: &[u8]) -> bool {
    if params.is_empty() {
        return false;
    }
    for piece in params.split(|byte| *byte == b';' || *byte == b':') {
        if piece.is_empty() {
            continue;
        }
        let Ok(text) = std::str::from_utf8(piece) else {
            continue;
        };
        let Ok(value) = text.parse::<u16>() else {
            continue;
        };
        if matches!(
            value,
            38 | 48 | 39 | 49 | 30..=37 | 40..=47 | 90..=97 | 100..=107
        ) {
            return true;
        }
    }
    false
}

pub fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                out.push_str(&format!("\\u{:04x}", u32::from(other)));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{COMBINING_NAME, WIDE_NAME, has_color_sgr, scan_cells, stations, visible};

    #[test]
    fn search_keeps_combining_and_wide_rows_until_filtered() {
        let rows = stations();
        assert_eq!(visible(&rows, "").len(), rows.len());
        assert_eq!(
            visible(&rows, "nav"),
            vec!["Navajo Voice", "Canyon Navigation"]
        );
        assert_eq!(visible(&rows, "東"), vec![WIDE_NAME]);
        assert!(
            visible(&rows, "cafe")
                .iter()
                .any(|name| *name == COMBINING_NAME)
        );
    }

    #[test]
    fn cell_scan_accepts_one_combining_cell_and_a_wide_spacer() {
        let cells = ["Cafe\u{0301}", "", "東", " ", "京", ""];
        let facts = scan_cells(6, 1, |x, y| {
            assert_eq!(y, 0);
            cells[usize::from(x)]
        });
        assert!(facts.combining_ok());
        assert!(facts.wide_ok());
    }

    #[test]
    fn cell_scan_flags_a_split_combining_mark() {
        let facts = scan_cells(1, 1, |_, _| "\u{0301}");
        assert!(facts.combining_split);
        assert!(!facts.combining_ok());
    }

    #[test]
    fn color_sgr_ignores_reset_bold_and_reverse() {
        assert!(!has_color_sgr(b"plain \x1b[0m \x1b[1m \x1b[7m"));
        assert!(has_color_sgr(b"\x1b[33m"));
        assert!(has_color_sgr(b"\x1b[38;5;220m"));
        assert!(has_color_sgr(b"\x1b[48:2:1:2:3m"));
        assert!(!has_color_sgr("Cafe\u{0301} 東京".as_bytes()));
    }
}
