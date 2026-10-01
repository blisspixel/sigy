# Atomic monitor processing admission

Date: 2026-10-01. Status: implemented and tested on Windows with 11 focused storage fixtures, five focused actor fixtures, final workspace verification, per-crate coverage, all 15 native fixtures and actual CLI inspection. This tightens the admission boundary of [monitor processing](0048-monitor-processing.md). See the [validation record](../../research/experiments/monitor-processing-2026-10-01.md). No roadmap stage exit is established.

## Problem

The monitor controller previously queued recognition or translation through the direct analysis path, scheduled workers, and then wrote its monitor step. These were separate transactions. A failed step insertion could therefore leave a queued or running job without its monitor receipt or recognition charge. A later pass could encounter that shared job, but recovery did not make the original admission atomic.

The ordinary step replay also compared only decision, reason and job identity. That comparison did not establish equality of the stored policy version, analysis identity and charged audio. Exact historical replay needs a complete stored request comparison.

## Decision

Queue a monitor processing job and its immutable monitor step in one immediate catalog transaction. Recognition admission also commits the exact decoded-audio charge on its UTC day and against the monitor's lifetime allowance. Translation commits its job reference and a zero-audio step. Both paths reuse the existing canonical recognition or translation enqueue validation and durable job pool.

Preparing and publishing an analysis pin remains a separate metadata operation. A refused job admission may leave that pin readable. It must leave no new job, monitor charge, queued receipt, lease or worker from the refused admission. Pin metadata alone grants no processing authority.

The storage result distinguishes whether the job and monitor step were newly created. Only a newly committed job requests scheduling through the existing service pool. Historical replay returns without scheduling. A fresh monitor receipt may refer to an already existing exact shared job; it charges that monitor once without creating or restarting the shared job. Canonical job replay preserves that job's history rather than rechecking its input or parent as a new job. Fresh monitor authority checks still apply, and an existing queued job remains subject to the pool's canonical claim validation.

No work token or worker launch may escape before the admission transaction commits. A fault after job insertion but before step insertion rolls back both. A fault after the commit preserves both for the existing job pool and level-triggered monitor controller to recover. Scheduling failure does not erase the committed charge or convert the queued receipt into a skip.

## Fresh admission and replay

Before a fresh insertion, recheck the expected monitor version and complete current action count inside the transaction. Pause, source removal and other intervening actions invalidate a stale fact snapshot even when the version number is unchanged. Validate the currently followed source, selected stage profile, recording identity and applicable candidate state. Recognition's proposed audio must equal the recording's stored decoded duration. Translation must refer to the exact recognition job cited by that monitor's recognition step and its published transcript revision. A later independent recognition or user correction does not silently become the monitor's selected result. For a newly created job, canonical enqueue validation still checks current input, profile binding, transcript parent, recognized text and queue bounds as applicable.

An existing monitor step is checked before live policy, media, candidate and clock conditions. Replay compares its full immutable payload and the canonical job's exact request. A changed policy version, recording, stage, analysis identity, job identity, outcome, charged audio or job request conflicts rather than becoming a new admission. Exact historical replay does not require today's policy or media to match and never charges another UTC day.

The existing step schema has no historical action-count field. Fresh admission checks the current action prefix; replay must not claim that an earlier prefix was stored or reconstructed. This increment does not add a schema field, change catalog or IPC version 42, or rewrite existing step history.

Fresh timestamps must be in range and must not precede the latest monitor policy, action or step timestamp. This compares monitor history, not every recording, pin or profile timestamp. A backward clock holds new admission without admitting work or consuming another day's allowance. Exact replay remains a history read. Daily charges use integer UTC days; lifetime charges include every policy version. Revising policy, restarting, a failed worker or a new day does not refill lifetime allowance or remove an earlier charge.

## Ownership and bounds

Stable canonical job identities continue sharing one recognition or translation across monitors with matching input and profile. Each monitor owns its receipt and cap charge. A receipt does not grant exclusive ownership or a new cancellation right over a shared job. Monitor pause stops future monitor admission and leaves admitted jobs and independent capture under their existing authority.

The controller retains its bounded fact reads, four-step planner limit per monitor per pass, five-second reconciliation interval and existing fair job claims. A full queue, backward clock, stale policy or action snapshot, pause, changed profile, unfollowed source and insufficient remaining caps hold admission without a skipped step. A later pass rereads current facts. Expected permanent processing refusals remain explicit skipped outcomes. Catalog faults propagate rather than being rendered as successful admission or language evidence.

Production callers can append a skipped step through the standalone refusal path. They cannot append a queued step through that path; queued steps require the atomic admission methods. Historical test fixtures may still construct earlier queued rows without granting production authority.

Monitor output describes admitted audio, recognition admissions and translation admissions. These counters count historical queued steps, including work that later failed or finished; they do not count currently queued jobs or successful processing. Existing public JSON field names remain compatible while human-readable labels explain this meaning.

This change uses local processing with zero paid allowance. It adds no provider transport, network destination, scheduler, worker pool, ledger, dependency or telemetry. [Monitor-owned capture](0065-monitor-owned-capture-schedules.md) retains its separate preconnection reservations. [Finite task evidence publication](0067-task-evidence-execution.md) continues using existing catalog evidence and gains no collection or processing grant from this decision.

## Evidence and limits

The [validation record](../../research/experiments/monitor-processing-2026-10-01.md) separates source review, focused fixtures and final integrated checks. Eleven storage fixtures passed in 6.25 seconds, including job, step and deferred-commit faults, catalog reopen, queued and completed shared jobs, exact and changed replay, UTC rollover, lifetime caps across revisions, queue pressure, source and profile checks, and exact translation lineage. Five actor fixtures passed in 0.61 seconds, covering receipt-abort scheduling order, temporary clock and action holds, direct enqueue replay and historical monitor replay. The actor fixtures supervise work with absent runtime assets; they establish no native executable containment or language quality. Service warnings-denied Clippy passed in 27.45 seconds.

Final workspace verification, coverage, all 15 existing native fixtures and actual CLI inspection passed. Catalog reopen is covered; abrupt service death precisely at this admission boundary, physical power loss and capture behavior under simultaneous faults require separate evidence.

SQLite transaction rollback and catalog reopen establish narrower evidence than physical power loss or long-term service reliability. The [private diagnostics and recovery review](../../research/32-private-diagnostics-and-recovery.md) records those limits and the primary persistence references. Language quality, sustained capacity, native OS network isolation, supported-platform qualification and general task planning remain open. Final results belong in [active work](../development/progress.md).
