use super::*;
use crate::{
    monitor::{ActionOrigin, MonitorSpec, MonitorTerm, Proposal},
    sources::{HttpSource, NetworkScope},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const START: i64 = 1_790_078_400_000;

fn spec(daily: u32, total: u64, bytes: u64) -> MonitorSpec {
    MonitorSpec {
        name: "News".into(),
        goal: "Follow news".into(),
        terms: vec![MonitorTerm {
            language: "ar".into(),
            text: "سد".into(),
        }],
        sources: vec!["a:v1".into(), "b:v1".into()],
        candidate_sources: vec!["c:v1".into()],
        schedules: vec![],
        daily_audio_seconds: 60,
        total_audio_seconds: 3600,
        recognition_profile: None,
        translation_profile: None,
        capture: Some(MonitorCaptureBounds {
            daily_seconds: daily,
            total_seconds: total,
            total_bytes: bytes,
        }),
    }
}

fn setup(path: &std::path::Path, policy: &MonitorSpec) -> TestResult {
    let mut store = Store::open(path)?;
    store.configure_dvr(
        512 * 1024 * 1024,
        64 * 1024 * 1024,
        14,
        std::env::current_exe()?.to_str().ok_or("executable path")?,
    )?;
    for id in ["a:v1", "b:v1", "c:v1"] {
        store.register_source(
            id,
            &HttpSource::new(
                "News",
                &format!("https://example.com/{id}"),
                NetworkScope::PublicInternet {},
            )?,
        )?;
    }
    store.create_monitor("news", policy, START - 1000)?;
    Ok(())
}

fn draft(id: &str, source: &str, seconds: i64) -> ScheduleDraft {
    ScheduleDraft {
        id: id.into(),
        source_revision: source.into(),
        zone: "Etc/UTC".into(),
        recurrence: "once".into(),
        civil_date: Some("2026-09-22".into()),
        weekday: None,
        hour: 12,
        minute: 0,
        second: 0,
        duration_seconds: seconds,
        maximum_bytes: 1024,
    }
}

fn owner(version: u32) -> MonitorScheduleOwner {
    MonitorScheduleOwner {
        monitor_id: "news".into(),
        version,
    }
}

fn assert_usage(store: &Store, now: i64, seconds: u64, bytes: u64, count: u32) -> Result<()> {
    let usage = store.monitor_capture_usage("news", now)?;
    assert_eq!(
        (
            usage.used_total_seconds,
            usage.reserved_total_bytes,
            usage.admissions
        ),
        (seconds, bytes, count)
    );
    Ok(())
}

#[test]
fn midnight_partitions_the_full_planned_window() -> Result<()> {
    assert_eq!(
        days(DAY_MS - 10_000, DAY_MS + 20_000)?,
        vec![(0, 10), (1, 20)]
    );
    assert_eq!(days(DAY_MS, DAY_MS + 30_000)?, vec![(1, 30)]);
    assert_eq!(days(DAY_MS - 30_000, DAY_MS)?, vec![(0, 30)]);
    assert!(days(1, 1001).is_err());
    Ok(())
}

#[test]
fn unused_policy_corruption_cannot_admit_or_reopen() -> TestResult {
    for field in ["'$.capture.total_bytes', 8192", "'$.sources[0]', 'c:v1'"] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("catalog.sqlite3");
        setup(&path, &spec(60, 120, 4096))?;
        let mut store = Store::open(&path)?;
        store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
        store.connection.execute_batch(&format!("DROP TRIGGER monitor_version_no_update; UPDATE monitor_versions SET spec_json = json_set(spec_json, {field});"))?;
        assert!(store.reconcile_schedules_at(START, true).is_err());
        assert_usage(&store, START, 0, 0, 0)?;
        let recordings: u32 =
            store
                .connection
                .query_row("SELECT count(*) FROM recordings", [], |row| row.get(0))?;
        assert_eq!(recordings, 0);
        drop(store);
        assert!(Store::open(&path).is_err());
    }
    Ok(())
}

#[test]
fn altered_waiting_plan_cannot_expand_the_owned_rule_or_charge_allowance() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(120, 240, 4096))?;
    let mut store = Store::open(&path)?;
    store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
    store
        .connection
        .execute("UPDATE schedule_occurrences SET maximum_bytes = 2048", [])?;
    assert!(store.reconcile_schedules_at(START, true).is_err());
    assert_usage(&store, START, 0, 0, 0)?;
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM recordings", [], |row| row
                .get::<_, u32>(0))?,
        0
    );
    drop(store);
    assert!(Store::open(&path).is_err());
    Ok(())
}

#[test]
fn lower_caps_preserve_earlier_admissions_and_refuse_further_work() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(120, 240, 4096))?;
    let mut store = Store::open(&path)?;
    store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
    let mut later = draft("b", "b:v1", 60);
    later.second = 1;
    store.create_monitor_schedule_at(&later, &owner(1), START - 1000)?;
    let launch = store
        .reconcile_schedules_at(START, true)?
        .launches
        .remove(0);
    store.fail_recording(&launch.job.version, &Error::InvalidInput("fixture"))?;
    let proof = store.schedule_occurrences("a")?[0]
        .capture_admission
        .clone();
    store.revise_monitor("news", 1, &spec(30, 30, 512), START)?;
    assert!(
        store
            .reconcile_schedules_at(START + 1000, true)?
            .launches
            .is_empty()
    );
    assert_eq!(
        store.monitor_capture_usage("news", START)?.refusals,
        vec![("total-cap".into(), 1)]
    );
    assert_usage(&store, START, 60, 1024, 1)?;
    drop(store);
    let reopened = Store::open(&path)?;
    assert_usage(&reopened, START, 60, 1024, 1)?;
    assert_eq!(
        reopened.schedule_occurrences("a")?[0].capture_admission,
        proof
    );
    assert_eq!(
        reopened.monitor_version("news", 1)?.spec.capture,
        spec(120, 240, 4096).capture
    );
    Ok(())
}

#[test]
fn admission_sequence_rejects_regressed_policy_and_action_provenance() -> TestResult {
    for corruption in [
        "UPDATE monitor_capture_admissions SET policy_version = 1, policy_sha256 = (SELECT spec_sha256 FROM monitor_versions WHERE version = 1), daily_cap = 120, total_cap = 360 WHERE ordinal = 2;",
        "UPDATE monitor_capture_admissions SET action_ordinal = 0 WHERE ordinal = 2;",
        "UPDATE monitor_capture_admissions SET action_ordinal = 2 WHERE ordinal = 1;",
        "UPDATE monitor_capture_admissions SET ordinal = 3 WHERE ordinal = 2;",
    ] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("catalog.sqlite3");
        setup(&path, &spec(120, 360, 4096))?;
        let mut store = Store::open(&path)?;
        store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
        let mut later = draft("b", "b:v1", 60);
        later.second = 1;
        store.create_monitor_schedule_at(&later, &owner(1), START - 1000)?;
        store.revise_monitor("news", 1, &spec(180, 300, 4096), START - 1000)?;
        store.propose_monitor_action(
            "news",
            "pause",
            ActionOrigin::User,
            &Proposal::Pause,
            START - 1000,
        )?;
        let first = store
            .reconcile_schedules_at(START, true)?
            .launches
            .remove(0);
        store.fail_recording(&first.job.version, &Error::InvalidInput("fixture"))?;
        store.revise_monitor("news", 2, &spec(120, 240, 4096), START + 1000)?;
        store.propose_monitor_action(
            "news",
            "resume",
            ActionOrigin::User,
            &Proposal::Resume,
            START + 1000,
        )?;
        store.reconcile_schedules_at(START + 1000, true)?;
        store.audit_monitor_capture()?;
        store
            .connection
            .execute_batch("DROP TRIGGER monitor_capture_admissions_immutable;")?;
        store.connection.execute_batch(corruption)?;
        drop(store);
        assert!(Store::open(&path).is_err());
    }
    Ok(())
}

#[test]
fn refusal_source_revision_and_owned_policy_chronology_fail_reopen_when_forged() -> TestResult {
    for corruption in [
        "DROP TRIGGER monitor_capture_refusals_immutable; UPDATE monitor_capture_refusals SET source_revision = 'c:v1';",
        "DROP TRIGGER monitor_capture_refusals_immutable; UPDATE monitor_capture_refusals SET rule_revision = rule_revision + 1;",
        "DROP TRIGGER monitor_capture_refusals_immutable; UPDATE monitor_capture_refusals SET policy_version = 1;",
        "DROP TRIGGER monitor_capture_admissions_immutable; UPDATE monitor_capture_admissions SET policy_version = 1, policy_sha256 = (SELECT spec_sha256 FROM monitor_versions WHERE version = 1);",
    ] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("catalog.sqlite3");
        let policy = spec(60, 120, 4096);
        setup(&path, &policy)?;
        let mut store = Store::open(&path)?;
        let mut revised = policy.clone();
        revised.name = "Revised".into();
        store.revise_monitor("news", 1, &revised, START - 1000)?;
        store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(2), START - 1000)?;
        store.create_monitor_schedule_at(&draft("b", "b:v1", 60), &owner(2), START - 1000)?;
        store.reconcile_schedules_at(START, true)?;
        assert_eq!(
            store.monitor_capture_usage("news", START)?.refusals,
            vec![("daily-cap".into(), 1)]
        );
        store.connection.execute_batch(corruption)?;
        drop(store);
        assert!(Store::open(&path).is_err());
    }
    Ok(())
}

#[test]
fn genuine_populated_v39_migration_preserves_policy_bytes_and_rolls_back_conflicts() -> TestResult {
    for conflict in [false, true] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("catalog.sqlite3");
        let mut connection = Connection::open(&path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/storage");
        let mut migrations = std::fs::read_dir(source)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        migrations.retain(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    path.extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
                        && name
                            .get(..3)
                            .and_then(|prefix| prefix.parse::<u32>().ok())
                            .is_some_and(|version| (1..=39).contains(&version))
                })
        });
        migrations.sort_by_cached_key(|path| {
            path.to_string_lossy().replace(
                "026-transcript-invariants.sql",
                "026-z-transcript-invariants.sql",
            )
        });
        // Schema 26 has a revision migration followed by its invariant migration.
        assert_eq!(migrations.len(), 40);
        let transaction = connection.transaction()?;
        for migration in migrations {
            let name = migration
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("migration name")?;
            match name.get(..3).ok_or("migration version")? {
                "030" => {
                    crate::storage::widen::migrate_030(&transaction)?;
                    transaction.execute_batch(&std::fs::read_to_string(migration)?)?;
                }
                "031" => crate::storage::job_pool::migrate_031(&transaction)?,
                "034" => crate::storage::recognition::migrate_034(&transaction)?,
                "036" => crate::storage::corrections::migrate_036(&transaction)?,
                "039" => crate::storage::briefings::migrate_039(&transaction)?,
                _ => transaction.execute_batch(&std::fs::read_to_string(migration)?)?,
            }
        }
        transaction.commit()?;
        let mut policy = spec(60, 120, 4096);
        policy.capture = None;
        let json = serde_json::to_string(&policy)?;
        let digest = policy.digest()?;
        connection.execute("INSERT INTO source_revisions(id, kind, name, endpoint, network_scope, created_ms) VALUES ('a:v1', 'http_audio', 'News', 'https://example.com/a', 'public_internet', ?1)", [START])?;
        connection.execute(
            "INSERT INTO monitors(id, created_ms) VALUES ('news', ?1)",
            [START],
        )?;
        connection.execute("INSERT INTO monitor_versions(monitor_id, version, spec_json, spec_sha256, created_ms) VALUES ('news', 1, ?1, ?2, ?3)", rusqlite::params![json, digest, START])?;
        if conflict {
            connection.execute_batch("CREATE TABLE monitor_capture_days(conflict TEXT);")?;
        }
        drop(connection);
        if conflict {
            assert!(Store::open(&path).is_err());
            let connection = Connection::open(&path)?;
            assert_eq!(
                connection.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))?,
                39
            );
            assert_eq!(
                connection.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = 'monitor_capture_rules'",
                    [],
                    |row| row.get::<_, u32>(0)
                )?,
                0
            );
        } else {
            let store = Store::open(&path)?;
            assert_eq!(store.monitor_version("news", 1)?.spec_sha256, digest);
            assert_eq!(
                store.connection.query_row(
                    "SELECT spec_json FROM monitor_versions",
                    [],
                    |row| row.get::<_, String>(0)
                )?,
                json
            );
            assert_usage(&store, START, 0, 0, 0)?;
            assert!(store.monitor_version("news", 1)?.spec.capture.is_none());
        }
    }
    Ok(())
}

#[test]
fn legacy_serialization_and_processing_caps_keep_their_original_identity() -> TestResult {
    let mut policy = spec(60, 3600, 4096);
    policy.capture = None;
    let old_json = serde_json::json!({
        "name":"News", "goal":"Follow news", "terms":[{"language":"ar","text":"سد"}],
        "sources":["a:v1","b:v1"], "candidate_sources":["c:v1"], "schedules":[],
        "daily_audio_seconds":60, "total_audio_seconds":3600, "recognition_profile":null,"translation_profile":null
    });
    let old: MonitorSpec = serde_json::from_value(old_json.clone())?;
    assert_eq!(serde_json::to_value(&policy)?, old_json);
    assert_eq!(old.digest()?, policy.digest()?);
    let domain_hash =
        crate::recognition::sha256_hex(&serde_json::to_vec(&("sigy-monitor-spec-v1", &policy))?);
    assert_eq!(policy.digest()?, domain_hash);
    let json = serde_json::to_string(&policy)?;
    assert_eq!(super::super::checked_spec(&json, &domain_hash)?, policy);
    let omitted = json
        .replace(",\"recognition_profile\":null", "")
        .replace(",\"translation_profile\":null", "");
    let historical =
        crate::recognition::sha256_hex(format!("[\"sigy-monitor-spec-v1\",{omitted}]").as_bytes());
    assert_eq!(super::super::checked_spec(&omitted, &historical)?, policy);
    assert_ne!(historical, domain_hash);
    let enabled = spec(60, 3600, 4096);
    assert_ne!(policy.digest()?, enabled.digest()?);
    assert_eq!(enabled.daily_audio_seconds, policy.daily_audio_seconds);
    for invalid in [
        MonitorCaptureBounds {
            daily_seconds: 0,
            total_seconds: 3600,
            total_bytes: 1,
        },
        MonitorCaptureBounds {
            daily_seconds: 60,
            total_seconds: 59,
            total_bytes: 1,
        },
        MonitorCaptureBounds {
            daily_seconds: 60,
            total_seconds: 3600,
            total_bytes: u64::MAX,
        },
    ] {
        policy.capture = Some(invalid);
        assert!(policy.validate().is_err());
    }
    Ok(())
}

#[test]
fn ownership_is_new_explicit_exact_and_cannot_adopt_standalone_rules() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(120, 3600, 4096))?;
    let mut store = Store::open(&path)?;
    let plan = draft("owned", "a:v1", 60);
    assert!(
        store
            .create_monitor_schedule_at(&plan, &owner(2), START - 1000)
            .is_err()
    );
    assert!(store.schedule("owned")?.is_none());
    let created = store.create_monitor_schedule_at(&plan, &owner(1), START - 1000)?;
    assert_eq!(created.rule.monitor_owner, Some(owner(1)));
    assert!(
        !store
            .create_monitor_schedule_at(&plan, &owner(1), START)?
            .newly_created
    );
    assert!(store.create_schedule_at(&plan, START).is_err());
    let standalone = draft("standalone", "b:v1", 60);
    store.create_schedule_at(&standalone, START - 1000)?;
    assert!(
        store
            .create_monitor_schedule_at(&standalone, &owner(1), START)
            .is_err()
    );
    assert!(
        store
            .create_monitor_schedule_at(&draft("candidate", "c:v1", 60), &owner(1), START)
            .is_err()
    );
    assert!(store.schedule("candidate")?.is_none());
    let mut oversized = draft("oversized", "a:v1", 60);
    oversized.maximum_bytes = 4097;
    assert!(
        store
            .create_monitor_schedule_at(&oversized, &owner(1), START)
            .is_err()
    );
    assert!(store.schedule("oversized")?.is_none());
    assert_usage(&store, START, 0, 0, 0)?;
    Ok(())
}

#[test]
fn late_admission_reserves_the_whole_plan_once_and_survives_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(120, 3600, 4096))?;
    let mut store = Store::open(&path)?;
    let plan = draft("late", "a:v1", 120);
    store.create_monitor_schedule_at(&plan, &owner(1), START - 1000)?;
    let launched = store.reconcile_schedules_at(START + 30_000, true)?;
    assert_eq!(launched.launches.len(), 1);
    assert_usage(&store, START, 120, 1024, 1)?;
    let recorded = store.recording("late:2026-09-22")?;
    assert_eq!(recorded.gaps[0].end_us, 30_000_000);
    let saved = store.schedule_occurrences("late")?;
    let admission = saved[0]
        .capture_admission
        .as_ref()
        .ok_or("capture provenance")?;
    assert_eq!(
        (
            admission.policy_version,
            admission.planned_seconds,
            admission.maximum_bytes
        ),
        (1, 120, 1024)
    );
    assert_eq!(admission.policy_sha256, spec(120, 3600, 4096).digest()?);
    assert!(
        store
            .reconcile_schedules_at(START + 30_001, true)?
            .launches
            .is_empty()
    );
    store.recover_captures()?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert!(
        reopened
            .reconcile_schedules_at(START + 40_000, true)?
            .launches
            .is_empty()
    );
    assert_usage(&reopened, START, 120, 1024, 1)?;
    reopened.audit_monitor_capture()?;
    Ok(())
}

#[test]
fn daily_cap_and_midnight_partition_are_shared_across_sources() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(20, 3600, 4096))?;
    let mut store = Store::open(&path)?;
    let mut crossing = draft("a", "a:v1", 20);
    crossing.hour = 23;
    crossing.minute = 59;
    crossing.second = 50;
    let start = START + 12 * 3_600_000 - 10_000;
    store.create_monitor_schedule_at(&crossing, &owner(1), start - 1000)?;
    let mut second = crossing.clone();
    second.id = "b".into();
    second.source_revision = "b:v1".into();
    second.duration_seconds = 30;
    store.create_monitor_schedule_at(&second, &owner(1), start - 1000)?;
    let batch = store.reconcile_schedules_at(start, true)?;
    assert_eq!(batch.launches.len(), 1);
    assert_eq!(
        store
            .monitor_capture_usage("news", start)?
            .used_today_seconds,
        10
    );
    assert_eq!(
        store
            .monitor_capture_usage("news", start + 20_000)?
            .used_today_seconds,
        10
    );
    assert!(
        store
            .reconcile_schedules_at(start + 1000, true)?
            .launches
            .is_empty()
    );
    assert_eq!(
        store.monitor_capture_usage("news", start)?.refusals,
        vec![("daily-cap".into(), 1)]
    );
    store.reconcile_schedules_at(start + 30_000, true)?;
    assert_eq!(
        store.schedule_occurrences("b")?[0].miss_reason.as_deref(),
        Some("elapsed")
    );
    assert_usage(&store, start, 20, 1024, 1)?;
    Ok(())
}

#[test]
fn lifetime_reservations_do_not_refund_failures_or_reset_with_versions() -> TestResult {
    for (total, bytes, reason) in [(60, 4096, "total-cap"), (120, 1024, "byte-cap")] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("catalog.sqlite3");
        let policy = spec(60, total, bytes);
        setup(&path, &policy)?;
        let mut store = Store::open(&path)?;
        store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
        let mut later = draft("b", "b:v1", 60);
        later.civil_date = Some("2026-09-23".into());
        store.create_monitor_schedule_at(&later, &owner(1), START - 1000)?;
        let launch = store
            .reconcile_schedules_at(START, true)?
            .launches
            .remove(0);
        store.fail_recording(&launch.job.version, &Error::InvalidInput("fixture failure"))?;
        let mut changed = policy.clone();
        changed.name = "New version".into();
        store.revise_monitor("news", 1, &changed, START + 1000)?;
        assert!(
            !store
                .create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START + 1000)?
                .newly_created
        );
        assert!(
            store
                .reconcile_schedules_at(START + DAY_MS, true)?
                .launches
                .is_empty()
        );
        assert_eq!(
            store
                .monitor_capture_usage("news", START + DAY_MS)?
                .refusals,
            vec![(reason.into(), 1)]
        );
        assert_usage(&store, START + DAY_MS, 60, 1024, 1)?;
        drop(store);
        assert_usage(&Store::open(&path)?, START + DAY_MS, 60, 1024, 1)?;
    }
    Ok(())
}

#[test]
fn processing_pause_does_not_pause_capture_and_disabling_leaves_standalone_authority() -> TestResult
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    let policy = spec(120, 3600, 4096);
    setup(&path, &policy)?;
    let mut store = Store::open(&path)?;
    store.create_monitor_schedule_at(&draft("owned", "a:v1", 60), &owner(1), START - 1000)?;
    store.propose_monitor_action(
        "news",
        "pause",
        ActionOrigin::User,
        &Proposal::Pause,
        START - 500,
    )?;
    assert_eq!(store.reconcile_schedules_at(START, true)?.launches.len(), 1);
    let mut next = draft("next", "a:v1", 60);
    next.civil_date = Some("2026-09-23".into());
    store.create_monitor_schedule_at(&next, &owner(1), START)?;
    let mut independent = next.clone();
    independent.id = "standalone".into();
    independent.source_revision = "b:v1".into();
    store.create_schedule_at(&independent, START)?;
    let mut disabled = policy;
    disabled.capture = None;
    store.revise_monitor("news", 1, &disabled, START + 1000)?;
    let launches = store.reconcile_schedules_at(START + DAY_MS, true)?.launches;
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].source_revision, "b:v1");
    assert_eq!(
        store.monitor_capture_usage("news", START)?.refusals,
        vec![("capture-disabled".into(), 1)]
    );
    assert_usage(&store, START, 60, 1024, 1)?;
    Ok(())
}

#[test]
fn source_removal_and_refused_schedule_revision_preserve_policy_history() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(120, 3600, 4096))?;
    let mut store = Store::open(&path)?;
    let mut plan = draft("removed", "a:v1", 60);
    store.create_monitor_schedule_at(&plan, &owner(1), START - 1000)?;
    store.propose_monitor_action(
        "news",
        "remove",
        ActionOrigin::User,
        &Proposal::RemoveSource {
            source: "a:v1".into(),
        },
        START - 500,
    )?;
    assert!(
        store
            .reconcile_schedules_at(START, true)?
            .launches
            .is_empty()
    );
    assert_eq!(
        store.monitor_capture_usage("news", START)?.refusals,
        vec![("source-not-followed".into(), 1)]
    );
    plan.minute = 1;
    store.revise_schedule_at(&plan, START)?;
    assert!(
        store
            .reconcile_schedules_at(START + 60_000, true)?
            .launches
            .is_empty()
    );
    assert_eq!(
        store.monitor_capture_usage("news", START)?.refusals,
        vec![("source-not-followed".into(), 2)]
    );
    let refused_starts: Vec<i64> = store
        .connection
        .prepare("SELECT start_ms FROM monitor_capture_refusals ORDER BY rule_revision")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert_eq!(refused_starts, vec![START, START + 60_000]);
    assert_usage(&store, START, 0, 0, 0)?;
    Ok(())
}

#[test]
fn quota_refusal_rolls_back_capture_charge_and_later_admission_is_exact() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(120, 3600, 4096))?;
    let mut store = Store::open(&path)?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        512,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable path")?,
    )?;
    store.create_monitor_schedule_at(&draft("quota", "a:v1", 60), &owner(1), START - 1000)?;
    assert!(store.reconcile_schedules_at(START, true)?.quota_exhausted);
    assert_usage(&store, START, 0, 0, 0)?;
    assert_eq!(store.schedule_occurrences("quota")?[0].state, "waiting");
    store.configure_dvr(
        4096,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable path")?,
    )?;
    assert_eq!(
        store
            .reconcile_schedules_at(START + 1000, true)?
            .launches
            .len(),
        1
    );
    assert_usage(&store, START, 60, 1024, 1)?;
    assert!(
        store
            .connection
            .execute(
                "UPDATE monitor_capture_admissions SET planned_seconds = 1",
                []
            )
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM monitor_capture_days", [])
            .is_err()
    );
    Ok(())
}

#[test]
fn populated_capture_and_refusal_history_survive_current_schema_backup_restore() -> TestResult {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("library");
    let mut library = crate::library::Library::open(&root, true)?;
    let path = root.join("catalog.sqlite3");
    // Initialize through the existing owner, rather than acquiring a second library lock.
    let store = library.store_mut();
    store.register_source(
        "a:v1",
        &HttpSource::new(
            "News",
            "https://example.com/a",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    store.register_source(
        "b:v1",
        &HttpSource::new(
            "News",
            "https://example.com/b",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    store.register_source(
        "c:v1",
        &HttpSource::new(
            "News",
            "https://example.com/c",
            NetworkScope::PublicInternet {},
        )?,
    )?;
    let executable = std::env::current_exe()?;
    store.configure_dvr(
        4096,
        64 * 1024 * 1024,
        14,
        executable.to_str().ok_or("executable path")?,
    )?;
    store.create_monitor("news", &spec(60, 120, 4096), START - 1000)?;
    store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
    store.create_monitor_schedule_at(&draft("b", "b:v1", 60), &owner(1), START - 1000)?;
    let launch = store
        .reconcile_schedules_at(START, true)?
        .launches
        .remove(0);
    store.fail_recording(&launch.job.version, &Error::InvalidInput("fixture"))?;
    store.revise_monitor("news", 1, &spec(120, 240, 4096), START + 1000)?;
    let second = store
        .reconcile_schedules_at(START + 1000, true)?
        .launches
        .remove(0);
    store.fail_recording(&second.job.version, &Error::InvalidInput("fixture"))?;
    store.revise_monitor("news", 2, &spec(30, 30, 512), START + 2000)?;
    store.connection.execute_batch("DROP TRIGGER monitor_capture_admissions_immutable; UPDATE monitor_capture_admissions SET rowid = -ordinal; CREATE TRIGGER monitor_capture_admissions_immutable BEFORE UPDATE ON monitor_capture_admissions BEGIN SELECT RAISE(ABORT, 'capture admission is immutable'); END;")?;
    store.audit_monitor_capture()?;
    let before = store.monitor_capture_usage("news", START)?;
    assert_eq!((before.admissions, before.used_total_seconds), (2, 120));
    let proof = store.schedule_occurrences("a")?[0]
        .capture_admission
        .clone();
    assert_eq!(before.refusals, vec![("daily-cap".into(), 1)]);
    assert!(path.is_file());
    let backup = dir.path().join("backup");
    let manifest = crate::backup::backup(&library, &backup)?;
    assert_eq!(manifest.schema_version, crate::storage::SCHEMA_VERSION);
    assert!(manifest.media.is_empty());
    drop(library);
    let restored = dir.path().join("restored");
    crate::backup::restore(&backup, &restored)?;
    let restored = crate::library::Library::open(&restored, false)?;
    assert_eq!(
        restored.store().monitor_capture_usage("news", START)?,
        before
    );
    assert_eq!(
        restored.store().schedule("a")?.ok_or("rule")?.monitor_owner,
        Some(owner(1))
    );
    assert_eq!(
        restored.store().schedule_occurrences("a")?[0].capture_admission,
        proof
    );
    restored.store().audit_monitor_capture()?;
    Ok(())
}

#[test]
fn altered_policy_bounds_or_daily_partition_fail_reopen_even_if_sums_match() -> TestResult {
    for corruption in [
        "DROP TRIGGER monitor_capture_admissions_immutable; UPDATE monitor_capture_admissions SET daily_cap = daily_cap + 1;",
        "DROP TRIGGER monitor_capture_days_immutable; UPDATE monitor_capture_days SET utc_day = utc_day + 1;",
        "DROP TRIGGER monitor_version_no_update; UPDATE monitor_versions SET spec_json = json_set(spec_json, '$.capture.daily_seconds', 61);",
    ] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("catalog.sqlite3");
        setup(&path, &spec(60, 120, 4096))?;
        let mut store = Store::open(&path)?;
        store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
        store.reconcile_schedules_at(START, true)?;
        store.connection.execute_batch(corruption)?;
        drop(store);
        assert!(Store::open(&path).is_err());
    }
    Ok(())
}

#[test]
fn owned_civil_windows_keep_misses_and_the_earlier_fall_offset_without_charges_for_misses()
-> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(900, 3600, 4096))?;
    let mut store = Store::open(&path)?;
    let mut spring = draft("spring", "a:v1", 60);
    spring.zone = "America/New_York".into();
    spring.civil_date = Some("2026-03-08".into());
    spring.hour = 2;
    spring.minute = 30;
    let transition = 1_772_953_200_000;
    store.create_monitor_schedule_at(&spring, &owner(1), transition - 60_000)?;
    assert!(
        store
            .reconcile_schedules_at(transition + 1000, true)?
            .launches
            .is_empty()
    );
    assert_eq!(
        store.schedule_occurrences("spring")?[0]
            .miss_reason
            .as_deref(),
        Some("spring_forward")
    );
    let elapsed = draft("elapsed", "a:v1", 60);
    store.create_monitor_schedule_at(&elapsed, &owner(1), START + 60_000)?;
    assert_eq!(
        store.schedule_occurrences("elapsed")?[0]
            .miss_reason
            .as_deref(),
        Some("elapsed")
    );
    assert_usage(&store, START, 0, 0, 0)?;
    let mut fall = draft("fall", "a:v1", 900);
    fall.zone = "America/New_York".into();
    fall.civil_date = Some("2026-11-01".into());
    fall.hour = 1;
    fall.minute = 30;
    let earlier = 1_793_511_000_000;
    store.create_monitor_schedule_at(&fall, &owner(1), earlier - 1000)?;
    assert_eq!(
        store
            .reconcile_schedules_at(earlier + 300_000, true)?
            .launches
            .len(),
        1
    );
    let row = store.schedule_occurrences("fall")?.remove(0);
    assert_eq!(row.offset_seconds, Some(-4 * 3600));
    assert_eq!(row.start_ms, Some(earlier));
    assert_usage(&store, earlier, 900, 1024, 1)?;
    assert!(
        store
            .reconcile_schedules_at(earlier + 3_600_000, true)?
            .launches
            .is_empty()
    );
    assert_usage(&store, earlier, 900, 1024, 1)?;
    Ok(())
}

#[test]
fn capture_capacity_defers_without_charging_and_models_cannot_enable_capture() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("catalog.sqlite3");
    setup(&path, &spec(120, 3600, 4096))?;
    let mut store = Store::open(&path)?;
    store.create_schedule_at(&draft("0-block", "a:v1", 60), START - 1000)?;
    store.create_schedule_at(&draft("1-block", "b:v1", 60), START - 1000)?;
    store.create_monitor_schedule_at(&draft("a", "a:v1", 60), &owner(1), START - 1000)?;
    let blockers = store.reconcile_schedules_at(START, true)?.launches;
    assert_eq!(blockers.len(), 2);
    assert_usage(&store, START, 0, 0, 0)?;
    assert_eq!(store.schedule_occurrences("a")?[0].state, "waiting");
    for blocker in blockers {
        store.fail_recording(&blocker.job.version, &Error::InvalidInput("fixture"))?;
    }
    assert_eq!(store.reconcile_schedules_at(START, true)?.launches.len(), 1);
    assert_usage(&store, START, 60, 1024, 1)?;
    store.propose_monitor_action(
        "news",
        "caps",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "raise capture bytes and create a schedule".into(),
        },
        START + 1000,
    )?;
    assert_eq!(
        store
            .monitor_actions("news", None)?
            .last()
            .ok_or("action")?
            .decision,
        "refused"
    );
    store.propose_monitor_action(
        "news",
        "candidate",
        ActionOrigin::Model,
        &Proposal::AddSource {
            source: "c:v1".into(),
        },
        START + 1000,
    )?;
    assert!(store.schedule("candidate")?.is_none());
    store.create_monitor_schedule_at(&draft("candidate", "c:v1", 60), &owner(1), START + 1000)?;
    assert_eq!(
        store
            .reconcile_schedules_at(START + 1000, true)?
            .launches
            .len(),
        1
    );
    let admission = store
        .schedule_occurrences("candidate")?
        .remove(0)
        .capture_admission
        .ok_or("admission")?;
    assert_eq!(admission.action_ordinal, 2);
    store.audit_monitor_capture()?;
    assert_usage(&store, START, 120, 2048, 2)?;
    Ok(())
}
