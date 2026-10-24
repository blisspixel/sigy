//! A tagged release must name itself in CHANGELOG.md.

use std::{env, fs, path::Path};

pub(crate) fn check_release_changelog(root: &Path) -> Result<(), String> {
    let Some(tag) = release_tag() else {
        return Ok(());
    };
    let changelog = fs::read_to_string(root.join("CHANGELOG.md"))
        .map_err(|error| format!("could not read CHANGELOG.md: {error}"))?;
    require_heading(&changelog, &tag)
}

pub(crate) fn require_heading(changelog: &str, tag: &str) -> Result<(), String> {
    if heading_present(changelog, tag) {
        Ok(())
    } else {
        Err(format!(
            "CHANGELOG.md has no heading for release {tag}; add ## {version} before publishing",
            version = tag.trim().trim_start_matches('v')
        ))
    }
}

fn release_tag() -> Option<String> {
    if let Ok(tag) = env::var("SIGY_RELEASE_TAG") {
        let tag = tag.trim();
        if !tag.is_empty() {
            return Some(tag.to_owned());
        }
    }
    if env::var("GITHUB_REF_TYPE").ok().as_deref() == Some("tag") {
        let tag = env::var("GITHUB_REF_NAME").ok()?;
        let tag = tag.trim();
        if !tag.is_empty() {
            return Some(tag.to_owned());
        }
    }
    None
}

pub(crate) fn heading_present(changelog: &str, tag: &str) -> bool {
    let version = tag.trim().trim_start_matches('v');
    if version.is_empty() {
        return false;
    }
    changelog.lines().any(|line| {
        let Some(rest) = line.strip_prefix("## ") else {
            return false;
        };
        let Some(rest) = rest.strip_prefix(version) else {
            return false;
        };
        rest.is_empty() || rest.starts_with(' ') || rest.starts_with('(')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_heading_matches_its_tag_and_not_a_longer_version() {
        let changelog = "\
## Unreleased

Nothing yet.

## 0.1.0-dev.20261011.3 (development prerelease, source only)
";
        assert!(heading_present(changelog, "v0.1.0-dev.20261011.3"));
        assert!(heading_present(changelog, "0.1.0-dev.20261011.3"));
        assert!(!heading_present(changelog, "v0.1.0-dev.20261011.30"));
        assert!(!heading_present(changelog, "v0.1.0-dev.20261011"));
        assert!(!heading_present(changelog, ""));
    }

    #[test]
    fn the_workspace_changelog_names_every_recorded_prerelease() -> Result<(), String> {
        let root = crate::workspace_root().map_err(|error| error.to_string())?;
        let changelog = fs::read_to_string(root.join("CHANGELOG.md"))
            .map_err(|error| format!("could not read CHANGELOG.md: {error}"))?;
        for version in [
            "0.1.0-dev.20260925",
            "0.1.0-dev.20260930",
            "0.1.0-dev.20260930.1",
            "0.1.0-dev.20261001",
            "0.1.0-dev.20261001.1",
            "0.1.0-dev.20261001.2",
            "0.1.0-dev.20261001.3",
            "0.1.0-dev.20261003",
            "0.1.0-dev.20261003.1",
            "0.1.0-dev.20261003.2",
            "0.1.0-dev.20261003.3",
            "0.1.0-dev.20261004",
            "0.1.0-dev.20261004.1",
            "0.1.0-dev.20261004.2",
            "0.1.0-dev.20261010",
            "0.1.0-dev.20261010.1",
            "0.1.0-dev.20261011",
            "0.1.0-dev.20261011.1",
            "0.1.0-dev.20261011.2",
            "0.1.0-dev.20261011.3",
            "0.1.0-dev.20261020",
            "0.1.0-dev.20261024",
        ] {
            assert!(
                heading_present(&changelog, version),
                "missing changelog heading {version}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_missing_heading_is_a_release_error() {
        let Err(message) = require_heading("# Changelog\n\n## Unreleased\n", "v0.0.0-missing")
        else {
            panic!("a missing heading must fail");
        };
        assert!(message.contains("0.0.0-missing"), "{message}");
        assert!(
            require_heading(
                "## 0.1.0-dev.20261011.3 (development prerelease, source only)\n",
                "v0.1.0-dev.20261011.3"
            )
            .is_ok()
        );
    }
}
