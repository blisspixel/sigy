# Finite task evidence publication

Date: 2026-09-30. Status: implemented narrow workflow, covered by local Windows verification and coverage gates. This extends [durable scope and checkpoints](0066-durable-task-workflows.md) with an explicit, narrowly delegated catalog workflow. General task planning, task-owned capture and live provider transports remain separate work.

## Decision

Execute one selected frozen checkpoint through the fixed `literal-briefing-v1` workflow. An explicit execution request authorizes at most 64 task-owned literal finding attempts and one briefing containing exactly those findings. Accepted scope, checkpoint identity, finite publication limit, zero paid allowance and stable effect identities are persisted before publication. Task creation, goal text and checkpoint text grant no execution authority.

Reuse canonical finding and briefing validation, limits, storage and grouping. The task path supplies exact briefing membership and frozen checkpoint coverage. Ordinary manual monitor briefings retain their existing monitor-wide membership behavior. An unrelated finding cannot enter a task briefing simply because it belongs to the same monitor.

This workflow produces cited artifacts from evidence already available in the catalog. It starts no capture, recognition, translation, network request or model. Collection and processing continue under their existing monitor, schedule and job authority. A successful publication establishes an artifact, not semantic understanding or completion of the user's goal.

## Admission and effects

One run per task is admitted with a caller request identity, checkpoint ordinal, maximum findings and expected initial generation zero. Validate current monitor version and complete action prefix, the selected checkpoint and nondecreasing time. A paused checkpoint cannot authorize publication. Exact replay of accepted admission remains readable after cancellation or policy drift; changed scope under the same identity conflicts.

Persist a deterministic finite plan bound to the original task and checkpoint hashes. Each effect uses a derived identity, and the catalog effect and receipt commit in one immediate transaction. Crash before commit leaves neither; crash after commit leaves both. Reconciliation cannot create an effect without accepted intent or replay another identity. Original finding availability is determined at publication and remains frozen in its receipt; current media availability is a separate read.

Every fresh effect rechecks the monitor version and complete action count. Drift revokes pending task publication without rewriting history. Cancellation uses a request identity and exact current generation; it stops only future task effects. Previously published artifacts, independent schedules, capture, monitor processing and shared jobs remain under their own authority.

The existing service tick advances at most four pending tasks, one catalog effect per visited task, with a rotating identifier cursor. The cursor is a fairness hint and carries no authority. Restart derives pending work from durable run state. A clock earlier than the last committed receipt holds publication until it catches up; it does not stop independent service work. An offline client may admit a run, inspect it or cancel it; execution waits until the service runs. No second scheduler, worker pool or budget ledger is added.

## Partial results and trust boundaries

Checkpoint citations without a translation revision or beyond the finding publisher's existing cue bounds become explicit skipped outcomes. The executor does not invent a translation, dispatch another job or expand a publisher limit. Expected resource and publication refusals become durable partial outcomes; they must not stop the entire service through its periodic error path. Catalog integrity faults still fail closed.

The briefing freezes the selected checkpoint's coverage. Truncation, absent coverage, gaps and unfinished or untranslated processing remain visible. Cancellation and revocation preserve committed receipts. Operational completion means the finite publication plan finished; wording, semantic support, independent corroboration and language quality remain unmeasured.

Models and retrieved content cannot supply findings, overwrite receipts, assert success, change the finite plan or widen this grant. Task mutations remain unavailable through MCP until authenticated, revocable delegation has its own contract and evidence. [Agentic research](../../research/15-agentic-analysis.md) and [private recovery research](../../research/32-private-diagnostics-and-recovery.md) motivate these boundaries without qualifying them.

## Verification requirements

The [validation record](../../research/experiments/task-execution-2026-09-30.md) records implemented fixtures and actual CLI inspection. A local empty-evidence rehearsal admitted a run offline at generation 1, then the service stored an empty briefing and reported a partial outcome at generation 2. Admission and inspection took 124.8542 ms and 143.8502 ms in single samples. Library files totaled 962,560 bytes after service stop; incremental storage, CPU time and peak memory were not measured. This rehearsal used no network acquisition, native processing, model invocation or paid dispatch. Two focused CLI/IPC fixtures passed. Full workspace verification passed 694 test executions; coverage passed all five per-crate 80% gates and all 15 native fixtures in 72.86 seconds; no stage exit or general task competence is established.

Check exact and conflicting replay, stale generations, scope drift before and between effects, cancellation, unsupported citations, exhausted publication limits and safe original-script rendering. Inject rollback after effect insertion but before receipt and compare committed results with uninterrupted execution. Restart and backup/restore midway through a populated workflow must preserve intent, effect identity, exact briefing membership and frozen coverage.

Exercise unrelated monitor findings, later translation and transcript revisions, retained finding followed by media deletion, and reopen after each legitimate transition. Admission checks must continue validating present media truth while historical audit preserves valid publication history. Verify bounded service-pass fairness and unchanged capture, processing jobs and exact budgets. Run `cargo verify`, all native fixtures through `cargo verify-coverage`, per-crate coverage gates, documentation checks and actual CLI inspection. Evidence and open limitations belong in [active work](../development/progress.md).
