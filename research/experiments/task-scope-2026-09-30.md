# Durable task scope and checkpoint validation

Recorded: 2026-09-30. Platform: Windows 11 x86_64, Rust 1.98.1. Status: local implementation evidence for catalog and IPC v41. The [decision](../../docs/decisions/0066-durable-task-workflows.md) defines this bounded observation slice; it does not implement a planner, task executor, A2A or provider transport. [Active work](../../docs/development/progress.md) records the final verification and coverage receipts.

## Contract and references

Accepted task scope binds a bounded original-script goal, an existing monitor version and all stored action ordinals, a finite capture-start window, the fixed `monitor-observation-v1` template and zero paid allowance. Service-generated checkpoints freeze coverage and deduplicated recording/transcript/translation references with their cue clocks. Existing monitor authority, worker admission, acquisition and accounting are unchanged.

[Agentic research](../15-agentic-analysis.md) motivates canonical service state, task-specific capability evaluation and separate protocol adapters. [Privacy and recovery research](../32-private-diagnostics-and-recovery.md) connects data minimization, SQLite durability assumptions, effect reconciliation and backup qualification to the acceptance matrix. Neither research record establishes supported model or platform profiles.

## Verification evidence

The final `cargo verify` passed 659 test executions, with 15 native fixtures left to the separate native/coverage gate. Formatting, warnings-denied Clippy, native-source hashes, build and a fresh dependency audit passed. The audit inspected 312 dependency entries against 1,277 advisories. The private verification log is `.agents/task-scope/final-verify.log`, SHA-256 `d23c30e56fa8c1958d0607d32f59fc71bd256a50655a2d1a275739f8e2eb1e69`.

The final `cargo verify-coverage` also passed, including all 15 native fixtures on one thread in 63.57 seconds. It included test sources and build scripts with no source exclusions. Per-crate covered/executable lines were `sigy` 14,280/16,553 (86.26%), `sigy-core` 1,436/1,521 (94.41%), `sigy-service` 37,904/41,516 (91.29%), `sigy-test-recognizer` 257/283 (90.81%) and `sigy-xtask` 1,130/1,215 (93.00%). Every crate passed the exact-integer 80% gate. The JSON receipt is `target/coverage-reports/workspace-4632-1790812369080694300.json`, SHA-256 `bc8cae7549a2c3ffb59895aa7fb8dd6300901a1cffbac6d5e02b208a2d8ac584`; its private command log is `.agents/task-scope/final-coverage.log`. These are Windows line measurements, not branch coverage, model quality or platform qualification.

The added fixtures check:

- exact and changed creation replay, missing/stale monitor policy, the complete action prefix and backward clocks;
- service-owned observations, expected checkpoint ordinals, changed request replay and policy drift without history replacement;
- 256-task and 128-checkpoint admission bounds, 16-identity pagination, SQL immutability, altered bytes and inconsistent snapshots despite recomputed checksums;
- populated v40 migration, conflicting-DDL rollback and checkpoint order after physical row IDs change, preserving the original immutability trigger in the snapshot;
- actual CLI/IPC clients, abrupt service death and restart, verified offline backup/restore and unchanged capture, processing and budget state;
- citation history over synthetic recognized original text, untranslated and translated revisions, a later transcript correction and deleting media, including successful catalog reopen;
- control-bearing and oversized goals, malformed windows, safe rendering and absence of goal text in ordinary request debug formatting.

Reproduce the focused cases with `cargo test --locked --package sigy-service storage::tasks -- --test-threads=2`, `cargo test --locked --package sigy-service task_checkpoints_preserve_cited_revisions -- --test-threads=2`, `cargo test --locked --package sigy --test service task_fixtures -- --test-threads=2` and `cargo test --locked --package sigy --test tasks_cli -- --test-threads=2`. The canonical complete entry point remains `cargo verify`; `cargo verify-coverage` enforces coverage and includes the native fixtures. These are implemented commands, not proposed CLI examples.

## Review and repairs

Review found that a regressed clock could create scope older than its accepted actions, making later inspection reject the newly admitted row. Admission now checks that chronology before persistence and the SQL trigger repeats it. Full audit reconstructs each task's historical policy and source context once rather than replaying action history for every checkpoint and citation. Current-scope checks use scalar version/action queries instead of unrelated processing aggregates.

Two earlier migration fixtures retained the new task tables while reconstructing older schemas. They now share the canonical monitor-dependent schema reset helper; production migration and integrity checks were not relaxed. The new citation-history fixture initially reused coverage-only data whose artificial gap overlapped published audio and whose schedule clock did not describe its capture. It now starts from a valid recognized catalog, proves that catalog reopens before task admission, and preserves the correction, translation and retention assertions. The original coverage-only tests remain unchanged.

## Rendered inspection and small resource sample

An actual CLI rehearsal registered two inert `unresolved.invalid` source revisions and an existing monitor, then created, checkpointed and inspected a task with goal `تابع المياه / suivre les eaux`. Registration and inspection performed no source acquisition. No recording, native worker or paid request was admitted. The displayed goal retained its original scripts. Output showed zero observed captures, window elapsed, one of 128 checkpoints, fixed zero paid allowance, coverage for both sources and unmeasured semantic completion. Historical checkpoint and listing output were inspected as well.

The private rehearsal is `.agents/task-scope/sample-ef5eeb624fb14c168e9cdfada070317d/`. Its binary SHA-256 is `dbd783e6f34cdceeb8486ad13c9f85cdc20b7da46d2cde882311d37160418621`. One sample during verification measured 71.071 ms for creation, 65.3276 ms for checkpointing and 70.8209 ms for inspection, including the CLI process and offline catalog open. Top-level library file allocation remained 913,408 bytes before and after, with existing pages reused. That allocation observation does not establish zero logical storage cost. CPU, memory, large-history reopen cost and simultaneous capacity are unmeasured. This is an interface and small workload observation, not a performance profile or threshold.

## Limits and next evidence

Checkpoint references do not establish currently retained audio, semantic support, independent corroboration, task execution or independently verifiable report bundles. The fixture text is synthetic and establishes no language or model quality. History is bounded and retained; no task-history deletion API exists. A stored goal cannot dispatch work or expand authority. No task mutation is exposed through MCP.

Next, bind narrowly delegated execution identities and reconcile a finite two-source workflow through existing service operations. Compare interrupted and uninterrupted effects and produce truthful partial outcomes. Local-model planning requires its own capability and task-competence evaluation. Full diagnostic canaries, real no-egress boundaries, host capacity, clean-host restore, physical power loss and unattended profiles remain separate gates. External spend for this increment is USD 0; cumulative settled spend and commitments remain USD 0 of the authorized USD 20.
