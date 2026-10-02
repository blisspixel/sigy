use super::*;

#[test]
fn later_rule_and_commit_faults_roll_back_the_whole_bounded_schedule_pass() -> TestResult {
    for fault in [
        "CREATE TRIGGER fixture BEFORE INSERT ON task_collection_admissions WHEN NEW.ordinal = 1 BEGIN SELECT RAISE(ABORT, 'fault'); END",
        "CREATE TABLE fixture_parent(id INTEGER PRIMARY KEY); CREATE TABLE fixture_child(id INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fixture AFTER INSERT ON task_collection_admissions WHEN NEW.ordinal = 1 BEGIN INSERT INTO fixture_child VALUES(1); END",
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = setup(&path)?;
        let mut plan = spec();
        plan.captures[1].start_ms = START;
        store.start_task_collection("task", "collect", &plan, 0, START - 1000)?;
        store.connection.execute_batch(fault)?;
        assert!(store.reconcile_schedules_at(START, true).is_err());
        assert_no_admission(&store)?;
        assert_eq!(
            store
                .monitor_capture_usage("monitor", START)?
                .used_total_seconds,
            0
        );
        assert!(
            store
                .task_collection("task")?
                .ok_or("collection")?
                .captures
                .iter()
                .all(|capture| capture.state == "waiting")
        );
        store.connection.execute_batch("DROP TRIGGER fixture")?;
        drop(store);
        let mut reopened = Store::open(&path)?;
        assert_eq!(
            reopened.reconcile_schedules_at(START, true)?.launches.len(),
            2
        );
        assert_eq!(count(&reopened, "task_collection_admissions")?, 2);
        assert!(
            reopened
                .reconcile_schedules_at(START, true)?
                .launches
                .is_empty()
        );
    }
    Ok(())
}

#[test]
fn quota_savepoint_returns_prior_committed_launches_and_charges_only_successful_rules() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    store.configure_dvr(
        1024,
        64 * 1024 * 1024,
        14,
        std::env::current_exe()?.to_str().ok_or("executable")?,
    )?;
    let mut plan = spec();
    plan.captures[1].start_ms = START;
    store.start_task_collection("task", "collect", &plan, 0, START - 1000)?;
    let mut batch = store.reconcile_schedules_at(START, true)?;
    assert!(batch.quota_exhausted);
    assert_eq!(batch.launches.len(), 1);
    assert_eq!(count(&store, "capture_jobs")?, 1);
    assert_eq!(count(&store, "task_collection_admissions")?, 1);
    assert_eq!(
        store
            .monitor_capture_usage("monitor", START)?
            .reserved_total_bytes,
        1024
    );
    store.fail_recording(
        &batch.launches.remove(0).job.version,
        &Error::InvalidInput("fixture"),
    )?;
    let held = store.reconcile_schedules_at(START, true)?;
    assert!(held.quota_exhausted);
    assert!(held.launches.is_empty());
    store.configure_dvr(
        2048,
        64 * 1024 * 1024,
        14,
        std::env::current_exe()?.to_str().ok_or("executable")?,
    )?;
    assert_eq!(store.reconcile_schedules_at(START, true)?.launches.len(), 1);
    assert_eq!(
        store
            .monitor_capture_usage("monitor", START)?
            .reserved_total_bytes,
        2048
    );
    assert_eq!(
        store
            .monitor_capture_usage("monitor", START)?
            .used_total_seconds,
        60
    );
    drop(store);
    assert_eq!(
        Store::open(&path)?
            .task_collection("task")?
            .ok_or("collection")?
            .captures
            .iter()
            .filter(|capture| capture.recording_id.is_some())
            .count(),
        2
    );
    Ok(())
}

#[test]
fn populated_midpoint_snapshot_resumes_future_collection_without_refilling_prior_reservations()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"))?;
    store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    store.reconcile_schedules_at(START, true)?;
    let before = store.task_collection("task")?.ok_or("collection")?;
    let usage = store.monitor_capture_usage("monitor", START)?;
    assert_eq!(
        (
            usage.admissions,
            usage.used_total_seconds,
            usage.reserved_total_bytes
        ),
        (1, 30, 1024)
    );
    let snapshot = directory.path().join("snapshot");
    store.snapshot_catalog(&snapshot)?;
    let mut restored = Store::open(&snapshot)?;
    assert_eq!(restored.task_collection("task")?, Some(before.clone()));
    assert_eq!(restored.recover_captures()?, 1);
    assert_eq!(restored.monitor_capture_usage("monitor", START)?, usage);
    let interrupted = restored.task_collection("task")?.ok_or("collection")?;
    assert_eq!(
        interrupted.captures[0].recording_state.as_deref(),
        Some("interrupted")
    );
    assert_eq!(interrupted.captures[1].state, "waiting");
    assert!(
        restored
            .reconcile_schedules_at(START, true)?
            .launches
            .is_empty()
    );
    assert_eq!(
        restored
            .reconcile_schedules_at(START + 60_000, true)?
            .launches
            .len(),
        1
    );
    assert_eq!(
        store
            .reconcile_schedules_at(START + 60_000, true)?
            .launches
            .len(),
        1
    );
    assert_eq!(
        restored.monitor_capture_usage("monitor", START)?,
        store.monitor_capture_usage("monitor", START)?
    );
    assert_eq!(
        restored.connection.query_row(
            "SELECT count(*) FROM task_collection_admissions",
            [],
            |row| row.get::<_, u32>(0)
        )?,
        2
    );
    let resumed = restored.cancel_task_collection("task", "cancel", 1, START + 60_000)?;
    let continuous = store.cancel_task_collection("task", "cancel", 1, START + 60_000)?;
    assert_eq!(resumed.grant_sha256, continuous.grant_sha256);
    assert_eq!(resumed.generation, continuous.generation);
    assert_eq!(
        restored.connection.query_row(
            "SELECT admitted_mask FROM task_collection_cancellations",
            [],
            |row| row.get::<_, u32>(0)
        )?,
        3
    );
    restored.audit_task_collections()?;
    Ok(())
}

#[test]
fn utc_midnight_and_failed_capture_keep_full_day_and_lifetime_charges_across_policy_versions()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    let midnight = (START / 86_400_000 + 1) * 86_400_000;
    let bounded = midnight_policy(30);
    store.revise_monitor("monitor", 1, &bounded, START)?;
    let scope = TaskSpec {
        goal: "Follow midnight".into(),
        monitor_id: "monitor".into(),
        monitor_version: 2,
        monitor_actions: 0,
        from_ms: midnight - 30_000,
        to_ms: midnight + 120_000,
    };
    store.create_task("midnight", &scope, START)?;
    let first = TaskCollectionSpec {
        captures: vec![TaskCaptureSpec {
            start_ms: midnight - 30_000,
            duration_seconds: 60,
            ..spec().captures[0].clone()
        }],
    };
    store.start_task_collection("midnight", "collect", &first, 0, START)?;
    let launch = store
        .reconcile_schedules_at(midnight - 30_000, true)?
        .launches
        .remove(0);
    assert_eq!(
        store
            .monitor_capture_usage("monitor", midnight - 1)?
            .used_today_seconds,
        30
    );
    assert_eq!(
        store
            .monitor_capture_usage("monitor", midnight)?
            .used_today_seconds,
        30
    );
    store.fail_recording(&launch.job.version, &Error::InvalidInput("fixture"))?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    let usage = reopened.monitor_capture_usage("monitor", midnight)?;
    assert_eq!(
        (usage.used_total_seconds, usage.reserved_total_bytes),
        (60, 1024)
    );
    let bounded = midnight_policy(60);
    reopened.revise_monitor("monitor", 2, &bounded, midnight)?;
    let next_scope = TaskSpec {
        monitor_version: 3,
        ..scope
    };
    reopened.create_task("later", &next_scope, midnight)?;
    let next = TaskCollectionSpec {
        captures: vec![TaskCaptureSpec {
            start_ms: midnight + 60_000,
            ..spec().captures[1].clone()
        }],
    };
    reopened.start_task_collection("later", "collect", &next, 0, midnight)?;
    assert!(
        reopened
            .reconcile_schedules_at(midnight + 60_000, true)?
            .launches
            .is_empty()
    );
    assert_eq!(
        reopened
            .monitor_capture_usage("monitor", midnight)?
            .used_total_seconds,
        60
    );
    assert_eq!(
        reopened
            .monitor_capture_usage("monitor", midnight)?
            .reserved_total_bytes,
        1024
    );
    assert_eq!(
        reopened
            .monitor_capture_usage("monitor", midnight)?
            .refusals,
        vec![("total-cap".into(), 1)]
    );
    assert_eq!(
        reopened
            .task_collection("later")?
            .ok_or("collection")?
            .captures[0]
            .state,
        "waiting"
    );
    Ok(())
}

#[test]
fn cancellation_before_capture_and_elapsed_windows_never_dispatch_or_reset_the_grant() -> TestResult
{
    for cancel in [true, false] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = setup(&path)?;
        let original = store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
        if cancel {
            store.connection.execute_batch("CREATE TRIGGER fixture BEFORE INSERT ON task_collection_cancellations BEGIN SELECT RAISE(ABORT, 'fault'); END")?;
            assert!(
                store
                    .cancel_task_collection("task", "cancel", 1, START)
                    .is_err()
            );
            assert_eq!(
                store
                    .task_collection("task")?
                    .ok_or("collection")?
                    .generation,
                1
            );
            store.connection.execute_batch("DROP TRIGGER fixture")?;
            store.cancel_task_collection("task", "cancel", 1, START)?;
            assert!(
                store
                    .reconcile_schedules_at(START, true)?
                    .launches
                    .is_empty()
            );
        }
        assert!(
            store
                .reconcile_schedules_at(START + 90_000, true)?
                .launches
                .is_empty()
        );
        assert_no_admission(&store)?;
        let view = store.task_collection("task")?.ok_or("collection")?;
        assert!(
            view.captures
                .iter()
                .all(|capture| capture.state == "missed")
        );
        assert_eq!(view.grant_sha256, original.grant_sha256);
        assert!(
            store
                .start_task_collection("task", "new", &spec(), 0, START + 90_000)
                .is_err()
        );
        drop(store);
        let mut reopened = Store::open(&path)?;
        assert_eq!(
            reopened.start_task_collection("task", "collect", &spec(), 0, 0)?,
            view
        );
    }
    Ok(())
}

#[test]
fn runtime_historical_scope_chronology_corruption_cannot_reserve_capture() -> TestResult {
    for change in ["action", "policy"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = setup(&path)?;
        let id = if change == "action" {
            store.propose_monitor_action(
                "monitor",
                "refused",
                ActionOrigin::User,
                &Proposal::Other {
                    request: "increase limits".into(),
                },
                START - 1900,
            )?;
            let mut scope = store.task("task")?.spec;
            scope.monitor_actions = 1;
            store.create_task("current", &scope, START - 1800)?;
            "current"
        } else {
            "task"
        };
        store.start_task_collection(id, "collect", &spec(), 0, START - 1000)?;
        let sql = if change == "action" {
            "DROP TRIGGER monitor_action_no_update; UPDATE monitor_actions SET created_ms = created_ms + 10000"
        } else {
            "DROP TRIGGER monitor_version_no_update; UPDATE monitor_versions SET created_ms = created_ms + 10000"
        };
        store.connection.execute_batch(sql)?;
        assert!(store.reconcile_schedules_at(START, true).is_err());
        assert_no_admission(&store)?;
        assert!(store.task_collection(id).is_err());
        drop(store);
        assert!(Store::open(&path).is_err());
    }
    Ok(())
}

#[test]
fn removed_grant_cannot_reset_lifetime_authority_while_owned_rules_remain() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"))?;
    store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    store.connection.execute_batch("DROP TRIGGER task_collection_rules_no_delete; DROP TRIGGER task_collections_no_delete; DELETE FROM task_collection_rules; DELETE FROM task_collections")?;
    assert!(store.task_collection("task").is_err());
    assert!(
        store
            .start_task_collection("task", "new", &spec(), 0, START)
            .is_err()
    );
    assert_eq!(count(&store, "schedule_rules")?, 2);
    assert!(store.reconcile_schedules_at(START, true).is_err());
    assert_no_admission(&store)?;
    Ok(())
}

#[test]
fn collection_history_and_owned_schedule_guards_reject_updates_and_deletes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"))?;
    store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    store.reconcile_schedules_at(START, true)?;
    store.cancel_task_collection("task", "cancel", 1, START)?;
    for table in [
        "task_collections",
        "task_collection_rules",
        "task_collection_admissions",
        "task_collection_cancellations",
        "schedule_rules",
    ] {
        assert!(
            store
                .connection
                .execute(&format!("UPDATE {table} SET rowid = rowid"), [])
                .is_err(),
            "{table}"
        );
        assert!(
            store
                .connection
                .execute(&format!("DELETE FROM {table}"), [])
                .is_err(),
            "{table}"
        );
    }
    store.audit_task_collections()?;
    Ok(())
}

#[test]
fn excessive_stored_rules_fail_before_schedule_admission() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"))?;
    let view = store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    store.connection.execute("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 255) INSERT INTO schedule_rules(id, source_revision, zone, recurrence, civil_date, weekday, hour, minute, second, duration_seconds, maximum_bytes, revision, created_ms, updated_ms) SELECT 'extra:' || n.i, r.source_revision, r.zone, r.recurrence, r.civil_date, r.weekday, r.hour, r.minute, r.second, r.duration_seconds, r.maximum_bytes, 0, r.created_ms, r.updated_ms FROM n JOIN schedule_rules r ON r.id = ?1", [&view.captures[0].rule_id])?;
    assert_eq!(count(&store, "schedule_rules")?, 257);
    assert!(store.reconcile_schedules_at(START, true).is_err());
    assert_no_admission(&store)?;
    Ok(())
}

fn midnight_policy(daily_seconds: u32) -> MonitorSpec {
    let mut bounded = policy();
    bounded.capture = Some(MonitorCaptureBounds {
        daily_seconds,
        total_seconds: 60,
        total_bytes: 1024,
    });
    bounded
}
