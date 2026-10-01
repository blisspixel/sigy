use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use processkit::{ProcessGroup, ProcessGroupOptions};
use scorer_probe::{
    judge_records::{CalibrationControl, CalibrationSet, Judgment, JudgmentOutcome, Judgments},
    selection::sha256,
};
use serde::Serialize;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    time::{Instant, sleep, timeout},
};

use crate::{
    RUBRIC, Result, TEMPLATE,
    assets::{self, Profile},
};

#[derive(Default, Serialize)]
struct Attempt {
    control_id: String,
    started_unix_seconds: u64,
    wall_ms: u64,
    stdout_sha256: String,
    stderr_sha256: String,
    exit_code: Option<i32>,
    failure: Option<String>,
    mechanism: String,
    peak_memory_bytes: Option<u64>,
    cpu_time_us: Option<u64>,
    group_empty: bool,
    process_started: bool,
    raw_outputs_complete: bool,
}

fn abstained(reason: &str) -> JudgmentOutcome {
    JudgmentOutcome::Abstained {
        reason: reason.into(),
    }
}

pub async fn batch(
    directory: &Path,
    profile: &Profile,
    controls: &CalibrationSet,
    profile_digest: &str,
    deadline: Instant,
) -> Result<()> {
    if !cfg!(windows) {
        return Err("this experiment requires the measured Windows Job Object boundary".into());
    }
    let order: Vec<_> = controls
        .controls
        .iter()
        .map(|control| &control.control_id)
        .collect();
    if sha256(&serde_json::to_vec(&order)?) != profile.order_sha256
        || controls.controls.len() != 126
        || controls.rubric_sha256 != profile.rubric_sha256
    {
        return Err("frozen control order, count or rubric mismatch".into());
    }
    assets::write_new(
        &directory.join("run-started.json"),
        &serde_json::to_vec(
            &serde_json::json!({"profile_sha256": profile_digest, "started_unix_seconds": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()}),
        )?,
    )?;
    let raw = directory.join("raw");
    std::fs::create_dir(&raw)?;
    assets::plain(&raw, true)?;
    let mut journal = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("attempts.jsonl"))?;
    let mut records = Vec::new();
    let mut cancelled = false;
    for control in &controls.controls {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let outcome = if cancelled {
            abstained("batch-cancelled-before-attempt")
        } else if remaining.is_zero() {
            abstained("batch-deadline-before-attempt")
        } else {
            let (outcome, receipt) = attempt(
                directory,
                &raw,
                profile,
                control,
                deadline.min(Instant::now() + Duration::from_secs(profile.limits.call_seconds)),
            )
            .await?;
            cancelled = receipt.failure.as_deref() == Some("cancelled");
            serde_json::to_writer(&mut journal, &receipt)?;
            writeln!(journal)?;
            journal.flush()?;
            journal.sync_all()?;
            println!(
                "attempt={} completed={} failure={}",
                &control.control_id[..12],
                records.len() + 1,
                receipt.failure.as_deref().unwrap_or("none")
            );
            outcome
        };
        records.push(Judgment {
            control_id: control.control_id.clone(),
            outcome,
        });
    }
    let judgments = Judgments {
        schema_version: 1,
        control_artifact_sha256: profile.controls_sha256.clone(),
        rubric_sha256: profile.rubric_sha256.clone(),
        judge_profile_sha256: profile_digest.into(),
        judge_family: "gemma-4-e2b-reference-assisted".into(),
        records,
    };
    scorer_probe::judge_records::validate_judgments(
        controls,
        &profile.controls_sha256,
        &judgments,
    )?;
    assets::json_new(&directory.join("judgments.json"), &judgments)?;
    println!(
        "judgments_sha256={} outcomes={}",
        sha256(&serde_json::to_vec_pretty(&judgments)?),
        judgments.records.len()
    );
    Ok(())
}

fn prompt(control: &CalibrationControl) -> Result<String> {
    let data = serde_json::to_string(&scorer_probe::judge_scoring::BlindedInput {
        control_id: &control.control_id,
        source_text: &control.source_text,
        english_reference: &control.english_reference,
        output_text: &control.output_text,
        untrusted_context: &control.untrusted_context,
    })?;
    // Replace only the two fixed placeholders, so data cannot inject a template substitution.
    Ok(TEMPLATE
        .replacen("{rubric}", RUBRIC, 1)
        .replacen("{data}", &data, 1))
}

fn command(profile: &Profile, directory: &Path, prompt: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(profile.runtime.join("llama-completion.exe"));
    command
        .env_clear()
        .current_dir(directory)
        .arg("-m")
        .arg(&profile.model.path)
        .args(&profile.arguments)
        .arg("-f")
        .arg(prompt)
        .arg("-jf")
        .arg(directory.join("output-schema.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.env("OMP_WAIT_POLICY", "PASSIVE");
    let mut path = profile.runtime.as_os_str().to_owned();
    if let Some(root) = std::env::var_os("SystemRoot") {
        path.push(";");
        path.push(Path::new(&root).join("System32"));
        command.env("SystemRoot", root);
    }
    command.env("PATH", path);
    command
}

async fn drain(group: &ProcessGroup) -> Result<(Option<u64>, Option<u64>)> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = group.stats()?;
        if snapshot.active_process_count == 0 {
            return Ok((
                snapshot.peak_memory_bytes,
                snapshot
                    .total_cpu_time
                    .and_then(|time| u64::try_from(time.as_micros()).ok()),
            ));
        }
        if Instant::now() >= deadline {
            let _ = group.kill_all();
            return Err("native group cleanup unproven; batch aborted".into());
        }
        sleep(Duration::from_millis(20)).await;
    }
}

async fn bounded_read(reader: impl AsyncRead + Unpin, ceiling: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(ceiling + 1).read_to_end(&mut bytes).await?;
    if bytes.len() as u64 > ceiling {
        return Err("native output ceiling".into());
    }
    Ok(bytes)
}

async fn attempt(
    directory: &Path,
    raw: &Path,
    profile: &Profile,
    control: &CalibrationControl,
    call_deadline: Instant,
) -> Result<(JudgmentOutcome, Attempt)> {
    let start = Instant::now();
    let mut receipt = Attempt {
        control_id: control.control_id.clone(),
        started_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        ..Attempt::default()
    };
    let prompt_path = raw.join(format!("{}.prompt", control.control_id));
    assets::write_new(&prompt_path, prompt(control)?.as_bytes())?;
    let Ok(group) = ProcessGroup::with_options(
        ProcessGroupOptions::default()
            .max_processes(profile.limits.processes)
            .max_memory(profile.limits.memory_bytes)
            .cpu_quota(f64::from(profile.limits.cpu_processors)),
    ) else {
        receipt.failure = Some("native-limits-unavailable".into());
        receipt.mechanism = "unavailable".into();
        receipt.group_empty = true;
        receipt.wall_ms = u64::try_from(start.elapsed().as_millis())?;
        return Ok((abstained("native-limits-unavailable"), receipt));
    };
    receipt.mechanism = group.mechanism().name().into();
    if Instant::now() >= call_deadline {
        group.kill_all()?;
        let _ = drain(&group).await?;
        receipt.failure = Some("deadline-before-launch".into());
        receipt.group_empty = true;
        receipt.wall_ms = u64::try_from(start.elapsed().as_millis())?;
        return Ok((abstained("deadline-before-launch"), receipt));
    }
    let Ok(mut child) = group.spawn(command(profile, directory, &prompt_path)) else {
        group.kill_all()?;
        let _ = drain(&group).await?;
        receipt.failure = Some("native-spawn-failed".into());
        receipt.group_empty = true;
        receipt.wall_ms = u64::try_from(start.elapsed().as_millis())?;
        return Ok((abstained("native-spawn-failed"), receipt));
    };
    receipt.process_started = true;
    let completed = collect(&mut child, profile, call_deadline).await;
    // Raw replies and judgments publish only after killing descendants and proving this group empty.
    group.kill_all()?;
    timeout(Duration::from_secs(5), child.wait()).await??;
    let (peak, cpu) = drain(&group).await?;
    receipt.peak_memory_bytes = peak;
    receipt.cpu_time_us = cpu;
    receipt.group_empty = true;
    receipt.wall_ms = u64::try_from(start.elapsed().as_millis())?;
    let outcome = interpret(completed, raw, control, &mut receipt)?;
    Ok((outcome, receipt))
}

type Completion = (std::process::ExitStatus, Vec<u8>, Vec<u8>);

async fn collect(
    child: &mut tokio::process::Child,
    profile: &Profile,
    call_deadline: Instant,
) -> Result<Completion> {
    let stdout = child.stdout.take().ok_or("native stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("native stderr unavailable")?;
    let completion = async {
        let (status, output, diagnostic) = tokio::try_join!(
            async { Ok::<_, Box<dyn std::error::Error + Send + Sync>>(child.wait().await?) },
            bounded_read(stdout, profile.limits.stdout_bytes),
            bounded_read(stderr, profile.limits.stderr_bytes)
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((status, output, diagnostic))
    };
    tokio::select! { result = tokio::time::timeout_at(call_deadline, completion) => match result { Ok(result) => result, Err(_) => Err("native-deadline".into()) }, signal = tokio::signal::ctrl_c() => { signal?; Err("cancelled".into()) } }
}

fn interpret(
    completed: Result<Completion>,
    raw: &Path,
    control: &CalibrationControl,
    receipt: &mut Attempt,
) -> Result<JudgmentOutcome> {
    Ok(match completed {
        Ok((status, output, diagnostic)) => {
            receipt.raw_outputs_complete = true;
            receipt.exit_code = status.code();
            receipt.stdout_sha256 = sha256(&output);
            receipt.stderr_sha256 = sha256(&diagnostic);
            assets::write_new(&raw.join(format!("{}.stdout", control.control_id)), &output)?;
            assets::write_new(
                &raw.join(format!("{}.stderr", control.control_id)),
                &diagnostic,
            )?;
            if status.success() {
                if let Ok(outcome) = crate::response::parse(&output, control) {
                    outcome
                } else {
                    receipt.failure = Some("invalid-json-status-reason-or-literal-evidence".into());
                    abstained("invalid-json-status-reason-or-literal-evidence")
                }
            } else {
                receipt.failure = Some("native-nonzero-exit".into());
                abstained("native-nonzero-exit")
            }
        }
        Err(error) => {
            let reason = if error.to_string() == "cancelled" {
                "cancelled"
            } else if error.to_string() == "native-deadline" {
                "native-deadline"
            } else {
                "native-output-or-wait-failed"
            };
            receipt.failure = Some(reason.into());
            abstained(reason)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use scorer_probe::judge_records::{Category, LabelBasis};

    #[test]
    fn native_collection_failures_remain_explicit_abstentions() -> Result<()> {
        let control = CalibrationControl {
            control_id: "synthetic failure fixture".into(),
            clip_id: "not frozen".into(),
            category: Category::Unchanged,
            source_text: "synthetic".into(),
            english_reference: "synthetic".into(),
            output_text: "synthetic".into(),
            untrusted_context: String::new(),
            label_basis: LabelBasis::Unchanged {},
        };
        for (failure, expected) in [
            ("cancelled", "cancelled"),
            ("native-deadline", "native-deadline"),
            ("native output ceiling", "native-output-or-wait-failed"),
        ] {
            let mut receipt = Attempt::default();
            let outcome = interpret(
                Err(failure.into()),
                Path::new("unused"),
                &control,
                &mut receipt,
            )?;
            let JudgmentOutcome::Abstained { reason } = outcome else {
                return Err("failure was not an abstention".into());
            };
            assert_eq!(reason, expected);
            assert_eq!(receipt.failure.as_deref(), Some(expected));
            assert!(!receipt.raw_outputs_complete);
        }
        Ok(())
    }
    #[tokio::test]
    async fn native_output_ceiling_is_exact_and_not_truncated_success() -> Result<()> {
        assert_eq!(bounded_read(b"1234".as_slice(), 4).await?, b"1234");
        assert!(bounded_read(b"12345".as_slice(), 4).await.is_err());
        Ok(())
    }

    #[test]
    #[ignore = "native child fixture; invoked only by the containment test"]
    fn native_fixture_child() -> Result<()> {
        match std::env::var("SIGY_JUDGE_FIXTURE")?.as_str() {
            "success" => println!("native fixture output"),
            "hold" => std::thread::sleep(Duration::from_secs(20)),
            "flood" => {
                std::io::stdout().write_all(&vec![b'x'; 1024 * 1024])?;
                std::thread::sleep(Duration::from_secs(20));
            }
            _ => return Err("unknown native fixture mode".into()),
        }
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn native_success_timeout_and_output_failure_prove_empty_groups() -> Result<()> {
        for mode in ["success", "hold", "flood"] {
            let group = ProcessGroup::with_options(
                ProcessGroupOptions::default()
                    .max_processes(1)
                    .max_memory(8 * 1024 * 1024 * 1024)
                    .cpu_quota(2.0),
            )?;
            let mut command = tokio::process::Command::new(std::env::current_exe()?);
            command
                .env_clear()
                .env("SIGY_JUDGE_FIXTURE", mode)
                .args([
                    "--ignored",
                    "--exact",
                    "execution::tests::native_fixture_child",
                    "--nocapture",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
            if let Some(profile_file) = std::env::var_os("LLVM_PROFILE_FILE") {
                command.env("LLVM_PROFILE_FILE", profile_file);
            }
            let mut child = group.spawn(command)?;
            let stdout = child.stdout.take().ok_or("missing fixture stdout")?;
            let result = timeout(Duration::from_secs(2), bounded_read(stdout, 1024)).await;
            match mode {
                "success" => {
                    assert!(std::str::from_utf8(&result??)?.contains("native fixture output"));
                }
                "hold" => assert!(result.is_err()),
                "flood" => assert!(result?.is_err()),
                _ => return Err("unknown fixture".into()),
            }
            group.kill_all()?;
            timeout(Duration::from_secs(5), child.wait()).await??;
            let (peak, cpu) = drain(&group).await?;
            assert!(peak.is_some_and(|bytes| bytes > 0));
            assert!(cpu.is_some());
            println!(
                "fixture={mode} mechanism={} group_empty=true peak_memory_bytes={peak:?} cpu_time_us={cpu:?}",
                group.mechanism().name()
            );
        }
        Ok(())
    }
}
