//! Typo hints for command names. Completions stay out of this build.

use clap::CommandFactory;

use crate::Cli;

/// Suggest one full command when a positional name is uniquely one or two edits away.
#[must_use]
pub(crate) fn suggestion(args: &[String]) -> Option<String> {
    let words = positionals(args);
    let first = words.first()?;
    let root = Cli::command();
    if let Some(child) = root.find_subcommand(first) {
        if !child.has_subcommands() {
            return None;
        }
        let second = words.get(1)?;
        if child.find_subcommand(second).is_some() {
            return None;
        }
        let best = unique_close(second, &names_of(child))?;
        return Some(format!("sigy {first} {best}"));
    }
    let best = unique_close(first, &names_of(&root))?;
    Some(format!("sigy {best}"))
}

fn positionals(args: &[String]) -> Vec<&str> {
    let mut skip_value = false;
    let mut words = Vec::new();
    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg.starts_with('-') {
            skip_value = arg == "--data-dir" || arg == "--color";
            continue;
        }
        words.push(arg.as_str());
    }
    words
}

fn names_of(command: &clap::Command) -> Vec<String> {
    command
        .get_subcommands()
        .map(|command| command.get_name().to_owned())
        .collect()
}

fn unique_close(word: &str, names: &[String]) -> Option<String> {
    if names.iter().any(|name| name == word) {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;
    for name in names {
        let score = distance(word, name);
        if score == 0 || score > 2 {
            continue;
        }
        match best {
            Some((best_score, _)) if score > best_score => {}
            Some((best_score, _)) if score == best_score => tied = true,
            _ => {
                tied = false;
                best = Some((score, name.as_str()));
            }
        }
    }
    if tied {
        None
    } else {
        best.map(|(_, name)| name.to_owned())
    }
}

fn distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.is_empty() {
        return right.len();
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (row, left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let cost = usize::from(left_char != right_char);
            let insert = current[column] + 1;
            let delete = previous[column + 1] + 1;
            let replace = previous[column] + cost;
            current[column + 1] = insert.min(delete).min(replace);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::{distance, suggestion, unique_close};
    use clap::Parser;

    #[test]
    fn radio_serch_suggests_search() {
        let args = ["radio".to_owned(), "serch".to_owned()];
        assert_eq!(suggestion(&args).as_deref(), Some("sigy radio search"));
        assert_eq!(
            suggestion(&["docter".to_owned()]).as_deref(),
            Some("sigy doctor")
        );
        assert!(suggestion(&["--help".to_owned()]).is_none());
        assert!(suggestion(&["radio".to_owned(), "--help".to_owned()]).is_none());
        assert!(suggestion(&["radio".to_owned(), "search".to_owned()]).is_none());
        assert!(suggestion(&["doctor".to_owned()]).is_none());
        assert_eq!(
            suggestion(&[
                "--data-dir".to_owned(),
                "lib".to_owned(),
                "docter".to_owned()
            ])
            .as_deref(),
            Some("sigy doctor")
        );
    }

    #[test]
    fn tied_names_and_long_edits_are_silent() {
        let names = ["aa".to_owned(), "ab".to_owned()];
        assert!(unique_close("a", &names).is_none());
        assert_eq!(distance("serch", "search"), 1);
        assert!(distance("serch", "podcast") > 2);
    }

    #[test]
    fn one_scope_uses_one_next_command() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let mut store = sigy_service::storage::Store::open(&root.path().join("catalog.sqlite3"))?;
        let doctor =
            sigy_service::control::apply(&mut store, sigy_service::control::Operation::Doctor {})?;
        let report = doctor.doctor.as_ref().ok_or("doctor report is missing")?;
        let directory = report
            .checks
            .iter()
            .find(|check| check.name == "directory")
            .ok_or("directory check")?;
        let empty = sigy_service::guidance::DirectoryHint::empty_fetch().command();
        assert_eq!(directory.command.as_deref(), Some(empty.as_str()));
        let rendered = crate::doctor_view::lines(&doctor, report, None);
        assert!(
            rendered.iter().any(|line| line.contains(&empty)),
            "{rendered:?}"
        );
        for line in &rendered {
            assert!(line.chars().count() <= 100, "{line}");
            if line.to_ascii_lowercase().contains("unmeasured") || line.contains("not measured yet")
            {
                assert!(!line.starts_with("ok"), "{line}");
            }
        }

        let searched = sigy_service::control::apply(
            &mut store,
            sigy_service::control::Operation::Radio {
                command: sigy_service::control::DirectoryOperation::Search {
                    filter: sigy_service::discovery::StationFilter::default(),
                    favorites_only: false,
                    after: None,
                    limit: 16,
                },
            },
        )?;
        let mut cli = crate::Cli::try_parse_from(["sigy", "radio", "search"])?;
        cli.explicit_data_dir = false;
        let crate::Command::Radio { command } = &cli.command else {
            return Err("search command".into());
        };
        let mut search_out = Vec::new();
        crate::radio::write_next(&mut search_out, command, &searched, &cli)?;
        let search_text = String::from_utf8(search_out)?;
        assert!(search_text.contains(&empty), "{search_text}");

        let country = sigy_service::discovery::StationFilter {
            country: "CA".into(),
            ..sigy_service::discovery::StationFilter::default()
        };
        let expected = sigy_service::guidance::DirectoryHint {
            filter: country.clone(),
            favorites: false,
            order: None,
            limit: None,
        }
        .command();
        let explored = sigy_service::control::apply(
            &mut store,
            sigy_service::control::Operation::Radio {
                command: sigy_service::control::DirectoryOperation::Explore {
                    filter: country.clone(),
                    favorites_only: false,
                },
            },
        )?;
        let mut explore = crate::Cli::try_parse_from(["sigy", "explore", "--country", "CA"])?;
        explore.explicit_data_dir = false;
        let crate::Command::Explore { filters, favorites } = &explore.command else {
            return Err("explore command".into());
        };
        let mut explore_out = Vec::new();
        crate::radio::write_explore_next(
            &mut explore_out,
            filters,
            *favorites,
            &explored,
            &explore,
        )?;
        let explore_text = String::from_utf8(explore_out)?;
        assert!(explore_text.contains(&expected), "{explore_text}");

        let mut model =
            crate::explorer::Explorer::new(crate::explorer::Modes::resolve(true, true, true), 0);
        model.initialize_search(country, None)?;
        assert_eq!(model.discovery_command(), expected);
        Ok(())
    }
}
