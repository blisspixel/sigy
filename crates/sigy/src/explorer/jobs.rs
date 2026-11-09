//! Read-only Jobs screen for running captures, schedule rules, and tasks.

use sigy_service::{
    control::{ScheduleOccurrenceView, ScheduleRuleView},
    storage::dvr::Recording,
    task::TaskView,
};
use std::cell::Cell;

use super::{
    state::{Effect, Key},
    text::sanitize,
};

pub const PAGE_LIMIT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JobSection {
    #[default]
    Captures,
    Schedules,
    Tasks,
}

impl JobSection {
    #[must_use]
    pub const fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Captures, true) | (Self::Tasks, false) => Self::Schedules,
            (Self::Schedules, true) | (Self::Captures, false) => Self::Tasks,
            (Self::Tasks, true) | (Self::Schedules, false) => Self::Captures,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Captures => "Captures",
            Self::Schedules => "Schedules",
            Self::Tasks => "Tasks",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureJob {
    pub id: String,
    pub source: String,
    pub elapsed_seconds: u64,
    pub bound_seconds: u64,
    pub max_bytes: u64,
    pub running: bool,
    pub management_cmd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleJob {
    pub id: String,
    pub source: String,
    pub zone: String,
    /// The rule's own civil clock. This is not a computed next occurrence.
    pub rule_time: String,
    pub management_cmd: String,
}

/// One visited page. `after` is `None` on the first page.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredCursor {
    after: Option<String>,
}

/// Cursors for the three bounded list reads. `None` is the first page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageCursors {
    pub captures: Option<String>,
    pub schedules: Option<String>,
    pub tasks: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PageMove {
    Forward {
        section: JobSection,
        cursors: PageCursors,
    },
    Back {
        section: JobSection,
        cursors: PageCursors,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskJob {
    pub id: String,
    pub goal: String,
    pub scope_window: String,
    pub state_or_checkpoint: String,
    pub management_cmd: String,
}

#[derive(Debug, Clone)]
pub enum JobDetail {
    Capture(Box<Recording>),
    Schedule(Box<ScheduleRuleView>, Vec<ScheduleOccurrenceView>),
    Task(Box<TaskView>),
}

impl PartialEq for JobDetail {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Capture(a), Self::Capture(b)) => {
                a.id == b.id && a.state == b.state && a.storage_state == b.storage_state
            }
            (Self::Schedule(ra, _), Self::Schedule(rb, _)) => {
                ra.id == rb.id && ra.revision == rb.revision
            }
            (Self::Task(ta), Self::Task(tb)) => ta.id == tb.id && ta.checkpoint == tb.checkpoint,
            _ => false,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Browser {
    captures: Vec<CaptureJob>,
    schedules: Vec<ScheduleJob>,
    tasks: Vec<TaskJob>,
    section: JobSection,
    selected: usize,
    detail: Option<JobDetail>,
    detail_offset: usize,
    captures_after: Option<String>,
    captures_next: Option<String>,
    captures_history: Vec<StoredCursor>,
    schedules_after: Option<String>,
    schedules_next: Option<String>,
    schedules_history: Vec<StoredCursor>,
    tasks_after: Option<String>,
    tasks_next: Option<String>,
    tasks_history: Vec<StoredCursor>,
    pending: Option<PageMove>,
    observed_ms: Option<i64>,
    connected: bool,
    viewport_width: Cell<usize>,
    message: Option<String>,
}

impl PartialEq for Browser {
    fn eq(&self, other: &Self) -> bool {
        self.captures == other.captures
            && self.schedules == other.schedules
            && self.tasks == other.tasks
            && self.section == other.section
            && self.selected == other.selected
            && self.detail == other.detail
            && self.detail_offset == other.detail_offset
            && self.captures_after == other.captures_after
            && self.captures_next == other.captures_next
            && self.captures_history == other.captures_history
            && self.schedules_after == other.schedules_after
            && self.schedules_next == other.schedules_next
            && self.schedules_history == other.schedules_history
            && self.tasks_after == other.tasks_after
            && self.tasks_next == other.tasks_next
            && self.tasks_history == other.tasks_history
            && self.pending == other.pending
            && self.observed_ms == other.observed_ms
            && self.connected == other.connected
            && self.message == other.message
    }
}

impl Browser {
    #[cfg(test)]
    #[must_use]
    pub const fn section(&self) -> JobSection {
        self.section
    }

    #[cfg(test)]
    #[must_use]
    pub const fn connected(&self) -> bool {
        self.connected
    }

    #[must_use]
    pub fn running_captures_count(&self) -> usize {
        self.captures.iter().filter(|c| c.running).count()
    }

    #[must_use]
    pub fn schedules_count(&self) -> usize {
        self.schedules.len()
    }

    #[must_use]
    pub fn tasks_count(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn home_summary(&self) -> String {
        let schedules = self.schedules_count();
        let tasks = self.tasks_count();
        let captures = self.running_captures_count();
        let sched_str = if schedules == 1 {
            "1 schedule".to_owned()
        } else {
            format!("{schedules} schedules")
        };
        let task_str = if tasks == 1 {
            "1 task".to_owned()
        } else {
            format!("{tasks} tasks")
        };
        let cap_str = if captures == 1 {
            "1 capture running".to_owned()
        } else {
            format!("{captures} captures running")
        };
        format!("{sched_str}, {task_str}, {cap_str}")
    }

    pub fn load_data(
        &mut self,
        captures: Vec<CaptureJob>,
        schedules: Vec<ScheduleJob>,
        tasks: Vec<TaskJob>,
        observed_ms: i64,
    ) {
        self.captures = captures;
        self.schedules = schedules;
        self.tasks = tasks;
        self.observed_ms = Some(observed_ms);
        self.connected = true;
        self.clamp_selection();
    }

    /// The cursors the next list read should send. A pending page wins.
    #[must_use]
    pub fn cursors(&self) -> PageCursors {
        match &self.pending {
            Some(PageMove::Forward { cursors, .. } | PageMove::Back { cursors, .. }) => {
                cursors.clone()
            }
            None => self.shown_cursors(),
        }
    }

    /// A list read finished. Each `next` value is the cursor after that page.
    pub fn commit_page(
        &mut self,
        captures_next: Option<String>,
        schedules_next: Option<String>,
        tasks_next: Option<String>,
    ) {
        if let Some(pending) = self.pending.take() {
            let cursors = match pending {
                PageMove::Forward { section, cursors } => {
                    self.push_history(section);
                    cursors
                }
                PageMove::Back { cursors, .. } => cursors,
            };
            self.captures_after = cursors.captures;
            self.schedules_after = cursors.schedules;
            self.tasks_after = cursors.tasks;
        }
        self.captures_next = captures_next;
        self.schedules_next = schedules_next;
        self.tasks_next = tasks_next;
        self.connected = true;
        self.message = None;
        self.clamp_selection();
    }

    /// A list read failed. The shown page and its cursor stay.
    pub fn abandon_page(&mut self) {
        if let Some(PageMove::Back { section, cursors }) = self.pending.take() {
            self.restore_back(section, &cursors);
        }
        self.connected = false;
    }

    fn shown_cursors(&self) -> PageCursors {
        PageCursors {
            captures: self.captures_after.clone(),
            schedules: self.schedules_after.clone(),
            tasks: self.tasks_after.clone(),
        }
    }

    pub fn mark_disconnected(&mut self) {
        self.abandon_page();
    }

    pub fn load_capture_detail(&mut self, recording: Recording) {
        self.detail = Some(JobDetail::Capture(Box::new(recording)));
        self.detail_offset = 0;
    }

    pub fn load_schedule_detail(
        &mut self,
        rule: ScheduleRuleView,
        occurrences: Vec<ScheduleOccurrenceView>,
    ) {
        self.detail = Some(JobDetail::Schedule(Box::new(rule), occurrences));
        self.detail_offset = 0;
    }

    pub fn load_task_detail(&mut self, task: TaskView) {
        self.detail = Some(JobDetail::Task(Box::new(task)));
        self.detail_offset = 0;
    }

    pub fn leave_detail(&mut self) -> bool {
        if self.detail.take().is_some() {
            self.detail_offset = 0;
            return true;
        }
        false
    }

    pub fn switch_section(&mut self, forward: bool) {
        self.section = self.section.cycle(forward);
        self.selected = 0;
        self.message = None;
        self.clamp_selection();
    }

    pub fn handle(&mut self, key: &Key) -> Option<Effect> {
        match key {
            Key::Tab => {
                if self.detail.is_none() {
                    self.switch_section(true);
                    return Some(Effect::None);
                }
            }
            Key::BackTab => {
                if self.detail.is_none() {
                    self.switch_section(false);
                    return Some(Effect::None);
                }
            }
            Key::Up => {
                if self.detail.is_some() {
                    self.detail_offset = self.detail_offset.saturating_sub(1);
                } else {
                    self.move_selection(false);
                }
                return Some(Effect::None);
            }
            Key::Down => {
                if self.detail.is_some() {
                    self.detail_offset = self.detail_offset.saturating_add(1);
                } else {
                    self.move_selection(true);
                }
                return Some(Effect::None);
            }
            Key::Enter => {
                if self.detail.is_none() {
                    return self.open_detail();
                }
            }
            Key::Char('n') => {
                if self.detail.is_none() {
                    return Some(self.page_next());
                }
            }
            Key::Char('p') => {
                if self.detail.is_none() {
                    return Some(self.page_prev());
                }
            }
            Key::Char('r') => return Some(Effect::JobsList),
            Key::Escape | Key::Backspace => {
                if self.leave_detail() {
                    return Some(Effect::None);
                }
                return None;
            }
            Key::Char('f' | 'v' | '/') => {
                self.message = Some("Jobs screen is read-only.".into());
                return Some(Effect::None);
            }
            _ => return None,
        }
        Some(Effect::None)
    }

    fn move_selection(&mut self, forward: bool) {
        let count = self.active_items_count();
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = if forward {
            self.selected.saturating_add(1).min(count.saturating_sub(1))
        } else {
            self.selected.saturating_sub(1)
        };
    }

    fn page_next(&mut self) -> Effect {
        if self.pending.is_some() {
            self.message = Some("A page read is still settling.".into());
            return Effect::None;
        }
        let Some(next) = self.active_next().cloned() else {
            self.message = Some("Already on the last page of this section.".into());
            return Effect::None;
        };
        let mut cursors = self.shown_cursors();
        self.write_active(&mut cursors, Some(next));
        let section = self.section;
        self.pending = Some(PageMove::Forward { section, cursors });
        self.selected = 0;
        self.message = None;
        Effect::JobsList
    }

    fn page_prev(&mut self) -> Effect {
        if self.pending.is_some() {
            self.message = Some("A page read is still settling.".into());
            return Effect::None;
        }
        let Some(previous) = self.take_history() else {
            self.message = Some("Already on the first page of this section.".into());
            return Effect::None;
        };
        let mut cursors = self.shown_cursors();
        self.write_active(&mut cursors, previous.after);
        let section = self.section;
        self.pending = Some(PageMove::Back { section, cursors });
        self.selected = 0;
        self.message = None;
        Effect::JobsList
    }

    fn active_next(&self) -> Option<&String> {
        match self.section {
            JobSection::Captures => self.captures_next.as_ref(),
            JobSection::Schedules => self.schedules_next.as_ref(),
            JobSection::Tasks => self.tasks_next.as_ref(),
        }
    }

    fn write_active(&self, cursors: &mut PageCursors, after: Option<String>) {
        match self.section {
            JobSection::Captures => cursors.captures = after,
            JobSection::Schedules => cursors.schedules = after,
            JobSection::Tasks => cursors.tasks = after,
        }
    }

    fn push_history(&mut self, section: JobSection) {
        let cursor = |after: &Option<String>| StoredCursor {
            after: after.clone(),
        };
        match section {
            JobSection::Captures => self.captures_history.push(cursor(&self.captures_after)),
            JobSection::Schedules => self.schedules_history.push(cursor(&self.schedules_after)),
            JobSection::Tasks => self.tasks_history.push(cursor(&self.tasks_after)),
        }
    }

    fn take_history(&mut self) -> Option<StoredCursor> {
        match self.section {
            JobSection::Captures => self.captures_history.pop(),
            JobSection::Schedules => self.schedules_history.pop(),
            JobSection::Tasks => self.tasks_history.pop(),
        }
    }

    fn restore_back(&mut self, section: JobSection, cursors: &PageCursors) {
        let cursor = |after: &Option<String>| StoredCursor {
            after: after.clone(),
        };
        match section {
            JobSection::Captures => self.captures_history.push(cursor(&cursors.captures)),
            JobSection::Schedules => self.schedules_history.push(cursor(&cursors.schedules)),
            JobSection::Tasks => self.tasks_history.push(cursor(&cursors.tasks)),
        }
    }

    fn clamp_selection(&mut self) {
        let count = self.active_items_count();
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    fn active_items_count(&self) -> usize {
        match self.section {
            JobSection::Captures => self.captures.len(),
            JobSection::Schedules => self.schedules.len(),
            JobSection::Tasks => self.tasks.len(),
        }
    }

    fn open_detail(&self) -> Option<Effect> {
        match self.section {
            JobSection::Captures => {
                let job = self.captures.get(self.selected)?;
                Some(Effect::JobCaptureShow { id: job.id.clone() })
            }
            JobSection::Schedules => {
                let job = self.schedules.get(self.selected)?;
                Some(Effect::JobScheduleShow { id: job.id.clone() })
            }
            JobSection::Tasks => {
                let job = self.tasks.get(self.selected)?;
                Some(Effect::JobTaskShow { id: job.id.clone() })
            }
        }
    }

    pub fn lines(&self, limit: usize, width: usize) -> Vec<String> {
        self.viewport_width.set(width.max(2));
        if let Some(detail) = &self.detail {
            return self.detail_lines(detail, limit);
        }
        let mut lines = Vec::new();
        let conn_status = if self.connected {
            "connected".to_owned()
        } else if let Some(ms) = self.observed_ms {
            format!("offline, snapshot {}", super::globe::utc_label(ms))
        } else {
            "snapshot unread".to_owned()
        };
        lines.push(format!(
            "Jobs: running captures, schedule rules and tasks. Read-only | {conn_status}"
        ));
        lines.push(format!(
            "Sections: {}  {}  {} | Tab switches | Enter inspects | n/p pages",
            format_section(JobSection::Captures, self.section, self.captures.len()),
            format_section(JobSection::Schedules, self.section, self.schedules.len()),
            format_section(JobSection::Tasks, self.section, self.tasks.len()),
        ));
        if let Some(message) = &self.message {
            lines.push(message.clone());
        }

        let count = self.active_items_count();
        if count == 0 {
            lines.push(format!(
                "No {} on this library.",
                self.section.label().to_lowercase()
            ));
            return lines;
        }

        let room = limit.saturating_sub(lines.len());
        match self.section {
            JobSection::Captures => {
                for (idx, job) in self.captures.iter().enumerate().take(room) {
                    let marker = if idx == self.selected { ">" } else { " " };
                    let rec_tag = if job.running { "[REC] " } else { "" };
                    lines.push(format!(
                        "{marker} {rec_tag}{} | {} | decoded {} | bound {}s | {}",
                        sanitize(&job.id, 24),
                        sanitize(&job.source, 24),
                        super::timeline::clock_label(job.elapsed_seconds.saturating_mul(1_000_000)),
                        job.bound_seconds,
                        job.management_cmd
                    ));
                }
            }
            JobSection::Schedules => {
                for (idx, job) in self.schedules.iter().enumerate().take(room) {
                    let marker = if idx == self.selected { ">" } else { " " };
                    lines.push(format!(
                        "{marker} {} | {} | {} | rule {} | {}",
                        sanitize(&job.id, 20),
                        sanitize(&job.source, 20),
                        sanitize(&job.zone, 24),
                        sanitize(&job.rule_time, 24),
                        job.management_cmd
                    ));
                }
            }
            JobSection::Tasks => {
                for (idx, job) in self.tasks.iter().enumerate().take(room) {
                    let marker = if idx == self.selected { ">" } else { " " };
                    lines.push(format!(
                        "{marker} {} | {} | {} | {}",
                        sanitize(&job.id, 20),
                        sanitize(&job.scope_window, 36),
                        sanitize(&job.state_or_checkpoint, 24),
                        job.management_cmd
                    ));
                }
            }
        }
        lines.truncate(limit);
        lines
    }

    fn detail_lines(&self, detail: &JobDetail, limit: usize) -> Vec<String> {
        let rows = match detail {
            JobDetail::Capture(record) => render_capture_detail(record),
            JobDetail::Schedule(rule, occurrences) => render_schedule_detail(rule, occurrences),
            JobDetail::Task(task) => render_task_detail(task),
        };
        rows.into_iter()
            .skip(self.detail_offset)
            .take(limit)
            .collect()
    }
}

fn format_section(section: JobSection, active: JobSection, count: usize) -> String {
    let label = section.label();
    if section == active {
        format!("[{label} ({count})]")
    } else {
        format!("{label} ({count})")
    }
}

fn render_capture_detail(record: &Recording) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Recording detail: {} | profile {} | state {} | storage {}",
            sanitize(&record.id, 40),
            record.profile.as_str(),
            sanitize(&record.state, 16),
            sanitize(&record.storage_state, 16)
        ),
        "Esc returns to Jobs. Read-only snapshot.".into(),
        format!(
            "Source: {} | retention {} | charged {} bytes",
            sanitize(&record.source_revision, 64),
            record.retention.as_str(),
            record.charged_bytes
        ),
    ];
    if let Some(reason) = &record.end_reason {
        lines.push(format!("Capture ended: {reason}. Decoded media verified."));
    }
    if let Some(detail) = &record.failure_detail {
        lines.push(format!("Failure: {detail}"));
    }
    for interval in &record.intervals {
        lines.push(format!(
            "  Interval {}: bytes {}-{}, decoded {}-{} us.",
            interval.ordinal,
            interval.byte_start,
            interval.byte_end,
            interval.decoded_start_us,
            interval.decoded_end_us
        ));
    }
    for gap in &record.gaps {
        lines.push(format!(
            "  Gap {}: {} from {} to {} us. No audio file.",
            gap.ordinal,
            gap.cause.as_str(),
            gap.start_us,
            gap.end_us
        ));
    }
    lines.push(format!(
        "Manage with: sigy record show {}",
        sanitize(&record.id, 64)
    ));
    lines
}

fn render_schedule_detail(
    rule: &ScheduleRuleView,
    occurrences: &[ScheduleOccurrenceView],
) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Schedule detail: {} | zone {} | recurrence {}",
            sanitize(&rule.id, 40),
            sanitize(&rule.zone, 32),
            sanitize(&rule.recurrence, 20)
        ),
        "Esc returns to Jobs. Read-only snapshot.".into(),
        format!(
            "Source: {} | duration {}s | maximum {} bytes | revision {}",
            sanitize(&rule.source_revision, 64),
            rule.duration_seconds,
            rule.maximum_bytes,
            rule.revision
        ),
    ];
    if let Some(owner) = &rule.monitor_owner {
        lines.push(format!(
            "Capture owner {} attached at monitor version {}.",
            sanitize(&owner.monitor_id, 40),
            owner.version
        ));
    }
    if rule.task_owned {
        lines.push("Task-owned finite rule.".into());
    }
    for occ in occurrences {
        let outcome = occ.miss_reason.as_deref().unwrap_or(occ.state.as_str());
        lines.push(format!(
            "  Occurrence {}: {} start {} end {} recording {}",
            sanitize(&occ.civil_date, 16),
            sanitize(outcome, 24),
            occ.start_ms
                .map_or_else(|| "none".into(), |v| v.to_string()),
            occ.end_ms.map_or_else(|| "none".into(), |v| v.to_string()),
            sanitize(occ.recording_id.as_deref().unwrap_or("none"), 40)
        ));
    }
    lines.push(format!(
        "Manage with: sigy schedule show {}",
        sanitize(&rule.id, 64)
    ));
    lines
}

fn render_task_detail(task: &TaskView) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Task detail: {} | template {} | allowance ${}",
            sanitize(&task.id, 40),
            sanitize(&task.template, 24),
            task.paid_allowance_usd
        ),
        "Esc returns to Jobs. Read-only snapshot.".into(),
        format!("Goal: {}", sanitize(&task.spec.goal, 120)),
        format!(
            "Monitor: {} v{} | Scope: [{}, {}) UTC",
            sanitize(&task.spec.monitor_id, 40),
            task.spec.monitor_version,
            super::globe::utc_label(task.spec.from_ms),
            super::globe::utc_label(task.spec.to_ms)
        ),
        format!(
            "Checkpoints: {} | Snapshots: {} | Scope current: {}",
            task.checkpoint, task.snapshots, task.scope_current
        ),
    ];
    if let Some(cp) = &task.latest_checkpoint {
        lines.push(format!(
            "Latest checkpoint {}: request {}",
            cp.ordinal,
            sanitize(&cp.request_id, 40)
        ));
    }
    lines.push(format!(
        "Manage with: sigy task show {}",
        sanitize(&task.id, 64)
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_browser() -> Browser {
        let mut browser = Browser::default();
        browser.load_data(
            vec![CaptureJob {
                id: "rec-morning".into(),
                source: "radio:v1".into(),
                elapsed_seconds: 45,
                bound_seconds: 300,
                max_bytes: 64 * 1024 * 1024,
                running: true,
                management_cmd: "sigy record show rec-morning".into(),
            }],
            vec![ScheduleJob {
                id: "morning-news".into(),
                source: "radio:v1".into(),
                zone: "Europe/London".into(),
                rule_time: "daily 08:00".into(),
                management_cmd: "sigy schedule show morning-news".into(),
            }],
            vec![TaskJob {
                id: "water-survey".into(),
                goal: "Follow water news".into(),
                scope_window: "[2026-11-08 00:00, 2026-11-08 06:00)".into(),
                state_or_checkpoint: "checkpoint 1".into(),
                management_cmd: "sigy task show water-survey".into(),
            }],
            100_000,
        );
        browser
    }

    #[test]
    fn home_summary_reflects_jobs_counts() {
        let browser = sample_browser();
        assert_eq!(
            browser.home_summary(),
            "1 schedule, 1 task, 1 capture running"
        );
    }

    #[test]
    fn section_switching_and_paging_are_bounded_and_read_only() {
        let mut browser = sample_browser();
        assert_eq!(browser.section(), JobSection::Captures);

        // Tab switches section
        assert_eq!(browser.handle(&Key::Tab), Some(Effect::None));
        assert_eq!(browser.section(), JobSection::Schedules);

        assert_eq!(browser.handle(&Key::Tab), Some(Effect::None));
        assert_eq!(browser.section(), JobSection::Tasks);

        assert_eq!(browser.handle(&Key::Tab), Some(Effect::None));
        assert_eq!(browser.section(), JobSection::Captures);

        // Tab/BackTab switches
        assert_eq!(browser.handle(&Key::BackTab), Some(Effect::None));
        assert_eq!(browser.section(), JobSection::Tasks);

        // Enter yields read show effect
        let effect = browser.handle(&Key::Enter);
        assert_eq!(
            effect,
            Some(Effect::JobTaskShow {
                id: "water-survey".into()
            })
        );
    }

    #[test]
    fn every_key_on_jobs_yields_only_read_or_navigation_effects() {
        let mut browser = sample_browser();
        for key in [
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Tab,
            Key::BackTab,
            Key::Char('n'),
            Key::Char('p'),
            Key::Char('r'),
            Key::Char('f'),
            Key::Char('v'),
            Key::Char('/'),
        ] {
            let effect = browser.handle(&key);
            assert!(
                matches!(effect, None | Some(Effect::None | Effect::JobsList)),
                "Key {key:?} returned unexpected mutating or external effect: {effect:?}"
            );
        }
    }

    #[test]
    fn disconnect_keeps_snapshot_and_age() {
        let mut browser = sample_browser();
        assert!(browser.connected());
        browser.mark_disconnected();
        assert!(!browser.connected());
        let lines = browser.lines(10, 80).join("\n");
        assert!(lines.contains("offline"));
        assert!(lines.contains("[REC] rec-morning"));
        let unread = Browser::default();
        assert!(unread.lines(4, 80).join("\n").contains("snapshot unread"));
        assert!(!unread.lines(4, 80).join("\n").contains("offline"));
    }

    #[test]
    fn paging_follows_the_service_cursor_and_keeps_rows_when_the_read_fails() {
        let mut browser = sample_browser();
        assert_eq!(browser.handle(&Key::Char('n')), Some(Effect::None));
        assert!(
            browser
                .lines(8, 80)
                .join("\n")
                .contains("Already on the last page")
        );
        browser.commit_page(Some("cap-2".into()), None, None);
        assert_eq!(browser.handle(&Key::Char('n')), Some(Effect::JobsList));
        assert_eq!(browser.cursors().captures.as_deref(), Some("cap-2"));
        assert_eq!(browser.handle(&Key::Char('n')), Some(Effect::None));
        assert!(browser.lines(8, 80).join("\n").contains("still settling"));
        browser.abandon_page();
        assert!(!browser.connected());
        assert_eq!(browser.cursors().captures, None);
        assert!(browser.lines(10, 80).join("\n").contains("rec-morning"));

        let mut browser = sample_browser();
        browser.commit_page(Some("cap-2".into()), None, None);
        assert_eq!(browser.handle(&Key::Char('n')), Some(Effect::JobsList));
        browser.commit_page(None, None, None);
        assert_eq!(browser.cursors().captures.as_deref(), Some("cap-2"));
        assert_eq!(browser.handle(&Key::Char('p')), Some(Effect::JobsList));
        assert_eq!(browser.cursors().captures, None);
        browser.abandon_page();
        assert_eq!(browser.cursors().captures.as_deref(), Some("cap-2"));
        assert!(
            browser
                .lines(10, 80)
                .join("\n")
                .contains("[REC] rec-morning")
        );
        assert!(!browser.lines(12, 40).join("\n").contains(")us"));
    }
}
