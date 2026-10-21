# 0098: Completed settlement slots

Date: 2026-10-20. Status: implemented for the live process. Schema v51 and local IPC v53 are unchanged. No dependency and no migration are introduced. This decision does not close S-03B or establish a roadmap stage exit.

## Problem

[Decision0097](0097-unresolved-completion-hold.md) selects an in-memory completed slot and leaves the `running` or `cancelling` job row as the durable hold. It does not choose how many times settlement may be retried, how long a lock wait may last, or when the actor must stop retrying. Without those bounds, a busy catalog can drop the only drained outcome or keep the service awake without a finish.

## Decision

A finished recognition or translation outcome moves into a completed slot before any later fallible settlement step. The slot keeps the committed work token and the drained outcome. It is not a live worker. Pool stop does not signal it. The pool stays occupied while the slot remains, and that occupancy counts against the existing per-family cap together with running workers. Verification keeps its generation check and does not use a slot. An error result from the worker means the drain was not proven. That result does not open a slot. The lease stays and the service stops.

Settlement of one parked outcome may call the existing finish path at most three times. The counter is local to the slot. It does not increment `job.attempt`, write `job_attempts`, start a recognizer or translator, or create another charge. It is separate from `job_pool::MAX_ATTEMPTS`, which remains the restart policy.

Each finish uses the catalog connection's existing five-second `busy_timeout` from `Store::open`. The 10 ms figure on retained-history and task-evidence scans is a query guard, not this writer lock. This decision does not install a shorter busy timeout.

The slot's monotonic deadline is 15 seconds from the moment it is taken: three attempts times that five-second lock wait. The clock is `std::time::Instant`. Tests advance it. A wall-clock jump does not extend the window. The one-second schedule tick can be coalesced or absent during shutdown, so the catalog thread owns the wait while a slot remains. A one-second pause is only the wake slice while an uncertain commit is held. It is not a fourth settlement timer. A busy retry does not insert that pause between attempts.

`SQLITE_BUSY` and `SQLITE_LOCKED`, including extended codes whose primary code is those values, are retryable because the attempt did not commit. Membership is read again before another finish. System I/O failure and `Error::Io` are uncertain. Membership is read again. If the same claim is already terminal, the slot is released as settled. If the row is still `running` or `cancelling`, finish is not called again. The slot stays until the deadline, then it is dropped and the row is left unchanged. A membership read that is itself busy or uncertain is treated as still active. Constraint, disk-full, corrupt, integrity, identity, `stale-worker`, and every other error refuse another publication. The slot is dropped and the original error stops the service. A missing row or a different generation, attempt, or owner is `stale-worker` and is not finished again.

A launch that has committed its claim, then fails profile lookup or spawn, settles the existing not-started or refused outcome through the same slot. If that settlement commits or stays pending, the service does not stop only because the worker process never started. An integrity fault on that settlement still stops the service and leaves the row running for existing recovery. The spawn error is not kept as the job reason once the refusal is held or committed.

Cancellation inside the existing finish transaction wins when a later retry calls finish. A successful finish releases the slot before `schedule`. A later scheduling failure cannot replay that publication. If an unrelated callback stops the service, a pending slot still advances from the catalog loop. Ordinary schedule reconciliation is not the only retry driver.

Exhaustion, either three tries or the 15 second deadline, drops the in-memory slot and leaves the `running` or `cancelling` row. That row is the visible hold. The service does not stop only because busy retries ran out. The unique running index still admits only one `running` or `cancelling` row of that family. Recovery is unchanged. `job_attempts` continues to mean only an attempt that restart ended.

If the process exits while a slot is occupied, the in-memory outcome is gone. The job row is left unchanged. The next start uses the existing recovery policy and does not replay bytes that were never stored.

Language evidence for a successful recognition remains best-effort and runs only after that recognition commit, including a retry whose re-read finds the row already terminal. Translation has no equivalent followup.

## Alternatives

A separate settlement timer, a 10 ms writer lock, and reuse of the restart attempt counter were rejected. The 10 ms guard would fail ordinary settlement under brief writer contention. Reusing `MAX_ATTEMPTS` would mix restart policy with a slot-local retry. Persisting drained transcript or translation bytes was rejected by Decision0097.

## Evidence

Focused settlement tests inject catalog faults and a monotonic clock. They cover busy, locked, and extended primary codes, uncertain I/O, constraint and other refusals, the third attempt, the deadline, a committed recognition launch refusal, a busy refusal that later commits, exhaustion that leaves the running row, uncertain I/O that does not finish again, a constraint fault that drops the slot, a changed claim, cancellation on the committing retry, an unproven drain that opens no slot, and shutdown while an expired slot remains. Translation covers a committed launch refusal and a busy slot that later commits. Outcomes are synthetic. No native recognizer or translator is started. The integrated `cargo verify` receipt belongs in active work. Coverage was not re-run.

## Limits

Process exit still drops an uncommitted completion. S-03B stays open. Language, platform, and acoustic qualification are unchanged. External spend is USD 0.
