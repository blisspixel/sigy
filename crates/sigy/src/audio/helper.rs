use super::protocol::{self, Event, Report, Status};

const FAILURE_CODES: &[&str] = &[
    "audio-device-unavailable",
    "audio-device-identity",
    "audio-device-config",
    "audio-sample-format-unsupported",
    "audio-device-open",
    "audio-buffer-estimate",
    "audio-device-start",
    "audio-device-failed",
    "audio-config-unsupported",
    "audio-config-bound",
    "audio-duration-bound",
    "audio-handshake-invalid",
    "audio-ring-bound",
    "audio-pcm-bound",
    "audio-preroll-bound",
    "audio-frame-bound",
    "audio-sample-invalid",
    "audio-channels-unsupported",
    "audio-empty-input",
    "audio-trailing-input",
    "audio-work-deadline",
    "audio-deadline-overflow",
    "audio-frame-overflow",
    "audio-windows-only",
];

pub(super) fn is_failure_code(code: &str) -> bool {
    FAILURE_CODES.contains(&code)
        || matches!(
            code,
            "audio-pcm-eof" | "audio-startup-failed" | "audio-output-incomplete"
        )
}

#[cfg(any(windows, test))]
pub(super) fn check_work(
    failed: bool,
    deadline: std::time::Instant,
    now: std::time::Instant,
) -> Result<(), super::Failure> {
    if failed {
        return Err("audio-device-failed".into());
    }
    if now >= deadline {
        return Err("audio-work-deadline".into());
    }
    Ok(())
}

pub(super) fn failure_code(error: &(dyn std::error::Error + 'static), startup: bool) -> String {
    let message = error.to_string();
    if is_failure_code(&message) {
        return message;
    }
    if !startup
        && error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::UnexpectedEof)
    {
        return "audio-pcm-eof".into();
    }
    if startup {
        "audio-startup-failed"
    } else {
        "audio-output-incomplete"
    }
    .into()
}

pub(super) fn startup_failure(error: &(dyn std::error::Error + 'static)) -> Event {
    Event::Report(Report {
        protocol: 1,
        status: Status::Failed,
        error: Some(failure_code(error, true)),
        decoded_frames: 0,
        content_frames: 0,
        clipped_samples: 0,
        underrun_frames: 0,
        drain_zero_frames: 0,
        callbacks: 0,
        queue_high_water_frames: 0,
        predicted_presentation_us: None,
        presentation_is_estimated: true,
        audibility_proven: false,
    })
}

pub(crate) fn run_helper() -> Result<(), super::Failure> {
    let mut input = std::io::stdin().lock();
    let limits = match protocol::read_limits(&mut input) {
        Ok(limits) => limits,
        Err(error) => {
            protocol::write_event(
                &mut std::io::stdout().lock(),
                &startup_failure(error.as_ref()),
            )?;
            return Err(error);
        }
    };
    #[cfg(windows)]
    {
        super::backend::run(&mut input, &mut std::io::stdout().lock(), limits)
    }
    #[cfg(not(windows))]
    {
        let _ = limits;
        let error: super::Failure = "audio-windows-only".into();
        protocol::write_event(
            &mut std::io::stdout().lock(),
            &startup_failure(error.as_ref()),
        )?;
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_input_and_content_share_the_same_expiring_work_boundary()
    -> Result<(), super::super::Failure> {
        let deadline = std::time::Instant::now();
        let earlier = deadline
            .checked_sub(std::time::Duration::from_millis(1))
            .ok_or("fixture clock bound")?;
        check_work(false, deadline, earlier)?;
        assert_eq!(
            check_work(false, deadline, deadline)
                .err()
                .ok_or("deadline accepted")?
                .to_string(),
            "audio-work-deadline"
        );
        assert_eq!(
            check_work(false, earlier, deadline)
                .err()
                .ok_or("late End accepted")?
                .to_string(),
            "audio-work-deadline"
        );
        assert_eq!(
            check_work(true, deadline, earlier)
                .err()
                .ok_or("failed device accepted")?
                .to_string(),
            "audio-device-failed"
        );
        Ok(())
    }

    #[test]
    fn startup_refusal_is_structured_zero_work_and_never_audibility_evidence()
    -> Result<(), super::super::Failure> {
        let refused: super::super::Failure = "audio-sample-format-unsupported".into();
        let event = startup_failure(refused.as_ref());
        let mut bytes = Vec::new();
        protocol::write_event(&mut bytes, &event)?;
        assert!(bytes.len() <= protocol::MESSAGE_BYTES);
        let (last, preceding) = bytes.split_last().ok_or("startup report empty")?;
        assert_eq!(*last, b'\n');
        assert!(!preceding.contains(&b'\n'));
        let Event::Report(report) = serde_json::from_slice(&bytes)? else {
            return Err("startup emitted Ready".into());
        };
        assert_eq!(report.status, Status::Failed);
        assert_eq!(
            report.error.as_deref(),
            Some("audio-sample-format-unsupported")
        );
        assert_eq!(
            (
                report.decoded_frames,
                report.content_frames,
                report.callbacks
            ),
            (0, 0, 0)
        );
        assert_eq!(
            (
                report.underrun_frames,
                report.drain_zero_frames,
                report.queue_high_water_frames
            ),
            (0, 0, 0)
        );
        assert!(report.presentation_is_estimated && !report.audibility_proven);
        assert_eq!(report.clipped_samples, 0);
        assert_eq!(report.predicted_presentation_us, None);
        Ok(())
    }

    #[test]
    fn known_failure_codes_survive_but_native_identity_and_controls_never_cross_wire() {
        for startup in [true, false] {
            let raw = std::io::Error::other("device Secret speaker C:/private/path\u{1b}[31m");
            let code = failure_code(&raw, startup);
            assert_eq!(
                code,
                if startup {
                    "audio-startup-failed"
                } else {
                    "audio-output-incomplete"
                }
            );
            assert!(!code.contains("Secret") && !code.contains('\u{1b}'));
        }
        let eof = std::io::Error::from(std::io::ErrorKind::UnexpectedEof);
        assert_eq!(failure_code(&eof, false), "audio-pcm-eof");
        for code in [
            "audio-frame-bound",
            "audio-sample-invalid",
            "audio-work-deadline",
            "audio-device-failed",
        ] {
            let error: super::super::Failure = code.into();
            assert_eq!(failure_code(error.as_ref(), false), code);
        }
    }
}
