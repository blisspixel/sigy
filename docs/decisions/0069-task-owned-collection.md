# Task-owned collection

Date: 2026-10-01. Status: implemented and locally verified. Hosted source-only publication requires passing exact-commit CI; its receipt is recorded in release metadata. Verification outcomes are recorded in [active work](../development/progress.md). This is a finite collection slice, without general planning or task-owned processing.

## Decision

An explicit collection grant binds one immutable [task scope](0066-durable-task-workflows.md) to one or two distinct, already registered and followed source revisions. Each entry is a UTC once capture with a whole-second start, a duration of 1 to 900 seconds and a byte ceiling of 1 to 256 MiB. Its entire planned interval must fit inside the task window. The aggregate duration and byte ceiling must fit the frozen monitor's capture lifetime limits. Paid allowance is zero.

One task can receive one collection grant in its lifetime. Exact replay returns that history without creating another rule or replenishing allowance. A changed request conflicts. The grant, all new civil schedule rules and their immutable ownership bindings commit in one transaction. Existing independent rules cannot be adopted. Task-owned rules cannot be revised through ordinary schedule commands.

The common civil scheduler admits these rules. Inside its existing DVR admission transaction, it checks the collection generation, cancellation, frozen task scope, exact monitor version and complete action prefix, source revision and original rule plan. The existing monitor capture policy and remaining shared daily, lifetime-second and lifetime-byte caps then apply. Full planned seconds, including a late prefix gap, and maximum bytes are reserved before connection. Failure, interruption, cancellation, restart and UTC rollover never refund or refill lifetime reservations.

A scheduler pass commits its bounded set of rule transitions together, using per-rule savepoints for quota deferral. A later rule or commit failure cannot leave an earlier admission without its returned launch. The actor dispatches a committed batch before fallible quota reclamation or another pass. These rules reuse the existing supervisor; no retry can reconnect an already admitted occurrence.

## Cancellation and inspection

Collection cancellation has its own generation and immutable request receipt. Its hashed receipt freezes the exact set of already admitted entries, even when admission and cancellation share a timestamp. It stops future collection admissions for that task. It preserves already admitted captures, their reservations, existing recordings and independently authorized schedules or processing. The existing `task cancel` publication command retains its separate scope.

A new monitor version or any new action, including a refusal, changes the frozen task scope and holds its future collection. Pause and resume are also actions. Already admitted capture continues, and independent monitor-owned schedules retain their existing processing-pause behavior. A user cannot silently refresh this task's grant to a later policy or use cancellation to obtain another lifetime grant.

Collection inspection follows only the grant's stored rule and occurrence bindings to their actual recording IDs and states. It distinguishes a planned occurrence from an admitted recording and a completed operation from usable audio. It does not treat unrelated recordings on the same source as task collection. Existing task checkpoints retain their broader monitor-observation meaning; they are not collection-owned processing outcomes.

## Persistence and trust

Catalog schema and local IPC advance together to v43. Stop an older service before replacing its binary. The new grant, entry and cancellation records are protected application data. A durable rule marker prevents a missing ownership binding from silently turning task authority into independent capture authority. Pre-existing independent rules retain their authority. Admission reuses the canonical task-scope validator, including policy and action chronology. Reopen audits validate hashes and exact task, rule, occurrence and admission provenance in addition to foreign keys. Backup and restore use the canonical verified catalog and media path.

The CLI and local service expose this explicit finite delegation. MCP gains no task mutation. Models, transcript text, network content and skills cannot create authority or raise budgets. No additional scheduler, worker pool, ledger, runtime or dependency is introduced.

[Task collection and scoped-effects research](../../research/33-task-collection-and-scoped-effects.md) records reviewed primary sources, alternatives and required evidence. Transaction, migration, replay, cancellation, corruption and native-media outcomes belong in the [dated validation record](../../research/experiments/task-collection-2026-10-01.md).

## Remaining work

Task-owned recognition and translation require canonical input/profile bindings, atomic job-interest and effect receipts, and cancellation that respects independent job interests. The current monitor can process a captured recording under its own existing authority; that does not implement task-owned processing. Automatic collection-to-evidence reconciliation, general local planning, hosted transport, A2A, language quality, sustained capacity, clean-host recovery and physical power-loss qualification remain open. No roadmap stage exit or supported release follows from this increment.
