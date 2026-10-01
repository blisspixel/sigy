//! Freshly hashed invalid specs must fail before file staging or native execution.

use super::{
    TestResult, chunk_spec, cues, input, job, profile, translation_job, translation_profile,
};
use crate::execution::{
    DecoderLimits, Executor, LocalProcessExecutor, LocalStage, RecognitionResult, TaskParams,
    TaskResult, TaskSpec, asr::decoder_options,
};

type Mutation = (&'static str, fn(&mut TaskSpec));

fn rejected(mut spec: TaskSpec, label: &str) -> TestResult {
    // Update the hash independently: failure must come from the contract, not a
    // stale digest. A serialized round trip cannot evade the same checks.
    spec.spec_sha256 = spec.digest()?;
    let wire = serde_json::to_vec(&spec)?;
    let parsed: TaskSpec = serde_json::from_slice(&wire)?;
    assert!(parsed.validate().is_err(), "accepted {label}");
    assert!(parsed.seal().is_err(), "sealed {label}");
    Ok(())
}

fn both_specs(root: &std::path::Path) -> crate::Result<[TaskSpec; 2]> {
    Ok([
        chunk_spec(&job("asr-limits"), &input(), &profile(root))?,
        crate::recognizer::translate::translation_spec(
            &translation_job(),
            &translation_profile(root),
            &cues(),
            Some("fr"),
        )?,
    ])
}

#[test]
fn fresh_hashes_cannot_authorize_zero_overflow_or_expanded_native_limits() -> TestResult {
    let root = tempfile::tempdir()?;
    let cases: &[Mutation] = &[
        ("zero processes", |s| s.limits.processes = 0),
        ("extra process", |s| s.limits.processes = 2),
        ("overflow processes", |s| s.limits.processes = u32::MAX),
        ("zero memory", |s| s.limits.memory_bytes = 0),
        ("below profile memory", |s| {
            s.limits.memory_bytes = (256 << 20) - 1;
        }),
        ("above profile memory", |s| {
            s.limits.memory_bytes = (64 << 30) + 1;
        }),
        ("overflow memory", |s| s.limits.memory_bytes = u64::MAX),
        ("zero CPU", |s| s.limits.cpu_rate = 0),
        ("CPU inconsistent with threads", |s| s.limits.cpu_rate = 1),
        ("overflow CPU", |s| s.limits.cpu_rate = u32::MAX),
        ("zero deadline", |s| s.limits.wall_ms = 0),
        ("below profile deadline", |s| s.limits.wall_ms = 999),
        ("overflow deadline", |s| s.limits.wall_ms = u64::MAX),
        ("zero output", |s| s.limits.output_bytes = 0),
        ("overflow output read ceiling", |s| {
            s.limits.output_bytes = u64::MAX;
        }),
        ("unknown engine", |s| s.engine = "future-engine-v1".into()),
        ("unknown template", |s| {
            s.template = "future-template-v1".into();
        }),
    ];
    for base in both_specs(root.path())? {
        for (label, change) in cases {
            let mut candidate = base.clone();
            change(&mut candidate);
            rejected(candidate, label)?;
        }
    }
    Ok(())
}

#[test]
fn recognition_requires_bounded_decoder_limits_and_its_known_parameters() -> TestResult {
    let root = tempfile::tempdir()?;
    let base = both_specs(root.path())?[0].clone();
    let cases: &[Mutation] = &[
        ("missing decoder", |s| s.limits.decoder = None),
        ("zero decoder memory", |s| {
            if let Some(d) = &mut s.limits.decoder {
                d.memory_bytes = 0;
            }
        }),
        ("decoder memory expansion", |s| {
            if let Some(d) = &mut s.limits.decoder {
                d.memory_bytes = (512 << 20) + 1;
            }
        }),
        ("overflow decoder memory", |s| {
            if let Some(d) = &mut s.limits.decoder {
                d.memory_bytes = u64::MAX;
            }
        }),
        ("decoder deadline expansion", |s| {
            if let Some(d) = &mut s.limits.decoder {
                d.wall_ms = 60_001;
            }
        }),
        ("zero decoder deadline", |s| {
            if let Some(d) = &mut s.limits.decoder {
                d.wall_ms = 0;
            }
        }),
        ("overflow decoder deadline", |s| {
            if let Some(d) = &mut s.limits.decoder {
                d.wall_ms = u64::MAX;
            }
        }),
        ("output expansion", |s| {
            s.limits.output_bytes = (1 << 20) + 1;
        }),
        ("deadline expansion", |s| s.limits.wall_ms = 3_600_001),
        ("unknown decoder", |s| {
            if let TaskParams::Recognition(p) = &mut s.params {
                p.decoder = "future-pcm-v1".into();
            }
        }),
        ("unknown sample rate", |s| {
            if let TaskParams::Recognition(p) = &mut s.params {
                p.sample_rate = 48_000;
            }
        }),
        ("zero threads", |s| {
            if let TaskParams::Recognition(p) = &mut s.params {
                p.threads = 0;
            }
        }),
        ("overflow threads", |s| {
            if let TaskParams::Recognition(p) = &mut s.params {
                p.threads = u32::MAX;
            }
        }),
        ("overflow chunk ordinal", |s| {
            if let TaskParams::Recognition(p) = &mut s.params {
                p.chunk_ordinal = Some(u32::MAX);
            }
        }),
        ("expanded window", |s| {
            if let TaskParams::Recognition(p) = &mut s.params {
                p.end_us = 30_000_001;
            }
        }),
        ("overflow offset", |s| {
            if let TaskParams::Recognition(p) = &mut s.params {
                p.start_us = u64::MAX - 1;
                p.end_us = u64::MAX;
                p.file_offset_us = Some(u64::MAX);
                p.slice_us = Some(1);
            }
        }),
    ];
    for (label, change) in cases {
        let mut candidate = base.clone();
        change(&mut candidate);
        rejected(candidate, label)?;
    }
    Ok(())
}

#[test]
fn translation_cannot_change_target_language_or_borrow_decoder_authority() -> TestResult {
    let root = tempfile::tempdir()?;
    let base = both_specs(root.path())?[1].clone();
    let cases: &[Mutation] = &[
        ("decoder in translation", |s| {
            s.limits.decoder = Some(DecoderLimits {
                memory_bytes: 1,
                wall_ms: 1,
            });
        }),
        ("output expansion", |s| {
            s.limits.output_bytes = (64 << 10) + 1;
        }),
        ("deadline expansion", |s| s.limits.wall_ms = 600_001),
        ("unknown target", |s| {
            if let TaskParams::Translation(p) = &mut s.params {
                p.target = "fr".into();
            }
        }),
        ("noncanonical source label", |s| {
            if let TaskParams::Translation(p) = &mut s.params {
                p.source_language = Some("fr-CA".into());
            }
        }),
        ("two source labels", |s| {
            if let TaskParams::Translation(p) = &mut s.params {
                p.source_language = Some("es,fr".into());
            }
        }),
        ("noncanonical declarations", |s| {
            if let TaskParams::Translation(p) = &mut s.params {
                p.declared_languages = "fr,es".into();
            }
        }),
        ("zero threads", |s| {
            if let TaskParams::Translation(p) = &mut s.params {
                p.threads = 0;
            }
        }),
        ("overflow threads", |s| {
            if let TaskParams::Translation(p) = &mut s.params {
                p.threads = u32::MAX;
            }
        }),
    ];
    for (label, change) in cases {
        let mut candidate = base.clone();
        change(&mut candidate);
        rejected(candidate, label)?;
    }
    Ok(())
}

#[test]
fn published_profile_boundaries_still_produce_valid_v1_specs() -> TestResult {
    use crate::recognition::{MAX_WORKER_MEMORY, MAX_WORKER_THREADS, MIN_WORKER_MEMORY};
    let root = tempfile::tempdir()?;
    for (threads, memory, deadline) in [
        (1, MIN_WORKER_MEMORY, 1_000),
        (MAX_WORKER_THREADS, MAX_WORKER_MEMORY, 3_600_000),
    ] {
        let mut p = profile(root.path());
        p.threads = threads;
        p.memory_bytes = memory;
        p.deadline_ms = deadline;
        p.profile_sha256 = p.identity()?;
        p.validate()?;
        let spec = chunk_spec(&job("asr-boundary"), &input(), &p)?;
        spec.validate()?;
        assert_eq!(spec.limits.cpu_rate, threads);
        let mut mt = translation_profile(root.path());
        mt.threads = threads;
        mt.memory_bytes = memory;
        mt.cue_deadline_ms = deadline.min(600_000);
        mt.profile_sha256 = mt.identity()?;
        mt.validate()?;
        crate::recognizer::translate::translation_spec(&translation_job(), &mt, &cues(), None)?
            .validate()?;
    }
    Ok(())
}

#[test]
fn undeclared_recognizer_labels_keep_the_existing_unsupported_language_path() -> TestResult {
    let root = tempfile::tempdir()?;
    for language in [None, Some("und"), Some("haw"), Some("unknown"), Some("en")] {
        crate::recognizer::translate::translation_spec(
            &translation_job(),
            &translation_profile(root.path()),
            &cues(),
            language,
        )?
        .validate()?;
    }
    Ok(())
}

#[test]
fn decoder_group_options_bound_cpu_as_well_as_memory_and_processes() {
    let options = decoder_options(DecoderLimits {
        memory_bytes: 512 << 20,
        wall_ms: 60_000,
    });
    assert_eq!(options.limits.max_processes, Some(1));
    assert_eq!(options.limits.max_memory, Some(536_870_912));
    assert_eq!(options.limits.cpu_quota, Some(1.0));
    assert!(options.escalate_to_kill);
}

#[tokio::test]
async fn malformed_but_freshly_hashed_specs_never_stage_or_spawn() -> TestResult {
    let root = tempfile::tempdir()?;
    // All hashes map to hostile missing files, and staging's parent is blocked by
    // a file. A contract rejection returns without touching either boundary.
    let sentinel = root.path().join("analysis-scratch");
    std::fs::write(&sentinel, b"unchanged")?;
    for mut spec in both_specs(root.path())? {
        spec.limits.output_bytes = u64::MAX;
        spec.spec_sha256 = spec.digest()?;
        assert!(spec.validate().is_err());
        let mut stage = LocalStage::new(root.path()).decoder(root.path().join("never-launch.exe"));
        for asset in &spec.assets {
            stage = stage.asset(asset.role, &asset.sha256, root.path().join("unavailable"));
        }
        if let Some(blob) = spec.blob() {
            stage = stage.blob(&blob.sha256, root.path().join("unavailable.wav"));
        }
        let expected = match &spec.params {
            TaskParams::Recognition(_) => TaskResult::Recognition(RecognitionResult::Failed(
                crate::recognition::LocalAsrFailure::RecognizerFailed,
            )),
            TaskParams::Translation(_) => TaskResult::Translation(
                crate::translation::TranslationResult::Failed("translator-failed"),
            ),
        };
        let hash = spec.spec_sha256.clone();
        let id = spec.task_id.clone();
        let (_stop, signal) = tokio::sync::watch::channel(false);
        let envelope = LocalProcessExecutor.execute(spec, stage, signal).await?;
        let (result, accounts) = envelope.accept(&id, 1, Some(&hash))?;
        assert_eq!(result, expected);
        assert!(accounts.is_empty());
        assert_eq!(std::fs::read(&sentinel)?, b"unchanged");
        assert!(!root.path().join("unavailable").exists());
    }
    Ok(())
}
