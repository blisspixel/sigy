# Finite task processing validation

Date: 2026-10-02. Status: local verification on a parallel build day; results below. This record covers [task-owned processing](../../docs/decisions/0072-task-owned-processing.md), which extends [task-owned collection](task-collection-2026-10-01.md) and reuses the [atomic monitor processing](monitor-processing-2026-10-01.md) admission pattern. No stage exit is established.

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

## Evidence limits

Synthetic recognizer and translator outcomes establish catalog behavior only. Actor fixtures supervise workers with absent runtime assets. The native fixture uses a stand-in recognizer and translator over generated tones. None of these establish native network isolation, recognition quality or translation quality. Disposable libraries and logs stay under ignored `.agents/task-processing/` and `target/`. No user library is modified. No paid inference, model download or hosted compute is used; external spend for this stream is USD 0.
