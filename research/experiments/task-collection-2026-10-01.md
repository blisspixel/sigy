# Finite task collection validation

Date: 2026-10-01. Status: local verification complete. Hosted source-only publication requires passing exact-commit CI; its receipt is recorded in release metadata. This record covers [task-owned collection](../../docs/decisions/0069-task-owned-collection.md), supported by [scoped-effects research](../33-task-collection-and-scoped-effects.md). No stage exit is established.

## Scope

One lifetime zero-USD task grant creates one or two new finite UTC once rules through the common civil scheduler. It binds the task scope, inspected monitor version and complete action prefix. Scheduler admission checks that delegation inside the existing DVR transaction, alongside full-window monitor capture reservations. Collection inspection follows exact occurrence and recording provenance. Cancellation affects only future task admissions.

This increment does not implement task-owned recognition or translation, collection-to-evidence reconciliation, a planning model or A2A. Existing monitor processing remains independent authority. Monitor observations and checkpoints are broader than the task's collection membership.

## Verification plan

| Boundary | Required checks |
| --- | --- |
| Finite delegation | One or two distinct authorized sources, exact task-window containment, duration and byte ceilings, complete action-prefix binding, zero paid allowance |
| Atomic creation | Later-entry failure and commit failure leave no grant or new schedule; exact replay leaves identities and reservations unchanged |
| Admission | Current frozen authority, original rule and exact occurrence, shared capture caps, full planned seconds and UTC-day partitions |
| Cancellation | Generation conflict, immutable replay, future-admission fence, admitted capture and independent work preserved; equal-clock receipt ordering |
| Recovery | Restart, populated backup/restore, no backfill or duplicate admission, clock regression, corruption and migration rollback |
| Actual recording | Loopback native media after client exit, exact task recording membership despite unrelated same-source recording, cancelled future grant makes no connection |
| Interface | Parser bounds and sanitized status; actual local CLI inspection and operation timing |
| Integrated gates | Full `cargo verify`, all-crate 80% coverage with native fixtures, documentation links/status checks, exact-commit Windows CI |

## Evidence limits

Disposable libraries, logs and receipts stay under ignored `.agents/task-collection/` and `target/`. No user library is modified. Local fixtures and simulated faults do not establish physical power-loss durability, clean-host restore, sustained throughput, language quality, a native no-egress boundary or a supported release. No paid inference, model download or increase to the Actions spending limit is needed. Cumulative external spend remains USD 0 of the authorized USD 20.

## Focused results and repairs

Twenty focused storage fixtures passed before the final lint-driven test decomposition. They cover grant and multi-rule rollback, deferred commit faults, quota savepoints, exact and conflicting replay, policy and complete-action drift, clock regression, midnight allocation, retained failure charges, missed windows, cancellation, populated midpoint catalog snapshot/recovery, immutability, migration rollback and hostile runtime catalog edits. The midpoint snapshot preserves an admitted first entry and a future second entry: recovery interrupts the first without refund and admits the second once. This is distinct from a media-inclusive verified backup.

Three actor fixtures passed. A committed schedule has its supervisor before a subsequent quota-reclamation failure. A SQL failure while settling a failed spawn is returned after the remaining launches are visited. A setup failure already stored by the recording path is recognized only by its exact generation, next revision and canonical failure receipt; it cannot authorize another failure write or conceal a stale capture.

The local control check passed four task tests, including finite grant/cancellation replay without publication, work dispatch or changed allowances. Seven CLI task parser/render tests and the ordinary schedule renderer check passed. The interface preserves bounded source count, whole-second UTC clock, duration/byte limits, separate cancellation generations and sanitized output.

Independent review exposed and repaired duplicated scope validation that omitted policy/action chronology, missing bindings that could fall back to independent authority, same-millisecond cancellation ordering, discarded earlier launches after a later scheduler fault, and swallowed settlement failures. The scheduler now uses one bounded pass transaction and per-rule savepoints, observes quota rollback errors, and returns launches only after commit. The actor dispatches a committed batch before fallible reclamation/retry. Unavoidable process death between commit and spawn still relies on canonical restart recovery.

Failed attempts remain recorded privately. Early hostile-catalog setup was rejected by an existing CHECK constraint; bypassing that constraint is confined to disposable corruption injection. Clock and quota fixtures initially made incorrect assumptions about independent capture and retained media liability. The native fixture initially required `running`, overlooking the valid `starting` state before EOF; its cancellation-before-completion assertion remains. The actor fixture initially inherited a 10,000-byte quota rather than its intended 64 MiB. Corrections preserve production policy and all intended checks. Lint failures require smaller functions, without allowances or removed cases.

The second full verification attempt exposed six existing monitor-coverage fixtures sharing a positional schedule insert. Naming the original columns explicitly preserves independent ownership through the migration default. The failed integrated receipt remains preserved; production insertion already used explicit columns.

## Recording, restore and interface inspection

The focused native fixture passed in 13.93 seconds with FFmpeg 9.0.1. Three bounded loopback requests produced retained audio: a prior independent recording, an admitted task capture cancelled while active, and a simultaneously active independent capture. A separately cancelled future grant made no request. Collection inspection excludes both unrelated recordings. The task monitor retained one admission, 10 planned seconds and 1,048,576 worst-case bytes after cancellation. After stopping the service, canonical backup verification and restore reproduced collection views, reservations and all three retained files byte for byte against the known WAV fixture. This is same-host generated-media evidence, without speech inference.

The actual CLI inspection passed 22 operations in a fresh private library. It registered two offline source revisions, created an original-script task, granted two future once captures, inspected exact JSON identities, replayed, detected a later monitor action, cancelled and replayed cancellation. Catalog-only backup/restore preserved the original grant and cancellation generation. No decoder or model profile was configured, and acquisition and inference were zero. Its 2,986,228 private bytes include the inspection library, catalog snapshot and restored copy.

Single operation samples were 41.35 ms for the grant, 31.47 ms for scope-drift inspection, 38.16 ms for cancellation and 143.92 ms for restore. These uncontrolled development-host samples, collected during compilation, are observations rather than capacity or latency guarantees. Private native and interface logs and JSON receipts remain under `.agents/task-collection/`.
The first integrated coverage attempt failed an existing service-stop client after it printed a valid response, with Windows heap-corruption status. Normal full verification and a focused instrumented reproduction passed. The cause remains unresolved, and the failed log and local Windows receipts are preserved. [Active work](../../docs/development/progress.md#tracked-intermittent-test-issues) tracks this open issue; acceptance conditions and coverage exclusions are unchanged.

## Integrated local results

`cargo verify` passed 736 test executions, with 16 native fixtures reserved for the additional gate. Formatting, warnings-denied Clippy, build and native-source hashes passed. `cargo audit` checked 312 dependencies against 1,279 advisories.

The final complete `cargo verify-coverage` run passed 732 ordinary instrumented test executions and all 16 native fixtures. The native target took 98.62 seconds on this Windows host with FFmpeg 9.0.1. Every workspace crate exceeded the exact-integer 80% line gate. Test sources and build scripts are included, with no source exclusions. These are compiled Windows line measurements, without branch or platform qualification.

| Workspace crate | Covered / executable lines | Line coverage |
| --- | --- | --- |
| `sigy` | 15,373 / 17,704 | 86.83% |
| `sigy-core` | 1,436 / 1,521 | 94.41% |
| `sigy-service` | 43,293 / 46,662 | 92.77% |
| `sigy-test-recognizer` | 257 / 283 | 90.81% |
| `sigy-xtask` | 1,130 / 1,215 | 93.00% |

The JSON receipt is `target/coverage-reports/workspace-9004-1790898460824751900.json`, SHA-256 `00e4217d182ac3716e5a20920b52e3209b683fb8c92b56671fd0053e1ef2ed43`. The successful normal log is `.agents/task-collection/verify.log`, SHA-256 `5a80c1a5afb8d74828739d2a47dff614cc40e9a80b348f943d39c3784f69873f`; the complete coverage log is `.agents/task-collection/coverage.log`, SHA-256 `49ebab18e9fc120346669dd5218545f7ab990935272f660c031276fbc463e73d`.

The client heap-corruption failure remains unresolved. A [subsequent dump investigation](../34-windows-control-shutdown.md) establishes recurrence in an earlier ordinary build and records 37 bounded diagnostic passes without identifying the corrupting write. A complete later gate pass does not repair the defect. The unverified transport draft is set aside at the user's requested stopping point. Failed attempts and local Windows receipts remain private; acceptance conditions and exclusions are unchanged.

Local validation is complete for this increment. Hosted source-only checkpoint publication requires passing exact-commit Windows CI; release metadata records its receipt. Task-owned processing, planning, operational language quality and the other declared qualification gates remain open.
