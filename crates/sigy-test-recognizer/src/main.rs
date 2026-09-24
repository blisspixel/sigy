//! A stand-in for `whisper-cli` used only by the native-media tests.
//!
//! The service clears the environment and fixes the argument template, so the fault
//! to inject is taken from this executable's file name: `recognizer-<mode>[.exe]` or,
//! as a stand-in for `llama-completion`, `translator-<mode>[.exe]`.
//! It never reads the network and writes only the requested output file.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

fn value(arguments: &[String], flag: &str) -> Option<String> {
    arguments
        .iter()
        .position(|argument| argument == flag)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
}

fn wav_duration_ms(path: &Path) -> std::io::Result<u64> {
    let mut header = [0_u8; 44];
    std::fs::File::open(path)?.read_exact(&mut header)?;
    let rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    let data = u32::from_le_bytes([header[40], header[41], header[42], header[43]]);
    if &header[..4] != b"RIFF" || rate != 16_000 || header[22] != 1 || header[34] != 16 {
        return Err(std::io::Error::other("unexpected worker audio format"));
    }
    Ok(u64::from(data) / 2 * 1000 / u64::from(rate))
}

fn transcript(segments: &[(u64, u64, &str)]) -> String {
    let rows: Vec<String> = segments
        .iter()
        .map(|(from, to, text)| {
            format!(
                "{{\"timestamps\":{{\"from\":\"\",\"to\":\"\"}},\"offsets\":{{\"from\":{from},\"to\":{to}}},\"text\":\"{text}\"}}"
            )
        })
        .collect();
    format!(
        "{{\"systeminfo\":\"fixture\",\"result\":{{\"language\":\"fr\"}},\"transcription\":[{}]}}",
        rows.join(",")
    )
}

fn run(mode: &str, arguments: &[String]) -> Result<u8, Box<dyn std::error::Error>> {
    // The adapter contract: explicit model, input, automatic language and speech gate.
    for flag in ["-m", "-f", "-l", "-vm", "-of", "-t"] {
        value(arguments, flag).ok_or(flag)?;
    }
    if value(arguments, "-l").as_deref() != Some("auto")
        || !arguments.iter().any(|argument| argument == "--vad")
        || !arguments.iter().any(|argument| argument == "--no-gpu")
        || std::env::var_os("SIGY_TEST_FFMPEG").is_some()
        || std::env::var("OMP_WAIT_POLICY").as_deref() != Ok("PASSIVE")
    {
        return Ok(9);
    }
    let input = PathBuf::from(value(arguments, "-f").ok_or("-f")?);
    let output = PathBuf::from(format!("{}.json", value(arguments, "-of").ok_or("-of")?));
    let duration = wav_duration_ms(&input)?;
    let body = match mode {
        "speech" => transcript(&[
            (0, duration.min(400), " bonjour "),
            (duration / 2, duration + 250, "le monde"),
        ]),
        "silent" => transcript(&[]),
        "garbage" => "{\"transcription\": [".to_owned(),
        "overlap" => transcript(&[(0, 600, "un"), (300, 800, "deux")]),
        "huge" => "x".repeat(2 * 1024 * 1024),
        "fail" => return Ok(3),
        "hang" => {
            std::thread::sleep(Duration::from_secs(600));
            return Ok(0);
        }
        "child" => {
            let own = std::env::current_exe()?;
            let started = std::process::Command::new(own).arg("--grandchild").spawn();
            let text = if started.is_ok() {
                "grandchild started"
            } else {
                "grandchild refused"
            };
            transcript(&[(0, duration.min(400), text)])
        }
        "memory" => {
            let mut blocks: Vec<Vec<u8>> = Vec::new();
            for _ in 0..4096 {
                let mut block = Vec::new();
                if block.try_reserve_exact(1024 * 1024).is_err() {
                    return Ok(4);
                }
                block.resize(1024 * 1024, 1);
                blocks.push(block);
            }
            std::hint::black_box(&blocks);
            transcript(&[(0, duration.min(400), "memory unbounded")])
        }
        _ => return Ok(8),
    };
    std::fs::write(output, body)?;
    Ok(0)
}

/// Stand-in for `llama-completion`: the prompt file ends with the source cue text.
fn translate(mode: &str, arguments: &[String]) -> Result<u8, Box<dyn std::error::Error>> {
    for flag in ["-m", "-f", "-n", "-t"] {
        value(arguments, flag).ok_or(flag)?;
    }
    if !arguments.iter().any(|argument| argument == "--offline")
        || std::env::var_os("SIGY_TEST_FFMPEG").is_some()
        || std::env::var("OMP_WAIT_POLICY").as_deref() != Ok("PASSIVE")
    {
        return Ok(9);
    }
    let prompt = std::fs::read_to_string(value(arguments, "-f").ok_or("-f")?)?;
    let source = prompt.rsplit("\n\n").next().unwrap_or_default();
    let mut stdout = std::io::stdout();
    match mode {
        "echo" => write!(stdout, "\u{1b}[33m\u{1b}[0mEN {source} [end of text]\n\n")?,
        "flood" => stdout.write_all(&vec![b'y'; 200 * 1024])?,
        "fail" => return Ok(3),
        "hang" => std::thread::sleep(Duration::from_secs(600)),
        _ => return Ok(8),
    }
    Ok(0)
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.iter().any(|argument| argument == "--grandchild") {
        std::thread::sleep(Duration::from_secs(600));
        return ExitCode::SUCCESS;
    }
    let stem = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    if let Some(mode) = stem.strip_prefix("translator-") {
        return match translate(mode, &arguments) {
            Ok(code) => ExitCode::from(code),
            Err(_) => ExitCode::from(7),
        };
    }
    let mode = stem.strip_prefix("recognizer-").unwrap_or_default();
    match run(mode, &arguments) {
        Ok(code) => ExitCode::from(code),
        Err(_) => ExitCode::from(7),
    }
}
