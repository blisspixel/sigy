//! Adapter parsing, profile identity and file-boundary checks. No native process runs here.

use super::*;
use crate::recognition::LocalAsrInput;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn input(start_us: u64, end_us: u64) -> LocalAsrInput {
    LocalAsrInput {
        recording_id: "one".into(),
        media_sha256: "a".repeat(64),
        timeline_sha256: "b".repeat(64),
        object_key: "recordings/one".into(),
        byte_length: 10,
        interval_ordinal: 0,
        start_us,
        end_us,
        source_sha256: "c".repeat(64),
    }
}

fn json(segments: &str) -> Vec<u8> {
    format!(r#"{{"systeminfo":"x","result":{{"language":"es"}},"transcription":[{segments}]}}"#)
        .into_bytes()
}

fn segment(from: i64, to: i64, text: &str) -> String {
    format!(
        r#"{{"timestamps":{{"from":"a","to":"b"}},"offsets":{{"from":{from},"to":{to}}},"text":{}}}"#,
        serde_json::to_string(text).unwrap_or_default()
    )
}

#[test]
fn segments_map_to_the_pinned_media_clock_in_original_script() -> TestResult {
    let pinned = input(5_000_000, 16_340_000);
    let body = json(
        &[
            segment(1310, 5310, " Una feria mundial,"),
            segment(5950, 9890, "  يقع النمر  "),
        ]
        .join(","),
    );
    let cues = parse_whisper_json(&body, &pinned, 181_440).map_err(str::to_owned)?;
    assert_eq!(cues.len(), 2);
    assert_eq!(
        (cues[0].ordinal, cues[0].start_us, cues[0].end_us),
        (0, 6_310_000, 10_310_000)
    );
    assert_eq!(cues[0].script, "Una feria mundial,");
    assert_eq!(cues[1].script, "يقع النمر");
    assert_eq!(cues[1].ordinal, 1);
    Ok(())
}

#[test]
fn empty_transcription_is_valid_no_text_and_blank_segments_are_skipped() -> TestResult {
    let pinned = input(0, 10_000_000);
    assert!(
        parse_whisper_json(&json(""), &pinned, 160_000)
            .map_err(str::to_owned)?
            .is_empty()
    );
    let blank = json(&segment(0, 1000, " \t "));
    assert!(
        parse_whisper_json(&blank, &pinned, 160_000)
            .map_err(str::to_owned)?
            .is_empty()
    );
    Ok(())
}

#[test]
fn a_segment_end_past_the_interval_is_bounded_but_a_start_past_audio_is_refused() -> TestResult {
    let pinned = input(0, 10_320_000);
    let cues = parse_whisper_json(&json(&segment(6000, 11_000, "fin")), &pinned, 165_120)
        .map_err(str::to_owned)?;
    assert_eq!(cues[0].end_us, 10_320_000);
    assert!(parse_whisper_json(&json(&segment(10_320, 11_000, "late")), &pinned, 165_120).is_err());
    Ok(())
}

#[test]
fn hostile_or_inconsistent_output_is_refused_whole() {
    let pinned = input(0, 10_000_000);
    let cases = [
        b"not json".to_vec(),
        b"{}".to_vec(),
        br#"{"transcription":"x"}"#.to_vec(),
        json(&segment(-1, 10, "negative")),
        json(&segment(500, 500, "empty span")),
        json(&segment(900, 400, "reversed")),
        json(&[segment(0, 2000, "a"), segment(1000, 3000, "overlap")].join(",")),
        json(&segment(0, 1000, "nul\u{0}byte")),
        json(&segment(0, 1000, &"x".repeat(4097))),
        json(&segment(i64::MAX, i64::MAX, "overflow")),
        vec![0xff, 0xfe, 0x00],
    ];
    for case in cases {
        assert!(
            parse_whisper_json(&case, &pinned, 160_000).is_err(),
            "{}",
            String::from_utf8_lossy(&case)
        );
    }
}

#[test]
fn cue_and_text_limits_are_enforced() {
    let pinned = input(0, 60_000_000);
    let many: Vec<String> = (0..=256)
        .map(|index| segment(index * 100, index * 100 + 50, "w"))
        .collect();
    assert!(parse_whisper_json(&json(&many.join(",")), &pinned, 960_000).is_err());
    let large: Vec<String> = (0..17)
        .map(|index| segment(index * 1000, index * 1000 + 500, &"y".repeat(4096)))
        .collect();
    assert!(parse_whisper_json(&json(&large.join(",")), &pinned, 960_000).is_err());
}

#[test]
fn wav_header_describes_sixteen_bit_mono_at_the_worker_rate() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("input.wav");
    write_wav(&path, &[1, 0, 2, 0])?;
    let bytes = std::fs::read(&path)?;
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into()?), 40);
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into()?), 1);
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into()?), SAMPLE_RATE);
    assert_eq!(u16::from_le_bytes(bytes[34..36].try_into()?), 16);
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(u32::from_le_bytes(bytes[40..44].try_into()?), 4);
    assert_eq!(&bytes[44..], &[1, 0, 2, 0]);
    Ok(())
}

fn runtime(root: &Path) -> std::io::Result<(PathBuf, PathBuf, PathBuf)> {
    let runtime = root.join("runtime");
    std::fs::create_dir(&runtime)?;
    std::fs::write(runtime.join("whisper-cli.exe"), b"executable bytes")?;
    std::fs::write(runtime.join("whisper.dll"), b"library bytes")?;
    std::fs::create_dir(runtime.join("ignored-subdirectory"))?;
    let model = root.join("model.bin");
    std::fs::write(&model, b"model bytes")?;
    let vad = root.join("vad.bin");
    std::fs::write(&vad, b"vad bytes")?;
    Ok((runtime, model, vad))
}

#[test]
fn profile_identity_covers_file_hashes_and_limits_but_not_locations() -> TestResult {
    let root = tempfile::tempdir()?;
    let (runtime_dir, model, vad) = runtime(root.path())?;
    let profile = describe_profile(
        "cpu",
        &runtime_dir,
        "whisper-cli.exe",
        &model,
        &vad,
        2,
        1 << 30,
        60_000,
    )?;
    profile.validate()?;
    assert_eq!(profile.runtime_files, 2);
    assert_eq!(profile.model_bytes, 11);
    let never = AtomicBool::new(false);
    assert!(verify_assets(&profile, &never)?);

    let other = tempfile::tempdir()?;
    let (moved_runtime, moved_model, moved_vad) = runtime(other.path())?;
    let moved = describe_profile(
        "cpu",
        &moved_runtime,
        "whisper-cli.exe",
        &moved_model,
        &moved_vad,
        2,
        1 << 30,
        60_000,
    )?;
    assert_eq!(moved.profile_sha256, profile.profile_sha256);

    let more_threads = describe_profile(
        "cpu",
        &runtime_dir,
        "whisper-cli.exe",
        &model,
        &vad,
        3,
        1 << 30,
        60_000,
    )?;
    assert_ne!(more_threads.profile_sha256, profile.profile_sha256);

    std::fs::write(runtime_dir.join("whisper.dll"), b"replaced library")?;
    assert!(!verify_assets(&profile, &never)?);
    std::fs::write(runtime_dir.join("injected.dll"), b"new file")?;
    assert!(!verify_assets(&profile, &never)?);
    Ok(())
}

#[test]
fn profile_description_refuses_invalid_inputs() -> TestResult {
    let root = tempfile::tempdir()?;
    let (runtime_dir, model, vad) = runtime(root.path())?;
    let describe = |executable: &str, threads: u32, memory: u64, deadline: u64| {
        describe_profile(
            "cpu",
            &runtime_dir,
            executable,
            &model,
            &vad,
            threads,
            memory,
            deadline,
        )
    };
    assert!(describe("missing.exe", 2, 1 << 30, 60_000).is_err());
    assert!(describe("../whisper-cli.exe", 2, 1 << 30, 60_000).is_err());
    assert!(describe("whisper-cli.exe", 0, 1 << 30, 60_000).is_err());
    assert!(describe("whisper-cli.exe", 65, 1 << 30, 60_000).is_err());
    assert!(describe("whisper-cli.exe", 2, 1 << 20, 60_000).is_err());
    assert!(describe("whisper-cli.exe", 2, 1 << 30, 999).is_err());
    assert!(describe("whisper-cli.exe", 2, 1 << 30, 3_600_001).is_err());
    assert!(
        describe_profile(
            "local-unmeasured",
            &runtime_dir,
            "whisper-cli.exe",
            &model,
            &vad,
            2,
            1 << 30,
            60_000
        )
        .is_err()
    );
    assert!(
        describe_profile(
            "cpu",
            &root.path().join("absent"),
            "whisper-cli.exe",
            &model,
            &vad,
            2,
            1 << 30,
            60_000
        )
        .is_err()
    );
    std::fs::write(root.path().join("empty.bin"), b"")?;
    assert!(
        describe_profile(
            "cpu",
            &runtime_dir,
            "whisper-cli.exe",
            &root.path().join("empty.bin"),
            &vad,
            2,
            1 << 30,
            60_000
        )
        .is_err()
    );
    let mut forged = describe("whisper-cli.exe", 2, 1 << 30, 60_000)?;
    forged.threads = 4;
    assert!(forged.validate().is_err());
    let mut relative = describe("whisper-cli.exe", 2, 1 << 30, 60_000)?;
    relative.model_path = "model.bin".into();
    assert!(relative.validate().is_err());
    Ok(())
}

#[test]
fn a_stop_request_interrupts_hashing_between_reads() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("large.bin");
    std::fs::write(&path, vec![7_u8; 1024 * 1024])?;
    let stop = AtomicBool::new(true);
    assert!(matches!(
        hash_file(&path, 2 * 1024 * 1024, &stop),
        Err(Error::Analysis("cancelled"))
    ));
    assert!(hash_file(&path, 1024, &AtomicBool::new(false)).is_err());
    Ok(())
}

#[test]
fn stale_scratch_is_removed_and_a_file_in_its_place_is_refused() -> TestResult {
    let root = tempfile::tempdir()?;
    clear_scratch(root.path())?;
    std::fs::create_dir_all(root.path().join(SCRATCH).join("job-g1"))?;
    std::fs::write(
        root.path().join(SCRATCH).join("job-g1").join("input.wav"),
        b"pcm",
    )?;
    clear_scratch(root.path())?;
    assert!(!root.path().join(SCRATCH).exists());
    std::fs::write(root.path().join(SCRATCH), b"not a directory")?;
    assert!(clear_scratch(root.path()).is_err());
    Ok(())
}
