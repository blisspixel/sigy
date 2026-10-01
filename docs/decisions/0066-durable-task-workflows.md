# Durable task scope and observed progress

Date: 2026-09-30. Status: implemented scope storage and service-observed checkpoints, covered by local Windows fixtures. No general task harness, planning transport or new delegated mutation is established by this record.

## Decision

The first task increment stores a durable accepted scope around existing topic monitoring. A task binds one inspected immutable monitor version, the count of all monitor actions the user inspected, a finite UTC evidence window and an explicit zero-USD allowance. The service stores its identity and structured progress in the existing catalog. It does not create another scheduler, worker queue, source grant or spending ledger.

This increment establishes the durable prerequisite for the [task workflow design](../design/task-workflows.md). A subsequent increment may reconcile that task against existing monitor-owned schedules, processing jobs, coverage and citations. Local planning, provider transport, skill-driven proposals and protocol adapters follow scoped delegation and task-specific evaluation. The [interoperability research](../../research/15-agentic-analysis.md) records those alternatives.

## First increment

The local CLI and IPC expose task creation and bounded read-only inspection. Creation accepts typed fields rather than a free-form executable plan:

- a caller-supplied idempotency identity and a bounded user goal;
- the existing monitor identity, exact version and action count the user inspected;
- a half-open UTC interval with a checked start before its end;
- a fixed versioned monitoring template and an explicit zero paid allowance.

Validate the referenced monitor version and all scalar and text bounds before persistence. A fresh submission must use the current inspected version and complete action prefix, including refusals. Source changes and pause proposals can change a monitor without appending a policy version, so version alone is insufficient. Exact replay of the same task identity and accepted scope returns the existing record even after the monitor changes. A changed payload under that identity conflicts and writes nothing. The immutable record preserves the original monitor specification digest. Task text cannot authorize any operation.

Creation alone starts no capture, recognition, translation, finding, briefing, network request or model invocation. Existing monitor versions, followed-source actions, processing caps and explicitly owned civil schedules remain their own execution authority. Task scope is a narrower description of requested work; it is never a replacement grant. A monitor revision can restrict future work without rewriting the task's accepted history.

## Progress and recovery

Progress consists of bounded service-observed checkpoints, stored separately from the immutable accepted scope. Append uses an expected checkpoint ordinal and request identity so a stale observation cannot replace newer progress. Each checkpoint freezes canonical monitor coverage, a bounded list of deduplicated citation pointers, the number of transcripts scanned, truncation, window expiry and monitor pause. Citation pointers name the original transcript and optional translation revision and the cue's media interval; they store no script copies. These observations do not establish retained audio availability, create a finding or hold media. They do not store hidden reasoning, full provider conversations or ordinary diagnostic payloads.

A checkpoint request replays the stored observation before current policy checks when its request identity and expected ordinal match. Reusing that identity with another expected ordinal conflicts. A fresh request requires the same current monitor version and action prefix and nondecreasing observation time. Policy drift holds further observation without rewriting the accepted scope or frozen checkpoints. Task inspection exposes that drift.

Only service code may append execution observations. A model, retrieved document, skill or external adapter cannot assert semantic success by submitting a checkpoint. Initial inspection must distinguish an accepted task with no execution observations from a running task. A successful process is not proof that the goal has been achieved.

The first storage increment does not resume operations by itself. Later reconciliation must read durable facts and derive the next permitted operation. Store its stable idempotency identity before effects, use the existing admission paths and recover the existing receipt after interruption. Do not infer that an accepted scope, an absent checkpoint or an expired client session permits a fresh request. Existing stale worker generations, attempts, read leases, uncertain liabilities and missed civil windows retain their behavior.

Cancellation of a future workflow must stop only its owned future work. It must not stop independent schedules or shared derivations another monitor still needs. Policy revision, retention expiry, input deletion, resource refusal and unavailable quality remain visible partial or blocked outcomes.

## Storage and integration

Catalog migration 041 adds strict `tasks` and `task_checkpoints` tables with explicit logical ordinals, immutable scope rows, append-only checkpoint rows and foreign keys to the inspected monitor version. The scope digest binds the fixed template, copied monitor specification digest, original stored scope serialization and zero allowance under a versioned domain envelope. Each checkpoint digest binds the task scope digest and original checkpoint serialization. Audit the raw stored bytes and decoded bounds on open. Do not reserialize an older scope to establish its historical digest.

At most 256 tasks and 128 checkpoints per task are retained. Checkpoint serialization is limited to 64 KiB, listing returns at most 16 identities, coverage keeps the existing bounded source and capture reads, and literal matching keeps the shared transcript and match-page limits. The initial contract has no history deletion or compaction API. A full task store requires a later explicitly designed maintenance increment rather than silently discarding accepted scopes.

Reopen reconstructs the historical policy, followed sources and pause state once per task, then validates its checkpoints against that context. It checks ordinal and clock order, copied scope fields, coverage source counts, deduplicated immutable citation references, literal-term support and known schedule references. It does not recompute frozen coverage from today's advancing job and retention state. A changed transcript revision cannot replace an older cited revision. Scope freshness checks use scalar version and action queries rather than loading unrelated processing and capture aggregates.

Storage APIs admit and inspect task scopes and append checked observations through the service. Local control exposes the matching operations and an optional bounded task page in the shared snapshot. Catalog and IPC advance together to v41. Keep list pages and checkpoint limits inside existing request and response bounds.

The existing offline catalog backup includes these rows. A populated migration and backup/restore fixture must prove that task identities, scope bytes and digests, monitor references and checkpoint order survive restore. Explicit ordinals establish order; implicit SQLite row IDs do not. Existing [backup semantics](0045-library-backup-and-restore.md) and [monitor-owned capture authority](0065-monitor-owned-capture-schedules.md) stay unchanged.

## Verification

The final `cargo verify` passed 659 test executions, formatting, warnings-denied Clippy, native-source hashes, build and a fresh dependency audit. Twelve storage fixtures cover exact and changed replay, policy and action drift, creation without new work, backward clocks, ordinal conflicts, checkpoint and task capacity, pagination, SQL immutability, altered scope and checkpoint bytes, internally inconsistent snapshots with recomputed hashes, migration rollback, a populated v40 migration and catalog snapshot/reopen after deliberately reversing physical checkpoint row IDs with its immutability guard restored. CLI/IPC fixtures cover abrupt service restart and verified backup/restore. A synthetic recognized-text fixture preserves exact citation revisions through translation changes, correction and deleting media, including catalog reopen. The [validation record](../../research/experiments/task-scope-2026-09-30.md) and [active work](../development/progress.md) record evidence and limitations. These fixtures establish no model quality or autonomous task completion.

Exercise valid creation, empty and oversized fields, reversed or overflowing windows, missing and stale monitor versions, malformed scope bytes, exact replay, changed replay and unauthorized checkpoint access. Check that invalid requests leave no partial row, replay writes no additional observation and creation causes no work or spend.

Test stale ordinal conflicts and competing progress proposals, restart with accepted and observed tasks, SQL refusal of scope and checkpoint edits, reopen refusal of changed scope bytes or references, migration rollback on a conflicting schema and a genuine populated previous-schema migration. Backup, restore and reopen must reproduce scope and checkpoint order after deliberately changing physical row IDs.

CLI and IPC fixtures inspect the accepted scope, zero allowance, lack of execution observations and bounded listing. Hostile text must render safely and remain inert. Run the repository verification and coverage gates appropriate to the changed crates. Acquisition and native-media verification are additionally required if the implementation changes those paths.

## Follow-on acceptance

The next slice advances one finite task over two already authorized sources and configured local profiles through scoped delegation and existing execution operations. The current checkpoint can already cite literal matching recording, transcript and translation revisions and report stage coverage, but it does not decide or execute the next operation. Compare interrupted and uninterrupted runs for admissions, charges, effects and evidence. Exercise processing pause, revoked policy, exhausted caps, missing audio and a failed-quality outcome.

That result is still not a general autonomous planner or professional qualification. Language evaluation, native containment, sustainable capacity, unattended recovery and clean-host restore remain independent evidence gates. External spend for the proposed initial slice is zero.
