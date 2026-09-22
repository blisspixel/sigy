//! Workspace verification. This is a development tool, not the Sigy application.

mod vendor;

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use vendor::check_vendor;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    Check(String),
    #[error("verification failed: cargo {0}")]
    Cargo(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sigy-xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let mut args = env::args().skip(1).filter(|arg| arg != "--");
    let command = args
        .next()
        .ok_or_else(|| Error::Check("use verify or verify-media".into()))?;
    let root = workspace_root()?;
    match command.as_str() {
        "verify" => verify(&root),
        "verify-media" => verify_media(&root, args.next()),
        other => Err(Error::Check(format!(
            "unknown command {other}; use verify or verify-media"
        ))),
    }
}

fn verify(root: &Path) -> Result<(), Error> {
    check_vendor(root).map_err(Error::Check)?;
    run_cargo(root, &["fmt", "--all", "--", "--check"])?;
    // The running sigy-xtask binary cannot be replaced on Windows, so the
    // product commands exclude this package and its own check uses another directory.
    let isolated = root.join("target").join("xtask-check");
    run_cargo_isolated(
        root,
        &[
            "clippy",
            "-p",
            "sigy-xtask",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        &isolated,
    )?;
    run_cargo(
        root,
        &[
            "clippy",
            "--workspace",
            "--exclude",
            "sigy-xtask",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run_cargo_isolated(
        root,
        &[
            "test",
            "-p",
            "sigy-xtask",
            "--locked",
            "--",
            "--test-threads=2",
        ],
        &isolated,
    )?;
    run_cargo(
        root,
        &[
            "test",
            "--workspace",
            "--exclude",
            "sigy-xtask",
            "--locked",
            "--",
            "--test-threads=2",
        ],
    )?;
    run_cargo(
        root,
        &[
            "build",
            "--workspace",
            "--exclude",
            "sigy-xtask",
            "--locked",
        ],
    )?;
    run_cargo(root, &["audit", "--deny", "warnings"])?;
    Ok(())
}

fn verify_media(root: &Path, decoder: Option<String>) -> Result<(), Error> {
    let ffmpeg = decoder_path(decoder)?;
    run_cargo_with(
        root,
        &[
            "test",
            "--locked",
            "-p",
            "sigy",
            "--test",
            "service",
            "--",
            "--ignored",
            "--test-threads=1",
        ],
        &[("SIGY_TEST_FFMPEG", ffmpeg.as_os_str())],
        None,
    )
}

fn workspace_root() -> Result<PathBuf, Error> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::Check("workspace root is missing".into()))?;
    Ok(fs::canonicalize(root)?)
}

fn cargo_program() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn run_cargo(root: &Path, args: &[&str]) -> Result<(), Error> {
    run_cargo_with(root, args, &[], None)
}

fn run_cargo_isolated(root: &Path, args: &[&str], target_dir: &Path) -> Result<(), Error> {
    run_cargo_with(root, args, &[], Some(target_dir))
}

fn run_cargo_with(
    root: &Path,
    args: &[&str],
    extra_env: &[(&str, &std::ffi::OsStr)],
    target_dir: Option<&Path>,
) -> Result<(), Error> {
    let command = args.join(" ");
    eprintln!("cargo {command}");
    let mut process = Command::new(cargo_program());
    process.args(args).current_dir(root);
    if let Some(target_dir) = target_dir {
        process.env("CARGO_TARGET_DIR", target_dir);
    }
    for (key, value) in extra_env {
        process.env(key, value);
    }
    let status = process.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Cargo(command))
    }
}

fn decoder_path(explicit: Option<String>) -> Result<PathBuf, Error> {
    let raw = if let Some(path) = explicit {
        path
    } else if let Ok(path) = env::var("SIGY_TEST_FFMPEG") {
        path
    } else {
        find_on_path("ffmpeg")?
    };
    let path = PathBuf::from(raw);
    if !path.is_file() {
        return Err(Error::Check(format!(
            "FFmpeg executable was not found at {}",
            path.display()
        )));
    }
    Ok(fs::canonicalize(&path)?)
}

fn find_on_path(name: &str) -> Result<String, Error> {
    let path = env::var_os("PATH").ok_or_else(|| Error::Check("PATH is not set".into()))?;
    let extensions = if cfg!(windows) {
        env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
    } else {
        String::new()
    };
    let mut suffixes = vec![String::new()];
    suffixes.extend(extensions.split(';').map(str::to_owned));
    for directory in env::split_paths(&path) {
        for suffix in &suffixes {
            let candidate = directory.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
    }
    Err(Error::Check(
        "FFmpeg was not found on PATH; set SIGY_TEST_FFMPEG".into(),
    ))
}
