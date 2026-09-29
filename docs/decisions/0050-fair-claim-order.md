# Fair claim order

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC stay v34. This is the claim-order part of roadmap operation 28 and increment 5 of the [scaling architecture](../design/scaling-architecture.md#increments). It amends the oldest-first claim in the [task contract and job pool](0043-task-contract-and-job-pool.md). Host budgets, measured capacity, and a stored live deadline remain open. No language is qualified.

## Decision

One local slot of each kind still runs at a time. The service chooses the next queued job whose transcript lineage is idle.

Claims rotate across source revisions. Inside one source, the oldest queued job goes first. Every fourth claim is reserved for the oldest waiting batch job, even when rotation would serve a busier source. A source that keeps enqueueing work cannot take that reserved claim.

The chooser also knows a live class. Live jobs, when any are waiting, take the three claims that are not reserved, and each source offers its earliest deadline. The catalog does not store a live deadline yet, so every job the service loads is batch and the deadline equals its enqueue time. Restart drops the rotation cursor. The next claim is the oldest eligible job, and rotation starts again from there.

Capture does not wait on this queue. A full queue still refuses a new job as `queue-full` and leaves the recording in place. The queue does not yet report that it cannot catch up, because that report needs a measured rate. Caps stay one verification, one recognition, and one translation. No paid request is created.

## Evidence

Chooser tests cover a fast live source that receives three claims and then yields the fourth to an older batch job, one source that stays in arrival order, three sources that rotate, and a reserved turn that keeps an old job when rotation would pass it. The existing pool tests still admit more than 300 jobs, and a fresh cursor still returns the oldest eligible job.

On 2026-09-29, `cargo verify` passed 473 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder and acquisition path are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix.

## Limitations

- No catalog job is marked live. A monitored station and a backlog share the same rotation until a deadline is stored.
- The rotation cursor is memory in the service process. A restart begins again at the oldest job.
- Caps stay one per kind. Host CPU, memory, and GPU budgets are not applied.
- Per-profile cost is not measured here, and coverage does not yet say the queue cannot catch up.
- The one-in-four share is the bound this increment enforces. It is not a measured capacity result.
