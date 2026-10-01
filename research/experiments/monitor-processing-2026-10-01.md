# Atomic monitor processing validation

Recorded: 2026-10-01. Platform: Windows x86_64, Rust 1.98.1. Status: focused fixtures, final integrated verification, coverage, all 15 native fixtures and actual CLI inspection passed. The [decision](../../docs/decisions/0068-atomic-monitor-processing.md) defines the admission contract. Synthetic evidence establishes admission behavior without operational qualification.

## Scope and baseline

The reviewed baseline is [monitor processing](../../docs/decisions/0048-monitor-processing.md), using the canonical analysis pin, recognition, translation and durable job-pool paths. Catalog and local IPC are version 42. The planner reads at most 16 candidates and 16 recognized results, plans at most four steps per monitor, and reconciles at most every five seconds. Existing capture reservations remain independent.

Recognition or translation previously committed its queued job and could schedule a worker before the monitor step was appended. A later cap, policy or storage refusal could leave the job admitted without its monitor receipt. The existing step replay compared decision, reason and job identity but omitted policy version, analysis identity and charged audio. These are source-review findings, not claims of an observed production incident.

The selected repair makes canonical job admission, monitor receipt and exact processing charge one transaction. Analysis pin preparation and publication stay separate metadata transactions. A refused processing admission may therefore leave a published pin, while leaving no new job, queued receipt, charge, lease or worker. No task-owned collection, scoped job cancellation, general planner or provider transport is introduced.

## Review invariants

- Exact canonical job replay remains before current input and queue-bound checks and returns without redispatch. Monitor replay also compares the complete stored step. An already terminal shared job does not restart when another monitor records its own exact reference.
- Fresh admission checks version, current action count, pause and current followed sources inside the transaction. Its clock comparison covers the latest monitor policy, action and step timestamps, not every recording, pin or profile timestamp. The stored step has no historical action-count field, so historical replay cannot assert one.
- Recognition charges the stored recording's decoded microseconds exactly once to that monitor on the admission's UTC day and to its lifetime total across all versions. Translation charges zero audio. Worker failure does not refund the charge.
- Fresh proposed duration, source, profile, candidate, recognition parent and translation revision must match canonical state. Translation binds to the exact job cited by the monitor's recognition step; an independent later recognition or correction cannot replace that result. A model or transcript cannot choose permissions, alter a cap or supply an admission receipt.
- Job and step insertion share one transaction. No scheduling, work token or worker may precede commit. Pin metadata prepared earlier does not authorize work.
- Queue-full, backward clock, stale monitor facts and insufficient remaining caps hold admission without a skipped receipt. Storage faults remain faults. A scheduling failure after commit preserves the queued job and charged receipt for recovery.
- Each monitor charges its own limits when reusing an exact shared job. No new exclusive job ownership or cancellation interest is inferred. Capture, independent schedules, job leases and native containment retain their existing contracts.
- A fresh monitor receipt over an existing exact job checks fresh monitor authority but preserves canonical job replay semantics. It creates no worker and does not revalidate that historical job's input or parent as a new job. Queued work still passes canonical input validation when claimed.
- Human-readable processing counters describe admitted audio and historical recognition or translation admissions. They are not current queue depth or successful processing; existing JSON names remain compatible.
- The standalone production refusal writer rejects queued outcomes. Only atomic job admission can write a new queued monitor step.

The canonical enqueue helpers must preserve direct analysis command behavior while allowing a caller-owned transaction. Recognition retains exact request replay, retained-input preparation, current-input and parent checks and queue admission. Translation retains exact request replay, recognized-text and revision checks and queue admission. These paths must not introduce nested transactions or dispatch while preparing an immutable request.

## Review repairs

Independent source review found that the initial translation eligibility query could accept a later independent recognition or correction sharing the same analysis and recording. It now binds the transcript's job identity to the exact recognition step and requires the recognition kind. This preserves the existing planner's selected result.

Review also found that generic refusal classification would turn a temporary monitor-clock or stale-authority condition into a permanent skip. The actor now holds those named conditions and retries from fresh facts. The production standalone writer accepts only skips; queued outcomes require atomic admission. Scheduling occurs after commit and outside refusal classification, so a later scheduling fault cannot rewrite the charged receipt as a skipped outcome.

Source comparison found no changes to capture reservation, scheduler or DVR mutation code. Passed actor fixtures inspect unchanged budget snapshots around aborted processing admission. That covers accounting separation under those faults; simultaneous live capture and pause behavior are narrower source-review claims until exercised separately.

The new actor fixtures claim jobs through the real pool and supervise worker completion using absent runtime assets. They test scheduling order, durable leases and cleanup, without starting a native recognizer or translator executable. Synthetic published recognition supplies translation input. These checks cannot prove model quality or native isolation.

## Focused evidence matrix

| Case | Required observation | Status |
| --- | --- | --- |
| Fresh recognition and translation | Matching job and monitor step commit together; exact charge is readable | Passed |
| Job, step and deferred-commit faults | Both new rows and usage roll back; published pin remains; reopen and retry admit once | Passed |
| Historical replay after action drift, deletion admission and UTC rollover | Original request and charge remain unchanged; no dispatch | Passed; exact replay after policy-version revision is not a separate focused case |
| Changed replay | Changed policy version, analysis, audio, hash, parent or job identity conflicts | Passed |
| Queued and completed shared jobs | Two monitors store separate charges while retaining one job, completion and attempt | Passed, including reopen |
| Failed shared attachment | Second monitor receipt rolls back without changing existing direct job or first charge | Passed |
| Fresh stale snapshot | Policy revision, action drift, pause or unfollowed source refuses admission before job insertion | Passed |
| Wrong duration or profile hash | Refusal leaves no new job or charge | Passed; other candidate changes remain source-reviewed |
| Clock bounds | Earlier monitor clock holds fresh admission; exact replay remains readable | Passed in storage and actor fixtures |
| Caps and queue pressure | UTC daily and all-version lifetime caps hold; full queue writes no step; exact existing job remains attachable | Passed; retry succeeds after capacity becomes available |
| Translation lineage | Independent later recognition and correction cannot replace the monitor's selected result | Passed |
| Direct analysis replay | Exact queued recognition and translation replay never schedules work | Passed; existing input and parent regressions also pass the workspace gate |
| Actor dispatch boundary | Receipt abort starts no worker; committed new job is claimed and supervises completion | Passed with absent runtime assets |
| Catalog reopen | Committed pair survives; rolled-back pair is absent; retry and accounting remain exact | Passed; abrupt service death at this boundary is untested |
| Capture independence | Processing faults preserve budget snapshots and change no capture source paths | Passed snapshots and source review; simultaneous live capture and pause remain separate evidence |
| Production write boundary | Standalone refusal writer rejects a queued step | Passed |

The fault fixtures inject job insertion failure, monitor step insertion failure and a deferred foreign-key failure at commit, then inspect jobs, steps, attempts, charges and the surviving metadata pin. Reopen and retry establish transaction recovery. Shared-job cases cover queued and succeeded jobs and a failed second-monitor attachment. Clock cases distinguish unchanged historical replay from fresh admission holds.

## Focused results

`cargo test -p sigy-service atomic_monitors -- --test-threads=2` passed all 11 fixtures after 17.73 seconds of compilation and 6.25 seconds of tests. The first run passed ten fixtures and failed a new terminal-job fixture because it incorrectly expected the interrupted-attempt table to contain a completed attempt. The fixture now checks the completed job's attempt number is one and interrupted-attempt rows remain zero. The corrected assertion matches the existing job-history contract; no production behavior was changed for that failure.

`cargo test -p sigy-service monitor_admission_tests -- --test-threads=2` passed all five fixtures, with no failures or ignored tests, in 0.61 seconds of tests. Both receipt-abort cases leave no new stage job, step, lease or supervised worker; removing the injected fault permits one claim and completion. Other fixtures cover stale-action and clock holds, direct queued replay and monitor history after pause and clock drift. No native recognizer or translator executable was started by these new fixtures.

`cargo clippy -p sigy-service --all-targets -- -D warnings` passed without warnings in 27.45 seconds. No schema or dependency changes were introduced. Focused outputs were not saved as separate log artifacts; no focused-log hash is claimed.

## Integrated checks and measurements

`cargo verify` passed 710 test executions, with 15 native-media tests ignored by that ordinary gate, formatting, warnings-denied workspace Clippy, build, native-source hashes and a fresh advisory audit of 312 dependencies against 1,278 advisories. The first attempt stopped at Rust module ordering; canonical formatting repaired it before the final gate. Its failed log remains private. The passed log is `.agents/monitor-admission/verify.log`, SHA-256 `891dab2fac9dc6ae7d47d0316dfb3fb4afd89c6630d1e8b84bb8deced9f6894c`.

An actual CLI inspection initialized a fresh private library, started the service, registered one source configuration without acquiring it, created a bounded monitor, read human and JSON status, paused processing, read again and stopped the service. No profile, schedule, capture or inference was configured. Human output displayed `Admitted audio` and historical recognition and translation admissions; JSON counter names remained compatible. Monitor creation took 23.71 ms and human inspection 21.85 ms in this single empty-library sample. The stopped library occupied 962,560 bytes. These are interface samples, not processing-admission latency, storage-growth or sustained-capacity measurements. The private receipt is `.agents/monitor-admission/inspection.json`, SHA-256 `c20af13cf856b32ce88948defda5b554a378954686b190223eccb9462c4fb17d`.

`cargo verify-coverage` passed every workspace crate's exact-integer 80% line gate without source exclusions and all 15 native fixtures in 65.55 seconds on the local FFmpeg 9.0.1 build. Service line coverage is 41,485 / 44,850 (92.49%); other crate counts are in active work. The JSON report is `target/coverage-reports/workspace-24656-1790864071544543400.json`, SHA-256 `d82bc6e08fb57978517976bc2c876e12bbdc6b80b4f7cbe3ed41b1114981270a`. The passed coverage log is `.agents/monitor-admission/coverage.log`, SHA-256 `8415d104aa641097b2792111b3d2e2a794cba3200a37a71b2be2f6c0e339fa9b`. Existing native fixtures include owned capture through recognition and translation and stalled clients during publication. They remain local Windows evidence rather than a qualified support matrix. Documentation link and whitespace checks pass; no register or schema change is needed.

CPU time, peak memory, admission latency, storage growth and sustained capacity are unmeasured for this increment. The source-level bounds remain admission and reconciliation limits rather than measured resource guarantees. External spend and commitments for this work are USD 0.

## Remaining qualification

The [private diagnostics and recovery review](../32-private-diagnostics-and-recovery.md) links the primary SQLite persistence references and distinguishes process termination, transaction failure, power loss and clean-host restoration. This repair does not add diagnostics uploads or automatically transmit catalog contents. Physical power-loss recovery, disk-pressure operation, sustained capacity, clean-host restore, native OS network isolation and supported-platform evidence remain separate gates.

Language quality and general agent task competence remain unqualified. The monitor controller continues deterministic bounded processing under stored user policy. Its admission receipt establishes authorized accounting and durable work state, not accuracy of the resulting transcript or translation. [Active work](../../docs/development/progress.md) is the home for final integrated receipts and current operation status.
