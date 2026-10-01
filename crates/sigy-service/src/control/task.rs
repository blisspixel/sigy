//! Task observations and explicit publication delegation share the canonical catalog actor.

use serde::{Deserialize, Serialize};

use super::{Snapshot, snapshot};
use crate::task::run::{TaskRunSpec, TaskRunView};
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
    /// Delegate one bounded frozen-checkpoint publication workflow.
    Execute {
        id: String,
        request_id: String,
        spec: Box<TaskRunSpec>,
        expected_generation: u32,
    },
    Execution {
        id: String,
    },
    Cancel {
        id: String,
        request_id: String,
        expected_generation: u32,
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
    Execution {
        run: Option<Box<TaskRunView>>,
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
    };
    let mut view = snapshot(store)?;
    view.task = Some(Box::new(page));
    Ok(view)
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
                capture: None,
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
}
