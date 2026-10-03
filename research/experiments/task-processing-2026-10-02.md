# Finite task processing validation

Date: 2026-10-02. Status: local verification complete on the stream branch; results below. Integrated gates, coverage and exact-commit CI belong to the coordinator. This record covers [task-owned processing](../../docs/decisions/0072-task-owned-processing.md), which extends [task-owned collection](task-collection-2026-10-01.md) and reuses the [atomic monitor processing](monitor-processing-2026-10-01.md) admission pattern. No stage exit is established.

## Scope

Durable job interests distinguish direct, monitor and task authority over one canonical recognition or translation job. One lifetime zero-USD processing grant per task binds named local profiles, their stored hashes and a finite recognition audio allowance. The service tick admits task recognition and translation of the grant's exact collected recordings, each with its receipt, interest and charge in one transaction. Cancellation fences future task admissions only. A read-only reconciliation follows collection through task processing to literal evidence, reporting uncovered and unprocessed time separately.

This increment does not implement a planning model, hosted transport, A2A, interest-aware cancellation of shared jobs, task publication directly from reconciled evidence, or language quality measurement.

## Verification plan

| Boundary | Required checks |
| --- | --- |
| Migration | Populated v43 catalog to v44 with labeled derived interests; interrupted migration leaves v43 unchanged |
| Atomic admission | Receipt, interest and deferred-commit faults leave no job, receipt, interest or charge; exact replay writes nothing; changed replay conflicts |
| Shared work | A monitor job is shared once; each authority charges its own allowance once; direct interest commits with its job |
| Authority | Missing collection, missing profile, scope drift, pause action, regressed clock, wrong profile, changed audio, uncollected recording and exhausted allowance |
| Lineage | Translation binds only the transcript revision the task's own recognition published; corrections and other revisions are refused |
| Cancellation | Generation conflict, immutable replay, future-admission fence, admitted jobs and independent interests continue, frozen receipt set |
| Recovery | Catalog reopen, populated backup and verified restore, hostile edits and immutability guards |
| Service tick | Bounded pass, repeated pass, receipt fault, restart, cancellation, monitor independence and a clock hold |
| Evidence | Interrupted versus uninterrupted runs, exact lineage after correction, missed capture, missing translation, no text, cancelled recognition, missing grant and cancellation stay partial |
| Native media | Loopback collection of two sources, stand-in recognizer and translator, processing after the granting client exits, cited evidence |
| Interface | Parser bounds, sanitized output, control replay, hostile request fields and actual offline CLI inspection |

## Focused results

Thirty new tests pass: 18 storage fixtures, five planner tests, three actor fixtures, one control fixture and three CLI parser and rendering tests. One native phase was added to the existing contained recognition fixture.

Storage fixtures cover the grant contract, exact and changed replay, missing collection and profile, the grant clock and scope drift. Admission fixtures inject a receipt trigger, an interest trigger and a deferred foreign-key commit fault; each leaves no job, receipt, interest or charge, and a reopened catalog then admits effects identical to an uninterrupted reference. A monitor-created job is attached once with separate monitor and task charges, and a direct admission writes its own interest. Refusals cover a regressed clock, changed audio, an uncollected recording, another profile hash, an exhausted lifetime allowance, scope drift after a pause action and cancellation. Translation is refused for a corrected revision and for an ungranted profile. Recovery fixtures migrate a populated catalog downgraded to v43 and check the labeled derived interests, including the documented loss of a direct receipt that an earlier catalog never stored; a conflicting object makes the migration fail and leaves v43 unchanged. Immutability triggers refuse updates and deletes, and five hostile edits with triggers dropped fail reopen. A library backup and verified restore reproduce the processing view, every job, receipt, interest and monitor step, and both retained files.

Evidence fixtures drive collection through processing to evidence twice. The interrupted run fails every task effect once at a rotating receipt, interest or commit boundary and closes and reopens the catalog after every effect; its jobs, receipts, interests, charges, citations and outcome equal the uninterrupted run, which reaches `cited` with no uncovered or unprocessed time. After a correction the evidence still cites the task's own revision while a later checkpoint does not; without a correction, every evidence citation appears in a checkpoint and the existing publication path publishes its finding and briefing. A missed capture, a missing translation grant, a transcript without text, a cancelled recognition job, a missing processing grant and processing cancellation each stay partial with named reasons.

Actor fixtures show a bounded tick admitting both collected recordings, the pass interval holding a second pass and a repeated pass changing nothing, with budgets unchanged. Processing cancellation holds only task admission while the monitor's own processing admits the same recordings. A grant stamped an hour after the live clock holds. A receipt fault on the second admission propagates as a catalog fault and rolls back that job; a restarted actor then admits the remaining work exactly once and records the first job's failed recognition as a translation refusal.

The native phase, under `cargo verify-media` with FFmpeg 9.0.1, records two loopback sources through task-owned once schedules, then the service processes them with the stand-in recognizer and translator after the granting client exits. Evidence reaches `cited` with two citations, zero uncovered and zero unprocessed time, 8,000,000 us charged to the task, four queued receipts with succeeded jobs held only by the task, and no monitor processing receipts.

## Interface inspection

An offline inspection drove 22 CLI operations against a fresh private library with two registered offline sources, stand-in profile files, a monitor, a task and a future collection grant. It showed the grant, its exact replay, a refused changed replay, the pending evidence before capture, a pause action holding processing as `scope-changed`, cancellation at generation 2, its exact replay, a refused stale cancellation, and evidence becoming `partial` with `scope-changed` and `uncovered-time`. No capture, acquisition, recognition or translation ran. Single operation samples on the loaded development host were 379 ms for the grant, 207 ms for inspection, 400 ms for cancellation and 292 to 793 ms for evidence. They are observations, not latency claims. The library occupied 1,064,960 bytes.

## Integrated local results

On the branch rebased onto `main` at 7bda2c4, with catalog schema v44 and local IPC v45, `cargo verify` passed 814 test executions with 17 ignored native fixtures, formatting, warnings-denied Clippy, build, native-source hashes and `cargo audit` over 312 dependencies and 1,288 advisories. `cargo verify-media` passed all 16 native fixtures in 167.26 seconds. An earlier `cargo verify-media` run on the branch rebased onto the shutdown repair also passed all 16 in 249.35 seconds. Seven parallel build streams loaded the host throughout; these timings are not measurements. Coverage was not run by this stream.

The collection native fixture that once failed under full host load compared a cancelled future occurrence that was still `waiting` live with the `missed` state its restored copy showed after a later schedule tick. It now waits for the settled `missed` state before comparing; its earlier assertions are unchanged, and it also asserts that the occurrence settles as missed. Private logs and receipts are under `.agents/task-processing/`.

## Evidence limits

Synthetic recognizer and translator outcomes establish catalog behavior only. Actor fixtures supervise workers with absent runtime assets. The native fixture uses a stand-in recognizer and translator over generated tones. None of these establish native network isolation, recognition quality or translation quality. Disposable libraries and logs stay under ignored `.agents/task-processing/` and `target/`. No user library is modified. No paid inference, model download or hosted compute is used; external spend for this stream is USD 0.
