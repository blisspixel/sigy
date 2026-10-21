# 0097: Unresolved completion hold

Date: 2026-10-20. Status: selected representation. This decision does not implement the slot. [Decision0098](0098-completed-settlement-slots.md) implements completed slots, classified retry and the shutdown driver. Catalog v51 and local IPC v53 stay unchanged. No dependency and no migration are introduced.

## Problem

[Decision0092](0092-atomic-worker-responses.md) reads a recognition or translation result inside its publication transaction. Its 2026-10-20 amendment binds finish to the committed attempt and claim owner. The actor still removes the worker before it reads the clock or settles. The drained outcome then exists only in that call. A settlement failure drops it. The pool reports idle when its worker maps are empty, so shutdown can leave while that outcome is uncommitted. Process death cannot reconstruct those bytes.

A new table of outcome bytes would have to rebuild `ReapedLocalAsr` or `TranslationOutcome` without the executor envelope that is allowed to construct them. Catalog bytes are not containment evidence. A second payload would also be another uncertain commit beside publication. `job_attempts` records only an attempt that restart ended, with outcome `interrupted` and reason `service-restarted`, and its primary key is one row per family, job and attempt. It cannot store a still-open completion.

## Decision

The unresolved hold has two parts. Only the in-memory part is new, and this decision does not implement it.

While this service process is alive, a finished recognition or translation outcome waits in a completed slot owned by the analysis actor. The slot keeps the committed work token and the drained outcome. It is not a running worker. Shutdown does not signal it as a live process, and it starts no recognizer or translator. Recognition and translation keep their existing per-family cap. The default cap is one running job of each kind. Running workers and completed slots of that family occupy the cap together, so an unsettled result cannot be dropped to admit another claim. The pool stays non-empty while a worker or a completed slot remains. Verification keeps its current map and does not use this slot. The slot is released only after the existing finish path commits. A later scheduling failure stays separate from that publication and cannot replay it.

The durable record is the job row that is already `running` or `cancelling`, with its generation, attempt and lease owner. Original-reader protection already follows that state. An unproven native stop already stays held across restart. This decision adds no column, table, outcome code or trigger.

If the process exits with a completed slot still occupied, the in-memory outcome is gone. The job row is left unchanged. The next start uses the existing recovery policy. It does not gain a new requeue path, and it does not replay bytes that were never stored. `job_attempts` continues to mean only an attempt that restart ended. Schema v51 needs no populated migration.

Retry attempt count, lock wait and monotonic deadline are unselected in this decision. [Decision0098](0098-completed-settlement-slots.md) selects three slot-local tries, the existing five-second catalog lock wait and a 15 second monotonic deadline, and implements classified retry, exhaustion and the shutdown driver. This decision does not enable those behaviors itself. It does not close S-03B or establish a roadmap stage exit.

## Alternatives

Storing the drained transcript or translation in a new hold table was rejected. Reconstructing a reaped result from that table would treat catalog bytes as containment evidence. A hash-only marker would not preserve the outcome and would still be a schema change. Widening `job_attempts` would collide with the restart row for the same attempt and would change a table whose checks admit only `interrupted`.

## Evidence

This is a source decision. The claim-binding tests and hosted verify of `dded2b6` cover the token check, not slots: [run 39509022917](https://github.com/blisspixel/sigy/actions/runs/39509022917). No new runtime test is added here.

The inspected contracts are `Pool::running`, `Pool::is_empty` and `PoolCaps::default`; `finish_recognition` and `finish_translation`, which remove the worker before the observed clock and finish; `recover`, which leaves an unproven native stop held and otherwise writes `job_attempts` before requeue or interrupt; the `job_attempts` checks; and `ReapedLocalAsr::from_envelope` and `TranslationOutcome::from_envelope`.

## Limits

This decision selects no retry numbers. Decision0098 selects them and implements the live slot. Process exit still drops an uncommitted completion. Language, platform and acoustic qualification are unchanged. External spend is USD 0.
