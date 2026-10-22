//! List explorer. The client uses the existing catalog owner and leaves on quit.

mod audio;
mod client;
pub(crate) mod context;
mod country;
mod fetch;
mod finding;
mod globe;
mod metadata;
mod monitor;
mod pause;
mod player;
mod retained;
mod screen;
mod search;
mod seek;
mod state;
mod terminal;
pub(crate) mod text;
mod timeline;

pub(crate) use globe::utc_label;

use std::time::{SystemTime, UNIX_EPOCH};
use std::{io::Write, path::Path};

pub(crate) use state::Explorer;
pub use state::{Emphasis, Modes};
pub(crate) use terminal::{
    Interrupted, await_stop, enable_session_modes, interactive_terminal, require_window,
    restored_modes, synchronized_output,
};

/// Plain refusal for a pipe, a redirect, or `TERM=dumb`.
#[derive(Debug)]
pub(crate) struct Refusal {
    next: String,
}

impl Refusal {
    pub(crate) const MESSAGE: &str = "The explorer needs an interactive terminal.";

    pub(crate) fn from_search(
        filter: &sigy_service::discovery::StationFilter,
        explicit_directory: Option<&str>,
    ) -> Self {
        let hint = sigy_service::guidance::DirectoryHint {
            filter: filter.clone(),
            favorites: false,
            order: None,
            limit: None,
        };
        let directory = explicit_directory
            .map(|value| text::sanitize(value, 4096))
            .filter(|value| !value.is_empty());
        Self {
            next: sigy_service::guidance::with_data_dir(&hint.command(), directory.as_deref()),
        }
    }

    #[must_use]
    pub(crate) fn next(&self) -> &str {
        &self.next
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\nNext: {}", Self::MESSAGE, self.next)
    }
}

impl std::error::Error for Refusal {}

pub fn run(
    data_dir: &Path,
    explicit_data_dir: bool,
    presentation: Modes,
    inspect: Option<&Path>,
    filter: sigy_service::discovery::StationFilter,
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    let now = now_ms()?;
    let mut model = Explorer::new(presentation, now);
    let explicit = explicit_data_dir.then(|| text::sanitize(&data_dir.display().to_string(), 4096));
    model.initialize_search(filter, explicit.as_deref())?;
    let desk = runtime.block_on(client::load_desk(data_dir, &model.current_search(), now))?;
    model.apply_desk(desk);
    terminal::drive(&runtime, data_dir, &mut model, inspect)?;
    if inspect.is_none() && model.rows().is_empty() {
        writeln!(
            std::io::stdout().lock(),
            "Current filters: {}",
            model.discovery_command()
        )?;
    }
    Ok(())
}

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    i64::try_from(millis).map_err(|_| "clock is out of range".into())
}

#[cfg(test)]
mod refusal_tests {
    use super::Refusal;

    #[test]
    fn refusal_keeps_filters_and_emits_no_escape() {
        let filter = sigy_service::discovery::StationFilter {
            name: "jazz".into(),
            country: "CA".into(),
            language: String::new(),
            tag: "news".into(),
            healthy_only: true,
        };
        let refusal = Refusal::from_search(&filter, Some("lib\u{1b}[31m"));
        let text = refusal.to_string();
        let expected = "The explorer needs an interactive terminal.\nNext: sigy --data-dir 'lib[31m' radio search --name 'jazz' --country 'CA' --tag 'news' --healthy --fetch";
        assert_eq!(text, expected);
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(
            text.chars()
                .filter(|character| *character != '\n')
                .all(|character| !character.is_control())
        );
        assert_eq!(
            text.lines().nth(1),
            Some(format!("Next: {}", refusal.next())).as_deref()
        );
    }
}
