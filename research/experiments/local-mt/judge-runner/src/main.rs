mod assets;
mod controls;
mod execution;
mod response;

use std::{path::Path, process::ExitCode};

use scorer_probe::{judge_records::CalibrationSet, selection::Selection};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const RUBRIC: &str = include_str!("../rubric.txt");
const SCHEMA: &str = include_str!("../output-schema.json");
const TEMPLATE: &str = "<|turn>user\n{rubric}\n\nDATA JSON:\n{data}<turn|>\n<|turn>model\n";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "judge-runner: {}",
                error
                    .to_string()
                    .chars()
                    .flat_map(char::escape_default)
                    .take(512)
                    .collect::<String>()
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_mins(45);
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, manifest, references, digest, runtime, model, output] if command == "freeze" => {
            let selection = Selection::from_frozen_bytes(&assets::read(Path::new(manifest), 2 * 1024 * 1024)?)?;
            let references = assets::pinned(Path::new(references), digest.to_str().ok_or("digest must be UTF-8")?, 2 * 1024 * 1024)?;
            let references = serde_json::from_slice(&references)?;
            let controls = controls::prepare(&selection, &references)?;
            assets::freeze(&controls, Path::new(runtime), Path::new(model), Path::new(output))
        }
        [command, manifest, directory, profile_digest] if command == "run" => {
            let selection = Selection::from_frozen_bytes(&assets::read(Path::new(manifest), 2 * 1024 * 1024)?)?;
            let directory_path = std::fs::canonicalize(Path::new(directory))?;
            let directory = directory_path.as_path();
            let profile_bytes = assets::pinned(&directory.join("profile.json"), profile_digest.to_str().ok_or("digest must be UTF-8")?, 2 * 1024 * 1024)?;
            let profile: assets::Profile = serde_json::from_slice(&profile_bytes)?;
            assets::validate_profile(&profile, directory)?;
            let bytes = assets::pinned(&directory.join("controls.json"), &profile.controls_sha256, 2 * 1024 * 1024)?;
            let controls: CalibrationSet = serde_json::from_slice(&bytes)?;
            scorer_probe::judge_records::validate(&selection, &controls)?;
            execution::batch(directory, &profile, &controls, &scorer_probe::selection::sha256(&profile_bytes), deadline).await
        }
        _ => Err("usage: judge-runner freeze MANIFEST REFERENCES REFERENCES_SHA256 RUNTIME_DIR GEMMA_MODEL NEW_OUTPUT_DIR\n       judge-runner run MANIFEST FROZEN_OUTPUT_DIR PROFILE_SHA256".into()),
    }
}

fn source_digest() -> String {
    let files: [(&str, &[u8]); 10] = [
        ("Cargo.toml", include_bytes!("../Cargo.toml")),
        ("Cargo.lock", include_bytes!("../Cargo.lock")),
        (
            "rust-toolchain.toml",
            include_bytes!("../rust-toolchain.toml"),
        ),
        ("src/main.rs", include_bytes!("main.rs")),
        ("src/assets.rs", include_bytes!("assets.rs")),
        ("src/controls.rs", include_bytes!("controls.rs")),
        ("src/execution.rs", include_bytes!("execution.rs")),
        ("src/response.rs", include_bytes!("response.rs")),
        ("rubric.txt", RUBRIC.as_bytes()),
        ("output-schema.json", SCHEMA.as_bytes()),
    ];
    let mut inventory = String::new();
    for (name, bytes) in files {
        inventory.push_str(name);
        inventory.push(':');
        inventory.push_str(&scorer_probe::selection::sha256(bytes));
        inventory.push('\n');
    }
    scorer_probe::selection::sha256(inventory.as_bytes())
}
