# Scaling architecture

Status: proposed design, 2026-09-24, updated 2026-09-25. Implemented: the task contract and job pool ([0043](../decisions/0043-task-contract-and-job-pool.md)) and the first stage controller ([0048](../decisions/0048-monitor-processing.md)). Nothing else here is implemented. The evidence and alternatives are in the [scaling and execution research](../../research/31-scaling-and-execution.md).

## Goal

The same contracts should serve one person on a laptop, a small Linux box with a GPU, a team server, and a large deployment with many sources and machines. Local-first stays the default. Scaling must not weaken the invariants that make results trustworthy: exact job generations, proof that work stopped before its result is accepted, immutable results, service-side validation, and exact money reservations before any paid work.

## Planes

| Plane | Owns | Runs in |
| --- | --- | --- |
| Capture | Acquisition, segment sealing, gaps, the library write token | The service; later one writer per shard of sources |
| Job | Durable work items, leases, fairness, admission, resource tokens, ledger reservations | The service catalog |
| Execution | Running one task and returning one result envelope | `Executor` implementations |
| Result | Validation, currency and parent checks, immutable publication, settlement | The service only |
| Control | CLI, TUI and MCP over versioned IPC, queue and backlog views | Clients |

### What crosses the job and execution boundary

A **task spec** carries the task ID and generation, the kind, a spec hash, content-addressed inputs (for example the decoded interval as `{sha256, bytes, media_type}`, or cue text inline with its hash), content-addressed assets (runtime, model, speech-activity model), the fixed engine template and parameters, limits, and the lease deadline.

A **result envelope** carries the task ID and generation, the spec hash, the outcome, typed bounded output with its hash, the executor's identity and evidence (what ran, where, and on which device), containment, usage, and cost (zero, or provider evidence).

**Never crosses:** source URLs, redirect policy, secrets or their names, catalog handles, library paths or object keys, budget identifiers. Remote workers fetch blobs with a capability scoped to one lease and exactly the hashes in its spec. The paid-provider executor stays inside the service, because it needs the secret reference and the ledger transaction.

### Containment

A result is accepted only with containment evidence:

- **Drained:** the local executor proved its process group, or a container runtime proved its container and cgroup, have no member left. This is today's rule.
- **Fenced:** a remote executor reported a terminal state from an authenticated platform poll, the collected result hash matches the worker's manifest, and any billed idle window has elapsed. A late result from an expired lease is refused as stale.

A deadline for remote work must be enforced by the platform (execution timeout, Kubernetes `activeDeadlineSeconds`), so a service crash cannot extend spend. If containment cannot be shown, the job becomes interrupted with a new generation and any reservation stays an uncertain liability.

## Job queue

A `JobQueue` trait fronts the queue: enqueue (idempotent, a changed request under the same ID is refused), claim (bumps the generation, which is the fencing token), heartbeat (fenced on the generation, returns cancelling), complete and fail (each needs containment evidence and runs publication in the same transaction), cancel, and reap.

- **SQLite** is the default for one machine: the current catalog, with no broker.
- **Postgres** with `FOR UPDATE SKIP LOCKED` is the server-grade implementation, added only when a second worker process exists. Claim, ledger reservation, fencing checks, publication and settlement stay in one database transaction.
- A message broker or workflow engine is not the job authority, because none can atomically check a generation against Sigy's results and ledger. A broker may later carry notifications only.

States: `queued`, `leased`, `succeeded`, `failed` (retryable failures return to `queued` with a backoff), `dead`, `cancelling`, `cancelled`, and `orphaned` for an expired lease without containment evidence. An orphaned local job returns to `queued` only after containment evidence or a proven host restart. Paid work is never redelivered: a paid attempt whose lease expires keeps its full reservation as an uncertain liability, and any retry is a new attempt with a new reservation.

Leases are short with heartbeats, measured on the catalog's clock. A worker that cannot renew stops its own contained work before the lease expires.

Fairness follows roadmap operation 28: a live class before batch, a guaranteed share for older work, round-robin across sources within a class, and admission against host CPU, memory and GPU budgets. Capture never waits on the queue; when the queue is full, the backlog is reported as unable to catch up.

## Executors

| Class | Isolation | Where |
| --- | --- | --- |
| Local process group | Job Object or cgroup limits, kill on close; no OS network sandbox | Every host (macOS fails closed today) |
| Local container | Rootless Podman on cgroup v2: `--network none`, read-only root, dropped capabilities, seccomp, limits; GPU through CDI or device nodes | Linux first; WSL2 on Windows as an option |
| Cluster job | Kubernetes Job with a pinned image digest, a deadline, no retries, default-deny egress, optional gVisor or Kata; models as OCI artifacts whose digest equals the model hash | User-owned clusters |
| Serverless GPU | Platform container with a hard execution timeout and zero platform retries | Opt-in paid route only |
| Hosted provider | Provider API (OpenRouter, audio-duration-billed ASR services) | Opt-in paid route only |

The scheduler prefers a qualified local CPU profile, then a qualified local GPU profile with device evidence, then a local container or user-owned cluster at zero cost. A remote paid executor is used only when a request or saved policy names that route, the lifetime allowance covers the full worst case, and local overload is not the trigger.

The worst-case cost of a serverless task is the ceiling rate multiplied by the startup timeout, execution timeout, overrun slack, billed idle time and minimum rounding. Platform budgets reset monthly or hourly, so they are only a second line of defense behind the lifetime ledger. Charges stay uncertain until reconciled against usage exports.

## Stage graph

Work is a graph of derivations over immutable facts: a sealed segment derives a pin, a pin derives recognition chunks, chunks derive one transcript revision, a transcript derives language evidence and a translation, and translations derive passage matches, findings and briefings. Each edge is a **stage**.

- **Level-triggered reconcilers, not events.** A stage controller reads catalog facts and asks one question: which derivations that policy wants are missing? It enqueues those, bounded per pass, with IDs derived from content (input hash, stage version, profile hash). A crash, restart, duplicate pass or second coordinator is harmless, because an enqueue of an existing ID is a replay. Nothing depends on an event being delivered once.
- **Policy is a filter on the graph, not a separate system.** A monitor is a set of standing requests over the graph ("these sources, these profiles, this many hours a day"). Its controller admits derivations oldest first until a cap is reached and records each step, so coverage, deferral and spend are read from the same rows.
- **Memoization by content.** A derivation keyed by what it reads is computed once. Two monitors on the same station and profile share one transcript. A recognized cue whose text hash, source language and profile match an earlier translation reuses it as a new revision that cites the earlier result, which matters for radio, where station identifiers, jingles and advertisements repeat all day. A reused result is labeled as reused, never as a fresh run.
- **Invalidation follows lineage.** A correction or a new revision marks downstream derivations stale through the same edges; recomputation goes through the normal queue and caps.
- **A sans-IO planning core.** Each controller is a pure function from a catalog snapshot to a bounded list of derivations, tested without a runtime, and the actor or a coordinator performs the I/O. The same functions drive one laptop or many workers.

## Throughput

Scaling out multiplies whatever each worker wastes, so per-worker efficiency comes first. Measured on the development host (see the linked research records):

- **Model load dominates short tasks.** llama.cpp spent 1.3 to 4.6 s loading a translation model before each cue ([Vulkan measurement](../../research/experiments/local-mt/vulkan-780m.md)); one process per cue is correct for containment but spends most of a short cue loading. The fix is a **warm worker**: one contained long-lived process per (profile, device) that keeps the model resident and takes a stream of tasks over a pipe. Containment and the drain proof then cover the worker's lifetime; each task result carries the worker incarnation and a sequence number; a fault, a deadline or a failed health check kills the whole group and returns its in-flight tasks to the queue under new generations. Warm workers must be measured against today's per-cue process before they replace it.
- **Batch where the device rewards it.** GPUs reach their throughput only with several requests in flight (llama.cpp parallel slots, batched decoding). A warm worker advertises its slot count, and the scheduler fills slots rather than launching processes.
- **Cut work before the model.** The whisper encoder always processes a padded 30 s window, and automatic language detection adds a second encoder pass ([32-clip calibration](../../research/experiments/local-asr/calibration-32.md)). So chunks are at most 30 s, speech-activity detection drops music and silence before the encoder, and a language detected with enough margin on one chunk can be passed as a hint to the next chunks of the same capture, recorded as a hint rather than a detection. Radio is often music; skipping it is the largest single saving.
- **Keep models where the work is.** A scheduler that sends a task to a worker already holding its model avoids reloads; a model swap is a cost in the plan, not a free action.

## Capacity and admission

Every (profile, device) pair gets a measured cost: load time, real-time factor per second of speech, memory, and slots. From these the scheduler computes demand against capacity before admitting standing work:

- **Live work** (a monitored live station) has a deadline: a chunk should be recognized within a bounded delay of its seal. It is scheduled earliest deadline first within the live class.
- **Batch work** (backlog, podcasts, reprocessing) fills idle capacity, with the guaranteed older-work share from operation 28.
- **Overload is reported, not hidden.** When demand exceeds measured capacity, the monitor's coverage shows the deficit (for example "needs about 1.8 times this host's recognition capacity") and the deferred time, oldest first. Overload never triggers paid processing and never silently drops captured audio, which stays retained for later catch-up within retention.
- **Capture is cheap and stays independent.** A radio stream is tens of kilobytes per second; one host can capture far more streams than it can recognize. Capture never waits on analysis.

## A spare GPU machine

The next step up from one laptop is a second machine with a GPU running the same binary as `sigy worker`. The worker holds no catalog. It connects out to the service over an authenticated channel (an SSH-forwarded local socket first, mutual TLS later), advertises its devices and qualified profiles with their measured costs, and pulls leases. Each lease carries a task spec and a scoped capability to fetch exactly the input chunks it names (a 30 s chunk of 16 kHz mono audio is about 1 MB before compression); results return as envelopes that the service validates and publishes. The worker keeps inputs only in per-lease scratch. Moving broadcast audio to another machine is a data-destination permission the user grants per worker, like a spending route. Losing the worker loses nothing but time: its leases expire and requeue under new generations.

Beyond that, the same pieces scale by substitution rather than redesign: Postgres for the catalog and queue when more than one coordinator process writes, an object store for blobs, containers or cluster jobs for executors, and capture sharded by source. Each substitution sits behind a trait with a shared conformance suite (`JobQueue`, `Executor`, `BlobStore`, catalog backend).

## Deployment profiles

| Profile | Catalog | Media and blobs | Execution | Queue |
| --- | --- | --- | --- | --- |
| Laptop | SQLite | Library files; the blob store maps a hash to the existing object | In-process local executors, one or two per kind | SQLite |
| Home server with GPU | SQLite | Same | Same binary, more slots; a GPU is a separate resource and a separate measured profile | SQLite |
| Team server | Postgres, behind the same storage conformance tests | S3-compatible object store | Several worker processes pulling leases over mutual TLS | Postgres |
| Large scale | Postgres, capture sharded by source | Object store with worker asset caches | Autoscaled containers, optional serverless GPU for batch | Postgres, or a dedicated queue only if measured as the bottleneck |

The same task spec, result envelope, validation code, generation fencing and ledger rules apply in every profile. Workers never write the catalog.

## Increments

1. **Task contract and local executor.** Extract `TaskSpec`, `ResultEnvelope`, containment and an `Executor` trait from the recognizer and translator, with a local stage that maps hashes to library files. No schema change. Exit: the native-media fault fixture passes unchanged, a golden test pins the serialized spec and its hash, and a test proves no spec contains a URL, path or secret name.
2. **Durable job pool.** Add queued state, leases, attempts, per-kind concurrency caps and per-lineage exclusivity, and remove the lifetime cap of 256 job rows, which a single continuous station exhausts in about four hours. Admission enqueues instead of refusing a busy worker. Restart requeues zero-cost local work under a new generation and never requeues paid work. Exit: bounded concurrency with every job published, a killed service leading to exactly one result per job, more than 300 jobs admitted, capture unaffected during recognition, and a backup, restore and interrupted-migration rehearsal.
3. **First stage controller.** Monitor processing as a level-triggered reconciler: published recordings of a monitor's sources are pinned, recognized and translated with content-derived job IDs, oldest first, charged against the monitor's daily and total audio caps, with every step recorded. Exit: caps hold exactly across restarts, two monitors on one source share one transcript, and deferred work appears in coverage.
4. **Chunked recognition.** Recordings longer than one chunk and multi-segment captures, 30 s chunks, one transcript revision per job.
5. **Fair scheduling and capacity (operation 28).** A pure scheduling function tested without a runtime, live and batch classes with deadlines and a guaranteed older-work share, per-source round-robin, host resource budgets, measured per-profile costs, and deficit reporting. Exit: simulation shows a fast source cannot starve the reserved share, and a three-station fixture keeps capture unaffected while reporting backlog honestly.
6. **Warm workers.** A long-lived contained worker per (profile, device) with a pipe protocol, measured against per-task processes on the same inputs; adopted only if faster at equal output and equal containment.
7. **Memoized translation** of repeated cue text, labeled as reuse.

Later, in order: a blob store trait with per-lease scratch; `sigy worker` on a second machine over an SSH-forwarded socket; mutual TLS with fenced containment; the paid-provider executor with a real transport; a storage backend trait with a shared conformance suite, then Postgres; an object-store blob store.

## Not in the first complete release

Postgres, object storage, container or serverless executors, sharded capture, distributed playback, a message broker, autoscaling and multi-tenant access. The first release ships increments 1 to 5 with the local executor and the in-service paid-provider executor behind the same trait; warm workers, memoization and one `sigy worker` on a second machine follow if measurement justifies them.

## Risks

- Fencing refuses late remote results but cannot prove a remote process stopped; remote routes must be labeled that way and their input blobs pinned until the lease resolves.
- Invariants currently live in SQLite triggers. A second backend must pass a shared conformance suite before it is trusted.
- Remote execution moves broadcast audio off the host, so the data destination is a route permission like spending.
- Results can differ by executor and device; every envelope records which executor produced it.
