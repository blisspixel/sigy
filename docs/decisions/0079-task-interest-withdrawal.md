# Explicit task interest withdrawal

Date: 2026-10-03. Status: implemented increment; focused verification is recorded below. Full integration gates and platform qualification remain separate. Catalog schema advances from v44 to v45 and local IPC from v45 to v46.

## Separate authority and compatible history

`task cancel-processing` preserves its original admission-only payload, hash and receipt. It stops future task admissions while admitted jobs continue. Migration does not withdraw those jobs or reinterpret an old cancellation replay.

The new explicit operation is:

```text
sigy task withdraw-processing TASK REQUEST --expected-processing-generation 1 --expected-withdrawal-generation 0
sigy task withdrawal TASK
```

The processing generation remains its existing 1 or 2. Withdrawal has a separate versioned intent and generation 1, admitted with expected withdrawal generation 0. An already cancelled legacy grant can explicitly withdraw with expected processing generation 2. Identical request and parameters replay the stored decisions, regardless of later clock or policy drift. Changed parameters conflict. Withdrawal leaves lifetime audio charges consumed and grants no replacement work, publication or collection cancellation.

In one immediate catalog transaction the service validates the stored processing grant and cancellation, freezes at most four queued processing receipts, records the task's exact interest withdrawals and determines remaining canonical authority. The new receipt independently fences future processing admission. Each decision names the canonical family/job and observed generation. Existing admission interests remain immutable. Inspection distinguishes historical task interests from withdrawn task interests.

Direct and monitor interests preserve shared jobs. Existing monitor pause and administrative whole-job analysis cancellation retain their separate meaning. Direct interests identify one authority class, not separately cancellable callers. Two tasks cannot currently share one physical job through task-owned collection: their unique occurrences and exact recording bindings differ. That case belongs to future scoped-sharing conformance, not fabricated production fixtures.

## Remaining authority and ended jobs

Remaining-owner lookup uses indexed legacy, direct and monitor existence probes. It examines at most 64 task-owner rows with exact withdrawal probes; a 65th row refuses the transaction with `remaining-authority-unproven`. No partial withdrawal, cancellation or receipt commits after an incomplete authority decision. This bounds supported query shapes and row work; it is not a hard filesystem or SQLite instruction deadline.

Migration adds immutable guards for jobs with migrated interest provenance. Missing historic direct provenance is not proof that no direct authority existed. Guarded jobs remain protected. No guard is inferred from a task's desired cancellation.

An unshared queued job ends without launch. An unshared running job enters cancelling at its existing generation. Successful completed results remain readable and can still be reused through exact fresh admission. New attachment cannot revive cancelling, cancelled, failed or interrupted work. Historical request replay remains readable. A new computation would require separately accepted attempt authority.

## Durable stop and physical completion

Before signalling an unshared native worker, the transaction stores an exact stop target binding family, job, generation, attempt, lease owner and task withdrawal. Response delivery is unnecessary: the actor also reconciles these targets against its bounded active-worker maps on scheduling passes.

Worker signalling requires a typed verification, recognition or translation family as well as job ID and generation, including administrative cancellation. Identical IDs or generations in another worker map cannot redirect cancellation to independent work.

Finalization requires the existing recognition or translation drained capability. Its transaction verifies the target against the canonical job's generation, attempt and lease owner, then stores an immutable completion receipt together with the cancelled job state. A successful callback arriving after committed cancellation cannot publish success. An earlier committed successful result is preserved. A stale attempt cannot complete another target or release its resources.

Completion receipts preserve the observed wall clock separately from the effective lifecycle timestamp. Effective completion is the maximum of the observed clock, job creation, attempt start and exact stop-target creation; the cancelled job's finished timestamp uses that same value. A backward clock therefore does not discard an already proven drain. This ordering timestamp does not establish elapsed duration or a corrected wall clock. The insert trigger and reopen audit enforce the binding. Storage failure rolls back both completion and finalization, allowing the storage caller to retry the same borrowed proof. The actor currently stops on a completion-commit failure and does not retain that proof across restart, so its conservative unknown-completion hold remains a limitation.

For targets created by this new operation, restart without a committed cleanup receipt preserves cancelling state, generation and read lease. It does not infer an empty group from owner death, missing process memory or Windows kill-on-close. Global scratch cleanup and new native recognition/translation claims remain held while any target has unproven completion. Doctor and withdrawal inspection expose that condition. Capture, control, inspection and independent verification remain available.

The global hold check probes existing queue indexes over cancelling jobs, with the existing ceiling of 1,024 open jobs per table, and exact target/completion keys. Completed immutable stop history does not expand each scheduling pass. Library-open integrity audits still inspect retained history.

There is intentionally no guessed-PID signalling, manual cleanup-proof fabrication or automatic post-crash release. The current process-group interface cannot reconstruct a proven empty group after owner death. Restoring a library with unresolved targets therefore retains the hold. A separately qualified recovery capability is needed to clear it safely. Older untracked native recovery retains its prior behavior; this increment does not claim that all historical recovery has gained the new proof contract.

## Focused acceptance

Storage fixtures cover queued cancellation, direct-first/task and monitor/task sharing, legacy cancellation replay, exact request conflict, charge preservation, failed attachment, successful result reuse, translation late success, immutable hostile edits, populated migration guards, receipt rollback and completion-commit failure. Reopen preserves unresolved generation/lease ownership and rejects invalid receipt bindings. Existing processing migration/backup fixtures remain applicable.

The Windows native fixture starts a deterministic bounded child under a real contained group, observes it running, withdraws through the catalog, signals through the actor's durable reconciliation path and accepts the actual empty-group snapshot through the existing executor envelope and recognition completion capability. It checks cancelled state, the completion receipt and worker-cost observation. It exercises native lifecycle and service finalization, not a recognizer model or language quality. Separate catalog completion fixtures explicitly use synthetic outcomes.

Focused Windows checks on 2026-10-03 passed:

| Command | Outcome |
| --- | --- |
| `cargo test -p sigy-service storage::tasks::processing::tests --lib -- --test-threads=2` | 31 passed |
| `cargo test -p sigy-service withdrawal --lib -- --test-threads=2` | 18 passed after the clock fix, including actual contained termination, both families' regressed-clock rollback/retry/reopen checks, the separate future-owner reference query and a populated snapshot migration fixture |
| `cargo test -p sigy-service family_and_generation_select_only_the_exact_colliding_worker --lib -- --test-threads=2` | 1 passed; all three maps share an ID/generation and unrelated stop flags remain false |
| `cargo test -p sigy-service task_processing --lib -- --test-threads=2` | 5 passed, including the real control-server restart hold |
| `cargo test -p sigy-service atomic_monitors --lib -- --test-threads=2` | 11 passed |
| `cargo test -p sigy task:: -- --test-threads=2` | 11 passed |
| `cargo clippy -p sigy-service -p sigy --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |

The filters overlap; their counts are not distinct totals. Full `cargo verify`, coverage, media gates, CI and integration receipts belong to active work and remain required before a qualified checkpoint.

Integrated verification repaired the intermediate snapshot/control lint findings without allowances. The final combined tree passes `cargo verify`, including warnings-denied Clippy; current full-gate receipts belong to active work.
