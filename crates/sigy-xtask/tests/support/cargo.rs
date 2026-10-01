//! Bounded offline command recorder for verifier contract tests.

use std::{fs::OpenOptions, io::Write, path::PathBuf, process::ExitCode};

fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let log = std::env::var("SIGY_TEST_CARGO_LOG")?;
    writeln!(
        OpenOptions::new().create(true).append(true).open(log)?,
        "{}",
        arguments.join(" ")
    )?;
    if std::env::var("SIGY_TEST_CARGO_FAIL")
        .is_ok_and(|step| arguments.join(" ").starts_with(&step))
    {
        return Ok(false);
    }
    if arguments.first().is_some_and(|word| word == "metadata") {
        print!(
            "{}",
            std::fs::read_to_string(std::env::var("SIGY_TEST_CARGO_METADATA")?)?
        );
    } else if arguments.iter().any(|word| word == "show-env") {
        println!("$env:CARGO_LLVM_COV=\"`u{{31}}\"");
    } else if let Some(index) = arguments.iter().position(|word| word == "--output-path") {
        let path = PathBuf::from(arguments.get(index + 1).ok_or("missing output path")?);
        std::fs::write(
            path,
            std::fs::read(std::env::var("SIGY_TEST_CARGO_REPORT")?)?,
        )?;
    }
    Ok(true)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
