//! Copyable next commands. A placeholder remains only when the person must choose it.

use std::fmt::Write;

use crate::discovery::StationFilter;

/// Search page order. Omitted from a command when the caller uses the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOrder {
    /// UUID order.
    Id,
    /// Pinned Unicode name order.
    Name,
}

/// One directory search the user can paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryHint {
    /// Cached-station filters. Empty fields are left out of the command.
    pub filter: StationFilter,
    /// Local favorites only. That command does not fetch.
    pub favorites: bool,
    /// Present only when the user selected an order.
    pub order: Option<SearchOrder>,
    /// Present only when the user selected a page size.
    pub limit: Option<u32>,
}

impl DirectoryHint {
    /// Empty cache, with no filters the user typed.
    #[must_use]
    pub fn empty_fetch() -> Self {
        Self {
            filter: StationFilter::default(),
            favorites: false,
            order: None,
            limit: None,
        }
    }

    /// `sigy radio search` plus the filters, favorites, order, limit and fetch flag.
    #[must_use]
    pub fn command(&self) -> String {
        let mut command = String::from("sigy radio search");
        push_filters(&mut command, &self.filter);
        if self.favorites {
            command.push_str(" --favorites");
        }
        if let Some(limit) = self.limit {
            let _ = write!(command, " --limit {limit}");
        }
        if let Some(order) = self.order {
            command.push_str(match order {
                SearchOrder::Id => " --order id",
                SearchOrder::Name => " --order name",
            });
        }
        if !self.favorites {
            command.push_str(" --fetch");
        }
        command
    }
}

/// Retry one directory fetch. The request ID is omitted because Sigy generates it.
#[must_use]
pub fn refresh_retry(
    filter: &StationFilter,
    limit: u32,
    offset: u32,
    mirror: Option<&str>,
    pin: Option<&str>,
) -> String {
    let mut command = String::from("sigy radio refresh");
    push_filters(&mut command, filter);
    if let Some(mirror) = mirror.filter(|value| !value.is_empty()) {
        let _ = write!(command, " --mirror {}", shell_quote(mirror));
    }
    if let Some(pin) = pin.filter(|value| !value.is_empty()) {
        let _ = write!(command, " --pin-address {pin}");
    }
    let _ = write!(command, " --limit {limit}");
    if offset > 0 {
        let _ = write!(command, " --offset {offset}");
    }
    command.push_str(" --wait");
    command
}

/// Inspect one refresh that already has an ID.
#[must_use]
pub fn refresh_status(id: &str) -> String {
    format!("sigy radio refresh-status {}", shell_quote(id))
}

/// Insert `--data-dir` after `sigy` when the user passed one.
#[must_use]
pub fn with_data_dir(command: &str, data_dir: Option<&str>) -> String {
    let Some(directory) = data_dir.filter(|value| !value.is_empty()) else {
        return command.to_owned();
    };
    let quoted = shell_quote(directory);
    match command.strip_prefix("sigy") {
        Some(rest) => format!("sigy --data-dir {quoted}{rest}"),
        None => format!("sigy --data-dir {quoted} {command}"),
    }
}

/// Quote one shell argument for the local platform.
#[must_use]
pub fn shell_quote(value: &str) -> String {
    #[cfg(windows)]
    let value = value.replace('\'', "''");
    #[cfg(not(windows))]
    let value = value.replace('\'', "'\"'\"'");
    format!("'{value}'")
}

fn push_filters(command: &mut String, filter: &StationFilter) {
    for (flag, value) in [
        ("name", filter.name.as_str()),
        ("country", filter.country.as_str()),
        ("language", filter.language.as_str()),
        ("tag", filter.tag.as_str()),
    ] {
        if !value.is_empty() {
            let _ = write!(command, " --{flag} {}", shell_quote(value));
        }
    }
    if filter.healthy_only {
        command.push_str(" --healthy");
    }
}

#[cfg(test)]
mod tests {
    use super::{DirectoryHint, refresh_retry, shell_quote, with_data_dir};
    use crate::discovery::StationFilter;

    #[test]
    fn empty_fetch_omits_generated_ids_and_starts_with_sigy() {
        let command = DirectoryHint::empty_fetch().command();
        assert_eq!(command, "sigy radio search --fetch");
        assert!(!command.contains("NEW_ID"));
    }

    #[test]
    fn filters_favorites_and_explicit_page_controls_are_preserved() {
        let hint = DirectoryHint {
            filter: StationFilter {
                name: "Québec".into(),
                country: "CA".into(),
                language: String::new(),
                tag: "news".into(),
                healthy_only: true,
            },
            favorites: false,
            order: Some(super::SearchOrder::Name),
            limit: None,
        };
        assert_eq!(
            hint.command(),
            "sigy radio search --name 'Québec' --country 'CA' --tag 'news' --healthy --order name --fetch"
        );
        let favorites = DirectoryHint {
            favorites: true,
            order: None,
            limit: None,
            ..hint
        };
        assert!(favorites.command().contains("--favorites"));
        assert!(!favorites.command().contains("--fetch"));
        assert_eq!(
            refresh_retry(
                &StationFilter {
                    name: "news".into(),
                    ..StationFilter::default()
                },
                100,
                0,
                Some("https://example.test"),
                None,
            ),
            "sigy radio refresh --name 'news' --mirror 'https://example.test' --limit 100 --wait"
        );
    }

    #[test]
    fn data_dir_is_inserted_once_after_sigy() {
        assert_eq!(
            with_data_dir("sigy radio search --fetch", None),
            "sigy radio search --fetch"
        );
        assert_eq!(
            with_data_dir("sigy radio search --fetch", Some("lib")),
            format!(
                "sigy --data-dir {} radio search --fetch",
                shell_quote("lib")
            )
        );
        assert_eq!(with_data_dir("sigy", Some("")), "sigy");
    }
}
