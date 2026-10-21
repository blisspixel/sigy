//! Process exit classes. Human text and JSON come from the same message.

use std::error::Error;

use sigy_service::Error as ServiceError;

use super::Command;
use crate::service::ServiceCommand;

pub(crate) struct Classified {
    pub code: &'static str,
    pub exit: u8,
}

/// The command returned a useful incomplete task result.
#[derive(Debug)]
pub(crate) struct Partial;

impl std::fmt::Display for Partial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the result is partial")
    }
}

impl Error for Partial {}

pub(crate) fn classify(error: &(dyn Error + 'static), command: &Command) -> Classified {
    if error.is::<super::explorer::Interrupted>() {
        return Classified {
            code: "interrupted",
            exit: 130,
        };
    }
    if error.is::<Partial>() {
        return Classified {
            code: "partial",
            exit: 5,
        };
    }
    if let Some(service) = error.downcast_ref::<ServiceError>() {
        return service_class(service, command);
    }
    let text = error.to_string();
    if text.starts_with("the background service is not running")
        || text.starts_with("the background service is stopping")
    {
        return Classified {
            code: "service_unavailable",
            exit: 3,
        };
    }
    Classified {
        code: "failure",
        exit: 1,
    }
}

fn service_class(error: &ServiceError, command: &Command) -> Classified {
    match error {
        ServiceError::ServiceRequired | ServiceError::ServiceStopped => unavailable(),
        ServiceError::Io(cause)
            if service_probe(command)
                && matches!(
                    cause.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
        {
            unavailable()
        }
        ServiceError::ProviderRefused(_)
        | ServiceError::DestinationDenied
        | ServiceError::SourceCapacity
        | ServiceError::CaptureCapacity
        | ServiceError::PodcastCapacity
        | ServiceError::StorageQuota
        | ServiceError::IdempotencyConflict
        | ServiceError::RequestState
        | ServiceError::DirectoryRefreshRate
        | ServiceError::StaleCapture
        | ServiceError::Budget(_)
        | ServiceError::NativeContainment(_) => refused(),
        ServiceError::Remote { code, message } => {
            if code == "stopping" || message == &ServiceError::ServiceRequired.to_string() {
                unavailable()
            } else {
                remote_class(code)
            }
        }
        _ => Classified {
            code: "failure",
            exit: 1,
        },
    }
}

fn service_probe(command: &Command) -> bool {
    matches!(
        command,
        Command::Service {
            command: ServiceCommand::Status | ServiceCommand::Stop
        }
    )
}

fn remote_class(code: &str) -> Classified {
    match code {
        "provider_refused"
        | "destination_denied"
        | "capacity"
        | "storage_quota"
        | "budget_rejected"
        | "idempotency_conflict"
        | "refused" => refused(),
        _ => Classified {
            code: "failure",
            exit: 1,
        },
    }
}

fn unavailable() -> Classified {
    Classified {
        code: "service_unavailable",
        exit: 3,
    }
}

fn refused() -> Classified {
    Classified {
        code: "refused",
        exit: 4,
    }
}

/// The first backticked `sigy` command in a human failure, when one is present.
#[must_use]
pub(crate) fn next_command(message: &str) -> Option<String> {
    let start = message.find('`')?;
    let rest = message.get(start.saturating_add(1)..)?;
    let end = rest.find('`')?;
    let command = rest.get(..end)?;
    command.starts_with("sigy").then(|| command.to_owned())
}

pub(crate) fn error_document(message: &str, code: &str, next: Option<&str>) -> String {
    let mut object = serde_json::Map::new();
    object.insert(
        "error".to_owned(),
        serde_json::Value::String(message.to_owned()),
    );
    object.insert(
        "code".to_owned(),
        serde_json::Value::String(code.to_owned()),
    );
    if let Some(next) = next.filter(|next| !next.is_empty()) {
        object.insert(
            "next".to_owned(),
            serde_json::Value::String(next.to_owned()),
        );
    }
    serde_json::Value::Object(object).to_string()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn command(args: &[&str]) -> Result<super::super::Command, Box<dyn Error>> {
        Ok(super::super::Cli::try_parse_from(args)?.command)
    }

    #[test]
    fn each_documented_runtime_code_has_one_class() -> Result<(), Box<dyn Error>> {
        let refresh = command(&["sigy", "radio", "refresh"])?;
        let status = command(&["sigy", "service", "status"])?;
        let record = command(&["sigy", "record", "list"])?;
        let required = classify(&ServiceError::ServiceRequired, &refresh);
        assert_eq!((required.code, required.exit), ("service_unavailable", 3));
        let probe = classify(
            &ServiceError::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
            &status,
        );
        assert_eq!(probe.exit, 3);
        let other = classify(
            &ServiceError::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
            &record,
        );
        assert_eq!((other.code, other.exit), ("failure", 1));
        let refused = classify(
            &ServiceError::ProviderRefused("local overload never opens a paid route"),
            &refresh,
        );
        assert_eq!((refused.code, refused.exit), ("refused", 4));
        assert_ne!(refused.exit, 0);
        let partial = classify(&Partial, &refresh);
        assert_eq!((partial.code, partial.exit), ("partial", 5));
        assert_ne!(partial.exit, 0);
        let stopped = classify(&super::super::explorer::Interrupted, &refresh);
        assert_eq!((stopped.code, stopped.exit), ("interrupted", 130));
        let remote = classify(
            &ServiceError::remote("capacity", ServiceError::CaptureCapacity.to_string()),
            &refresh,
        );
        assert_eq!(remote.exit, 4);
        let next = next_command("Start it with `sigy service start`, then repeat");
        assert_eq!(next.as_deref(), Some("sigy service start"));
        assert_eq!(next_command("no command here"), None);
        let document = error_document("offline", "service_unavailable", next.as_deref());
        let value: serde_json::Value = serde_json::from_str(&document)?;
        assert_eq!(value["error"], "offline");
        assert_eq!(value["code"], "service_unavailable");
        assert_eq!(value["next"], "sigy service start");
        Ok(())
    }
}
