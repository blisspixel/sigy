//! Next steps for common command failures. The cause stays; only a hint is added.

use std::{error::Error, io};

use sigy_service::Error as ServiceError;

use crate::{
    Cli, Command, LibraryCommand, dvr::RecordCommand, listen::ListenCommand,
    podcast::PodcastCommand, radio::PolicyCommand, radio::RadioCommand, schedule::ScheduleCommand,
    service::ServiceCommand, sources::PlaylistCommand, sources::SourceCommand,
};

type Failure = Box<dyn Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    NotFound,
    NoService,
    ServiceRequired,
    Busy,
    StationId,
}

/// Replaces a bare failure with what happened and what to run next, when that is known.
pub fn explain(cli: &Cli, error: Failure) -> Failure {
    let Some(kind) = classify(error.as_ref(), &cli.command) else {
        return error;
    };
    let same = if cli.explicit_data_dir {
        " with the same --data-dir"
    } else {
        ""
    };
    let message = match kind {
        Kind::NotFound => missing(&cli.command, same),
        Kind::NoService => no_service(&cli.command, same),
        Kind::ServiceRequired => format!(
            "this command needs the background service, and it is not running. Start it with `sigy service start`{same}, then repeat the command"
        ),
        Kind::Busy => busy(&cli.command, same),
        Kind::StationId => format!(
            "that is not a station ID. Station IDs are directory UUIDs; `sigy radio search`{same} prints one at the start of each result"
        ),
    };
    message.into()
}

fn classify(error: &(dyn Error + 'static), command: &Command) -> Option<Kind> {
    let service = error.downcast_ref::<ServiceError>()?;
    let remote = |expected: &ServiceError| matches!(service, ServiceError::Remote(message) if *message == expected.to_string());
    match service {
        ServiceError::NotFound => Some(Kind::NotFound),
        ServiceError::ServiceRequired => Some(Kind::ServiceRequired),
        ServiceError::LibraryBusy => Some(Kind::Busy),
        ServiceError::InvalidInput("station UUID") => Some(Kind::StationId),
        ServiceError::Io(cause)
            if matches!(
                command,
                Command::Service {
                    command: ServiceCommand::Status | ServiceCommand::Stop
                }
            ) && matches!(
                cause.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Some(Kind::NoService)
        }
        _ if remote(&ServiceError::NotFound) => Some(Kind::NotFound),
        _ if remote(&ServiceError::InvalidInput("station UUID")) => Some(Kind::StationId),
        _ => None,
    }
}

fn no_service(command: &Command, same: &str) -> String {
    if matches!(
        command,
        Command::Service {
            command: ServiceCommand::Stop
        }
    ) {
        return "no service is running for this library, so nothing was stopped".into();
    }
    format!("no service is running for this library. Start it with `sigy service start`{same}")
}

fn busy(command: &Command, same: &str) -> String {
    match command {
        Command::Library {
            command: LibraryCommand::Init,
        } => format!(
            "the background service owns this library, so it is already initialized. `sigy library status`{same} reads it through the service"
        ),
        Command::Service {
            command: ServiceCommand::Run { .. },
        } => format!(
            "a service is already running for this library. `sigy service status`{same} shows it"
        ),
        _ => format!(
            "the background service owns this library. Stop it with `sigy service stop`{same} before this command"
        ),
    }
}

fn missing(command: &Command, same: &str) -> String {
    let (what, list) = match command {
        Command::Radio { command } => radio_missing(command),
        Command::Record { command } => record_missing(command),
        Command::Listen { command } => listen_missing(command),
        Command::Source { command } => source_missing(command),
        Command::Schedule { command } => schedule_missing(command),
        Command::Podcast { command } => podcast_missing(command),
        Command::Monitor { .. } => (
            "that monitor or finding was not found".into(),
            "monitor list",
        ),
        Command::Task { .. } => ("that task was not found".into(), "task list"),
        _ => ("the named record was not found".into(), ""),
    };
    if list.is_empty() {
        return format!("{what}. Check the identifier");
    }
    format!("{what}. `sigy {list}`{same} shows existing IDs")
}

fn named(kind: &str, id: &str) -> String {
    format!(
        "no {kind} has ID {}",
        crate::explorer::text::sanitize(id, 128)
    )
}

fn radio_missing(command: &RadioCommand) -> (String, &'static str) {
    match command {
        RadioCommand::Show { id }
        | RadioCommand::Favorite { id }
        | RadioCommand::Unfavorite { id }
        | RadioCommand::Add { id, .. }
        | RadioCommand::Click { station: id, .. } => (named("cached station", id), "radio search"),
        RadioCommand::RefreshStatus { id } => (named("directory refresh", id), "radio status"),
        RadioCommand::ClickStatus { id } => (named("directory click request", id), ""),
        RadioCommand::Policy {
            command: PolicyCommand::Show { id: Some(id) } | PolicyCommand::Clear { id },
        } => (named("saved directory policy", id), "radio policy show"),
        _ => (
            "the named station or request was not found".into(),
            "radio search",
        ),
    }
}

fn record_missing(command: &RecordCommand) -> (String, &'static str) {
    match command {
        RecordCommand::Start { source, .. } | RecordCommand::Hls { source, .. } => {
            (named("registered source revision", source), "source list")
        }
        RecordCommand::Stop { id }
        | RecordCommand::Pause { id }
        | RecordCommand::Show { id }
        | RecordCommand::Path { id }
        | RecordCommand::Metadata { id }
        | RecordCommand::Keep { id }
        | RecordCommand::Archive { id }
        | RecordCommand::Temporary { id }
        | RecordCommand::Processed { id, .. }
        | RecordCommand::Delete { id }
        | RecordCommand::Hold { id, .. } => (named("recording", id), "record list"),
        RecordCommand::List { .. } => ("the named recording was not found".into(), "record list"),
    }
}

fn listen_missing(command: &ListenCommand) -> (String, &'static str) {
    match command {
        ListenCommand::File { id, .. } | ListenCommand::Attach { recording: id, .. } => {
            (named("recording", id), "record list")
        }
        ListenCommand::Source { revision, .. } => {
            (named("registered source revision", revision), "source list")
        }
        ListenCommand::Stop { id } | ListenCommand::Status { id } => (named("listen", id), ""),
        ListenCommand::Pause { session }
        | ListenCommand::Live { session }
        | ListenCommand::Seek { session, .. }
        | ListenCommand::Play { session, .. }
        | ListenCommand::Detach { session }
        | ListenCommand::Session { session } => (
            format!(
                "{}. Playheads last only while the service runs; attach again with `sigy listen attach`",
                named("attached playhead", session)
            ),
            "",
        ),
    }
}

fn source_missing(command: &SourceCommand) -> (String, &'static str) {
    match command {
        SourceCommand::Show { revision_id } => (
            named("registered source revision", revision_id),
            "source list",
        ),
        SourceCommand::Playlist {
            command: PlaylistCommand::Resolve { revision, .. },
        } => (named("registered source revision", revision), "source list"),
        SourceCommand::Playlist {
            command: PlaylistCommand::Status { id } | PlaylistCommand::Accept { id, .. },
        } => (named("playlist request", id), ""),
        SourceCommand::Add { .. } | SourceCommand::List { .. } => {
            ("the named source was not found".into(), "source list")
        }
    }
}

fn schedule_missing(command: &ScheduleCommand) -> (String, &'static str) {
    match command {
        ScheduleCommand::Show { id } | ScheduleCommand::Revise { id, .. } => {
            (named("schedule rule", id), "schedule list")
        }
        ScheduleCommand::Create { source, .. } => (
            format!(
                "{}, or the named monitor does not exist",
                named("registered source revision", source)
            ),
            "source list",
        ),
        ScheduleCommand::List { .. } => {
            ("the named schedule was not found".into(), "schedule list")
        }
    }
}

fn podcast_missing(command: &PodcastCommand) -> (String, &'static str) {
    match command {
        PodcastCommand::RefreshStatus { id } => (named("feed refresh", id), ""),
        PodcastCommand::TextShow { id } => (named("publisher text fetch", id), ""),
        PodcastCommand::Text { subscription, .. }
        | PodcastCommand::Download { subscription, .. } => (
            format!(
                "the subscription {} or its episode was not found. `sigy podcast episodes SUBSCRIPTION` lists episode IDs",
                crate::explorer::text::sanitize(subscription, 128)
            ),
            "podcast list",
        ),
        PodcastCommand::Show { id }
        | PodcastCommand::Unsubscribe { id }
        | PodcastCommand::Refresh {
            subscription: id, ..
        }
        | PodcastCommand::Episodes {
            subscription: id, ..
        } => (named("podcast subscription", id), "podcast list"),
        PodcastCommand::Subscribe { .. } | PodcastCommand::List { .. } => (
            "the named subscription was not found".into(),
            "podcast list",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::explain;
    use crate::Cli;
    use clap::Parser;
    use sigy_service::Error as ServiceError;

    fn message(words: &[&str], error: ServiceError) -> Result<String, Box<dyn std::error::Error>> {
        let mut cli = Cli::try_parse_from(words)?;
        cli.explicit_data_dir = cli.data_dir.is_some();
        Ok(explain(&cli, Box::new(error)).to_string())
    }

    #[test]
    fn missing_records_name_the_identifier_and_the_listing_command()
    -> Result<(), Box<dyn std::error::Error>> {
        let local = message(
            &["sigy", "record", "show", "evening"],
            ServiceError::NotFound,
        )?;
        assert_eq!(
            local,
            "no recording has ID evening. `sigy record list` shows existing IDs"
        );
        let remote = message(
            &[
                "sigy",
                "--data-dir",
                "lib",
                "radio",
                "refresh-status",
                "first",
            ],
            ServiceError::Remote(ServiceError::NotFound.to_string()),
        )?;
        assert!(
            remote.contains("no directory refresh has ID first"),
            "{remote}"
        );
        assert!(
            remote.contains("`sigy radio status` with the same --data-dir"),
            "{remote}"
        );
        let source = message(
            &[
                "sigy",
                "record",
                "start",
                "r1",
                "--source",
                "news\u{1b}[2J:v1",
            ],
            ServiceError::NotFound,
        )?;
        assert!(
            source.contains("no registered source revision has ID news[2J:v1"),
            "{source}"
        );
        assert!(!source.contains('\u{1b}'));
        assert!(source.contains("`sigy source list`"));
        for words in [
            &["sigy", "radio", "favorite", "x"][..],
            &["sigy", "listen", "file", "x"],
            &["sigy", "listen", "seek", "x", "--seek-us", "1"],
            &["sigy", "source", "playlist", "status", "x"],
            &[
                "sigy", "schedule", "create", "x", "--source", "s", "--zone", "UTC",
            ],
            &[
                "sigy",
                "podcast",
                "download",
                "x",
                "--episode",
                "e",
                "--id",
                "d",
                "--revision",
                "r",
            ],
            &["sigy", "podcast", "episodes", "x"],
            &["sigy", "monitor", "show", "x"],
            &["sigy", "task", "show", "x"],
            &["sigy", "analysis", "show", "x"],
        ] {
            let text = message(words, ServiceError::NotFound)?;
            assert!(!text.contains("record not found"), "{text}");
            assert!(
                text.contains('x') || text.contains("Check the identifier"),
                "{text}"
            );
        }
        Ok(())
    }

    #[test]
    fn service_state_failures_say_what_to_run() -> Result<(), Box<dyn std::error::Error>> {
        let missing = || ServiceError::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
        let status = message(&["sigy", "service", "status"], missing())?;
        assert!(status.starts_with("no service is running"), "{status}");
        assert!(status.contains("`sigy service start`"));
        let stop = message(&["sigy", "--data-dir", "lib", "service", "stop"], missing())?;
        assert_eq!(
            stop,
            "no service is running for this library, so nothing was stopped"
        );
        let explicit = message(
            &["sigy", "--data-dir", "lib", "service", "status"],
            missing(),
        )?;
        assert!(
            explicit.ends_with("`sigy service start` with the same --data-dir"),
            "{explicit}"
        );
        let other = message(&["sigy", "record", "list"], missing())?;
        assert!(other.starts_with("filesystem operation failed"), "{other}");
        let required = message(
            &["sigy", "radio", "refresh", "a"],
            ServiceError::ServiceRequired,
        )?;
        assert!(required.contains("`sigy service start`"), "{required}");
        let busy = message(&["sigy", "library", "init"], ServiceError::LibraryBusy)?;
        assert!(busy.contains("already initialized"), "{busy}");
        let run = message(&["sigy", "service", "run"], ServiceError::LibraryBusy)?;
        assert!(run.contains("already running"), "{run}");
        let backup = message(&["sigy", "library", "status"], ServiceError::LibraryBusy)?;
        assert!(backup.contains("`sigy service stop`"), "{backup}");
        let station = message(
            &["sigy", "radio", "show", "bad"],
            ServiceError::Remote(ServiceError::InvalidInput("station UUID").to_string()),
        )?;
        assert!(station.contains("`sigy radio search`"), "{station}");
        let untouched = message(&["sigy", "radio", "show", "bad"], ServiceError::Timeout)?;
        assert_eq!(untouched, ServiceError::Timeout.to_string());
        Ok(())
    }
}
