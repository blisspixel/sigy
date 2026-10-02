//! Sequential, reservation-first dispatch of one frozen manifest.
//!
//! For each request: refuse replay, admit against the batch allocation,
//! software cap and ceiling, verify the frozen body, append the reservation,
//! send once, store the raw response, then append exactly one outcome. A cost
//! or token count above its bound records a breach that freezes every batch.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use scorer_probe::selection::sha256;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{
    Result,
    client::{Sent, Transport},
    files,
    ledger::{Event, Ledger},
    outcome::{self, Interpreted, Settlement},
    plan::{Manifest, Planned},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub index: u32,
    pub item_id: String,
    pub request_id: String,
    pub outcome: String,
    pub settlement: Option<Settlement>,
    pub reason: Option<String>,
    pub http_status: Option<u16>,
    pub error_message: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub breach: Option<String>,
    pub response_sha256: Option<String>,
}

#[derive(Debug)]
pub struct Options {
    pub batch: String,
    pub deadline: Instant,
    pub max_consecutive_failures: u32,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub attempted: u32,
    pub settled: u32,
    pub uncertain: u32,
    pub not_sent: u32,
    pub replay_skipped: u32,
    pub settled_micro: u64,
    pub stopped: Option<String>,
}

#[must_use]
pub fn request_id(manifest_sha256: &str, index: u32) -> String {
    sha256(format!("sigy-hosted-eval-request-v1:{manifest_sha256}:{index}").as_bytes())
}

fn breach(planned: &Planned, settlement: &Settlement) -> Option<String> {
    if settlement.micro > planned.reservation_micro {
        return Some("reported cost above reservation".into());
    }
    if settlement
        .usage
        .prompt
        .is_some_and(|tokens| tokens > planned.bounds.prompt_tokens)
    {
        return Some("prompt tokens above bound".into());
    }
    if settlement
        .usage
        .completion
        .is_some_and(|tokens| tokens > planned.bounds.completion_tokens)
    {
        return Some("completion tokens above bound".into());
    }
    None
}

fn receipt(index: u32, planned: &Planned, request_id: &str, outcome: &str) -> Receipt {
    Receipt {
        index,
        item_id: planned.item_id.clone(),
        request_id: request_id.into(),
        outcome: outcome.into(),
        settlement: None,
        reason: None,
        http_status: None,
        error_message: None,
        elapsed_ms: None,
        breach: None,
        response_sha256: None,
    }
}

/// Dispatch every planned request once, in manifest order.
pub(crate) async fn run<T: Transport>(
    transport: &T,
    directory: &Path,
    manifest: &Manifest,
    manifest_sha256: &str,
    ledger: &mut Ledger,
    options: &Options,
) -> Result<Summary> {
    let responses = directory.join("responses");
    std::fs::create_dir_all(&responses)?;
    let receipts = directory.join("receipts.jsonl");
    let mut summary = Summary::default();
    let mut consecutive_failures = 0;
    for planned in &manifest.requests {
        let index = planned.index;
        let id = request_id(manifest_sha256, index);
        if ledger.state.requests.contains_key(&id) {
            summary.replay_skipped += 1;
            continue;
        }
        if Instant::now() >= options.deadline {
            summary.stopped = Some("batch deadline reached".into());
            break;
        }
        if let Err(error) = ledger.state.admit(
            &options.batch,
            planned.reservation_micro,
            planned.hard_bound_micro,
        ) {
            summary.stopped = Some(format!("admission refused before send: {error}"));
            break;
        }
        let body = files::pinned(
            &Manifest::body_path(directory, index),
            &planned.body_sha256,
            16 * 1024 * 1024,
        )?;
        ledger.append(Event::Reserve {
            request_id: id.clone(),
            batch: options.batch.clone(),
            micro: planned.reservation_micro,
            hard_bound_micro: planned.hard_bound_micro,
            model: manifest.route.model.clone(),
            manifest_sha256: manifest_sha256.into(),
            index,
        })?;
        summary.attempted += 1;
        let sent = transport.post("/chat/completions", body).await;
        let mut record = receipt(index, planned, &id, "uncertain");
        if let Sent::Completed {
            status,
            body,
            elapsed_ms,
        } = &sent
        {
            files::write_new(&responses.join(format!("{index:04}.json")), body)?;
            record.response_sha256 = Some(sha256(body));
            record.http_status = Some(*status);
            record.elapsed_ms = Some(*elapsed_ms);
            record.error_message = outcome::error_message(body);
        }
        let mut stop = settle(ledger, planned, &sent, &mut record, &mut summary)?;
        consecutive_failures = if record.outcome == "settled" {
            0
        } else {
            consecutive_failures + 1
        };
        files::append_line(&receipts, &serde_json::to_vec(&record)?)?;
        println!(
            "request={index} outcome={} cost_usd={} provider={}",
            record.outcome,
            crate::money::usd(
                record
                    .settlement
                    .as_ref()
                    .map_or(0, |settlement| settlement.micro)
            ),
            record
                .settlement
                .as_ref()
                .map_or("none", |settlement| settlement.provider.as_str())
        );
        if stop.is_none() && consecutive_failures >= options.max_consecutive_failures {
            stop = Some("consecutive failures reached the batch limit".into());
        }
        if stop.is_some() {
            summary.stopped = stop;
            break;
        }
    }
    Ok(summary)
}

/// Append exactly one outcome for a sent request; return a stop reason, if any.
fn settle(
    ledger: &mut Ledger,
    planned: &Planned,
    sent: &Sent,
    record: &mut Receipt,
    summary: &mut Summary,
) -> Result<Option<String>> {
    let id = record.request_id.clone();
    let mut stop = None;
    match outcome::interpret(sent) {
        Interpreted::Settled(settlement) => {
            ledger.settle(Event::Settle {
                request_id: id.clone(),
                micro: settlement.micro,
                cost_text: settlement.cost_text.clone(),
                generation_id: settlement.generation_id.clone(),
                provider: settlement.provider.clone(),
            })?;
            if let Some(reason) = breach(planned, &settlement) {
                ledger.append(Event::Breach {
                    request_id: id,
                    reason: reason.clone(),
                })?;
                stop = Some(format!("billing breach: {reason}"));
                record.breach = Some(reason);
            }
            summary.settled += 1;
            summary.settled_micro += settlement.micro;
            record.outcome = "settled".into();
            record.settlement = Some(*settlement);
        }
        Interpreted::Uncertain {
            reason,
            generation_id,
            status,
        } => {
            ledger.append(Event::Uncertain {
                request_id: id,
                reason: reason.clone(),
                generation_id,
            })?;
            summary.uncertain += 1;
            if matches!(status, Some(401..=403)) {
                stop = Some(format!("provider refused the key or credit: {reason}"));
            }
            record.reason = Some(reason);
        }
        Interpreted::NotSent(reason) => {
            ledger.append(Event::Release {
                request_id: id,
                reason: reason.into(),
            })?;
            summary.not_sent += 1;
            record.outcome = "not_sent".into();
            record.reason = Some(reason.into());
        }
    }
    Ok(stop)
}

/// Read all receipts of a plan directory, keyed by request index.
pub fn receipts(directory: &Path) -> Result<std::collections::BTreeMap<u32, Receipt>> {
    let path = directory.join("receipts.jsonl");
    let mut map = std::collections::BTreeMap::new();
    if !path.exists() {
        return Ok(map);
    }
    let text = String::from_utf8(files::read(&path, 64 * 1024 * 1024)?)?;
    for line in text.lines() {
        let receipt: Receipt = serde_json::from_str(line)?;
        if map.insert(receipt.index, receipt).is_some() {
            return Err("duplicate receipt index".into());
        }
    }
    Ok(map)
}

#[derive(Debug, Serialize)]
pub struct KeyStatus {
    pub limit: Option<String>,
    pub limit_remaining: Option<String>,
    pub limit_reset: Option<String>,
    pub usage: Option<String>,
    pub is_free_tier: Option<bool>,
}

fn raw_text(value: Option<&RawValue>) -> Option<String> {
    value
        .map(|raw| outcome::label(raw.get(), 64))
        .filter(|text| text != "null")
}

/// Read the current key's limit and usage. The key label is never recorded.
pub(crate) async fn key_status<T: Transport>(
    transport: &T,
    ledger: &mut Ledger,
) -> Result<KeyStatus> {
    #[derive(Deserialize)]
    struct Wire<'a> {
        #[serde(borrow)]
        data: Data<'a>,
    }
    #[derive(Deserialize)]
    struct Data<'a> {
        #[serde(borrow)]
        limit: Option<&'a RawValue>,
        #[serde(borrow)]
        limit_remaining: Option<&'a RawValue>,
        limit_reset: Option<String>,
        #[serde(borrow)]
        usage: Option<&'a RawValue>,
        is_free_tier: Option<bool>,
    }
    let Sent::Completed {
        status: 200, body, ..
    } = transport.get("/key").await
    else {
        return Err("key status request failed".into());
    };
    let wire: Wire<'_> = serde_json::from_slice(&body)?;
    let status = KeyStatus {
        limit: raw_text(wire.data.limit),
        limit_remaining: raw_text(wire.data.limit_remaining),
        limit_reset: wire
            .data
            .limit_reset
            .map(|reset| outcome::label(&reset, 32)),
        usage: raw_text(wire.data.usage),
        is_free_tier: wire.data.is_free_tier,
    };
    ledger.append(Event::KeyCheck {
        limit: status.limit.clone(),
        limit_remaining: status.limit_remaining.clone(),
        limit_reset: status.limit_reset.clone(),
        usage: status.usage.clone(),
        is_free_tier: status.is_free_tier,
    })?;
    Ok(status)
}

/// Settle uncertain requests whose generation record is now available.
pub(crate) async fn reconcile<T: Transport>(transport: &T, ledger: &mut Ledger) -> Result<u32> {
    let pending: Vec<(String, String)> = ledger
        .state
        .requests
        .iter()
        .filter(|(_, request)| request.settled.is_none() && !request.released)
        .filter_map(|(id, request)| {
            request
                .uncertain_generation
                .clone()
                .map(|generation| (id.clone(), generation))
        })
        .collect();
    let mut settled = 0;
    for (request_id, generation) in pending {
        let path = format!("/generation?id={generation}");
        let Sent::Completed {
            status: 200, body, ..
        } = transport.get(&path).await
        else {
            continue;
        };
        let (micro, cost_text, provider) = outcome::generation_cost(&body, &generation)?;
        if ledger.settle(Event::Settle {
            request_id,
            micro,
            cost_text,
            generation_id: generation,
            provider,
        })? {
            settled += 1;
        }
    }
    Ok(settled)
}

/// Request timeout used for every paid call.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use super::*;
    use crate::{
        ledger::tests::scratch,
        plan::{self, Inputs, Task, tests as fixtures},
    };

    struct Fake {
        replies: RefCell<VecDeque<Sent>>,
        sent: RefCell<Vec<Vec<u8>>>,
    }

    impl Fake {
        fn new(replies: Vec<Sent>) -> Self {
            Self {
                replies: RefCell::new(replies.into()),
                sent: RefCell::new(Vec::new()),
            }
        }
    }

    impl Transport for Fake {
        async fn post(&self, _: &str, body: Vec<u8>) -> Sent {
            tokio::task::yield_now().await;
            self.sent.borrow_mut().push(body);
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or(Sent::Unsent("fixture-exhausted"))
        }

        async fn get(&self, path: &str) -> Sent {
            tokio::task::yield_now().await;
            self.sent.borrow_mut().push(path.as_bytes().to_vec());
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or(Sent::Unsent("fixture-exhausted"))
        }
    }

    fn ok(cost: &str, generation: &str, prompt: u64) -> Sent {
        Sent::Completed {
            status: 200,
            body: format!(r#"{{"id":"{generation}","provider":"Fixture","choices":[{{"message":{{"content":"fixture"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":{prompt},"completion_tokens":5,"cost":{cost}}}}}"#).into_bytes(),
            elapsed_ms: 3,
        }
    }

    fn setup(
        name: &str,
        count: usize,
        allocation: u64,
    ) -> Result<(std::path::PathBuf, Manifest, String, Ledger)> {
        let base = scratch(name)?;
        let route = fixtures::route();
        let endpoints = fixtures::endpoints();
        let catalog = fixtures::catalog();
        let drafts = (0..count)
            .map(|index| fixtures::draft(&format!("item-{index}"), false))
            .collect();
        let (manifest, digest) = plan::write(
            &Inputs {
                task: Task::Translate,
                route: &route,
                catalog: &catalog,
                endpoints: &endpoints,
                digests: std::collections::BTreeMap::new(),
                instruction: plan::TRANSLATION_INSTRUCTION,
            },
            drafts,
            &base.join("plan"),
        )?;
        let mut ledger =
            Ledger::create(&base.join("ledger.jsonl"), 1_000_000, 2_000_000, "fixture")?;
        ledger.append(Event::Allocate {
            batch: "batch".into(),
            micro: allocation,
            note: String::new(),
        })?;
        Ok((base.join("plan"), manifest, digest, ledger))
    }

    fn options(limit: u32) -> Options {
        Options {
            batch: "batch".into(),
            deadline: Instant::now() + Duration::from_secs(60),
            max_consecutive_failures: limit,
        }
    }

    #[tokio::test]
    async fn settles_replays_and_retains_ambiguous_liability() -> Result<()> {
        let (directory, manifest, digest, mut ledger) = setup("run-main", 4, 1_000_000)?;
        let reservation = manifest.requests[0].reservation_micro;
        let fake = Fake::new(vec![
            ok("0.000123", "gen-a", 10),
            Sent::Ambiguous("timeout-after-send"),
            Sent::Completed {
                status: 500,
                body: br#"{"error":{"message":"upstream"}}"#.to_vec(),
                elapsed_ms: 1,
            },
            Sent::Unsent("connect-failed"),
        ]);
        let summary = run(
            &fake,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(10),
        )
        .await?;
        assert_eq!(
            (
                summary.attempted,
                summary.settled,
                summary.uncertain,
                summary.not_sent
            ),
            (4, 1, 2, 1)
        );
        assert_eq!(summary.settled_micro, 123);
        let totals = ledger.state.totals(Some("batch"));
        assert_eq!(totals.settled, 123);
        assert_eq!(
            totals.outstanding,
            manifest.requests[1].reservation_micro + manifest.requests[2].reservation_micro
        );
        assert!(reservation > 0);
        let receipts = receipts(&directory)?;
        assert_eq!(receipts.len(), 4);
        assert_eq!(receipts[&2].error_message.as_deref(), Some("upstream"));
        // A replay sends nothing, including for the ambiguous and released requests.
        let replay = Fake::new(vec![ok("0.1", "gen-x", 1)]);
        let again = run(
            &replay,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(10),
        )
        .await?;
        assert_eq!((again.attempted, again.replay_skipped), (0, 4));
        assert!(replay.sent.borrow().is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn insufficient_allocation_refuses_before_any_send() -> Result<()> {
        let (directory, manifest, digest, mut ledger) = setup("run-allocation", 3, 1)?;
        let fake = Fake::new(vec![ok("0.000001", "gen-a", 10)]);
        let summary = run(
            &fake,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(10),
        )
        .await?;
        assert_eq!(summary.attempted, 0);
        assert!(
            summary
                .stopped
                .is_some_and(|reason| reason.starts_with("admission refused"))
        );
        assert!(fake.sent.borrow().is_empty());
        assert!(ledger.state.requests.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn breaches_auth_failures_and_failure_streaks_stop_the_batch() -> Result<()> {
        let (directory, manifest, digest, mut ledger) = setup("run-breach", 3, 1_000_000)?;
        let fake = Fake::new(vec![ok("0.9", "gen-a", 10), ok("0.000001", "gen-b", 10)]);
        let summary = run(
            &fake,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(10),
        )
        .await?;
        assert_eq!(summary.attempted, 1);
        assert!(
            summary
                .stopped
                .is_some_and(|reason| reason.contains("breach"))
        );
        assert!(ledger.state.admit("batch", 1, 1).is_err());

        let (directory, manifest, digest, mut ledger) = setup("run-tokens", 3, 1_000_000)?;
        let fake = Fake::new(vec![ok("0.000001", "gen-a", 10_000_000)]);
        let summary = run(
            &fake,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(10),
        )
        .await?;
        assert!(
            summary
                .stopped
                .is_some_and(|reason| reason.contains("prompt tokens"))
        );

        let (directory, manifest, digest, mut ledger) = setup("run-auth", 3, 1_000_000)?;
        let fake = Fake::new(vec![Sent::Completed {
            status: 402,
            body: b"{}".to_vec(),
            elapsed_ms: 1,
        }]);
        let summary = run(
            &fake,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(10),
        )
        .await?;
        assert!(
            summary
                .stopped
                .is_some_and(|reason| reason.contains("refused"))
        );
        assert_eq!(ledger.state.totals(None).uncertain_requests, 1);

        let (directory, manifest, digest, mut ledger) = setup("run-streak", 3, 1_000_000)?;
        let fake = Fake::new(vec![
            Sent::Unsent("connect-failed"),
            Sent::Unsent("connect-failed"),
        ]);
        let summary = run(
            &fake,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(2),
        )
        .await?;
        assert_eq!(summary.not_sent, 2);
        assert!(
            summary
                .stopped
                .is_some_and(|reason| reason.contains("consecutive"))
        );

        let (directory, manifest, digest, mut ledger) = setup("run-deadline", 1, 1_000_000)?;
        let mut expired = options(2);
        expired.deadline = Instant::now();
        let summary = run(
            &Fake::new(Vec::new()),
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &expired,
        )
        .await?;
        assert!(
            summary
                .stopped
                .is_some_and(|reason| reason.contains("deadline"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn tampered_body_is_refused_after_admission_without_send() -> Result<()> {
        let (directory, manifest, digest, mut ledger) = setup("run-tamper", 1, 1_000_000)?;
        let path = Manifest::body_path(&directory, 0);
        let mut body = std::fs::read(&path)?;
        body.push(b' ');
        std::fs::remove_file(&path)?;
        std::fs::write(&path, body)?;
        let fake = Fake::new(vec![ok("0.000001", "gen-a", 10)]);
        assert!(
            run(
                &fake,
                &directory,
                &manifest,
                &digest,
                &mut ledger,
                &options(2)
            )
            .await
            .is_err()
        );
        assert!(fake.sent.borrow().is_empty());
        assert!(ledger.state.requests.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn key_status_and_reconciliation_use_free_reads() -> Result<()> {
        let (directory, manifest, digest, mut ledger) = setup("run-reconcile", 2, 1_000_000)?;
        let fake = Fake::new(vec![
            Sent::Completed {
                status: 200,
                body: br#"{"id":"gen-r"}"#.to_vec(),
                elapsed_ms: 1,
            },
            Sent::Ambiguous("timeout-after-send"),
        ]);
        run(
            &fake,
            &directory,
            &manifest,
            &digest,
            &mut ledger,
            &options(10),
        )
        .await?;
        assert_eq!(ledger.state.totals(None).uncertain_requests, 2);
        let lookups = Fake::new(vec![Sent::Completed {
            status: 200,
            body: br#"{"data":{"id":"gen-r","total_cost":0.000042,"provider_name":"Fixture"}}"#
                .to_vec(),
            elapsed_ms: 1,
        }]);
        assert_eq!(reconcile(&lookups, &mut ledger).await?, 1);
        assert_eq!(lookups.sent.borrow()[0], b"/generation?id=gen-r");
        let totals = ledger.state.totals(None);
        assert_eq!((totals.settled, totals.uncertain_requests), (42, 1));
        assert_eq!(reconcile(&Fake::new(Vec::new()), &mut ledger).await?, 0);

        let key = Fake::new(vec![Sent::Completed {
            status: 200,
            body: br#"{"data":{"label":"sk-or-v1-abc...xyz","limit":20,"limit_remaining":19.5,"limit_reset":null,"usage":0.5,"is_free_tier":false}}"#.to_vec(),
            elapsed_ms: 1,
        }]);
        let status = key_status(&key, &mut ledger).await?;
        assert_eq!(status.limit.as_deref(), Some("20"));
        assert_eq!(status.limit_remaining.as_deref(), Some("19.5"));
        assert_eq!(status.limit_reset, None);
        let ledger_path = directory
            .parent()
            .ok_or("plan has no parent")?
            .join("ledger.jsonl");
        let text = std::fs::read_to_string(ledger_path)?;
        assert!(!text.contains("sk-or"));
        assert!(
            key_status(&Fake::new(Vec::new()), &mut ledger)
                .await
                .is_err()
        );
        Ok(())
    }
}
