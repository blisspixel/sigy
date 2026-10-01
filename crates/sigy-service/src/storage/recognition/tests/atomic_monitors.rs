//! Catalog-only fault fixtures. No native process or model quality is exercised.

use super::*;
use crate::{
    storage::monitors::{MonitorJobAdmission, MonitorJobScope, StepRecord},
    translation::TranslationRequest,
};

fn fixture(path: &Path) -> Result<Store> {
    let mut store = setup(path, false)?;
    store.add_recognition_profile(&stored_profile("cpu", 2)?, 1)?;
    store.add_translation_profile(&super::translation::profile()?, 1)?;
    let mut policy = follow("radio:v1");
    policy.recognition_profile = Some("cpu".into());
    policy.translation_profile = Some("mt".into());
    store.create_monitor("desk", &policy, 2)?;
    store.create_monitor("other", &policy, 2)?;
    Ok(store)
}

fn scope(monitor: &str) -> MonitorJobScope<'_> {
    MonitorJobScope {
        monitor_id: monitor,
        policy_version: 1,
        action_count: 0,
        recording_id: "one",
        audio_us: 1_000_000,
    }
}

fn recognition(store: &Store, id: &str) -> Result<LocalAsrRequest> {
    let profile = store.recognition_profile("cpu")?;
    Ok(LocalAsrRequest {
        id: id.into(),
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
        ..request(id, 0)
    })
}

fn translation(store: &Store, id: &str) -> Result<TranslationRequest> {
    let profile = store.translation_profile("mt")?;
    Ok(TranslationRequest {
        id: id.into(),
        transcript_id: "pin".into(),
        transcript_revision: 1,
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
    })
}

fn count(store: &Store, table: &str) -> Result<u32> {
    Ok(store
        .connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?)
}

fn heard(store: &mut Store) -> Result<LocalAsrRequest> {
    let request = recognition(store, "asr")?;
    store.enqueue_monitor_recognition(&scope("desk"), &request, 10)?;
    let work = store
        .claim_local_asr("asr", "fixture", 11)?
        .ok_or(Error::StorageIntegrity)?;
    store.finish_local_asr(&work, &proof(&work, "water report"), 12)?;
    Ok(request)
}

#[test]
fn monitor_atomic_shared_job_commits_exact_usage_once_and_replays_history() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    let request = recognition(&store, "shared")?;
    let first = store.enqueue_monitor_recognition(&scope("desk"), &request, 10)?;
    assert_eq!(
        first,
        MonitorJobAdmission {
            job_created: true,
            step_created: true
        }
    );
    let shared = store.enqueue_monitor_recognition(&scope("other"), &request, 11)?;
    assert_eq!(
        shared,
        MonitorJobAdmission {
            job_created: false,
            step_created: true
        }
    );
    assert_eq!(count(&store, "analysis_jobs")?, 1);
    assert_eq!(count(&store, "monitor_steps")?, 2);
    assert_eq!(count(&store, "job_attempts")?, 0);
    let job = store.local_asr_job("shared")?;
    assert_eq!(job.state, "queued");
    assert!(job.started_ms.is_none());
    for id in ["desk", "other"] {
        assert_eq!(store.monitor_processing(id, 11)?.used_total_us, 1_000_000);
    }
    store.propose_monitor_action("desk", "pause", ActionOrigin::User, &Proposal::Pause, 12)?;
    store.begin_delete("one", false)?;
    let old = MonitorJobScope {
        action_count: 999,
        ..scope("desk")
    };
    assert_eq!(
        store.enqueue_monitor_recognition(&old, &request, -1)?,
        MonitorJobAdmission {
            job_created: false,
            step_created: false
        }
    );
    let charged: (i64, i64) = store.connection.query_row(
        "SELECT charged_day, created_ms FROM monitor_steps WHERE monitor_id = 'desk'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(charged, (0, 10));
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert!(
        !reopened
            .enqueue_monitor_recognition(&scope("desk"), &request, 86_400_000)?
            .step_created
    );
    assert_eq!(
        reopened
            .monitor_processing("desk", 86_400_000)?
            .used_today_us,
        0
    );
    assert_eq!(
        reopened
            .monitor_processing("desk", 86_400_000)?
            .used_total_us,
        1_000_000
    );
    assert_eq!(reopened.local_asr_job("shared")?, job);
    Ok(())
}

#[test]
fn monitor_atomic_changed_replay_cannot_rebind_audio_policy_pin_or_job_request() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    let request = recognition(&store, "shared")?;
    store.enqueue_monitor_recognition(&scope("desk"), &request, 10)?;
    for changed in [
        MonitorJobScope {
            audio_us: 2,
            ..scope("desk")
        },
        MonitorJobScope {
            policy_version: 2,
            ..scope("desk")
        },
    ] {
        assert!(matches!(
            store.enqueue_monitor_recognition(&changed, &request, 11),
            Err(Error::IdempotencyConflict)
        ));
    }
    for change in ["pin", "hash", "parent", "job"] {
        let mut changed = request.clone();
        match change {
            "pin" => changed.analysis_id = "other-pin".into(),
            "hash" => changed.profile_sha256 = "a".repeat(64),
            "parent" => changed.parent_revision = 1,
            _ => changed.id = "other-job".into(),
        }
        assert!(matches!(
            store.enqueue_monitor_recognition(&scope("desk"), &changed, 11),
            Err(Error::IdempotencyConflict)
        ));
    }
    assert_eq!(count(&store, "analysis_jobs")?, 1);
    assert_eq!(
        store.monitor_processing("desk", 11)?.used_total_us,
        1_000_000
    );
    Ok(())
}

#[test]
fn monitor_atomic_fresh_authority_and_exact_recorded_audio_fail_without_admission() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    let request = recognition(&store, "fresh")?;
    assert!(
        store
            .enqueue_monitor_recognition(
                &MonitorJobScope {
                    audio_us: 999_999,
                    ..scope("desk")
                },
                &request,
                10
            )
            .is_err()
    );
    assert!(matches!(
        store.enqueue_monitor_recognition(&scope("desk"), &request, 1),
        Err(Error::Analysis("monitor-admission-clock"))
    ));
    let mut wrong_profile = request.clone();
    wrong_profile.profile_sha256 = "a".repeat(64);
    assert!(matches!(
        store.enqueue_monitor_recognition(&scope("desk"), &wrong_profile, 10),
        Err(Error::Analysis("monitor-profile-changed"))
    ));
    store.propose_monitor_action(
        "desk",
        "refused",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "Do anything".into(),
        },
        10,
    )?;
    assert_eq!(store.monitor_facts("desk", 11)?.action_count, 1);
    assert!(matches!(
        store.enqueue_monitor_recognition(&scope("desk"), &request, 11),
        Err(Error::Analysis("monitor-actions-changed"))
    ));
    store.propose_monitor_action("desk", "pause", ActionOrigin::User, &Proposal::Pause, 12)?;
    assert!(matches!(
        store.enqueue_monitor_recognition(
            &MonitorJobScope {
                action_count: 2,
                ..scope("desk")
            },
            &request,
            13
        ),
        Err(Error::Analysis("monitor-paused"))
    ));
    let mut revised = store.monitor_version("other", 1)?.spec;
    revised.sources = vec![];
    // A valid changed monitor follows a newly registered revision instead.
    store.register_source(
        "outside:v1",
        &HttpSource::new(
            "Outside",
            "https://example.com/other",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    revised.sources.push("outside:v1".into());
    store.revise_monitor("other", 1, &revised, 14)?;
    assert!(matches!(
        store.enqueue_monitor_recognition(&scope("other"), &request, 15),
        Err(Error::Analysis("monitor-policy-changed"))
    ));
    assert!(matches!(
        store.enqueue_monitor_recognition(
            &MonitorJobScope {
                policy_version: 2,
                ..scope("other")
            },
            &request,
            15
        ),
        Err(Error::Analysis("monitor-source-not-followed"))
    ));
    assert_eq!(count(&store, "analysis_jobs")?, 0);
    assert_eq!(count(&store, "monitor_steps")?, 0);
    Ok(())
}

#[test]
fn monitor_atomic_job_step_and_deferred_commit_faults_roll_back_both_rows() -> TestResult {
    for stage in ["job", "step", "commit"] {
        let root = tempfile::tempdir()?;
        let path = root.path().join("catalog");
        let mut store = fixture(&path)?;
        let request = recognition(&store, "fresh")?;
        match stage {
            "job" => store.connection.execute_batch("CREATE TRIGGER injected_fault BEFORE INSERT ON analysis_jobs BEGIN SELECT RAISE(ABORT, 'job fault'); END")?,
            "step" => store.connection.execute_batch("CREATE TRIGGER injected_fault BEFORE INSERT ON monitor_steps BEGIN SELECT RAISE(ABORT, 'step fault'); END")?,
            _ => store.connection.execute_batch("CREATE TABLE fixture_parent(id INTEGER PRIMARY KEY); CREATE TABLE fixture_debt(id INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER injected_fault AFTER INSERT ON monitor_steps BEGIN INSERT INTO fixture_debt VALUES (1); END")?,
        }
        assert!(
            store
                .enqueue_monitor_recognition(&scope("desk"), &request, 10)
                .is_err(),
            "{stage}"
        );
        assert_eq!(count(&store, "analysis_jobs")?, 0, "{stage}");
        assert_eq!(count(&store, "monitor_steps")?, 0, "{stage}");
        assert_eq!(count(&store, "job_attempts")?, 0, "{stage}");
        assert_eq!(
            count(&store, "analysis_inputs")?,
            1,
            "published metadata pin is outside this admission"
        );
        store
            .connection
            .execute_batch("DROP TRIGGER injected_fault")?;
        drop(store);
        let mut reopened = Store::open(&path)?;
        assert!(
            reopened
                .enqueue_monitor_recognition(&scope("desk"), &request, 11)?
                .job_created
        );
        assert_eq!(
            reopened.monitor_processing("desk", 11)?.used_total_us,
            1_000_000
        );
    }
    Ok(())
}

#[test]
fn monitor_atomic_failed_attachment_preserves_direct_and_shared_job_and_charges() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    let request = recognition(&store, "direct")?;
    let direct = store.enqueue_local_asr(&request, 9)?.0;
    assert!(
        !store
            .enqueue_monitor_recognition(&scope("desk"), &request, 10)?
            .job_created
    );
    store.connection.execute_batch("CREATE TRIGGER injected_fault BEFORE INSERT ON monitor_steps WHEN NEW.monitor_id = 'other' BEGIN SELECT RAISE(ABORT, 'attachment fault'); END")?;
    assert!(
        store
            .enqueue_monitor_recognition(&scope("other"), &request, 11)
            .is_err()
    );
    assert_eq!(store.local_asr_job("direct")?, direct);
    assert_eq!(
        store.monitor_processing("desk", 11)?.used_total_us,
        1_000_000
    );
    assert_eq!(store.monitor_processing("other", 11)?.used_total_us, 0);
    store
        .connection
        .execute_batch("DROP TRIGGER injected_fault")?;
    assert_eq!(
        store.enqueue_monitor_recognition(&scope("other"), &request, 12)?,
        MonitorJobAdmission {
            job_created: false,
            step_created: true
        }
    );
    assert_eq!(count(&store, "analysis_jobs")?, 1);
    assert_eq!(count(&store, "monitor_steps")?, 2);
    Ok(())
}

#[test]
fn monitor_atomic_daily_caps_roll_over_without_refilling_lifetime_or_replay_charge() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    let mut policy = store.monitor_version("desk", 1)?.spec;
    policy.daily_audio_seconds = 1;
    policy.total_audio_seconds = 2;
    store.revise_monitor("desk", 1, &policy, 3)?;
    let first_scope = MonitorJobScope {
        policy_version: 2,
        ..scope("desk")
    };
    let first_request = recognition(&store, "first")?;
    store.enqueue_monitor_recognition(&first_scope, &first_request, 86_399_999)?;
    publish_capture(&mut store, "two", "radio:v1", 1_000_000)?;
    store.admit_analysis("two-pin", "two", false, 10)?;
    store.publish_analysis("two-pin", 1)?;
    let mut second_request = recognition(&store, "second")?;
    second_request.analysis_id = "two-pin".into();
    let second_scope = MonitorJobScope {
        recording_id: "two",
        ..first_scope
    };
    assert!(matches!(
        store.enqueue_monitor_recognition(&second_scope, &second_request, 86_399_999),
        Err(Error::Analysis("monitor-daily-cap"))
    ));
    assert_eq!(count(&store, "analysis_jobs")?, 1);
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert!(
        reopened
            .enqueue_monitor_recognition(&second_scope, &second_request, 86_400_000)?
            .job_created
    );
    assert!(
        !reopened
            .enqueue_monitor_recognition(&first_scope, &first_request, 86_400_001)?
            .step_created
    );
    let usage = reopened.monitor_processing("desk", 86_400_001)?;
    assert_eq!(
        (usage.used_today_us, usage.used_total_us),
        (1_000_000, 2_000_000)
    );
    publish_capture(&mut reopened, "three", "radio:v1", 1_000_000)?;
    reopened.admit_analysis("three-pin", "three", false, 86_400_002)?;
    reopened.publish_analysis("three-pin", 1)?;
    policy.daily_audio_seconds = 2;
    reopened.revise_monitor("desk", 2, &policy, 86_400_003)?;
    let mut third_request = recognition(&reopened, "third")?;
    third_request.analysis_id = "three-pin".into();
    let third_scope = MonitorJobScope {
        policy_version: 3,
        recording_id: "three",
        ..scope("desk")
    };
    assert!(matches!(
        reopened.enqueue_monitor_recognition(&third_scope, &third_request, 172_800_000),
        Err(Error::Analysis("monitor-total-cap"))
    ));
    assert_eq!(count(&reopened, "analysis_jobs")?, 2);
    assert_eq!(count(&reopened, "monitor_steps")?, 2);
    let usage = reopened.monitor_processing("desk", 172_800_000)?;
    assert_eq!((usage.used_today_us, usage.used_total_us), (0, 2_000_000));
    Ok(())
}

#[test]
fn monitor_atomic_full_queue_refuses_fresh_work_but_preserves_shared_attachment() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    let reserved_request = recognition(&store, "reserved")?;
    let reserved = store.enqueue_local_asr(&reserved_request, 9)?.0;
    // Clone bounded valid queued catalog requests without claiming native workers.
    store.connection.execute_batch("WITH RECURSIVE n(v) AS (SELECT 1 UNION ALL SELECT v + 1 FROM n WHERE v < 1023) INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, state, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage) SELECT 'fill-' || n.v, j.generation, j.analysis_id, j.analysis_revision, j.recording_id, j.profile, j.kind, j.profile_sha256, j.expected_parent_revision, j.state, j.expected_bytes, j.expected_files, j.manifest_sha256, j.amount_micros, j.created_ms, j.lineage FROM analysis_jobs j CROSS JOIN n WHERE j.id = 'reserved'")?;
    let fresh = recognition(&store, "fresh")?;
    assert!(matches!(
        store.enqueue_monitor_recognition(&scope("desk"), &fresh, 10),
        Err(Error::Analysis("queue-full"))
    ));
    assert_eq!(count(&store, "analysis_jobs")?, 1024);
    assert_eq!(count(&store, "monitor_steps")?, 0);
    assert_eq!(store.monitor_processing("desk", 10)?.used_total_us, 0);
    assert_eq!(
        store.enqueue_monitor_recognition(&scope("other"), &reserved_request, 11)?,
        MonitorJobAdmission {
            job_created: false,
            step_created: true
        }
    );
    assert_eq!(store.local_asr_job("reserved")?, reserved);
    store.cancel_local_asr("fill-1", 1)?;
    assert_eq!(
        store.enqueue_monitor_recognition(&scope("desk"), &fresh, 12)?,
        MonitorJobAdmission {
            job_created: true,
            step_created: true
        }
    );
    assert!(
        !store
            .enqueue_monitor_recognition(&scope("desk"), &fresh, 13)?
            .step_created
    );
    assert_eq!(count(&store, "analysis_jobs")?, 1025);
    assert_eq!(count(&store, "monitor_steps")?, 2);
    assert_eq!(count(&store, "job_attempts")?, 0);
    for id in ["desk", "other"] {
        assert_eq!(store.monitor_processing(id, 13)?.used_total_us, 1_000_000);
    }
    Ok(())
}

#[test]
fn monitor_atomic_terminal_shared_job_attachment_preserves_completion_and_attempt() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    let request = heard(&mut store)?;
    let completed = store.local_asr_job("asr")?;
    assert_eq!(completed.state, "succeeded");
    assert_eq!(completed.attempt, 1);
    // Completed attempts live in the job; this table records interrupted attempts.
    assert_eq!(count(&store, "job_attempts")?, 0);
    assert_eq!(
        store.enqueue_monitor_recognition(&scope("other"), &request, 13)?,
        MonitorJobAdmission {
            job_created: false,
            step_created: true
        }
    );
    assert_eq!(store.local_asr_job("asr")?, completed);
    assert_eq!(count(&store, "job_attempts")?, 0);
    assert_eq!(
        store.monitor_processing("other", 13)?.used_total_us,
        1_000_000
    );
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert!(
        !reopened
            .enqueue_monitor_recognition(&scope("other"), &request, 86_400_000)?
            .step_created
    );
    assert_eq!(reopened.local_asr_job("asr")?, completed);
    assert_eq!(count(&reopened, "job_attempts")?, 0);
    assert_eq!(count(&reopened, "monitor_steps")?, 2);
    Ok(())
}

#[test]
fn monitor_atomic_translation_step_failure_rolls_back_job_and_preserves_recognition() -> TestResult
{
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog");
    let mut store = fixture(&path)?;
    heard(&mut store)?;
    let request = translation(&store, "mt-new")?;
    let scope = MonitorJobScope {
        audio_us: 0,
        ..scope("desk")
    };
    store.connection.execute_batch("CREATE TRIGGER injected_fault BEFORE INSERT ON monitor_steps WHEN NEW.stage = 'translation' BEGIN SELECT RAISE(ABORT, 'translation step fault'); END")?;
    assert!(
        store
            .enqueue_monitor_translation(&scope, &request, 13)
            .is_err()
    );
    assert_eq!(count(&store, "translation_jobs")?, 0);
    assert_eq!(count(&store, "monitor_steps")?, 1);
    assert_eq!(store.local_asr_job("asr")?.state, "succeeded");
    store
        .connection
        .execute_batch("DROP TRIGGER injected_fault")?;
    assert!(
        store
            .enqueue_monitor_translation(&scope, &request, 14)?
            .job_created
    );
    store.propose_monitor_action("desk", "pause", ActionOrigin::User, &Proposal::Pause, 15)?;
    assert!(
        !store
            .enqueue_monitor_translation(&scope, &request, -1)?
            .step_created
    );
    assert_eq!(
        store.monitor_processing("desk", 16)?.used_total_us,
        1_000_000
    );
    drop(store);
    assert_eq!(
        Store::open(&path)?.translation_job("mt-new")?.state,
        "queued"
    );
    Ok(())
}

#[test]
fn monitor_atomic_translation_rejects_unrelated_later_recognition_and_correction() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    heard(&mut store)?;
    let mut later = recognition(&store, "independent-later")?;
    later.parent_revision = 1;
    let work = store
        .admit_local_asr(&later, 13)?
        .1
        .ok_or(Error::StorageIntegrity)?;
    store.finish_local_asr(&work, &proof(&work, "independent report"), 14)?;
    let mut request = translation(&store, "mt-independent")?;
    request.transcript_revision = 2;
    let scope = MonitorJobScope {
        audio_us: 0,
        ..scope("desk")
    };
    assert!(matches!(
        store.enqueue_monitor_translation(&scope, &request, 15),
        Err(Error::Analysis("monitor-translation-unsupported"))
    ));
    store.correct_transcript("pin", 2, 0, "corrected independent report", 16)?;
    request.transcript_revision = 3;
    assert!(matches!(
        store.enqueue_monitor_translation(&scope, &request, 17),
        Err(Error::Analysis("monitor-translation-unsupported"))
    ));
    assert_eq!(count(&store, "translation_jobs")?, 0);
    assert_eq!(count(&store, "monitor_steps")?, 1);
    Ok(())
}

#[test]
fn monitor_atomic_production_skip_api_cannot_store_a_queued_step() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = fixture(&root.path().join("catalog"))?;
    let record = StepRecord {
        monitor_id: "desk",
        recording_id: "one",
        stage: "recognition",
        policy_version: 1,
        outcome: Ok(("pin", "asr")),
        audio_us: 1_000_000,
    };
    assert!(store.record_monitor_skip(&record, 10).is_err());
    assert_eq!(count(&store, "monitor_steps")?, 0);
    Ok(())
}
