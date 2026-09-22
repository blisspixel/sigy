//! Measurement harness for one list and search workload.
//!
//! This package is excluded from the Sigy workspace. It does not open the catalog.

#![deny(unsafe_code)]

mod drive;
mod modes;
mod probe;
mod textutil;

use std::env;
use std::error::Error;
use std::process::ExitCode;

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("terminal measure: {error}");
            ExitCode::from(1)
        }
    }
}

fn dispatch() -> Result<ExitCode, Box<dyn Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    match args.next() {
        Some(command) if command == "probe" => {
            probe::run(args)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(command) if command == "font" => {
            let path = args.next().ok_or("font requires a report path")?;
            let face = modes::font_face().unwrap_or_default();
            let session = env::var_os("WT_SESSION").is_some();
            let line = format!(
                "{{\"font\":{},\"wt\":{}}}\n",
                textutil::json_string(&face),
                session
            );
            std::fs::write(path, line)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(command) if command == "measure" => {
            drive::run(args)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(flag) if flag.starts_with('-') => {
            let mut forwarded = vec![flag];
            forwarded.extend(args);
            drive::run(forwarded.into_iter())?;
            Ok(ExitCode::SUCCESS)
        }
        None => {
            drive::run(args)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => Err(format!("unknown command {other}; use measure or probe").into()),
    }
}
