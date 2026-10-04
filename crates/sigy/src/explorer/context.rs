//! Original-script cached source context. No station request or playback authority.

use std::cell::Cell;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use sigy_service::{control::DirectoryCatalog, discovery::Station};

use super::{
    state::{Focus, PlaybackView, StationRow},
    text::{age_label, known, sanitize},
};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct StationContext {
    pub row: StationRow,
    pub catalog: Option<DirectoryCatalog>,
    pub return_focus: Focus,
    library: String,
    playback: Option<PlaybackView>,
    offset: usize,
    maximum_offset: Cell<usize>,
}

impl StationContext {
    pub fn new(
        row: StationRow,
        catalog: Option<DirectoryCatalog>,
        return_focus: Focus,
        library: String,
        playback: Option<PlaybackView>,
    ) -> Self {
        Self {
            row,
            catalog,
            return_focus,
            library,
            playback,
            offset: 0,
            maximum_offset: Cell::new(0),
        }
    }

    pub fn scroll(&mut self, forward: bool) {
        let current = self.offset.min(self.maximum_offset.get());
        self.offset = if forward {
            current.saturating_add(1).min(self.maximum_offset.get())
        } else {
            current.saturating_sub(1)
        };
    }

    pub fn visible(&self, width: usize, height: usize, now_ms: i64) -> Vec<Line<'static>> {
        let rows = wrap(self.details(now_ms), width.max(2));
        let maximum = rows.len().saturating_sub(height.max(1));
        self.maximum_offset.set(maximum);
        rows.into_iter()
            .skip(self.offset.min(maximum))
            .take(height)
            .map(Line::raw)
            .collect()
    }

    pub fn position(&self) -> String {
        format!(
            "row {} / {}",
            self.offset.min(self.maximum_offset.get()) + 1,
            self.maximum_offset.get() + 1
        )
    }

    fn details(&self, now_ms: i64) -> Vec<String> {
        let mut lines = vec![
            format!(
                "Name: {}",
                sanitize(
                    self.row
                        .metadata
                        .as_ref()
                        .map_or(self.row.name.as_str(), |station| station.name.as_str()),
                    256
                )
            ),
            format!(
                "Directory check: {} | observation age {}",
                self.row.directory_health.label(),
                age_label(self.row.observed_ms, now_ms)
            ),
            "Directory labels describe the listing, not measured reception or speech.".into(),
        ];
        if let Some(station) = &self.row.metadata {
            metadata_lines(&mut lines, station);
        } else {
            lines.push("Full cached metadata unavailable; this is the page preview.".into());
            lines.push(format!(
                "Directory languages: {}",
                known(&self.row.directory_languages)
            ));
        }
        lines.extend([
            String::new(),
            "Audio: inspection only. No player or station connection was started.".into(),
            "Recognized language and captions: not inspected.".into(),
            "Registered revisions and recordings: not resolved for this station.".into(),
            "Matching a name or URL would not establish that relationship.".into(),
            String::new(),
            self.library.clone(),
        ]);
        if let Some(playback) = &self.playback {
            lines.extend([
                format!("Library play-session snapshot: {} | {}", sanitize(&playback.id, 64), sanitize(&playback.state, 32)),
                format!("Session source revision: {}", sanitize(&playback.source_revision, 64)),
                "This session is not linked to this station; its metadata does not prove audible output.".into(),
            ]);
        } else {
            lines.push("Library play-session snapshot: none.".into());
        }
        lines.extend([
            String::new(),
            inspection_command(&self.row.id),
            "Use the same library's --data-dir when it is explicitly selected.".into(),
            "Esc returns to the same search and selected identity. Quit leaves background work running.".into(),
        ]);
        lines
    }
}

fn inspection_command(id: &str) -> String {
    let uuid = id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        });
    if uuid {
        format!("CLI inspection: sigy radio show {id}")
    } else {
        "CLI inspection unavailable: malformed station identity.".into()
    }
}

fn metadata_lines(lines: &mut Vec<String>, station: &Station) {
    lines.extend([
        format!(
            "Country: {} | region: {}",
            known(&station.country),
            known(&sanitize(&station.state, 128))
        ),
        format!(
            "Codec: {} | bitrate: {}",
            known(&sanitize(&station.codec, 32)),
            if station.bitrate_kbps == 0 {
                "unknown".into()
            } else {
                format!("{} kbit/s (directory)", station.bitrate_kbps)
            }
        ),
        format!(
            "Stream listing: {}",
            if station.hls {
                "HLS"
            } else {
                "audio; transport not qualified here"
            }
        ),
    ]);
    for (label, values) in [
        ("Languages", &station.languages),
        ("Language codes", &station.language_codes),
        ("Tags", &station.tags),
    ] {
        if values.is_empty() {
            lines.push(format!("{label}: unknown"));
        } else {
            for value in values.iter().take(32) {
                lines.push(format!("{label}: {}", known(&sanitize(value, 128))));
            }
        }
    }
    lines.push(match (station.latitude, station.longitude) {
        (Some(lat), Some(lon)) => format!(
            "Directory coordinates: {lat:.5}, {lon:.5}; not a measured transmitter position."
        ),
        _ => "Directory coordinates: unknown.".into(),
    });
    lines.push(format!(
        "Listed stream origin: {}",
        sanitize(&station.stream_origin, 2048)
    ));
    lines.push(format!(
        "Cached refresh: {}",
        sanitize(&station.refresh_id, 128)
    ));
}

/// Hard wrap preserves original grapheme clusters, including combining script.
pub(super) fn wrap(lines: Vec<String>, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for text in lines {
        let line = Line::raw(text);
        let mut row = String::new();
        let mut cells = 0;
        for grapheme in line.styled_graphemes(Style::default()) {
            let next = Span::raw(grapheme.symbol).width();
            if cells + next > width && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                cells = 0;
            }
            row.push_str(grapheme.symbol);
            cells += next;
        }
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::state::Health;

    fn context() -> StationContext {
        StationContext::new(
            StationRow {
                id: "00000000-0000-4000-8000-000000000001".into(),
                name: "preview".into(),
                favorite: false,
                directory_health: Health::Unknown,
                directory_languages: String::new(),
                observed_ms: 1000,
                hls: true,
                coordinates: None,
                metadata: Some(Station {
                    provider: "radio_browser".into(),
                    id: "00000000-0000-4000-8000-000000000001".into(),
                    name: format!("{}Cafe\u{301} 東京 العربية", "n".repeat(180)),
                    country: "CA".into(),
                    state: String::new(),
                    languages: vec![String::new()],
                    language_codes: Vec::new(),
                    tags: (0..32).map(|i| format!("tag-{i}")).collect(),
                    codec: String::new(),
                    bitrate_kbps: 0,
                    hls: true,
                    last_check_ok: None,
                    latitude: None,
                    longitude: None,
                    stream_origin: "https://example.invalid".into(),
                    observed_ms: 1000,
                    refresh_id: "fixture".into(),
                }),
            },
            None,
            Focus::Results,
            "Library capture snapshot: 0".into(),
            Some(PlaybackView {
                id: "unrelated-playback".into(),
                state: "running".into(),
                source_revision: "unrelated-source".into(),
                format: None,
                failure: None,
            }),
        )
    }

    #[test]
    fn metadata_is_full_bounded_unknown_and_separate_from_library_activity() {
        let context = context();
        let text = context.details(2000).join("\n");
        assert!(text.contains(&"n".repeat(180)));
        assert!(text.contains("Languages: unknown") && text.contains("Language codes: unknown"));
        assert!(text.contains("Codec: unknown | bitrate: unknown"));
        assert!(text.contains("HLS") && text.contains("Directory coordinates: unknown"));
        assert!(text.contains("unrelated-source") && text.contains("not linked to this station"));
        assert!(text.contains("Tags: tag-31"));
        assert_eq!(
            context
                .row
                .metadata
                .as_ref()
                .map(|s| s.languages[0].as_str()),
            Some("")
        );
    }

    #[test]
    fn wrapped_graphemes_match_independent_expected_rows_and_resize_remains_reachable() {
        assert_eq!(
            wrap(vec!["ABe\u{301}中क्\u{200d}षC".into()], 3),
            ["ABe\u{301}", "中", "क्\u{200d}षC"]
        );
        let mut context = context();
        let original = context.details(2000).join("");
        let rows = context.visible(20, 10_000, 2000);
        assert_eq!(
            rows.iter().map(ToString::to_string).collect::<String>(),
            original
        );
        context.visible(20, 1, 2000);
        for _ in 0..10_000 {
            context.scroll(true);
        }
        assert!(
            context.visible(20, 1, 2000)[0]
                .to_string()
                .contains("running")
        );
        let resized = context.visible(132, 10, 2000);
        assert!(!resized.is_empty());
        context.scroll(false);
        assert!(!context.visible(2, 1, 2000).is_empty());
    }

    #[test]
    fn unsafe_display_characters_never_survive_context_rendering() {
        let mut context = context();
        let Some(station) = &mut context.row.metadata else {
            panic!("metadata");
        };
        station.name = "clean\u{1b}[31m\n\u{202e}name".into();
        station.tags = vec!["tag\u{0007}\u{2066}end".into()];
        station.latitude = Some(45.5);
        station.longitude = Some(-73.5);
        station.bitrate_kbps = 128;
        station.hls = false;
        context.row.id = "bad; command\u{1b}[2J\n".into();
        let text = context.details(500).join(" ");
        assert!(!text.chars().any(char::is_control));
        assert!(!text.contains('\u{202e}') && !text.contains('\u{2066}'));
        assert!(text.contains("45.50000, -73.50000") && text.contains("128 kbit/s"));
        assert!(text.contains("observation age unknown"));
        assert!(text.contains("CLI inspection unavailable"));
        assert!(!text.contains("bad; command"));
        context.row.metadata = None;
        context.playback = None;
        assert!(context.details(2000).join(" ").contains("snapshot: none"));
    }
}
