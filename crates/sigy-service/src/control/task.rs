//! Task observations and explicit publication or collection grants share the catalog actor.

use serde::{Deserialize, Serialize};

use super::{Snapshot, snapshot};
use crate::task::collection::{TaskCollectionSpec, TaskCollectionView};
use crate::task::evidence::TaskEvidenceView;
use crate::task::processing::{TaskProcessingSpec, TaskProcessingView};
use crate::task::run::{TaskEvidenceBriefing, TaskRunSpec, TaskRunView, TaskSnapshotRunSpec};
use crate::task::snapshot::TaskEvidenceSnapshot;
use crate::task::withdrawal::TaskWithdrawalView;
use crate::{
    Result,
    storage::Store,
    task::{TaskCheckpoint, TaskSpec, TaskView},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskOperation {
    Create {
        id: String,
        spec: Box<TaskSpec>,
    },
    Show {
        id: String,
    },
    List {
        after: Option<String>,
        limit: u32,
    },
    Checkpoint {
        id: String,
        request_id: String,
        expected_checkpoint: u32,
    },
    ShowCheckpoint {
        id: String,
        ordinal: u32,
    },
    /// Freeze exact task-owned evidence without granting publication or processing.
    FreezeEvidence {
        id: String,
        request_id: String,
        expected_snapshot: u32,
    },
    ShowEvidenceSnapshot {
        id: String,
        ordinal: u32,
    },
    /// Delegate one bounded frozen-checkpoint publication workflow.
    Execute {
        id: String,
        request_id: String,
        spec: Box<TaskRunSpec>,
        expected_generation: u32,
    },
    /// Publish from one exact frozen task evidence snapshot.
    Publish {
        id: String,
        request_id: String,
        spec: Box<TaskSnapshotRunSpec>,
        expected_generation: u32,
    },
    EvidenceBriefing {
        id: String,
    },
    Execution {
        id: String,
    },
    Cancel {
        id: String,
        request_id: String,
        expected_generation: u32,
    },
    /// Grant one finite collection using new task-owned once schedules.
    Collect {
        id: String,
        request_id: String,
        spec: Box<TaskCollectionSpec>,
        expected_generation: u32,
    },
    Collection {
        id: String,
    },
    CancelCollection {
        id: String,
        request_id: String,
        expected_generation: u32,
    },
    /// Grant finite local processing of this task's exact collected recordings.
    Process {
        id: String,
        request_id: String,
        spec: Box<TaskProcessingSpec>,
        expected_generation: u32,
    },
    Processing {
        id: String,
    },
    CancelProcessing {
        id: String,
        request_id: String,
        expected_generation: u32,
    },
    WithdrawProcessing {
        id: String,
        request_id: String,
        expected_processing_generation: u32,
        expected_withdrawal_generation: u32,
    },
    Withdrawal {
        id: String,
    },
    /// Reconcile collection through task processing to literal evidence. Read-only.
    Evidence {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskPage {
    Task {
        task: Box<TaskView>,
        created: Option<bool>,
    },
    List {
        ids: Vec<String>,
        next_after: Option<String>,
    },
    Checkpoint {
        checkpoint: Box<TaskCheckpoint>,
    },
    EvidenceSnapshot {
        snapshot: Box<TaskEvidenceSnapshot>,
    },
    EvidenceBriefing {
        briefing: Option<Box<TaskEvidenceBriefing>>,
    },
    Execution {
        run: Option<Box<TaskRunView>>,
    },
    Collection {
        collection: Option<Box<TaskCollectionView>>,
    },
    Processing {
        processing: Option<Box<TaskProcessingView>>,
    },
    Evidence {
        evidence: Option<Box<TaskEvidenceView>>,
    },
    Withdrawal {
        withdrawal: Option<Box<TaskWithdrawalView>>,
    },
}

pub(super) fn apply(store: &mut Store, command: TaskOperation) -> Result<Snapshot> {
    let now = crate::storage::now_ms()?;
    let page = match command {
        TaskOperation::Create { id, spec } => {
            let created = store.create_task(&id, &spec, now)?;
            TaskPage::Task {
                task: Box::new(store.task(&id)?),
                created: Some(created),
            }
        }
        TaskOperation::Show { id } => TaskPage::Task {
            task: Box::new(store.task(&id)?),
            created: None,
        },
        TaskOperation::List { after, limit } => {
            let (ids, next_after) = store.task_ids(after.as_deref(), limit)?;
            TaskPage::List { ids, next_after }
        }
        TaskOperation::Checkpoint {
            id,
            request_id,
            expected_checkpoint,
        } => TaskPage::Checkpoint {
            checkpoint: Box::new(store.checkpoint_task(
                &id,
                &request_id,
                expected_checkpoint,
                now,
            )?),
        },
        TaskOperation::ShowCheckpoint { id, ordinal } => TaskPage::Checkpoint {
            checkpoint: Box::new(store.task_checkpoint(&id, ordinal)?),
        },
        TaskOperation::Evidence { id } => TaskPage::Evidence {
            evidence: store.task_evidence(&id)?.map(Box::new),
        },
        TaskOperation::FreezeEvidence {
            id,
            request_id,
            expected_snapshot,
        } => TaskPage::EvidenceSnapshot {
            snapshot: Box::new(store.freeze_task_evidence(
                &id,
                &request_id,
                expected_snapshot,
                now,
            )?),
        },
        TaskOperation::ShowEvidenceSnapshot { id, ordinal } => TaskPage::EvidenceSnapshot {
            snapshot: Box::new(store.task_evidence_snapshot(&id, ordinal)?),
        },
        TaskOperation::EvidenceBriefing { id } => TaskPage::EvidenceBriefing {
            briefing: store.task_evidence_briefing(&id)?.map(Box::new),
        },
        TaskOperation::Withdrawal { id } => TaskPage::Withdrawal {
            withdrawal: store.task_withdrawal(&id)?.map(Box::new),
        },
        TaskOperation::WithdrawProcessing {
            id,
            request_id,
            expected_processing_generation,
            expected_withdrawal_generation,
        } => TaskPage::Withdrawal {
            withdrawal: Some(Box::new(store.withdraw_task_processing(
                &id,
                &request_id,
                expected_processing_generation,
                expected_withdrawal_generation,
                now,
            )?)),
        },
        grant => grant_page(store, grant, now)?,
    };
    let mut view = snapshot(store)?;
    view.task = Some(Box::new(page));
    Ok(view)
}

fn publication_page(store: &mut Store, command: TaskOperation, now: i64) -> Result<TaskPage> {
    Ok(match command {
        TaskOperation::Execute {
            id,
            request_id,
            spec,
            expected_generation,
        } => {
            store.start_task_run(&id, &request_id, &spec, expected_generation, now)?;
            TaskPage::Execution {
                run: store.task_run(&id)?.map(Box::new),
            }
        }
        TaskOperation::Publish {
            id,
            request_id,
            spec,
            expected_generation,
        } => TaskPage::Execution {
            run: Some(Box::new(store.start_task_snapshot_run(
                &id,
                &request_id,
                &spec,
                expected_generation,
                now,
            )?)),
        },
        TaskOperation::Execution { id } => {
            store.task(&id)?;
            TaskPage::Execution {
                run: store.task_run(&id)?.map(Box::new),
            }
        }
        TaskOperation::Cancel {
            id,
            request_id,
            expected_generation,
        } => {
            store.cancel_task_run(&id, &request_id, expected_generation, now)?;
            TaskPage::Execution {
                run: store.task_run(&id)?.map(Box::new),
            }
        }
        _ => return Err(crate::Error::InvalidInput("task publication operation")),
    })
}

/// Explicit finite delegations: publication, collection and processing grants.
fn grant_page(store: &mut Store, command: TaskOperation, now: i64) -> Result<TaskPage> {
    Ok(match command {
        publication @ (TaskOperation::Execute { .. }
        | TaskOperation::Publish { .. }
        | TaskOperation::Execution { .. }
        | TaskOperation::Cancel { .. }) => publication_page(store, publication, now)?,
        TaskOperation::Collect {
            id,
            request_id,
            spec,
            expected_generation,
        } => {
            store.start_task_collection(&id, &request_id, &spec, expected_generation, now)?;
            TaskPage::Collection {
                collection: store.task_collection(&id)?.map(Box::new),
            }
        }
        TaskOperation::Collection { id } => {
            store.task(&id)?;
            TaskPage::Collection {
                collection: store.task_collection(&id)?.map(Box::new),
            }
        }
        TaskOperation::CancelCollection {
            id,
            request_id,
            expected_generation,
        } => {
            store.cancel_task_collection(&id, &request_id, expected_generation, now)?;
            TaskPage::Collection {
                collection: store.task_collection(&id)?.map(Box::new),
            }
        }
        TaskOperation::Process {
            id,
            request_id,
            spec,
            expected_generation,
        } => TaskPage::Processing {
            processing: Some(Box::new(store.start_task_processing(
                &id,
                &request_id,
                &spec,
                expected_generation,
                now,
            )?)),
        },
        TaskOperation::Processing { id } => {
            store.task(&id)?;
            TaskPage::Processing {
                processing: store.task_processing(&id)?.map(Box::new),
            }
        }
        TaskOperation::CancelProcessing {
            id,
            request_id,
            expected_generation,
        } => TaskPage::Processing {
            processing: Some(Box::new(store.cancel_task_processing(
                &id,
                &request_id,
                expected_generation,
                now,
            )?)),
        },
        // Scope and observation requests are answered by `apply` and grant nothing here.
        TaskOperation::Create { .. }
        | TaskOperation::Show { .. }
        | TaskOperation::List { .. }
        | TaskOperation::Checkpoint { .. }
        | TaskOperation::ShowCheckpoint { .. }
        | TaskOperation::FreezeEvidence { .. }
        | TaskOperation::ShowEvidenceSnapshot { .. }
        | TaskOperation::EvidenceBriefing { .. }
        | TaskOperation::Evidence { .. }
        | TaskOperation::Withdrawal { .. }
        | TaskOperation::WithdrawProcessing { .. } => {
            return Err(crate::Error::InvalidInput("task operation"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control::{Operation, Request, TaskOperation},
        monitor::{MonitorSpec, MonitorTerm},
        sources::{HttpSource, NetworkScope},
    };

    fn fixture(store: &mut Store) -> Result<TaskSpec> {
        store.register_source(
            "source",
            &HttpSource::new(
                "Station",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
        store.create_monitor(
            "monitor",
            &MonitorSpec {
                name: "Water".into(),
                goal: "Observe water reports".into(),
                terms: vec![MonitorTerm {
                    language: "und".into(),
                    text: "water".into(),
                }],
                sources: vec!["source".into()],
                candidate_sources: vec![],
                schedules: vec![],
                daily_audio_seconds: 60,
                total_audio_seconds: 60,
                recognition_profile: None,
                translation_profile: None,
                capture: Some(crate::monitor::MonitorCaptureBounds {
                    daily_seconds: 60,
                    total_seconds: 60,
                    total_bytes: 2048,
                }),
            },
            1,
        )?;
        Ok(TaskSpec {
            goal: "Observe water reports".into(),
            monitor_id: "monitor".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: 0,
            to_ms: 1_000,
        })
    }

    #[test]
    fn bounded_operations_observe_without_dispatching_or_changing_allowances() -> Result<()> {
        let root = tempfile::tempdir()?;
        let mut store = Store::open(&root.path().join("catalog.sqlite"))?;
        let spec = fixture(&mut store)?;
        let before = snapshot(&store)?;
        let create = apply(
            &mut store,
            TaskOperation::Create {
                id: "task".into(),
                spec: Box::new(spec.clone()),
            },
        )?;
        assert!(matches!(
            create.task.as_deref(),
            Some(TaskPage::Task {
                created: Some(true),
                ..
            })
        ));
        let replay = apply(
            &mut store,
            TaskOperation::Create {
                id: "task".into(),
                spec: Box::new(spec),
            },
        )?;
        assert!(matches!(
            replay.task.as_deref(),
            Some(TaskPage::Task {
                created: Some(false),
                ..
            })
        ));
        let first = apply(
            &mut store,
            TaskOperation::Checkpoint {
                id: "task".into(),
                request_id: "check".into(),
                expected_checkpoint: 0,
            },
        )?;
        let shown = apply(
            &mut store,
            TaskOperation::ShowCheckpoint {
                id: "task".into(),
                ordinal: 1,
            },
        )?;
        assert_eq!(first.task, shown.task);
        let list = apply(
            &mut store,
            TaskOperation::List {
                after: None,
                limit: 1,
            },
        )?;
        assert_eq!(
            list.task.as_deref(),
            Some(&TaskPage::List {
                ids: vec!["task".into()],
                next_after: None
            })
        );
        let show = apply(&mut store, TaskOperation::Show { id: "task".into() })?;
        assert!(
            matches!(show.task.as_deref(), Some(TaskPage::Task { created: None, task }) if task.checkpoint == 1)
        );
        assert_eq!(
            serde_json::to_value(before.budgets)?,
            serde_json::to_value(show.budgets)?
        );
        assert_eq!(show.captures.active, 0);
        assert_eq!(show.captures.scheduled, 0);
        assert!(!show.provider_dispatch_available);
        Ok(())
    }

    #[test]
    fn request_schema_and_diagnostics_do_not_accept_extra_authority_or_print_goals() -> Result<()> {
        let request = Request::new(Operation::Task {
            command: TaskOperation::Create {
                id: "task".into(),
                spec: Box::new(TaskSpec {
                    goal: "private-canary-goal".into(),
                    monitor_id: "monitor".into(),
                    monitor_version: 1,
                    monitor_actions: 0,
                    from_ms: 0,
                    to_ms: 1,
                }),
            },
        });
        assert!(!format!("{request:?}").contains("private-canary"));
        let mut value = serde_json::to_value(&request)?;
        value["operation"]["command"]["authority"] = serde_json::json!("shell");
        assert!(serde_json::from_value::<Request>(value).is_err());
        let encoded = serde_json::to_vec(&request)?;
        assert!(encoded.len() < crate::control::MAX_REQUEST_BYTES);
        let decoded: Request = serde_json::from_slice(&encoded)?;
        assert_eq!(decoded.version, crate::control::PROTOCOL_VERSION);
        Ok(())
    }

    #[test]
    fn explicit_execution_requires_checked_delegation_and_durable_generation() -> Result<()> {
        let root = tempfile::tempdir()?;
        let mut store = Store::open(&root.path().join("catalog.sqlite"))?;
        let spec = fixture(&mut store)?;
        apply(
            &mut store,
            TaskOperation::Create {
                id: "task".into(),
                spec: Box::new(spec),
            },
        )?;
        let execution = |expected_generation| TaskOperation::Execute {
            id: "task".into(),
            request_id: "delegate".into(),
            spec: Box::new(TaskRunSpec {
                checkpoint_ordinal: 1,
                maximum_findings: 4,
            }),
            expected_generation,
        };
        assert!(apply(&mut store, execution(0)).is_err());
        assert!(store.task_run("task")?.is_none());
        apply(
            &mut store,
            TaskOperation::Checkpoint {
                id: "task".into(),
                request_id: "observed".into(),
                expected_checkpoint: 0,
            },
        )?;
        assert!(apply(&mut store, execution(1)).is_err());
        let admitted = apply(&mut store, execution(0))?;
        let replay = apply(&mut store, execution(0))?;
        assert_eq!(admitted.task, replay.task);
        assert!(
            matches!(admitted.task.as_deref(), Some(TaskPage::Execution { run: Some(run) }) if run.generation == 1 && run.state == crate::task::run::TaskRunState::Running)
        );
        let cancelled = apply(
            &mut store,
            TaskOperation::Cancel {
                id: "task".into(),
                request_id: "stop".into(),
                expected_generation: 1,
            },
        )?;
        assert!(
            matches!(cancelled.task.as_deref(), Some(TaskPage::Execution { run: Some(run) }) if run.generation == 2 && run.state == crate::task::run::TaskRunState::Cancelled)
        );
        let read = apply(&mut store, TaskOperation::Execution { id: "task".into() })?;
        assert_eq!(cancelled.task, read.task);
        assert_eq!(cancelled.captures.active, 0);
        assert!(!cancelled.provider_dispatch_available);
        let mut value = serde_json::to_value(Request::new(Operation::Task {
            command: execution(0),
        }))?;
        value["operation"]["command"]["spec"]["paid_allowance"] = serde_json::json!("20");
        assert!(serde_json::from_value::<Request>(value).is_err());
        Ok(())
    }

    #[test]
    fn collection_control_replays_finite_grant_and_cancellation_without_dispatch() -> Result<()> {
        use crate::task::collection::TaskCaptureSpec;
        let root = tempfile::tempdir()?;
        let mut store = Store::open(&root.path().join("catalog.sqlite"))?;
        let now = crate::storage::now_ms()?;
        let base = fixture(&mut store)?;
        let spec = TaskSpec {
            from_ms: now - 1000,
            to_ms: now + 60_000,
            ..base
        };
        apply(
            &mut store,
            TaskOperation::Create {
                id: "task".into(),
                spec: Box::new(spec),
            },
        )?;
        let before = apply(&mut store, TaskOperation::Collection { id: "task".into() })?;
        assert_eq!(
            before.task.as_deref(),
            Some(&TaskPage::Collection { collection: None })
        );
        let request = TaskOperation::Collect {
            id: "task".into(),
            request_id: "grant".into(),
            spec: Box::new(TaskCollectionSpec {
                captures: vec![TaskCaptureSpec {
                    source_revision: "source".into(),
                    start_ms: (now / 1000 + 5) * 1000,
                    duration_seconds: 10,
                    maximum_bytes: 1024,
                }],
            }),
            expected_generation: 0,
        };
        let admitted = apply(&mut store, request.clone())?;
        let replay = apply(&mut store, request.clone())?;
        assert_eq!(admitted.task, replay.task);
        assert!(
            matches!(admitted.task.as_deref(), Some(TaskPage::Collection { collection: Some(view) }) if view.generation == 1 && view.captures.len() == 1 && view.captures[0].recording_id.is_none())
        );
        let cancel = TaskOperation::CancelCollection {
            id: "task".into(),
            request_id: "stop".into(),
            expected_generation: 1,
        };
        let cancelled = apply(&mut store, cancel.clone())?;
        assert_eq!(cancelled.task, apply(&mut store, cancel)?.task);
        assert!(
            matches!(cancelled.task.as_deref(), Some(TaskPage::Collection { collection: Some(view) }) if view.generation == 2 && view.cancelled && view.hold_reason.as_deref() == Some("cancelled"))
        );
        assert_eq!(cancelled.task, apply(&mut store, request.clone())?.task);
        assert_eq!(cancelled.captures.active, before.captures.active);
        assert_eq!(
            serde_json::to_value(cancelled.budgets)?,
            serde_json::to_value(before.budgets)?
        );
        assert!(store.task_run("task")?.is_none());
        let encoded = serde_json::to_value(Request::new(Operation::Task { command: request }))?;
        let mut hostile = encoded.clone();
        hostile["operation"]["command"]["spec"]["shell"] = serde_json::json!("echo");
        assert!(serde_json::from_value::<Request>(hostile).is_err());
        let mut hostile = encoded;
        hostile["operation"]["command"]["spec"]["paid_allowance"] = serde_json::json!("20");
        assert!(serde_json::from_value::<Request>(hostile).is_err());
        Ok(())
    }

    fn profile() -> Result<crate::recognition::RecognitionProfile> {
        let root = std::env::temp_dir();
        let mut profile = crate::recognition::RecognitionProfile {
            id: "asr".into(),
            engine: crate::recognition::WHISPER_CPP_CLI.into(),
            runtime_dir: root.join("sigy-absent-runtime").display().to_string(),
            executable: "whisper-cli.exe".into(),
            runtime_sha256: "1".repeat(64),
            runtime_files: 3,
            runtime_bytes: 300,
            model_path: root.join("sigy-absent-model.bin").display().to_string(),
            model_sha256: "2".repeat(64),
            model_bytes: 1000,
            vad_path: root.join("sigy-absent-vad.bin").display().to_string(),
            vad_sha256: "3".repeat(64),
            vad_bytes: 10,
            threads: 2,
            memory_bytes: 1 << 30,
            deadline_ms: 60_000,
            profile_sha256: String::new(),
        };
        profile.profile_sha256 = profile.identity()?;
        Ok(profile)
    }

    #[test]
    fn processing_control_replays_finite_grant_and_cancellation_without_dispatch() -> Result<()> {
        use crate::task::{collection::TaskCaptureSpec, processing::TaskProcessingSpec};
        let root = tempfile::tempdir()?;
        let mut store = Store::open(&root.path().join("catalog.sqlite"))?;
        let now = crate::storage::now_ms()?;
        let base = fixture(&mut store)?;
        store.add_recognition_profile(&profile()?, now - 3000)?;
        store.create_task(
            "task",
            &TaskSpec {
                from_ms: now - 1000,
                to_ms: now + 60_000,
                ..base
            },
            now - 2000,
        )?;
        let request = TaskOperation::Process {
            id: "task".into(),
            request_id: "process".into(),
            spec: Box::new(TaskProcessingSpec {
                recognition_profile: "asr".into(),
                translation_profile: None,
                maximum_audio_seconds: 30,
            }),
            expected_generation: 0,
        };
        assert!(apply(&mut store, request.clone()).is_err());
        store.start_task_collection(
            "task",
            "collect",
            &TaskCollectionSpec {
                captures: vec![TaskCaptureSpec {
                    source_revision: "source".into(),
                    start_ms: (now / 1000 + 5) * 1000,
                    duration_seconds: 10,
                    maximum_bytes: 1024,
                }],
            },
            0,
            now - 1000,
        )?;
        let before = apply(&mut store, TaskOperation::Processing { id: "task".into() })?;
        assert_eq!(
            before.task.as_deref(),
            Some(&TaskPage::Processing { processing: None })
        );
        let admitted = apply(&mut store, request.clone())?;
        assert_eq!(admitted.task, apply(&mut store, request.clone())?.task);
        assert!(
            matches!(admitted.task.as_deref(), Some(TaskPage::Processing { processing: Some(view) }) if view.generation == 1 && view.steps.is_empty() && view.paid_allowance_usd == "0.000000")
        );
        let cancel = TaskOperation::CancelProcessing {
            id: "task".into(),
            request_id: "stop".into(),
            expected_generation: 1,
        };
        let cancelled = apply(&mut store, cancel.clone())?;
        assert_eq!(cancelled.task, apply(&mut store, cancel)?.task);
        assert!(
            matches!(cancelled.task.as_deref(), Some(TaskPage::Processing { processing: Some(view) }) if view.generation == 2 && view.cancelled)
        );
        assert_eq!(
            cancelled.task,
            apply(&mut store, TaskOperation::Processing { id: "task".into() })?.task
        );
        assert_eq!(
            serde_json::to_value(cancelled.budgets)?,
            serde_json::to_value(before.budgets)?
        );
        assert_eq!(cancelled.captures.active, before.captures.active);
        assert!(matches!(
            grant_page(&mut store, TaskOperation::Show { id: "task".into() }, now),
            Err(crate::Error::InvalidInput("task operation"))
        ));
        let evidence = apply(&mut store, TaskOperation::Evidence { id: "task".into() })?;
        assert!(
            matches!(evidence.task.as_deref(), Some(TaskPage::Evidence { evidence: Some(view) }) if view.outcome == crate::task::evidence::TaskOutcome::Pending && view.citations.is_empty() && view.entries[0].pending)
        );
        assert_eq!(evidence.captures.active, before.captures.active);
        assert!(
            apply(
                &mut store,
                TaskOperation::Evidence {
                    id: "absent".into()
                }
            )
            .is_err()
        );
        let encoded = serde_json::to_value(Request::new(Operation::Task { command: request }))?;
        for field in ["paid_allowance", "shell", "job"] {
            let mut hostile = encoded.clone();
            hostile["operation"]["command"]["spec"][field] = serde_json::json!("20");
            assert!(serde_json::from_value::<Request>(hostile).is_err());
        }
        Ok(())
    }
}
