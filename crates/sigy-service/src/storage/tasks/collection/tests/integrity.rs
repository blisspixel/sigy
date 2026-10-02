use super::*;

#[test]
fn domain_and_scope_bounds_refuse_authority_expansion_before_rules() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = setup(&directory.path().join("catalog"))?;
    for plan in invalid_plans() {
        assert!(
            store
                .start_task_collection("task", "collect", &plan, 0, START - 1000)
                .is_err()
        );
        assert_eq!(count(&store, "task_collections")?, 0);
        assert_eq!(count(&store, "schedule_rules")?, 0);
    }
    assert!(
        store
            .start_task_collection("task", "collect", &spec(), 0, START + 100_000)
            .is_err()
    );
    assert!(
        store
            .start_task_collection("task", "collect", &spec(), 0, START - 2001)
            .is_err()
    );
    assert!(
        store
            .start_task_collection("task", "collect", &spec(), 1, START - 1000)
            .is_err()
    );
    let mut json = serde_json::to_value(spec())?;
    if let serde_json::Value::Object(ref mut fields) = json {
        fields.insert("shell".into(), serde_json::json!("execute"));
    }
    assert!(serde_json::from_value::<TaskCollectionSpec>(json).is_err());
    assert!(matches!(
        store.task_collection("absent"),
        Err(Error::NotFound)
    ));
    assert!(
        store
            .cancel_task_collection("absent", "cancel", 1, START)
            .is_err()
    );
    Ok(())
}

#[test]
fn task_rules_cannot_be_revised_or_lose_binding_into_independent_authority() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    let view = store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    let mut replacement = draft(&view.grant_sha256, 0, &spec().captures[0])?;
    replacement.maximum_bytes *= 2;
    assert!(store.revise_schedule_at(&replacement, START).is_err());
    assert!(
        store
            .connection
            .execute(
                "UPDATE schedule_rules SET task_owned = 0 WHERE id = ?1",
                [&replacement.id]
            )
            .is_err()
    );
    store.connection.execute_batch(
        "DROP TRIGGER task_collection_rules_no_delete; DELETE FROM task_collection_rules",
    )?;
    assert!(store.reconcile_schedules_at(START, true).is_err());
    assert_no_admission(&store)?;
    assert!(store.revise_schedule_at(&replacement, START).is_err());
    assert!(store.audit_task_collections().is_err());
    drop(store);
    assert!(Store::open(&path).is_err());
    Ok(())
}

#[test]
fn altered_task_or_waiting_occurrence_cannot_admit_before_reopen() -> TestResult {
    for corruption in [
        "DROP TRIGGER task_no_update; UPDATE tasks SET spec_json = json_set(spec_json, '$.monitor_actions', 1)",
        "UPDATE schedule_occurrences SET maximum_bytes = 2048",
        "UPDATE schedule_occurrences SET id = id || ':other'",
        "DROP TRIGGER task_collections_no_update; UPDATE task_collections SET grant_sha256 = replace(grant_sha256, substr(grant_sha256, 1, 1), 'z')",
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("catalog");
        let mut store = setup(&path)?;
        store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
        store.propose_monitor_action(
            "monitor",
            "refused",
            ActionOrigin::User,
            &Proposal::Other {
                request: "increase budget".into(),
            },
            START - 500,
        )?;
        if !corruption.is_empty() {
            store
                .connection
                .pragma_update(None, "ignore_check_constraints", true)?;
        }
        store.connection.execute_batch(corruption)?;
        assert!(store.reconcile_schedules_at(START, true).is_err());
        assert_no_admission(&store)?;
        assert!(store.task_collection("task").is_err());
        drop(store);
        assert!(Store::open(&path).is_err());
    }
    Ok(())
}

#[test]
fn cancellation_freezes_exact_admission_prefix_even_at_the_same_millisecond() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    store.start_task_collection("task", "collect", &spec(), 0, START - 1000)?;
    store.reconcile_schedules_at(START, true)?;
    let cancelled = store.cancel_task_collection("task", "cancel", 1, START)?;
    assert_eq!(cancelled.updated_ms, START);
    assert_eq!(
        store.connection.query_row(
            "SELECT admitted_mask FROM task_collection_cancellations",
            [],
            |row| row.get::<_, u32>(0)
        )?,
        1
    );
    assert!(store.connection.execute("INSERT INTO task_collection_admissions(task_id, ordinal, occurrence_id, generation, admitted_ms) SELECT task_id, 1, occurrence_id, 1, admitted_ms FROM task_collection_admissions", []).is_err());
    store.connection.execute_batch("DROP TRIGGER task_collection_cancellations_no_update; UPDATE task_collection_cancellations SET admitted_mask = 0")?;
    assert!(store.task_collection("task").is_err());
    drop(store);
    assert!(Store::open(&path).is_err());
    Ok(())
}

#[test]
fn v42_migration_preserves_independent_matching_names_and_rollback_has_no_partial_ddl() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog");
    let mut store = setup(&path)?;
    let independent = draft(&"a".repeat(64), 0, &spec().captures[0])?;
    store.create_schedule_at(&independent, START - 1000)?;
    let before = store.task("task")?;
    remove_collection_schema(&store)?;
    store.connection.pragma_update(None, "user_version", 42)?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert_eq!(reopened.task("task")?, before);
    assert!(
        !reopened
            .schedule(&independent.id)?
            .ok_or("rule")?
            .task_owned
    );
    assert_eq!(
        reopened.reconcile_schedules_at(START, true)?.launches.len(),
        1
    );
    assert_eq!(count(&reopened, "task_collections")?, 0);
    let conflict_path = directory.path().join("conflict");
    let conflict = setup(&conflict_path)?;
    remove_collection_schema(&conflict)?;
    conflict.connection.execute_batch(
        "CREATE TABLE task_collection_rules(conflict INTEGER); PRAGMA user_version = 42",
    )?;
    drop(conflict);
    assert!(Store::open(&conflict_path).is_err());
    let raw = Connection::open(&conflict_path)?;
    assert_eq!(
        raw.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        42
    );
    assert_eq!(
        raw.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name = 'task_collections'",
            [],
            |row| row.get::<_, u32>(0)
        )?,
        0
    );
    assert_eq!(
        raw.query_row(
            "SELECT count(*) FROM pragma_table_info('schedule_rules') WHERE name = 'task_owned'",
            [],
            |row| row.get::<_, u32>(0)
        )?,
        0
    );
    Ok(())
}

fn invalid_plans() -> Vec<TaskCollectionSpec> {
    let mut plans = vec![TaskCollectionSpec { captures: vec![] }];
    for invalid in [
        TaskCaptureSpec {
            source_revision: "../other".into(),
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            source_revision: "c:v1".into(),
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            start_ms: START + 1,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            start_ms: -1000,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            start_ms: i64::MAX - 807,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            start_ms: START - 1000,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            duration_seconds: 0,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            duration_seconds: 901,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            maximum_bytes: 0,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            maximum_bytes: 268_435_457,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            maximum_bytes: 4097,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            duration_seconds: 121,
            ..spec().captures[0].clone()
        },
        TaskCaptureSpec {
            start_ms: START + 899_000,
            ..spec().captures[0].clone()
        },
    ] {
        plans.push(TaskCollectionSpec {
            captures: vec![invalid],
        });
    }
    plans.push(TaskCollectionSpec {
        captures: vec![spec().captures[0].clone(); 2],
    });
    plans.push(TaskCollectionSpec {
        captures: vec![spec().captures[0].clone(); 3],
    });
    plans
}
