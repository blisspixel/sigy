//! Per-workspace-package line coverage, with no package or source exclusions.

use std::{collections::BTreeSet, fs, path::Path, process::Command};

use serde::Deserialize;

use crate::Error;

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    target_directory: std::path::PathBuf,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: String,
}

#[derive(Debug, Deserialize)]
struct Report {
    data: Vec<Dataset>,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct Dataset {
    files: Vec<File>,
}

#[derive(Debug, Deserialize)]
struct File {
    filename: String,
    summary: Summary,
}

#[derive(Debug, Deserialize)]
struct Summary {
    lines: Lines,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
struct Lines {
    count: u64,
    covered: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct CrateCoverage {
    name: String,
    lines: Lines,
}

pub(super) fn verify(root: &Path, ffmpeg: &Path) -> Result<(), Error> {
    let metadata = cargo_output(
        root,
        &["metadata", "--locked", "--no-deps", "--format-version", "1"],
        &[],
    )?;
    let parsed: Metadata = serde_json::from_slice(&metadata)
        .map_err(|error| Error::Check(format!("coverage workspace metadata: {error}")))?;
    let (_lock, target, report) = prepare(&parsed.target_directory)?;
    collect(root, &target, ffmpeg)?;
    let report_text = report
        .to_str()
        .ok_or_else(|| Error::Check("coverage report path is not Unicode".into()))?;
    crate::run_cargo_with(
        root,
        &[
            "llvm-cov",
            "report",
            "--locked",
            "--json",
            "--summary-only",
            "--include-build-script",
            "--no-default-ignore-filename-regex",
            "--output-path",
            report_text,
        ],
        &[("CARGO_LLVM_COV_TARGET_DIR", target.as_os_str())],
        None,
    )?;
    eprintln!("Coverage report: {}", report.display());
    validate_report(&metadata, &fs::read(&report)?)
}

/// Validate an existing receipt without collecting coverage or launching tests.
pub(super) fn verify_report(manifest: &Path, report: &Path) -> Result<(), Error> {
    let manifest = fs::canonicalize(manifest)?;
    let root = manifest
        .parent()
        .ok_or_else(|| Error::Check("coverage manifest has no parent directory".into()))?;
    let manifest_text = manifest
        .to_str()
        .ok_or_else(|| Error::Check("coverage manifest path is not Unicode".into()))?;
    let bytes = fs::read(report)?;
    let metadata = cargo_output(
        root,
        &[
            "metadata",
            "--manifest-path",
            manifest_text,
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ],
        &[],
    )?;
    eprintln!("Coverage report: {}", report.display());
    validate_report(&metadata, &bytes)
}

fn validate_report(metadata: &[u8], report: &[u8]) -> Result<(), Error> {
    let rows = measure(metadata, report).map_err(Error::Check)?;
    validate_owned_files(metadata, report).map_err(Error::Check)?;
    for row in &rows {
        let hundredths = u128::from(row.lines.covered) * 10_000 / u128::from(row.lines.count);
        eprintln!(
            "{}: {}.{}% lines ({}/{})",
            row.name,
            hundredths / 100,
            format_args!("{:02}", hundredths % 100),
            row.lines.covered,
            row.lines.count
        );
    }
    enforce(&rows).map_err(Error::Check)
}

fn validate_owned_files(metadata: &[u8], report: &[u8]) -> Result<(), String> {
    let metadata: Metadata = serde_json::from_slice(metadata).map_err(|error| error.to_string())?;
    let report: Report = serde_json::from_slice(report).map_err(|error| error.to_string())?;
    let roots = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .map(|package| {
            let manifest = fs::canonicalize(&package.manifest_path)
                .map_err(|error| format!("coverage manifest {}: {error}", package.manifest_path))?;
            let parent = manifest.parent().ok_or("coverage manifest has no parent")?;
            Ok(format!("{}/", normalize(&parent.to_string_lossy())))
        })
        .collect::<Result<Vec<_>, String>>()?;
    for file in &report.data[0].files {
        let path = normalize(&file.filename);
        if roots.iter().any(|root| path.starts_with(root)) {
            let canonical = fs::canonicalize(&file.filename)
                .map_err(|error| format!("coverage source {}: {error}", file.filename))?;
            if !canonical.is_file() || normalize(&canonical.to_string_lossy()) != path {
                return Err(format!(
                    "coverage source path is not canonical: {}",
                    file.filename
                ));
            }
        }
    }
    Ok(())
}

fn prepare(
    target_root: &Path,
) -> Result<(fs::File, std::path::PathBuf, std::path::PathBuf), Error> {
    fs::create_dir_all(target_root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(target_root.join("coverage.lock"))?;
    lock.try_lock().map_err(|error| match error {
        fs::TryLockError::WouldBlock => {
            Error::Check("another coverage verification owns this target directory".into())
        }
        fs::TryLockError::Error(error) => Error::Io(error),
    })?;
    let target = target_root.join("llvm-cov-target");
    fs::create_dir_all(&target)?;
    let reports = target_root.join("coverage-reports");
    fs::create_dir_all(&reports)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| Error::Check(error.to_string()))?
        .as_nanos();
    let report = reports.join(format!("workspace-{}-{stamp}.json", std::process::id()));
    fs::File::create_new(&report)?;
    Ok((lock, target, report))
}

fn collect(root: &Path, target: &Path, ffmpeg: &Path) -> Result<(), Error> {
    let target_value = target.as_os_str();
    let environment = [("CARGO_LLVM_COV_TARGET_DIR", target_value)];
    crate::run_cargo_with(
        root,
        &["llvm-cov", "clean", "--workspace"],
        &environment,
        None,
    )?;
    crate::run_cargo_with(
        root,
        &[
            "llvm-cov",
            "--workspace",
            "--locked",
            "--no-report",
            "--",
            "--test-threads=2",
        ],
        &environment,
        None,
    )?;

    // Build the native fault fixture with the same instrumentation and artifact
    // directory. Read assignments as data, never execute a shell expression.
    let assignments = cargo_output(root, &["llvm-cov", "show-env", "--pwsh"], &environment)?;
    let assignments = parse_environment(&assignments)?;
    let mut process = Command::new(crate::cargo_program());
    process
        .current_dir(root)
        .args(["build", "--locked", "-p", "sigy-test-recognizer"]);
    process.envs(assignments).env("CARGO_TARGET_DIR", target);
    if !process.status()?.success() {
        return Err(Error::Cargo("build coverage native fixture".into()));
    }

    crate::run_cargo_with(
        root,
        &[
            "llvm-cov",
            "--workspace",
            "--locked",
            "--no-report",
            "--",
            "--ignored",
            "--test-threads=1",
        ],
        &[
            ("CARGO_LLVM_COV_TARGET_DIR", target_value),
            ("SIGY_TEST_FFMPEG", ffmpeg.as_os_str()),
        ],
        None,
    )?;
    Ok(())
}

fn cargo_output(
    root: &Path,
    args: &[&str],
    environment: &[(&str, &std::ffi::OsStr)],
) -> Result<Vec<u8>, Error> {
    let mut process = Command::new(crate::cargo_program());
    process.current_dir(root).args(args);
    for (key, value) in environment {
        process.env(key, value);
    }
    let output = process.output()?;
    if !output.status.success() {
        return Err(Error::Check(format!(
            "cargo {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output.stdout)
}

fn parse_environment(bytes: &[u8]) -> Result<Vec<(String, String)>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|error| Error::Check(error.to_string()))?;
    text.lines()
        .map(|line| {
            let (key, value) = line
                .strip_prefix("$env:")
                .ok_or_else(|| Error::Check("invalid coverage environment prefix".into()))?
                .split_once('=')
                .ok_or_else(|| Error::Check("invalid coverage environment assignment".into()))?;
            if key.is_empty()
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(Error::Check("invalid coverage environment key".into()));
            }
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .ok_or_else(|| Error::Check("invalid coverage environment quoting".into()))?;
            Ok((key.to_owned(), decode_unicode(value)?))
        })
        .collect()
}

// cargo-llvm-cov --pwsh encodes every character as `u{hex}, on every host.
// Decode this documented data format without evaluating a shell expression.
fn decode_unicode(mut encoded: &str) -> Result<String, Error> {
    let mut value = String::new();
    while !encoded.is_empty() {
        let (hex, rest) = encoded
            .strip_prefix("`u{")
            .and_then(|tail| tail.split_once('}'))
            .ok_or_else(|| Error::Check("invalid coverage Unicode escape".into()))?;
        let ch = u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| Error::Check("invalid coverage Unicode scalar".into()))?;
        value.push(ch);
        encoded = rest;
    }
    Ok(value)
}

fn normalize(path: &str) -> String {
    let path = path
        .strip_prefix("\\\\?\\")
        .unwrap_or(path)
        .replace('\\', "/");
    if cfg!(windows) {
        path.to_ascii_lowercase()
    } else {
        path
    }
}

fn measure(metadata: &[u8], report: &[u8]) -> Result<Vec<CrateCoverage>, String> {
    let metadata: Metadata = serde_json::from_slice(metadata).map_err(|error| error.to_string())?;
    let report: Report = serde_json::from_slice(report).map_err(|error| error.to_string())?;
    if report.kind != "llvm.coverage.json.export" || report.data.len() != 1 {
        return Err("expected one LLVM coverage dataset".into());
    }
    let packages: Vec<_> = metadata
        .packages
        .into_iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .collect();
    if packages.is_empty() || packages.len() != metadata.workspace_members.len() {
        return Err("workspace package inventory is incomplete".into());
    }
    let mut rows: Vec<_> = packages
        .iter()
        .map(|package| CrateCoverage {
            name: package.name.clone(),
            lines: Lines::default(),
        })
        .collect();
    let roots: Vec<_> = packages
        .iter()
        .map(|package| {
            let manifest = normalize(&package.manifest_path);
            manifest
                .strip_suffix("Cargo.toml")
                .or_else(|| manifest.strip_suffix("cargo.toml"))
                .map(str::to_owned)
                .ok_or_else(|| "invalid workspace manifest path".to_owned())
        })
        .collect::<Result<_, _>>()?;
    let mut seen = BTreeSet::new();
    for file in &report.data[0].files {
        let path = normalize(&file.filename);
        if !seen.insert(path.clone()) || file.summary.lines.covered > file.summary.lines.count {
            return Err("duplicate coverage file or impossible line counts".into());
        }
        let owners: Vec<_> = roots
            .iter()
            .enumerate()
            .filter(|(_, root)| path.starts_with(root.as_str()))
            .map(|(index, _)| index)
            .collect();
        if owners.len() > 1 {
            return Err("coverage file has ambiguous package ownership".into());
        }
        if let Some(index) = owners.first() {
            let lines = &mut rows[*index].lines;
            lines.count = lines
                .count
                .checked_add(file.summary.lines.count)
                .ok_or("coverage line count overflow")?;
            lines.covered = lines
                .covered
                .checked_add(file.summary.lines.covered)
                .ok_or("covered line count overflow")?;
        }
    }
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    if rows.iter().any(|row| row.lines.count == 0) {
        return Err("workspace crate has no measured executable lines".into());
    }
    Ok(rows)
}

fn enforce(rows: &[CrateCoverage]) -> Result<(), String> {
    let shortfalls: Vec<_> = rows
        .iter()
        .filter(|row| u128::from(row.lines.covered) * 100 < u128::from(row.lines.count) * 80)
        .map(|row| row.name.as_str())
        .collect();
    if shortfalls.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "80% line coverage required per crate; below threshold: {}",
            shortfalls.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_runs_cannot_share_profiles_and_receipts_are_never_replaced() -> Result<(), Error>
    {
        let root = tempfile::tempdir()?;
        let (lock, _, first) = prepare(root.path())?;
        fs::write(&first, b"prior measured receipt")?;
        assert!(prepare(root.path()).is_err());
        drop(lock);
        let (_lock, _, second) = prepare(root.path())?;
        assert_ne!(first, second);
        assert_eq!(fs::read(first)?, b"prior measured receipt");
        Ok(())
    }

    fn metadata() -> Vec<u8> {
        br#"{"packages":[{"id":"a","name":"alpha","manifest_path":"/project/crates/a/Cargo.toml"},{"id":"b","name":"beta","manifest_path":"/project/crates/b/Cargo.toml"}],"workspace_members":["a","b"],"target_directory":"/unused"}"#.to_vec()
    }

    fn report(files: &[(&str, u64, u64)]) -> Result<Vec<u8>, serde_json::Error> {
        let files: Vec<_> = files
            .iter()
            .map(|(path, count, covered)| {
                serde_json::json!({
                    "filename": path, "summary": {"lines": {"count": count, "covered": covered}}
                })
            })
            .collect();
        serde_json::to_vec(
            &serde_json::json!({"type":"llvm.coverage.json.export", "data":[{"files":files}]}),
        )
    }

    #[test]
    fn each_crate_must_pass_even_when_the_workspace_average_passes()
    -> Result<(), Box<dyn std::error::Error>> {
        let rows = measure(
            &metadata(),
            &report(&[
                ("/project/crates/a/src/lib.rs", 1000, 1000),
                ("/project/crates/b/src/lib.rs", 100, 79),
            ])?,
        )?;
        let error = enforce(&rows).err().ok_or("under-covered crate passed")?;
        assert!(error.ends_with("beta"));
        let rows = measure(
            &metadata(),
            &report(&[
                ("/project/crates/a/src/lib.rs", 100, 80),
                ("/project/crates/b/src/lib.rs", 5, 4),
            ])?,
        )?;
        enforce(&rows)?;
        Ok(())
    }

    #[test]
    fn sums_all_package_files_without_counting_dependencies_or_prefix_neighbors()
    -> Result<(), Box<dyn std::error::Error>> {
        let rows = measure(
            &metadata(),
            &report(&[
                ("/project/crates/a/src/lib.rs", 10, 6),
                ("/project/crates/a/tests/integration.rs", 10, 10),
                ("/project/crates/ab/src/lib.rs", 100, 0),
                ("/registry/dependency/src/lib.rs", 100, 0),
                ("/project/crates/b/src/lib.rs", 20, 16),
            ])?,
        )?;
        assert_eq!(
            rows[0].lines,
            Lines {
                count: 20,
                covered: 16
            }
        );
        assert_eq!(
            rows[1].lines,
            Lines {
                count: 20,
                covered: 16
            }
        );
        enforce(&rows)?;
        Ok(())
    }

    #[test]
    fn missing_crates_duplicates_and_impossible_counts_fail_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        for files in [
            vec![("/project/crates/a/src/lib.rs", 1, 1)],
            vec![("/project/crates/a/src/lib.rs", 1, 2)],
            vec![
                ("/project/crates/a/src/lib.rs", 1, 1),
                ("/project/crates/a/src/lib.rs", 1, 1),
            ],
            vec![
                ("/project/crates/a/src/lib.rs", 0, 0),
                ("/project/crates/b/src/lib.rs", 1, 1),
            ],
            vec![
                ("/project/crates/a/src/lib.rs", u64::MAX, 1),
                ("/project/crates/a/src/main.rs", 1, 1),
            ],
        ] {
            assert!(measure(&metadata(), &report(&files)?).is_err());
        }
        assert!(measure(b"{}", &report(&[])?).is_err());
        assert!(measure(&metadata(), b"{}").is_err());
        assert!(measure(&metadata(), br#"{"type":"other","data":[]}"#).is_err());
        Ok(())
    }

    #[test]
    fn exact_threshold_does_not_round_a_shortfall_up() {
        let rows = [CrateCoverage {
            name: "large".into(),
            lines: Lines {
                count: u64::MAX,
                covered: u64::MAX / 5 * 4 - 1,
            },
        }];
        assert!(enforce(&rows).is_err());
    }

    #[test]
    fn environment_assignments_preserve_paths_and_are_never_evaluated() -> Result<(), Error> {
        for original in [
            "C:\\path with spaces\\%p.profraw",
            "/tmp/it's quoted/\"runtime\"",
            "-C\x1finstrument-coverage",
            "$(untrusted);&",
            "العربية",
            "",
        ] {
            let encoded = original.escape_unicode().to_string().replace('\\', "`");
            let assignment = format!("$env:KEY=\"{encoded}\"\n");
            assert_eq!(
                parse_environment(assignment.as_bytes())?,
                vec![("KEY".into(), original.into())]
            );
        }
        assert!(parse_environment(b"broken").is_err());
        assert!(parse_environment(b"bad key=value").is_err());
        for invalid in [
            "$env:bad key=\"\"",
            "$env:KEY=value",
            "$env:KEY=\"`u{d800}\"",
            "$env:KEY=\"`u{110000}\"",
            "$env:KEY=\"`u{gg}\"",
            "$env:KEY=\"literal\"",
            "$env:KEY=\"`u{31\"",
        ] {
            assert!(parse_environment(invalid.as_bytes()).is_err());
        }
        assert!(parse_environment(&[255]).is_err());
        Ok(())
    }
}
