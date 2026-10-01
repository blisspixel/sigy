//! Existing receipts can be checked without collecting profiles or running models.

use std::{fs, path::Path, process::Command};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn invoke(root: &Path, manifest: &Path, report: &Path) -> std::io::Result<std::process::Output> {
    Command::new(env!("CARGO_BIN_EXE_sigy-xtask"))
        .arg("verify-coverage-report")
        .arg(manifest)
        .arg(report)
        .env("CARGO", env!("CARGO_BIN_EXE_sigy-test-cargo"))
        .env("SIGY_TEST_CARGO_LOG", root.join("commands.log"))
        .env("SIGY_TEST_CARGO_METADATA", root.join("metadata.json"))
        .env_remove("SIGY_TEST_CARGO_FAIL")
        .output()
}

fn fixture(root: &Path, packages: &[&str], covered: u64) -> TestResult {
    let mut metadata = Vec::new();
    let mut files = Vec::new();
    for name in packages {
        let directory = root.join(name);
        fs::create_dir_all(directory.join("src"))?;
        let manifest = directory.join("Cargo.toml");
        let source = directory.join("src/main.rs");
        fs::write(&manifest, "fixture manifest")?;
        fs::write(&source, "fn main() {}")?;
        metadata.push(serde_json::json!({"id":name,"name":name,"manifest_path":manifest}));
        files.push(serde_json::json!({"filename":source,"summary":{"lines":{"count":100,"covered":covered}}}));
    }
    fs::write(
        root.join("metadata.json"),
        serde_json::to_vec(
            &serde_json::json!({"packages":metadata,"workspace_members":packages,"target_directory":root.join("target")}),
        )?,
    )?;
    fs::write(
        root.join("report.json"),
        serde_json::to_vec(
            &serde_json::json!({"type":"llvm.coverage.json.export","data":[{"files":files}]}),
        )?,
    )?;
    Ok(())
}

#[test]
fn standalone_and_multi_package_receipts_use_the_same_exact_gate() -> TestResult {
    for packages in [&["research"][..], &["alpha", "beta"][..]] {
        let temporary = tempfile::tempdir()?;
        let root = fs::canonicalize(temporary.path())?;
        fixture(&root, packages, 80)?;
        let manifest = root.join(packages[0]).join("Cargo.toml");
        let report = root.join("report.json");
        let original = fs::read(&report)?;
        let output = invoke(&root, &manifest, &report)?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        for name in packages {
            assert!(stderr.contains(&format!("{name}: 80.00% lines (80/100)")));
        }
        assert_eq!(fs::read(&report)?, original);
        assert!(!root.join("target").exists());
        let commands = fs::read_to_string(root.join("commands.log"))?;
        assert_eq!(commands.lines().count(), 1);
        assert!(commands.starts_with("metadata --manifest-path "));
        assert!(commands.ends_with(" --locked --no-deps --format-version 1\n"));
        fixture(&root, packages, 79)?;
        let output = invoke(&root, &manifest, &report)?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("below threshold:"));
    }
    Ok(())
}

#[test]
fn missing_report_manifest_and_owned_source_fail_closed() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let root = fs::canonicalize(temporary.path())?;
    fixture(&root, &["research"], 100)?;
    let manifest = root.join("research/Cargo.toml");
    let report = root.join("report.json");
    assert!(
        !invoke(&root, &manifest, &root.join("missing.json"))?
            .status
            .success()
    );
    assert!(
        !invoke(&root, &root.join("missing.toml"), &report)?
            .status
            .success()
    );
    fs::remove_file(root.join("research/src/main.rs"))?;
    let output = invoke(&root, &manifest, &report)?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("coverage source"));
    for arguments in [
        vec!["verify-coverage-report"],
        vec!["verify-coverage-report", "one"],
        vec!["verify-coverage-report", "one", "two", "extra"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sigy-xtask"))
            .args(arguments)
            .output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    }
    Ok(())
}
