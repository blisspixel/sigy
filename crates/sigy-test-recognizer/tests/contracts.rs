//! The fault fixture must enforce the adapter's argument and CPU contracts.

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn launch(
    root: &Path,
    stem: &str,
    arguments: &[String],
) -> Result<(i32, Vec<u8>), Box<dyn std::error::Error>> {
    let executable = root.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_sigy-test-recognizer"), &executable)?;
    let output = root.join(format!("{stem}.stdout"));
    let mut child = Command::new(executable)
        .args(arguments)
        .env("OMP_WAIT_POLICY", "PASSIVE")
        .env_remove("SIGY_TEST_FFMPEG")
        .stdout(Stdio::from(fs::File::create(&output)?))
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err("fixture exceeded five-second contract test deadline".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok((
        status.code().ok_or("fixture terminated without code")?,
        fs::read(output)?,
    ))
}

fn wav() -> Vec<u8> {
    let mut header = vec![0; 44];
    header[..4].copy_from_slice(b"RIFF");
    header[22] = 1;
    header[24..28].copy_from_slice(&16_000_u32.to_le_bytes());
    header[34] = 16;
    header[40..44].copy_from_slice(&32_000_u32.to_le_bytes());
    header
}

fn recognizer_args(root: &Path) -> Vec<String> {
    [
        "-m",
        "model",
        "-f",
        "input.wav",
        "-l",
        "auto",
        "-vm",
        "vad",
        "-of",
        "output",
        "-t",
        "2",
        "--vad",
        "--no-gpu",
    ]
    .into_iter()
    .map(|word| match word {
        "model" | "input.wav" | "vad" | "output" => root.join(word).to_string_lossy().into_owned(),
        _ => word.to_owned(),
    })
    .collect()
}

#[test]
fn speech_silence_and_fault_payloads_follow_the_selected_mode() -> TestResult {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("input.wav"), wav())?;
    fs::write(root.path().join("model.once"), b"already ran")?;
    let arguments = recognizer_args(root.path());
    for (mode, code, expected) in [
        ("speech", 0, Some("bonjour")),
        ("silent", 0, Some("\"transcription\":[]")),
        ("garbage", 0, Some("{\"transcription\": [")),
        ("overlap", 0, Some("deux")),
        ("huge", 0, None),
        ("once", 0, Some("encore")),
        ("fail", 3, None),
        ("unknown", 8, None),
    ] {
        assert_eq!(
            launch(root.path(), &format!("recognizer-{mode}"), &arguments)?.0,
            code
        );
        if let Some(expected) = expected {
            assert!(fs::read_to_string(root.path().join("output.json"))?.contains(expected));
        }
        if mode == "huge" {
            assert_eq!(
                fs::metadata(root.path().join("output.json"))?.len(),
                2 * 1024 * 1024
            );
        }
    }
    Ok(())
}

#[test]
fn missing_arguments_wrong_language_and_bad_audio_fail_without_publication() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut arguments = recognizer_args(root.path());
    assert_eq!(launch(root.path(), "recognizer-speech", &[])?.0, 7);
    let language = arguments
        .iter()
        .position(|word| word == "auto")
        .ok_or("auto")?;
    arguments[language] = "en".into();
    assert_eq!(launch(root.path(), "recognizer-speech", &arguments)?.0, 9);
    arguments[language] = "auto".into();
    fs::write(root.path().join("input.wav"), b"truncated")?;
    assert_eq!(launch(root.path(), "recognizer-speech", &arguments)?.0, 7);
    let mut invalid = wav();
    invalid[22] = 2;
    fs::write(root.path().join("input.wav"), invalid)?;
    assert_eq!(launch(root.path(), "recognizer-speech", &arguments)?.0, 7);
    assert!(!root.path().join("output.json").exists());
    Ok(())
}

#[test]
fn translation_echo_flood_and_cpu_contract_are_predictable() -> TestResult {
    let root = tempfile::tempdir()?;
    let prompt = root.path().join("prompt.txt");
    fs::write(&prompt, "instructions\n\nbonjour le monde")?;
    let mut arguments: Vec<_> = [
        "-m",
        "model",
        "-f",
        "prompt",
        "-n",
        "128",
        "-t",
        "2",
        "--offline",
        "-dev",
        "none",
        "-ngl",
        "0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    arguments[3] = prompt.to_string_lossy().into_owned();
    let (code, stdout) = launch(root.path(), "translator-echo", &arguments)?;
    assert_eq!(code, 0);
    assert!(String::from_utf8(stdout)?.contains("EN bonjour le monde [end of text]"));
    assert_eq!(
        launch(root.path(), "translator-flood", &arguments)?.1.len(),
        200 * 1024
    );
    assert_eq!(launch(root.path(), "translator-fail", &arguments)?.0, 3);
    assert_eq!(launch(root.path(), "translator-unknown", &arguments)?.0, 8);
    assert_eq!(launch(root.path(), "translator-echo", &[])?.0, 7);
    arguments[10] = "gpu".into();
    assert_eq!(launch(root.path(), "translator-echo", &arguments)?.0, 9);
    Ok(())
}
