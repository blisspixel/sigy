//! Worker-map routing fixture; no recognizer or translator is launched.

use super::*;
use crate::storage::job_pool::JobKind;

fn idle_worker() -> (super::super::Worker, watch::Receiver<bool>) {
    let (stop, signal) = watch::channel(false);
    let task = tokio::spawn(async {});
    (super::super::Worker { stop, task }, signal)
}

fn recognition_work(actor: &mut Actor) -> Result<crate::recognition::LocalAsrWork> {
    let profile = recognition_profile()?;
    actor
        .library
        .store_mut()
        .add_recognition_profile(&profile, 1)?;
    let request = crate::recognition::LocalAsrRequest {
        id: "collision".into(),
        analysis_id: "pin".into(),
        analysis_revision: 1,
        profile: profile.id,
        profile_sha256: profile.profile_sha256,
        parent_revision: 0,
    };
    actor.library.store_mut().enqueue_local_asr(&request, 2)?;
    actor
        .library
        .store_mut()
        .claim_local_asr(&request.id, "owner", 3)?
        .ok_or(Error::StorageIntegrity)
}

fn translation_work() -> crate::translation::TranslationWork {
    crate::translation::TranslationWork {
        job: crate::translation::TranslationJob {
            request: crate::translation::TranslationRequest {
                id: "collision".into(),
                transcript_id: "fixture".into(),
                transcript_revision: 1,
                profile: "fixture".into(),
                profile_sha256: "1".repeat(64),
            },
            generation: 1,
            state: "running".into(),
            reason: None,
            amount_usd: "0".into(),
            created_ms: 1,
            finished_ms: None,
            attempt: 1,
            started_ms: Some(2),
        },
        cues: Vec::new(),
        language: None,
    }
}

#[tokio::test]
async fn family_and_generation_select_only_the_exact_colliding_worker() -> TestResult {
    let root = tempfile::tempdir()?;
    let (sender, _receiver) = mpsc::channel(8);
    let mut actor = actor(root.path(), &sender)?;
    let work = recognition_work(&mut actor)?;
    let (worker, verification) = idle_worker();
    actor.pool.verification.insert(
        "collision".into(),
        super::super::VerificationWorker {
            generation: 1,
            worker,
        },
    );
    let (worker, recognition) = idle_worker();
    actor.pool.recognition.insert(
        "collision".into(),
        super::super::RecognitionWorker {
            generation: 1,
            worker,
            work,
        },
    );
    let (worker, translation) = idle_worker();
    actor.pool.translation.insert(
        "collision".into(),
        super::super::TranslationWorker {
            generation: 1,
            worker,
            work: translation_work(),
        },
    );
    for (index, kind) in [
        JobKind::Verification,
        JobKind::Recognition,
        JobKind::Translation,
    ]
    .into_iter()
    .enumerate()
    {
        actor.pool.signal(kind, "collision", 2);
        assert_eq!(
            [
                *verification.borrow(),
                *recognition.borrow(),
                *translation.borrow()
            ],
            [false; 3]
        );
        actor.pool.signal(kind, "missing", 1);
        assert_eq!(
            [
                *verification.borrow(),
                *recognition.borrow(),
                *translation.borrow()
            ],
            [false; 3]
        );
        actor.pool.signal(kind, "collision", 1);
        let mut expected = [false; 3];
        expected[index] = true;
        assert_eq!(
            [
                *verification.borrow(),
                *recognition.borrow(),
                *translation.borrow()
            ],
            expected
        );
        reset(&actor)?;
    }
    Ok(())
}

fn reset(actor: &Actor) -> TestResult {
    actor
        .pool
        .verification
        .get("collision")
        .ok_or("verification")?
        .worker
        .stop
        .send_replace(false);
    actor
        .pool
        .recognition
        .get("collision")
        .ok_or("recognition")?
        .worker
        .stop
        .send_replace(false);
    actor
        .pool
        .translation
        .get("collision")
        .ok_or("translation")?
        .worker
        .stop
        .send_replace(false);
    Ok(())
}
