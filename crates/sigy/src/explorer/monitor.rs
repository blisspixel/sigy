//! Bounded, read-only monitor navigation and evidence summaries.

use sigy_service::monitor::{MonitorCoverage, MonitorMatches, MonitorView};
use std::cell::Cell;

use super::{state::Effect, text::sanitize, timeline::clock_label};

#[cfg(test)]
pub(super) use tests::{fixture, passage_fixture};

/// Coverage or passages on an open topic. The list is neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MonitorTab {
    #[default]
    Coverage,
    Passages,
}

impl MonitorTab {
    #[must_use]
    pub const fn switch(self) -> Self {
        match self {
            Self::Coverage => Self::Passages,
            Self::Passages => Self::Coverage,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TopicSummary {
    name: String,
    version: u32,
    paused: bool,
    passages: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct TopicDetail {
    monitor: MonitorView,
    coverage: MonitorCoverage,
    matches: MonitorMatches,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Browser {
    ids: Vec<String>,
    selected: usize,
    summaries: Vec<(String, TopicSummary)>,
    stored: Option<TopicDetail>,
    tab: MonitorTab,
    passage: usize,
    passage_open: bool,
    offset: usize,
    showing_detail: bool,
    list_read: bool,
    observed_ms: Option<i64>,
    viewport_width: Cell<usize>,
}

impl Browser {
    pub fn selected(&self) -> Option<&str> {
        self.ids.get(self.selected).map(String::as_str)
    }

    pub fn load_ids(&mut self, ids: Vec<String>) {
        let previous = self.selected().map(str::to_owned);
        self.ids = ids.into_iter().take(256).collect();
        self.selected = previous
            .and_then(|id| self.ids.iter().position(|item| *item == id))
            .unwrap_or(0);
        if self
            .stored
            .as_ref()
            .map(|detail| detail.monitor.id.as_str())
            != self.selected()
        {
            self.stored = None;
            self.observed_ms = None;
        }
        self.showing_detail = false;
        self.passage_open = false;
        self.list_read = true;
        self.offset = 0;
    }

    pub fn move_selection(&mut self, forward: bool) {
        if self.passage_open {
            let length = self.wrapped_detail().len();
            self.offset = step(self.offset, length, forward);
            return;
        }
        if self.showing_detail && self.tab == MonitorTab::Passages {
            let length = self
                .stored
                .as_ref()
                .map_or(0, |detail| detail.matches.matches.len());
            self.passage = step(self.passage, length, forward);
            return;
        }
        if self.showing_detail {
            let length = self.wrapped_detail().len();
            self.offset = step(self.offset, length, forward);
            return;
        }
        self.selected = step(self.selected, self.ids.len(), forward);
    }

    /// Leaves one level: a passage, then the tabs, then the list.
    pub fn leave_detail(&mut self) -> bool {
        if self.passage_open {
            self.passage_open = false;
            self.offset = 0;
            return true;
        }
        if !self.showing_detail {
            return false;
        }
        self.showing_detail = false;
        self.offset = 0;
        true
    }

    /// Drops an open topic when the workspace changes. The list summaries stay.
    pub fn close_detail(&mut self) {
        self.showing_detail = false;
        self.passage_open = false;
        self.offset = 0;
    }

    #[must_use]
    pub const fn showing_detail(&self) -> bool {
        self.showing_detail
    }

    #[must_use]
    pub const fn showing_passage_detail(&self) -> bool {
        self.passage_open
    }

    #[must_use]
    pub const fn active_tab(&self) -> MonitorTab {
        self.tab
    }

    pub fn switch_tab(&mut self) {
        if self.showing_detail && !self.passage_open {
            self.tab = self.tab.switch();
            self.offset = 0;
        }
    }

    /// Opens the selected passage. `false` when the topic has no passage.
    pub fn open_passage(&mut self) -> bool {
        let Some(count) = self
            .stored
            .as_ref()
            .map(|detail| detail.matches.matches.len())
        else {
            return false;
        };
        if count == 0 {
            return false;
        }
        self.tab = MonitorTab::Passages;
        self.showing_detail = true;
        self.passage = self.passage.min(count - 1);
        self.passage_open = true;
        self.offset = 0;
        true
    }

    #[must_use]
    pub fn selected_passage_recording(&self) -> Option<&str> {
        let detail = self.stored.as_ref()?;
        let passage = detail.matches.matches.get(self.passage)?;
        if passage.recording_id.is_empty() {
            None
        } else {
            Some(passage.recording_id.as_str())
        }
    }

    #[cfg(test)]
    pub fn back(&mut self) {
        let _ = self.leave_detail();
    }

    /// Monitor ids from the latest applied list. Zero can be an empty read.
    #[must_use]
    pub const fn listed(&self) -> usize {
        self.ids.len()
    }

    /// A list read has been applied. An empty browser before that is unread.
    #[must_use]
    pub const fn list_read(&self) -> bool {
        self.list_read
    }

    pub fn reload(&self) -> Effect {
        if self.showing_detail {
            self.open()
        } else {
            Effect::MonitorList
        }
    }

    pub fn open(&self) -> Effect {
        self.selected()
            .map_or(Effect::None, |id| Effect::MonitorDetail {
                id: id.to_owned(),
            })
    }

    pub fn load_detail(
        &mut self,
        monitor: &MonitorView,
        coverage: &MonitorCoverage,
        matches: &MonitorMatches,
    ) -> bool {
        if self.selected() != Some(monitor.id.as_str())
            || coverage.id != monitor.id
            || matches.id != monitor.id
            || coverage.version != monitor.version.version
            || matches.version != monitor.version.version
            || coverage.from_ms != matches.from_ms
            || coverage.to_ms != matches.to_ms
        {
            return false;
        }
        let name = if monitor.version.spec.name.is_empty() {
            monitor.id.clone()
        } else {
            monitor.version.spec.name.clone()
        };
        let summary = TopicSummary {
            name,
            version: monitor.version.version,
            paused: monitor.paused,
            passages: matches.matches.len(),
        };
        if let Some(existing) = self.summaries.iter_mut().find(|(id, _)| id == &monitor.id) {
            existing.1 = summary;
        } else {
            self.summaries.push((monitor.id.clone(), summary));
        }
        self.stored = Some(TopicDetail {
            monitor: monitor.clone(),
            coverage: coverage.clone(),
            matches: matches.clone(),
        });
        self.tab = MonitorTab::Coverage;
        self.passage = 0;
        self.passage_open = false;
        self.showing_detail = true;
        self.observed_ms = Some(coverage.to_ms);
        self.offset = 0;
        true
    }

    pub fn snapshot_label(&self) -> String {
        self.observed_ms.map_or_else(
            || "not read".into(),
            |unix_ms| {
                let minute = super::globe::utc_label(unix_ms);
                format!(
                    "{}:{:02}.{:03} UTC",
                    minute.trim_end_matches(" UTC"),
                    unix_ms.div_euclid(1000).rem_euclid(60),
                    unix_ms.rem_euclid(1000)
                )
            },
        )
    }

    pub fn lines(&self, limit: usize) -> Vec<String> {
        if self.showing_detail {
            let rows = self.wrapped_detail();
            let offset = self.offset.min(rows.len().saturating_sub(1));
            return rows.into_iter().skip(offset).take(limit).collect();
        }
        let mut lines = vec![
            "Watch topics: arrows select, Enter opens, Tab switches Coverage and Passages.".into(),
        ];
        if self.ids.is_empty() {
            lines.push("No monitors. Create one with sigy monitor create.".into());
        }
        let start = self.selected.saturating_sub(limit.saturating_sub(2));
        lines.extend(
            self.ids
                .iter()
                .enumerate()
                .skip(start)
                .take(limit.saturating_sub(1))
                .map(|(index, id)| self.list_row(index, id)),
        );
        lines
    }

    fn list_row(&self, index: usize, id: &str) -> String {
        let marker = if index == self.selected { ">" } else { " " };
        let Some(summary) = self
            .summaries
            .iter()
            .find(|(stored, _)| stored == id)
            .map(|(_, summary)| summary)
        else {
            return format!("{marker} {}", sanitize(id, 64));
        };
        let state = if summary.paused { "paused" } else { "active" };
        let passages = if summary.passages == 1 {
            "1 passage".to_owned()
        } else {
            format!("{} passages", summary.passages)
        };
        format!(
            "{marker} {}  v{}  {state}  {passages}",
            sanitize(&summary.name, 48),
            summary.version
        )
    }

    pub fn lines_at_width(&self, limit: usize, width: usize) -> Vec<String> {
        self.viewport_width.set(width.max(2));
        self.lines(limit)
    }

    fn wrapped_detail(&self) -> Vec<String> {
        let width = self.viewport_width.get();
        let rows = self.detail_rows();
        crate::explorer::context::wrap(rows, if width == 0 { usize::MAX } else { width })
    }

    fn detail_rows(&self) -> Vec<String> {
        let Some(detail) = &self.stored else {
            return vec!["Topic detail is unread. Enter loads it.".into()];
        };
        if self.passage_open {
            return passage_detail(detail, self.passage);
        }
        match self.tab {
            MonitorTab::Coverage => coverage_rows(detail),
            MonitorTab::Passages => passage_rows(detail, self.passage),
        }
    }
}

fn step(position: usize, length: usize, forward: bool) -> usize {
    if length == 0 {
        return 0;
    }
    if forward {
        position.saturating_add(1).min(length - 1)
    } else {
        position.saturating_sub(1)
    }
}

fn coverage_rows(detail: &TopicDetail) -> Vec<String> {
    let monitor = &detail.monitor;
    let coverage = &detail.coverage;
    let spec = &monitor.version.spec;
    let mut lines = vec![
        tab_heading(MonitorTab::Coverage),
        format!(
            "Monitor {} v{}: processing {}. Classification off.",
            sanitize(&monitor.id, 64),
            monitor.version.version,
            if monitor.paused { "paused" } else { "active" }
        ),
        "Arrows scroll. Tab switches to passages. Esc returns. Read-only.".into(),
        format!("Goal: {}", sanitize(&spec.goal, 160)),
        format!(
            "Window: capture starts in [{}, {}).",
            super::globe::utc_label(coverage.from_ms),
            super::globe::utc_label(coverage.to_ms)
        ),
    ];
    if coverage.sources.is_empty() {
        lines.push("No sources in this window.".into());
    }
    for source in &coverage.sources {
        lines.push(format!(
            "Source {}: captures {}, published {}, audio {}ms, gaps {} ({}ms), untranslated {}",
            sanitize(&source.source, 48),
            source.captures,
            source.published,
            source.recorded_us / 1000,
            source.gaps,
            source.gap_us / 1000,
            source.untranslated_cues
        ));
    }
    for schedule in &coverage.schedules {
        lines.push(format!(
            "Schedule {}: admitted {}, elapsed {}, spring-forward {}, waiting {}",
            sanitize(&schedule.schedule, 48),
            schedule.admitted,
            schedule.missed_elapsed,
            schedule.missed_spring_forward,
            schedule.waiting
        ));
    }
    lines.push(
        "Wording is uncertain. A literal match misses paraphrases; check the original.".into(),
    );
    lines
}

fn passage_rows(detail: &TopicDetail, selected: usize) -> Vec<String> {
    let matches = &detail.matches;
    let mut lines = vec![
        tab_heading(MonitorTab::Passages),
        format!(
            "Literal passages: {} in {} transcripts{}. These are not stored findings.",
            matches.matches.len(),
            matches.transcripts_scanned,
            if matches.more {
                "; result limit reached"
            } else {
                ""
            }
        ),
        "Enter shows revisions and the exact interval. o opens the recording.".into(),
    ];
    if matches.matches.is_empty() {
        lines.push("No literal passages in this window.".into());
        return lines;
    }
    for (index, passage) in matches.matches.iter().enumerate() {
        let marker = if index == selected { ">" } else { " " };
        let english = passage
            .english
            .as_deref()
            .map_or_else(|| "untranslated".into(), passage_preview);
        lines.push(format!(
            "{marker} {} | {english} | {}",
            passage_preview(&passage.original),
            clock_label(passage.start_us)
        ));
    }
    lines.push(
        "Wording is uncertain. A literal match misses paraphrases; check the original.".into(),
    );
    lines
}

fn passage_detail(detail: &TopicDetail, index: usize) -> Vec<String> {
    let Some(passage) = detail.matches.matches.get(index) else {
        return vec!["No passage is selected.".into()];
    };
    let english = passage
        .english
        .as_deref()
        .map_or_else(|| "not available".into(), |text| sanitize(text, 4096));
    vec![
        format!(
            "Passage {} r{} cue {}",
            sanitize(&passage.transcript_id, 64),
            passage.transcript_revision,
            passage.cue_ordinal
        ),
        format!(
            "Translation revision {}",
            passage
                .translation_revision
                .map_or_else(|| "none".into(), |revision| revision.to_string())
        ),
        format!("Recording {}", sanitize(&passage.recording_id, 64)),
        format!(
            "Exact interval: [{}, {})us",
            passage.start_us, passage.end_us
        ),
        format!("Original: {}", sanitize(&passage.original, 4096)),
        format!("English: {english}"),
        "Esc returns to the passage list. o opens this recording. Read-only.".into(),
        "Wording is uncertain. A literal match misses paraphrases; check the original.".into(),
    ]
}

fn tab_heading(active: MonitorTab) -> String {
    let coverage = if active == MonitorTab::Coverage {
        "[Coverage]"
    } else {
        "Coverage"
    };
    let passages = if active == MonitorTab::Passages {
        "[Passages]"
    } else {
        "Passages"
    };
    format!("{coverage}  {passages}  Tab switches")
}

/// Display a bounded preview without splitting an original grapheme.
fn passage_preview(value: &str) -> String {
    let clean = sanitize(value, 4096);
    let line = ratatui::text::Line::raw(clean);
    let mut output = String::new();
    let mut characters = 0;
    for grapheme in line.styled_graphemes(ratatui::style::Style::default()) {
        let next = grapheme.symbol.chars().count();
        if characters + next > 160 {
            output.push_str(" [preview clipped; inspect the stored cue]");
            return output;
        }
        output.push_str(grapheme.symbol);
        characters += next;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passage_previews_mark_cuts_and_preserve_whole_graphemes() {
        assert_eq!(passage_preview(&"a".repeat(160)), "a".repeat(160));
        assert_eq!(
            passage_preview(&format!("{}e\u{301}", "a".repeat(158))),
            format!("{}e\u{301}", "a".repeat(158))
        );
        assert_eq!(
            passage_preview(&format!("{}e\u{301}X", "a".repeat(159))),
            format!(
                "{} [preview clipped; inspect the stored cue]",
                "a".repeat(159)
            )
        );
        assert_eq!(passage_preview("東京\u{1b}[31m"), "東京[31m");
    }

    #[test]
    fn wrapped_passage_suffix_and_original_graphemes_are_reachable_after_resize() {
        let text = passage_text();
        let mut browser = passage_fixture();
        assert!(browser.open_passage());
        let rows = browser.lines_at_width(200, 20);
        assert!(rows.concat().contains(&text));
        assert!(
            rows.iter()
                .all(|row| ratatui::text::Line::raw(row.as_str()).width() <= 20)
        );
        let mut seen = String::new();
        for _ in 0..rows.len() {
            seen.push_str(&browser.lines_at_width(1, 20).concat());
            browser.move_selection(true);
        }
        assert!(seen.contains("東京Cafe\u{301} FINAL-SUFFIX"));
        assert_ne!(browser.lines_at_width(1, 132), [] as [String; 0]);
    }
    use sigy_service::monitor::{
        MonitorCaptureUsage, MonitorProcessing, MonitorSpec, MonitorTerm, MonitorVersion,
        PassageMatch, SourceCoverage,
    };

    fn monitor() -> MonitorView {
        MonitorView {
            id: "news".into(),
            version: MonitorVersion {
                monitor_id: "news".into(),
                version: 1,
                created_ms: 0,
                spec_sha256: "fixture".into(),
                spec: MonitorSpec {
                    name: "Reports".into(),
                    goal: "Follow reports".into(),
                    terms: vec![MonitorTerm {
                        language: "ar".into(),
                        text: "سد".into(),
                    }],
                    sources: vec!["radio:v1".into()],
                    candidate_sources: vec![],
                    schedules: vec![],
                    daily_audio_seconds: 60,
                    total_audio_seconds: 3600,
                    recognition_profile: None,
                    translation_profile: None,
                    capture: None,
                },
            },
            paused: false,
            active_sources: vec!["radio:v1".into()],
            actions: 0,
            processing: MonitorProcessing::default(),
            capture_usage: MonitorCaptureUsage::default(),
        }
    }

    pub(crate) fn fixture() -> Browser {
        fixture_with_original("تقارير عن سد\u{1b}[31m".into())
    }

    fn passage_text() -> String {
        format!("{}東京Cafe\u{301} FINAL-SUFFIX", "prefix ".repeat(17))
    }

    pub(crate) fn passage_fixture() -> Browser {
        fixture_with_original(passage_text())
    }

    fn fixture_with_original(original: String) -> Browser {
        let monitor = monitor();
        let coverage = MonitorCoverage {
            id: "news".into(),
            version: 1,
            from_ms: 0,
            to_ms: 60_000,
            daily_audio_seconds: 60,
            sources: vec![SourceCoverage {
                source: "radio:v1".into(),
                captures: 2,
                published: 1,
                recorded_us: 30_000_000,
                gaps: 1,
                gap_us: 5_000_000,
                transcribed: 1,
                transcribed_us: 30_000_000,
                untranslated_cues: 1,
                ..SourceCoverage::default()
            }],
            schedules: vec![],
        };
        let matches = MonitorMatches {
            id: "news".into(),
            version: 1,
            from_ms: 0,
            to_ms: 60_000,
            transcripts_scanned: 1,
            matches: vec![PassageMatch {
                source: "radio:v1".into(),
                recording_id: "recording".into(),
                capture_start_ms: 1,
                transcript_id: "pin".into(),
                transcript_revision: 1,
                cue_ordinal: 0,
                start_us: 1_000_000,
                end_us: 3_000_000,
                term_language: "ar".into(),
                term: "سد".into(),
                field: "original".into(),
                original,
                translation_revision: Some(1),
                english: Some("Reports about a dam".into()),
            }],
            more: false,
        };
        let mut browser = Browser::default();
        browser.load_ids(vec!["news".into()]);
        assert!(browser.load_detail(&monitor, &coverage, &matches));
        browser
    }

    #[test]
    fn navigation_is_bounded_and_read_only() {
        let mut browser = Browser::default();
        browser.load_ids(vec!["first".into(), "second".into()]);
        assert_eq!(browser.open(), Effect::MonitorDetail { id: "first".into() });
        for _ in 0..300 {
            browser.move_selection(true);
        }
        assert_eq!(browser.selected(), Some("second"));
        browser.load_ids(vec!["second".into(), "third".into()]);
        assert_eq!(browser.selected(), Some("second"));
        browser.load_ids(vec![]);
        assert!(browser.list_read());
        assert_eq!(browser.listed(), 0);
        assert_eq!(browser.open(), Effect::None);
        assert_eq!(browser.reload(), Effect::MonitorList);
        let unread = Browser::default();
        assert!(!unread.list_read());
        assert_eq!(unread.listed(), 0);
        browser.load_ids((0..300).map(|index| format!("monitor-{index}")).collect());
        for _ in 0..300 {
            browser.move_selection(true);
        }
        assert_eq!(browser.selected(), Some("monitor-255"));
    }

    #[test]
    fn details_scroll_and_keep_stage_counts_and_uncertainty_separate() {
        let mut browser = fixture();
        let coverage = browser.lines(64).join("\n");
        assert!(coverage.contains("Classification off"));
        assert!(coverage.contains("[Coverage]"));
        assert!(coverage.contains("captures 2, published 1, audio 30000ms"));
        assert!(coverage.contains("gaps 1 (5000ms)"));
        assert!(coverage.contains("untranslated 1"));
        assert!(coverage.contains("Wording is uncertain"));
        assert!(!coverage.contains(")us"));
        browser.switch_tab();
        assert_eq!(browser.active_tab(), MonitorTab::Passages);
        let passages = browser.lines(64).join("\n");
        assert!(passages.contains("[Passages]"));
        assert!(passages.contains("These are not stored findings"));
        assert!(passages.contains("تقارير عن سد"));
        assert!(passages.contains("Reports about a dam"));
        assert!(!passages.contains(")us"));
        assert!(browser.open_passage());
        let exact = browser.lines(64).join("\n");
        assert!(exact.contains("[1000000, 3000000)us"));
        assert!(exact.contains("English: Reports about a dam"));
        assert!(!exact.contains('\u{1b}'));
        browser.back();
        browser.back();
        let list = browser.lines(8).join("\n");
        assert!(list.contains("> Reports  v1  active  1 passage"));
        assert!(!list.contains(")us"));
    }

    #[test]
    fn stale_version_does_not_replace_a_previous_detail() {
        let mut browser = fixture();
        let previous = browser.lines(64);
        let mut current = monitor();
        current.version.version = 2;
        let coverage = MonitorCoverage {
            id: "news".into(),
            version: 1,
            from_ms: 0,
            to_ms: 60_000,
            daily_audio_seconds: 60,
            sources: vec![],
            schedules: vec![],
        };
        let matches = MonitorMatches {
            id: "news".into(),
            version: 1,
            from_ms: 0,
            to_ms: 60_000,
            transcripts_scanned: 0,
            matches: vec![],
            more: false,
        };
        assert!(!browser.load_detail(&current, &coverage, &matches));
        assert_eq!(browser.lines(64), previous);
    }

    #[test]
    fn hostile_ids_cannot_reach_terminal_controls() {
        let mut browser = Browser::default();
        browser.load_ids(vec!["name\u{1b}[31m\nsecond".into()]);
        let lines = browser.lines(3).join("\n");
        assert!(!lines.contains('\u{1b}'));
        assert_eq!(lines.lines().count(), 2);
    }
}
