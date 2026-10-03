//! The service tick admits task-owned processing from durable receipts only. Workers run
//! with absent runtime assets; this is scheduling evidence, not containment or quality.

use super::*;
use crate::{
    monitor::{MonitorCaptureBounds, MonitorSpec, MonitorTerm},
    storage::now_ms,
    task::{
        TaskSpec,
        collection::{TaskCaptureSpec, TaskCollectionSpec},
        processing::TaskProcessingSpec,
    },
};

/// Seconds-aligned history in the recent past, so the live tick clock is never behind it.
fn base() -> Result<i64> {
    Ok((now_ms()? / 1000 - 600) * 1000)
}

fn media(ordinal: usize) -> Vec<u8> {
    format!("task actor fixture {ordinal}").into_bytes()
}

/// Two followed sources, a monitor, a task and its finite collection grant.
fn arrange(store: &mut crate::storage::Store, start: i64, monitor_processing: bool) -> Result<()> {
    store.add_recognition_profile(&recognition_profile()?, start - 5000)?;
    for source in ["a:v1", "b:v1"] {
        store.register_source(
            source,
            &HttpSource::new(
                "Fixture",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
    }
    let spec = MonitorSpec {
        sources: vec!["a:v1".into(), "b:v1".into()],
        terms: vec![MonitorTerm {
            language: "und".into(),
            text: "agua".into(),
        }],
        capture: Some(MonitorCaptureBounds {
            daily_seconds: 60,
            total_seconds: 60,
            total_bytes: 4096,
        }),
        ..monitor_spec(monitor_processing.then_some("asr"))
    };
    store.create_monitor("watch", &spec, start - 3000)?;
    store.create_task(
        "task",
        &TaskSpec {
            goal: "Follow agua".into(),
            monitor_id: "watch".into(),
            monitor_version: 1,
            monitor_actions: 0,
            from_ms: start,
            to_ms: start + 300_000,
        },
        start - 2000,
    )?;
    let captures = (0..2)
        .map(|ordinal| TaskCaptureSpec {
            source_revision: if ordinal == 0 { "a:v1" } else { "b:v1" }.into(),
            start_ms: start + ordinal * 60_000,
            duration_seconds: 1,
            maximum_bytes: 1024,
        })
        .collect();
    store.start_task_collection(
        "task",
        "collect",
        &TaskCollectionSpec { captures },
        0,
        start - 1000,
    )?;
    Ok(())
}

/// Two collected recordings with retained files and an optional later processing grant.
fn collected(actor: &mut Actor, monitor_processing: bool, grant: Option<i64>) -> Result<i64> {
    let start = base()?;
    arrange(actor.library.store_mut(), start, monitor_processing)?;
    for ordinal in 0..2 {
        let at = start + i64::try_from(ordinal).map_err(|_| Error::StorageIntegrity)? * 60_000;
        let mut launches = actor
            .library
            .store_mut()
            .reconcile_schedules_at(at, true)?
            .launches;
        let launch = launches.pop().ok_or(Error::StorageIntegrity)?;
        let bytes = media(ordinal);
        actor.library.store_mut().publish_recording(
            &launch.job.version,
            &Publication {
                bytes: bytes.len() as u64,
                sha256: hex(&Sha256::digest(&bytes)),
                format: "wav",
                decoded_microseconds: 1_000_000,
                end_reason: "end_of_body",
                http_route: vec![HttpHop {
                    origin: "https://example.com".into(),
                    peer: ([8, 8, 8, 8], 443).into(),
                    status: 200,
                }],
                observations: Vec::new(),
                segments_sealed: false,
                gap: None,
            },
        )?;
        let recording = actor.library.store().recording(launch.job.version.id())?;
        std::fs::write(
            crate::recordings::media_path(
                actor.library.directory(),
                &recording.intervals[0].object_key,
            )?,
            bytes,
        )?;
    }
    if let Some(at) = grant {
        actor.library.store_mut().start_task_processing(
            "task",
            "process",
            &TaskProcessingSpec {
                recognition_profile: "asr".into(),
                translation_profile: None,
                maximum_audio_seconds: 10,
            },
            0,
            start + at,
        )?;
    }
    Ok(start)
}

fn steps(actor: &Actor) -> Result<u32> {
    count(actor, "SELECT count(*) FROM task_processing_steps")
}

#[cfg(windows)]
#[tokio::test]
async fn withdrawal_signals_a_running_contained_worker_and_commits_drain_proof() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    collected(&mut actor, false, Some(100_000))?;
    let now = now_ms()?;
    let facts = actor
        .library
        .store()
        .task_processing_facts("task", now)?
        .ok_or("facts")?;
    let entry = facts.captures.first().ok_or("entry")?;
    let request = actor.recognize(&entry.recording_id, "asr", now)?;
    actor.library.store_mut().enqueue_task_recognition(
        &crate::storage::tasks::processing::TaskJobScope {
            task_id: "task",
            ordinal: entry.ordinal,
            recording_id: &entry.recording_id,
            audio_us: 1_000_000,
        },
        &request,
        now,
    )?;
    let work = actor
        .library
        .store_mut()
        .claim_local_asr(&request.id, &actor.pool.owner, now)?
        .ok_or("claim")?;
    let job = work.job.clone();
    let (stop, signal) = watch::channel(false);
    let (started, running) = oneshot::channel();
    let callback = sender.clone();
    let worker = tokio::spawn(async move {
        let result = crate::execution::contained_cancellation_fixture(
            &job.request.id,
            job.generation,
            signal,
            started,
        )
        .await
        .and_then(|envelope| {
            crate::recognition::ReapedLocalAsr::from_envelope(&job, envelope, None)
        });
        let _ = callback
            .send(Message::RecognitionFinished {
                id: job.request.id,
                generation: job.generation,
                result,
            })
            .await;
    });
    actor.pool.recognition.insert(
        request.id.clone(),
        super::super::RecognitionWorker {
            generation: 1,
            worker: crate::control::actor::Worker { stop, task: worker },
            work,
        },
    );
    tokio::time::timeout(Duration::from_secs(5), running).await??;
    let view =
        actor
            .library
            .store_mut()
            .withdraw_task_processing("task", "withdraw", 1, 0, now_ms()?)?;
    assert!(view.completion_unproven);
    assert!(
        actor
            .library
            .store_mut()
            .begin_delete(&entry.recording_id, false)
            .is_err()
    );
    // Exercise the durable tick handoff, rather than relying on delivery of the response.
    actor.schedule()?;
    let message = completion(&mut receiver).await?;
    if let Message::RecognitionFinished {
        result: Err(error), ..
    } = &message
    {
        return Err(format!("contained fixture completion: {error}").into());
    }
    assert_eq!(deliver(&mut actor, message), (false, false));
    assert!(!actor.library.store().native_completion_unproven()?);
    assert_eq!(
        actor.library.store().local_asr_job(&request.id)?.state,
        "cancelled"
    );
    assert_eq!(
        count(&actor, "SELECT count(*) FROM native_stop_completions")?,
        1
    );
    assert_eq!(
        count(&actor, "SELECT count(*) FROM worker_observations")?,
        1
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unproven_restart_preserves_scratch_and_serves_control_without_native_launch() -> TestResult
{
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    collected(&mut actor, false, Some(100_000))?;
    actor.pool.accepting = false;
    actor.reconcile_task_processing()?;
    let processing = actor
        .library
        .store()
        .task_processing("task")?
        .ok_or("processing")?;
    let job = processing.steps[0]
        .job_id
        .as_deref()
        .ok_or("job")?
        .to_owned();
    actor
        .library
        .store_mut()
        .claim_local_asr(&job, "dead-owner", now_ms()?)?
        .ok_or("claim")?;
    actor
        .library
        .store_mut()
        .withdraw_task_processing("task", "withdraw", 1, 0, now_ms()?)?;
    let scratch = root.path().join("analysis-scratch");
    std::fs::create_dir_all(&scratch)?;
    std::fs::write(scratch.join("retained-marker"), b"unproven attempt")?;
    drop(actor);
    let library = Library::open(root.path(), false)?;
    let service = tokio::spawn(crate::control::run(library, std::future::pending()));
    let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(snapshot) = crate::control::request(root.path(), Operation::Doctor {}).await {
                break snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(
        snapshot
            .doctor
            .ok_or("doctor")?
            .checks
            .iter()
            .any(|check| check.name == "native recovery"
                && check.detail.contains("completion unproven"))
    );
    let state = crate::control::request(
        root.path(),
        Operation::Task {
            command: crate::control::TaskOperation::Withdrawal { id: "task".into() },
        },
    )
    .await?;
    let Some(crate::control::TaskPage::Withdrawal {
        withdrawal: Some(view),
    }) = state.task.as_deref()
    else {
        return Err("withdrawal inspection unavailable".into());
    };
    assert!(view.completion_unproven);
    assert!(scratch.join("retained-marker").is_file());
    crate::control::request(root.path(), Operation::Status {}).await?;
    crate::control::request(root.path(), Operation::Stop {}).await?;
    tokio::time::timeout(Duration::from_secs(10), service).await???;
    let reopened = Library::open(root.path(), false)?;
    assert_eq!(reopened.store().local_asr_job(&job)?.state, "cancelling");
    assert_eq!(reopened.store().local_asr_job(&job)?.generation, 1);
    assert!(reopened.store().native_completion_unproven()?);
    assert!(scratch.join("retained-marker").is_file());
    Ok(())
}

#[tokio::test]
async fn the_tick_admits_owned_jobs_once_and_a_repeated_pass_changes_nothing() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    collected(&mut actor, false, Some(100_000))?;
    let budgets = serde_json::to_value(crate::control::snapshot(actor.library.store())?.budgets)?;
    actor.reconcile_tasks()?;
    assert_eq!(steps(&actor)?, 2);
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM job_interests WHERE authority = 'task'"
        )?,
        2
    );
    assert_eq!(actor.pool.recognition.len(), 1);
    let view = actor
        .library
        .store()
        .task_processing("task")?
        .ok_or("processing")?;
    assert_eq!(view.charged_audio_us, 2_000_000);
    actor.reconcile_tasks()?;
    assert_eq!(steps(&actor)?, 2, "the pass interval holds a second pass");
    actor.task_processing.next_ms = 0;
    actor.reconcile_tasks()?;
    assert_eq!(steps(&actor)?, 2);
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM analysis_jobs WHERE kind = 'local_asr'"
        )?,
        2
    );
    assert_eq!(
        serde_json::to_value(crate::control::snapshot(actor.library.store())?.budgets)?,
        budgets
    );
    let message = completion(&mut receiver).await?;
    assert!(matches!(&message, Message::RecognitionFinished { .. }));
    assert_eq!(deliver(&mut actor, message), (false, false));
    Ok(())
}

#[tokio::test]
async fn cancellation_drift_and_a_regressed_clock_hold_only_task_admissions() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    let start = collected(&mut actor, true, Some(100_000))?;
    actor
        .library
        .store_mut()
        .cancel_task_processing("task", "stop", 1, start + 100_001)?;
    actor.reconcile_tasks()?;
    assert_eq!(steps(&actor)?, 0);
    actor.next_monitor_pass_ms = 0;
    actor.reconcile_monitors()?;
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM monitor_steps WHERE decision = 'queued'"
        )?,
        2
    );
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM job_interests WHERE authority = 'monitor'"
        )?,
        2
    );
    let other = tempfile::tempdir()?;
    let mut later = super::actor(other.path(), &sender)?;
    // A grant stamped an hour after the live clock: the tick holds until time catches up.
    let start = collected(&mut later, false, Some(4_200_000))?;
    later.reconcile_tasks()?;
    assert_eq!(
        steps(&later)?,
        0,
        "a receipt after the live clock holds admission"
    );
    let facts = later
        .library
        .store()
        .task_processing_facts("task", now_ms()?)?
        .ok_or("facts")?;
    assert_eq!(facts.hold, Some("clock"));
    let view = later
        .library
        .store()
        .task_processing("task")?
        .ok_or("processing")?;
    assert_eq!(
        (view.created_ms, view.charged_audio_us),
        (start + 4_200_000, 0)
    );
    assert!(later.idle());
    Ok(())
}

#[tokio::test]
async fn a_receipt_fault_rolls_back_its_job_and_a_restart_admits_the_rest_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, mut receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    collected(&mut actor, false, Some(100_000))?;
    let fault = rusqlite::Connection::open(root.path().join("catalog.sqlite3"))?;
    fault.execute_batch("CREATE TRIGGER task_receipt_fault BEFORE INSERT ON task_processing_steps WHEN NEW.ordinal = 1 BEGIN SELECT RAISE(ABORT, 'injected task receipt'); END")?;
    assert!(matches!(actor.reconcile_tasks(), Err(Error::Database(_))));
    assert_eq!(steps(&actor)?, 1);
    assert_eq!(
        count(
            &actor,
            "SELECT count(*) FROM analysis_jobs WHERE kind = 'local_asr'"
        )?,
        1
    );
    assert_eq!(actor.pool.recognition.len(), 1);
    fault.execute_batch("DROP TRIGGER task_receipt_fault")?;
    drop(fault);
    let message = completion(&mut receiver).await?;
    assert!(matches!(&message, Message::RecognitionFinished { .. }));
    deliver(&mut actor, message);
    let library = actor.library;
    drop(library);
    let mut restarted = Actor {
        library: crate::library::Library::open(root.path(), false)?,
        runtime: tokio::runtime::Handle::current(),
        sender: sender.downgrade(),
        workers: HashMap::new(),
        directory_worker: None,
        playlist_worker: None,
        click_worker: None,
        podcast_worker: None,
        text_worker: None,
        pool: super::super::Pool::new("local-restarted".into()),
        listen_workers: HashMap::new(),
        playback: PlaySessions::default(),
        acquirer: HttpAcquirer::default(),
        next_monitor_pass_ms: 0,
        task_cursor: None,
        task_processing: super::super::super::task::ProcessingPass::default(),
    };
    restarted.reconcile_tasks()?;
    // The first job failed without runtime assets, so its translation is refused once.
    let receipts = "SELECT count(*) FROM task_processing_steps WHERE (ordinal = 0 AND stage = 'translation' AND reason = 'recognition-failed') OR (ordinal = 1 AND stage = 'recognition' AND decision = 'queued')";
    assert_eq!(count(&restarted, receipts)?, 2);
    assert_eq!(steps(&restarted)?, 3);
    restarted.task_processing.next_ms = 0;
    restarted.reconcile_tasks()?;
    assert_eq!(steps(&restarted)?, 3);
    assert_eq!(
        count(
            &restarted,
            "SELECT count(*) FROM analysis_jobs WHERE kind = 'local_asr'"
        )?,
        2
    );
    Ok(())
}
