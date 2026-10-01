//! Bounded, read-only monitor navigation and evidence summaries.

use sigy_service::monitor::{MonitorCoverage, MonitorMatches, MonitorView};

use super::{state::Effect, text::sanitize};

#[cfg(test)]
pub(super) use tests::fixture;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Browser {
    ids: Vec<String>,
    selected: usize,
    detail: Vec<String>,
    detail_id: Option<String>,
    offset: usize,
    showing_detail: bool,
    observed_ms: Option<i64>,
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
        if self.detail_id.as_deref() != self.selected() {
            self.detail.clear();
            self.detail_id = None;
            self.observed_ms = None;
        }
        self.showing_detail = false;
        self.offset = 0;
    }

    pub fn move_selection(&mut self, forward: bool) {
        let (position, length) = if self.showing_detail {
            (&mut self.offset, self.detail.len())
        } else {
            (&mut self.selected, self.ids.len())
        };
        *position = if forward {
            position.saturating_add(1).min(length.saturating_sub(1))
        } else {
            position.saturating_sub(1)
        };
    }

    pub fn back(&mut self) {
        self.showing_detail = false;
        self.offset = 0;
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
        self.detail = detail_lines(monitor, coverage, matches);
        self.detail_id = Some(monitor.id.clone());
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
            return self
                .detail
                .iter()
                .skip(self.offset)
                .take(limit)
                .cloned()
                .collect();
        }
        let mut lines =
            vec!["Monitors: arrows select, Enter reads, r reloads. Classification off.".into()];
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
                .map(|(index, id)| {
                    format!(
                        "{} {}",
                        if index == self.selected { ">" } else { " " },
                        sanitize(id, 64)
                    )
                }),
        );
        lines
    }
}

fn detail_lines(
    monitor: &MonitorView,
    coverage: &MonitorCoverage,
    matches: &MonitorMatches,
) -> Vec<String> {
    let spec = &monitor.version.spec;
    let mut lines = vec![
        format!(
            "Monitor {} v{}: processing {}. Classification off.",
            sanitize(&monitor.id, 64),
            monitor.version.version,
            if monitor.paused { "paused" } else { "active" }
        ),
        "Arrows scroll; Esc returns; r reloads. Read-only snapshot.".into(),
        format!("Goal: {}", sanitize(&spec.goal, 160)),
        format!(
            "Profiles: recognition {}; translation {}",
            spec.recognition_profile
                .as_deref()
                .map_or_else(|| "none".into(), |profile| sanitize(profile, 64)),
            spec.translation_profile
                .as_deref()
                .map_or_else(|| "none".into(), |profile| sanitize(profile, 64))
        ),
        format!(
            "Caps: {}s/day, {}s total. Used {}ms today, {}ms total.",
            spec.daily_audio_seconds,
            spec.total_audio_seconds,
            monitor.processing.used_today_us / 1000,
            monitor.processing.used_total_us / 1000
        ),
        format!(
            "Coverage: capture starts in [{}, {}).",
            super::globe::utc_label(coverage.from_ms),
            super::globe::utc_label(coverage.to_ms)
        ),
    ];
    if let Some(capture) = &spec.capture {
        lines.push(format!(
            "Capture caps: {}s/UTC day, {}s lifetime, {} bytes lifetime.",
            capture.daily_seconds, capture.total_seconds, capture.total_bytes
        ));
    } else {
        lines.push(
            "Owned capture admissions disabled. Independent schedules keep their policy.".into(),
        );
    }
    let usage = &monitor.capture_usage;
    lines.push(format!(
        "Capture use: {}s today, {}s lifetime, {} bytes reserved lifetime; {} admissions.",
        usage.used_today_seconds,
        usage.used_total_seconds,
        usage.reserved_total_bytes,
        usage.admissions
    ));
    for (reason, count) in &usage.refusals {
        lines.push(format!(
            "Capture refused: {} ({count})",
            sanitize(reason, 80)
        ));
    }
    append_coverage(&mut lines, coverage);
    append_passages(&mut lines, matches);
    lines.push(
        "Wording is uncertain. A literal match misses paraphrases; check the original.".into(),
    );
    lines
}

fn append_coverage(lines: &mut Vec<String>, coverage: &MonitorCoverage) {
    for source in &coverage.sources {
        lines.push(format!(
            "Source {}: captures {}, published {}, audio {}ms",
            sanitize(&source.source, 64),
            source.captures,
            source.published,
            source.recorded_us / 1000
        ));
        lines.push(format!(
            "  Gaps {} ({}ms), pinned {}, transcribed {} ({}ms), no text {}",
            source.gaps,
            source.gap_us / 1000,
            source.pinned,
            source.transcribed,
            source.transcribed_us / 1000,
            source.no_text
        ));
        lines.push(format!(
            "  Cues: translated {}, untranslated {}, no translation {}{}",
            source.translated_cues,
            source.untranslated_cues,
            source.cues_without_translation,
            if source.truncated {
                "; capture limit reached"
            } else {
                ""
            }
        ));
        for (reason, count) in &source.untranslated_reasons {
            lines.push(format!("  Untranslated {}: {count}", sanitize(reason, 64)));
        }
    }
    for schedule in &coverage.schedules {
        lines.push(format!(
            "Schedule {}: admitted {}, elapsed {}, spring-forward {}, waiting {}",
            sanitize(&schedule.schedule, 64),
            schedule.admitted,
            schedule.missed_elapsed,
            schedule.missed_spring_forward,
            schedule.waiting
        ));
    }
}

fn append_passages(lines: &mut Vec<String>, matches: &MonitorMatches) {
    lines.push(format!(
        "Literal passages: {} in {} transcripts{}. These are not stored findings.",
        matches.matches.len(),
        matches.transcripts_scanned,
        if matches.more {
            "; result limit reached"
        } else {
            ""
        }
    ));
    for passage in &matches.matches {
        lines.push(format!(
            "{} r{} cue {}: [{}, {})us",
            sanitize(&passage.transcript_id, 64),
            passage.transcript_revision,
            passage.cue_ordinal,
            passage.start_us,
            passage.end_us
        ));
        lines.push(format!(
            "  Recording {}; {} term {}",
            sanitize(&passage.recording_id, 64),
            sanitize(&passage.field, 16),
            sanitize(&passage.term, 64)
        ));
        lines.push(format!("  Original: {}", sanitize(&passage.original, 160)));
        lines.push(format!(
            "  English r{}: {}",
            passage
                .translation_revision
                .map_or_else(|| "none".into(), |revision| revision.to_string()),
            passage
                .english
                .as_deref()
                .map_or_else(|| "not available".into(), |text| sanitize(text, 160))
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                original: "تقارير عن سد\u{1b}[31m".into(),
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
        assert_eq!(browser.open(), Effect::None);
        assert_eq!(browser.reload(), Effect::MonitorList);
        browser.load_ids((0..300).map(|index| format!("monitor-{index}")).collect());
        for _ in 0..300 {
            browser.move_selection(true);
        }
        assert_eq!(browser.selected(), Some("monitor-255"));
    }

    #[test]
    fn details_scroll_and_keep_stage_counts_and_uncertainty_separate() {
        let mut browser = fixture();
        let lines = browser.lines(64).join("\n");
        assert!(lines.contains("Classification off"));
        assert!(lines.contains("captures 2, published 1, audio 30000ms"));
        assert!(lines.contains("Gaps 1 (5000ms)"));
        assert!(lines.contains("untranslated 1"));
        assert!(lines.contains("These are not stored findings"));
        assert!(lines.contains("تقارير عن سد"));
        assert!(lines.contains("English r1: Reports about a dam"));
        assert!(!lines.contains('\u{1b}'));
        browser.move_selection(true);
        assert!(browser.lines(1)[0].contains("Arrows scroll"));
        browser.back();
        assert!(browser.lines(3).join("\n").contains("> news"));
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
