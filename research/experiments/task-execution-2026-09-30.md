# Finite task evidence execution validation

Recorded: 2026-09-30. Platform: Windows 11 x86_64, Rust 1.98.1. Status: local integrated verification passed; no operational qualification. The [decision](../../docs/decisions/0067-task-evidence-execution.md) defines the fixed selected-checkpoint publication workflow. [Active work](../../docs/development/progress.md) records final gate results and receipt identities.

## Scope

One explicit delegation accepts a selected frozen checkpoint and at most 64 finding attempts plus one exact-membership briefing. Immutable intent precedes effects. The existing service tick visits at most four pending tasks and advances one catalog effect per visited task. A finding or briefing and its receipt commit in one immediate transaction. Expected publication refusals roll back their effect savepoint and become partial receipts. No acquisition, native worker, model inference, provider request or new ledger is introduced.

This is a real catalog workflow over existing evidence, not a natural-language planner or a task-owned collection pipeline. It grants no MCP task mutation access. Synthetic recognition and translation fixtures establish reference and execution behavior, not language quality or semantic support. [Agentic research](../15-agentic-analysis.md) and [private diagnostics and recovery research](../32-private-diagnostics-and-recovery.md) explain the wider contract and open qualification matrix.

## Evidence cases

- A populated recognized and translated catalog executes directly and after reopening between finding and briefing. Both runs retain the same grant, effect identities, receipts, exact membership and coverage. An unrelated monitor finding stays outside the task briefing.
- A scope with two sources but no capture on the second stays partial. A fully observed one-source publication finishes its finite plan without claiming semantic goal success. A later correction and whole-recording deletion preserve its original finding and briefing history through reopen.
- Cancellation between effects preserves the committed finding, stops the future briefing and leaves independent monitor processing, capture and accounting untouched. Policy drift records revocation before another effect.
- A checkpoint without a translation produces an explicit skipped finding and empty partial briefing, with no translation job. Existing cue and publisher limits stay enforced.
- Injected receipt failure rolls back the briefing, members and coverage. Reopen and retry publish once. A helper snapshot failure similarly leaves no partial header or members.
- Manual effect-ID collisions produce partial receipts without claiming the independent artifact. An artifact independently published after a skipped receipt, including the same millisecond, does not rewrite that outcome or make reopen fail.
- Findings remain readable after a whole-recording deletion and after a released sealed segment. New citations reject a released interval as retained and may record its original as expired. Release fixtures preserve byte accounting and pass whole-DVR audit.
- CLI/IPC cases cover offline admission and cancellation, service start, abrupt death and restart, current generation, exact replay, stale scope and unchanged independent schedules. The empty-evidence process fixture establishes durable admitted or terminal state across restart; it does not establish process death precisely between positive effects.
- Actor fixtures enforce four-task passes, identifier rotation, no unowned work admission and backward-clock holds without stopping the service. Schema migration and corruption fixtures preserve scope and refuse altered grants, plans and inconsistent ownership.

## Review and repairs

Ordinary monitor briefings intentionally collect every monitor finding. A task-specific canonical helper now selects exactly its owned IDs and copies the selected checkpoint coverage; ordinary behavior is unchanged. Historical finding audit formerly treated today's deleting/deleted recording state as corruption of an earlier retained citation. Audit now validates immutable original hash, cue, interval and gap history. Versioned migration adds present-time released-segment checks while keeping earlier immutable publication statements readable.

Review also repaired claimed briefing IDs on failed effects, partial helper writes, pre-existing effect-ID collisions and audit dependence on permanent artifact absence after a refusal. An expected refusal remains a refusal even if a later independent command publishes an artifact under that identity. Successful task-owned receipts still require exact reference, clock, membership and coverage matches. Millisecond timestamps cannot determine ordering between independent commands inside the same millisecond; no ownership is inferred from a skipped receipt.

The first integrated run rejected a retention fixture that directly released its only segment and attempted to reduce published media bytes to zero. The normal file-release path already uses whole-recording deletion for that case. The repaired fixture creates two sealed segments and invokes the real Library release path. A defensive storage guard now refuses direct last-file release before inserting a receipt. A separate filesystem-failure and restart fixture proves that staged whole-recording deletion preserves byte/hash/clock history and releases quota once. Both focused checks and service-wide warnings-denied Clippy passed before the final integrated gate.

## Actual CLI rehearsal

A bounded local library contained a monitor and frozen checkpoint but no recording, transcript, translation or citation. The offline execution command accepted checkpoint 1 with a four-finding ceiling and reported `running` at generation 1, zero planned findings and no briefing. It explicitly stated that offline admission waits for the service to run. Starting the service committed one empty briefing and its receipt. Inspection reported `partial` at generation 2, zero published or skipped findings, and the stored `coverage-partial` reason. Empty evidence did not become semantic success.

The inspected admission and completion output separated immutable checkpoint identity, the publication ceiling, committed artifacts, current generation and evidence limits. The published briefing identifier matched the effect receipt. This rehearsal used no network acquisition, native processing, model invocation or paid request. It is an empty-evidence interface check; populated synthetic fixtures separately exercise positive publication, exact membership and interruption.

| Measurement | Observed value | Limit |
| --- | --- | --- |
| Execution admission wall time | 124.8542 ms | One local CLI sample, including process and catalog work |
| Execution inspection wall time | 143.8502 ms | One local CLI sample; no latency distribution |
| Library files after service stop | 962,560 bytes | Whole rehearsal-library total, not incremental workflow storage |
| Model invocations | 0 | No language or planner quality measurement |
| Paid amount | USD 0.000000 | No provider dispatch |
| CPU time and peak memory | Unmeasured | No capacity or resource qualification |

Private receipts remain under ignored `.agents/task-execution/rehearsal/`. Their SHA-256 identities are:

- `admission.txt`: `6ee38ac32f231fb67c03cd55f794f95702e980ad1bf3f10e131f4efee6ef9c96`.
- `completion.txt`: `810437d6b0cf3e4f1eecfa813cd86aada553ee14378582557ca530e6c90f1a7b`.
- `resources.json`: `4274b6b94c3c3752a7b6c078bc219926616b6e7d015c3158e5768667fa116280`.

The two actual CLI/IPC fixtures passed in a focused Windows run in 5.29 seconds. That run covered offline admission and cancellation with independent schedule and accounting state preserved, plus abrupt restart, empty-evidence partial publication, stale-policy refusal and exact replay. Full workspace verification and coverage passed; their final results belong in [active work](../../docs/development/progress.md).

## Integrated failure evidence

The ordinary workspace gate passed 694 test executions. The first coverage attempt then failed one CLI fixture with Windows I/O error 5 (`PermissionDenied`, Access is denied). Its original error lacked invocation and I/O-stage context, so the cause remains unresolved. The failed log is preserved with SHA-256 `af3abe40e92a897dfc99851ddcf87d588b0e32cb3704b72ca0216dfa01f5bc98`. Diagnostics now identify a fixed safe task subcommand while preserving the original error kind and text. No private goal, identifier or raw argument list is printed. The focused instrumented test passed both cases in 0.46 seconds; the I/O stage and cause remain unresolved. Acceptance conditions are unchanged, with no retry or ignored test added. Final gate evidence and any nonrecurrence do not establish the cause.

## Final integrated gates

`cargo verify` passed 694 test executions, formatting, warnings-denied Clippy, build, native-source hashes and a fresh dependency audit of 312 crates against 1,277 advisories. `cargo verify-coverage` passed the exact-integer 80% gate separately for every workspace crate, without source exclusions. All 15 native fixtures passed in 72.86 seconds on FFmpeg 9.0.1. The instrumented CLI I/O failure did not recur; that does not identify its cause.

| Crate | Covered / executable lines | Line coverage |
| --- | --- | --- |
| `sigy` | 14,808 / 17,112 | 86.53% |
| `sigy-core` | 1,436 / 1,521 | 94.41% |
| `sigy-service` | 40,171 / 43,745 | 91.82% |
| `sigy-test-recognizer` | 257 / 283 | 90.81% |
| `sigy-xtask` | 1,130 / 1,215 | 93.00% |

The private LLVM report is `target/coverage-reports/workspace-25728-1790824872577135400.json`, SHA-256 `b0fc1fde6ab2ce5a8c92d79cc70730cc335b428dec6b6df6fd93d45ce204bd29`. The final verification log has SHA-256 `0168a60c7b6a79b9d3a9c26a59429fd915846301e1f995fce9111942ebcf5e0f`; the coverage log has SHA-256 `44ceac6be23483bf26d7eee2184af6e881cf972b54215015b996f0d40873b14a`. Logs remain under ignored `.agents/task-execution/`. Counts describe compiled Windows lines, including tests and build scripts; they do not establish branch coverage, platform support or semantic task competence.

## Remaining evidence

Physical power loss, disk-pressure operation, clean-host restoration, sustained capacity, OS network isolation, native containment and general model task competence remain separate gates. SQLite rollback and catalog reopen are narrower than those guarantees. A verified media backup/restore midway through a populated publication run and abrupt process death precisely between positive effects still require separate evidence. The publication template uses already available catalog evidence and does not qualify general agent delegation or end-to-end task-owned capture and processing. External spend and commitments for this increment are USD 0.
