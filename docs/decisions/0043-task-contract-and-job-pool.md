# Task contract and durable job pool

Date: 2026-09-24. Status: implemented and tested on Windows x86_64 (catalog and local IPC v31). This record implements increments 1 and 2 of the [scaling architecture](../design/scaling-architecture.md). It changes how local verification, recognition and translation jobs are admitted and recovered; it does not change what they compute.

## Task contract

Recognition and translation now run through one contract in `sigy-service::execution`, which has no catalog dependency.

- A **task spec** carries the task ID, generation, kind, engine, fixed template, profile hash, inputs, assets, parameters and limits, plus a SHA-256 over its canonical form: compact JSON of every other field in declaration order. Inputs are content-addressed: a retained interval as `{sha256, bytes, media_type}`, or cue text inline with its hash. Assets are the runtime directory manifest, the model and the speech-activity model, each by hash, size and file count. Media types are Sigy tokens such as `audio-wav`, not MIME strings. A spec is refused when its hash does not match, an inline text hash is wrong, or a name or template contains a path separator, `://` or a control character.
- A **local stage** maps those hashes to files that already exist: the retained library object and the profile files. The spec itself names no source URL, library path, object key, catalog handle, secret or environment variable.
- The **local process executor** runs a spec and returns a **result envelope** bound to the task ID, generation and spec hash, with the typed output, its hash, the executor class (`local-process-v1` on `cpu`) and its **containment**. The only containment today is `Drained`: every started process group reported no active member, or no process started. Only the execution module can construct that proof, so no other code can produce a completion. The service accepts an envelope only for the exact job generation and spec it sent, with a matching output hash and the local executor class.

Behavior is unchanged: the same hashing, decoding, process limits, cancellation, deadlines, parsing and reasons. A golden test pins one serialized recognition spec and its hash, which is also checked against an independent SHA-256 of the same text. Another test serializes recognition and translation specs built from a temporary library and asserts that none contains `://`, a path separator, the library directory, the object key or any environment-variable name of the test process.

## Job pool

Catalog migration 031 rebuilds `analysis_jobs` and `translation_jobs` from their stored v30 definitions with checked text replacements, adds `lineage`, `attempt`, `lease_owner`, `lease_expires_ms` and `started_ms`, and adds an immutable `job_attempts` table. The whole migration runs in the open transaction; a failure leaves the catalog on v30.

- **Admission enqueues.** `analysis verify`, `analysis transcribe` and `analysis translate` insert a `queued` job after the same request checks as before; they no longer fail as `worker-busy`. Exact replay returns the stored job, and a changed request under the same ID is refused.
- **Scheduling.** The service claims the oldest queued job of each kind whose transcript lineage has no active job, up to a per-kind cap (one verification, one recognition and one translation at a time on this host; not yet configurable). A unique index allows one running or cancelling job per lineage: the analysis input for verification and recognition, the transcript for translation. A claim rechecks the input, transcript parent or recognized text and fails the job with a reason when it moved on; only a committed claim returns a work token. Capture workers never wait on the pool.
- **Bounds.** The lifetime cap of 256 job rows per table and its triggers are removed. At most 1024 queued, running and cancelling jobs may exist per table; terminal history is unbounded and immutable. The generation bound stays 64, because a generation can only reach the attempt limit plus one.
- **Leases.** A claim records the owning service process and an expiry equal to the task's own deadlines plus ten minutes. The expiry is informational; nothing reaps a live lease while the service runs. A queued job holds no read lease, so its recording can still be deleted; the claim then fails the job as `input-no-longer-current`. A running or cancelling job keeps the durable read lease as before.
- **Restart.** Every lease of the previous process is ended. Each ended attempt is recorded in `job_attempts`. A running job returns to `queued` with generation and attempt increased by one, up to three attempts; a cancelling job, or one at its limit, becomes `interrupted` with a new generation. A late completion of the old generation is refused as stale. These tables hold only zero-cost local work; paid provider attempts keep their own reservations and are never requeued. Native recognition and translation are requeued only where the platform ends the dead service's process groups: on Windows the Job Object's kill-on-close does this, and the media test observes it. On Linux, where cgroup teardown is not yet tested, a restart still interrupts native work.
- **Cancellation.** A queued job is cancelled at once. A running job becomes `cancelling`, keeps its lease until its worker stops, and ends `cancelled`; cancellation still wins over a queued result.

SQL triggers enforce the transitions: claim from `queued` only, requeue only with a recorded attempt, a new generation and a cleared lease, and no change to a terminal row. Local IPC v31 adds `attempt` and `started_ms` to job views. `analysis job` shows the attempt and explains a queued state.

## Evidence

Unit tests cover the spec, envelope binding, executor refusal of tampered specs, more than 300 jobs admitted over time, the open-work bound in Rust and SQL, attempts up to the limit, SQL refusal of invalid transitions and terminal edits, a populated v30 catalog whose rows survive migration and whose running row gains a lease, a test downgrade that reproduces the genuine v30 schema, and an interrupted migration that leaves v30 unchanged. An actor test runs five queued verifications one at a time in admission order. The native-media fixture drives the real CLI and service: five recognitions admitted at once publish in order with at most one running, a second capture completes while a recognizer runs, and a killed service's recognizer does not outlive it, after which the job is requeued under generation 2 and publishes exactly one transcript revision.

## Limitations

- Caps are fixed at one per kind; host CPU, memory and GPU budgets, fairness between sources and live classes are operation 28.
- Leases have no heartbeat or reaper; they matter only across a restart of the single local service.
- Requeue depends on the previous service's process groups ending with it; this is observed on Windows only.
- A queued job does not protect its input from retention, so backlog can fail jobs as `input-no-longer-current` under quota pressure. Backlog reporting is not implemented.
- The task spec serializes inline cue text as content; a spoken URL can appear in it.
- The spec and envelope do not cross a process boundary yet; remote executors, fenced containment and a blob store remain later increments.
