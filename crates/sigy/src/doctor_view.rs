//! Human doctor report. JSON keeps the stored names, bytes and money.

use std::io::{self, Write};

use sigy_service::control::{DoctorCheck, DoctorReport, DoctorState, Snapshot};
use sigy_service::guidance::with_data_dir;

use crate::copy::compact_usd;
use crate::explorer::text::bytes_label;
use crate::style::{Ink, Tone};

/// Write the human doctor report. Each plain line is at most 100 characters.
pub fn render(
    stdout: &mut impl Write,
    view: &Snapshot,
    report: &DoctorReport,
    data_dir: Option<&str>,
    ink: Ink,
) -> io::Result<()> {
    for line in lines(view, report, data_dir) {
        writeln!(stdout, "{}", paint_state(ink, &line))?;
    }
    Ok(())
}

pub(crate) fn lines(view: &Snapshot, report: &DoctorReport, data_dir: Option<&str>) -> Vec<String> {
    let blocked = count(report, &DoctorState::Blocked);
    let attention = count(report, &DoctorState::Attention);
    let mut out = vec![format!(
        "Doctor: {blocked} blocked, {attention} need attention. No network request was made."
    )];
    push_service(&mut out, view, data_dir);
    let provider_ok = report
        .checks
        .iter()
        .any(|check| check.name == "provider" && check.state == DoctorState::Ok);
    for state in [
        DoctorState::Blocked,
        DoctorState::Attention,
        DoctorState::Ok,
    ] {
        for check in &report.checks {
            if check.state == state && !is_idle(check, provider_ok) {
                push_check(&mut out, view, check, provider_ok, data_dir);
            }
        }
    }
    push_idle(&mut out, report, provider_ok);
    out
}

pub(crate) fn failure_line(report: &DoctorReport, strict: bool, json: bool) -> Option<String> {
    let blocked = count(report, &DoctorState::Blocked);
    let attention = count(report, &DoctorState::Attention);
    if blocked == 0 && !(strict && attention > 0) {
        return None;
    }
    if json {
        return Some("doctor found a check that needs action".into());
    }
    Some(match (blocked, attention) {
        (1, 0) => "1 blocked check".into(),
        (blocked, 0) => format!("{blocked} blocked checks"),
        (0, 1) => "1 check needs attention".into(),
        (0, attention) => format!("{attention} checks need attention"),
        (blocked, attention) => format!("{blocked} blocked, {attention} need attention"),
    })
}

fn count(report: &DoctorReport, state: &DoctorState) -> usize {
    report
        .checks
        .iter()
        .filter(|check| &check.state == state)
        .count()
}

fn push_service(out: &mut Vec<String>, view: &Snapshot, data_dir: Option<&str>) {
    if let Some(service) = &view.service {
        let state = if service.stopping {
            "stopping"
        } else {
            "running"
        };
        out.push(format!(
            "Service: process {} is {state}.",
            service.process_id
        ));
    } else {
        out.push("Service: not running.".into());
        push_next(out, "sigy service start", data_dir);
    }
}

fn push_check(
    out: &mut Vec<String>,
    view: &Snapshot,
    check: &DoctorCheck,
    provider_ok: bool,
    data_dir: Option<&str>,
) {
    let word = match check.state {
        DoctorState::Blocked => "blocked",
        DoctorState::Attention => "attention",
        DoctorState::Ok => "ok",
    };
    let prefix = format!("{word:<10}{:<20}", label(&check.name));
    let continuation = " ".repeat(prefix.chars().count());
    push_wrapped(
        out,
        &prefix,
        &continuation,
        &human_detail(view, check, provider_ok),
    );
    if let Some(command) = &check.command {
        push_next(out, command, data_dir);
    }
}

fn push_next(out: &mut Vec<String>, command: &str, data_dir: Option<&str>) {
    push_wrapped(
        out,
        "          Next: ",
        "                ",
        &with_data_dir(command, data_dir),
    );
}

fn push_idle(out: &mut Vec<String>, report: &DoctorReport, provider_ok: bool) {
    let names = report
        .checks
        .iter()
        .filter(|check| is_idle(check, provider_ok))
        .map(|check| {
            let mut name = label(&check.name);
            if check.detail.to_ascii_lowercase().contains("unmeasured") {
                let (mark, _) = crate::doctor_label(&check.state, &check.detail);
                name = format!("{name} ({mark})");
            }
            name
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        return;
    }
    push_wrapped(out, "idle      ", "          ", &names.join(", "));
}

fn is_idle(check: &DoctorCheck, provider_ok: bool) -> bool {
    check.state == DoctorState::Ok
        && check.command.is_none()
        && matches!(
            check.name.as_str(),
            "podcasts"
                | "publisher_text"
                | "captures"
                | "recognition"
                | "directory_refresh"
                | "native recovery"
                | "provider"
        )
        && (check.name != "provider" || provider_ok)
}

fn human_detail(view: &Snapshot, check: &DoctorCheck, provider_ok: bool) -> String {
    match check.name.as_str() {
        "quota" => quota_text(view).unwrap_or_else(|| check.detail.clone()),
        "budget" if provider_ok && check.state == DoctorState::Ok => {
            format!("{}, {}", paid(view), budget_text(view, check))
        }
        "budget" => budget_text(view, check),
        "decoder" if check.state == DoctorState::Attention => "No FFmpeg configured.".into(),
        "decoder" if check.state == DoctorState::Blocked => {
            "The configured decoder is not a file.".into()
        }
        "directory" => directory_text(view).unwrap_or_else(|| check.detail.clone()),
        _ => check.detail.clone(),
    }
}

fn quota_text(view: &Snapshot) -> Option<String> {
    let dvr = view.dvr.as_ref()?;
    Some(format!(
        "{} free, {}-day retention",
        bytes_label(dvr.available_bytes),
        dvr.retention_days
    ))
}

fn budget_text(view: &Snapshot, check: &DoctorCheck) -> String {
    view.budgets
        .iter()
        .find(|budget| budget.scope == "global")
        .map_or_else(
            || check.detail.clone(),
            |budget| format!("limit {}", compact_usd(&budget.limit_usd)),
        )
}

fn paid(view: &Snapshot) -> &'static str {
    if view.provider_dispatch_available {
        "paid dispatch reported on"
    } else {
        "paid dispatch off"
    }
}

fn directory_text(view: &Snapshot) -> Option<String> {
    let directory = view.directory.as_ref()?;
    let cached = directory.cached_stations;
    let stale = directory.stale_stations;
    Some(if cached == 0 {
        "No stations cached yet.".into()
    } else if stale > 0 {
        format!("{stale} of {cached} cached stations are older than 24 hours.")
    } else {
        format!("{cached} cached stations are within 24 hours.")
    })
}

fn label(name: &str) -> String {
    match name {
        "catalog" => "Catalog",
        "native recovery" => "Native recovery",
        "provider" => "Paid processing",
        "budget" => "Spending",
        "decoder" => "Decoder",
        "decoder_containment" => "Native containment",
        "quota" => "Storage",
        "directory" => "Directory",
        "directory_refresh" => "Directory fetch",
        "podcasts" => "Podcasts",
        "publisher_text" => "Publisher text",
        "captures" => "Captures",
        "recognition" => "Recognition",
        other => return other.replace('_', " "),
    }
    .to_owned()
}

fn push_wrapped(out: &mut Vec<String>, prefix: &str, continuation: &str, text: &str) {
    let mut rest = text;
    let mut first = true;
    loop {
        let lead = if first { prefix } else { continuation };
        first = false;
        let room = 100usize.saturating_sub(lead.chars().count()).max(1);
        if rest.chars().count() <= room {
            out.push(format!("{lead}{rest}"));
            break;
        }
        let mut end = 0usize;
        let mut last_space = None;
        for (seen, (index, character)) in rest.char_indices().enumerate() {
            if seen == room {
                break;
            }
            end = index + character.len_utf8();
            if character == ' ' {
                last_space = Some(index);
            }
        }
        let split = last_space.filter(|index| *index > 0).unwrap_or(end);
        if split == 0 {
            out.push(format!("{lead}{rest}"));
            break;
        }
        let (head, tail) = rest.split_at(split);
        out.push(format!("{lead}{}", head.trim_end()));
        let trimmed = tail.trim_start();
        if trimmed.is_empty() || trimmed == rest {
            break;
        }
        rest = trimmed;
    }
}

fn paint_state(ink: Ink, line: &str) -> String {
    for (word, tone) in [
        ("blocked", Tone::Fail),
        ("attention", Tone::Attention),
        ("ok", Tone::Ok),
        ("idle", Tone::Muted),
    ] {
        if let Some(rest) = line.strip_prefix(word) {
            return format!("{}{rest}", ink.tint(tone, word));
        }
    }
    line.to_owned()
}
