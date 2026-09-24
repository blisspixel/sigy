# Scaling architecture

Status: proposed design, 2026-09-24. Nothing here is implemented beyond what the linked decisions record. The evidence and alternatives are in the [scaling and execution research](../../research/31-scaling-and-execution.md).

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
3. **Fair scheduling (operation 28).** A pure scheduling function tested without a runtime, live and batch classes with a guaranteed older-work share, per-source round-robin, host resource budgets, chunked recognition of continuous captures, and backlog reporting. Exit: simulation shows a fast source cannot starve the reserved share, and a three-station fixture keeps capture unaffected while reporting backlog honestly.

Later, in order: a blob store trait with per-lease scratch; a `sigy worker` process over a local pipe; remote workers over mutual TLS with fenced containment; the paid-provider executor with a real transport; a storage backend trait with a shared conformance suite, then Postgres; an object-store blob store.

## Not in the first complete release

Postgres, object storage, remote, container or serverless executors, sharded capture, distributed playback, a message broker, autoscaling and multi-tenant access. The first release ships increments 1 to 3 with the local executor and the in-service paid-provider executor behind the same trait.

## Risks

- Fencing refuses late remote results but cannot prove a remote process stopped; remote routes must be labeled that way and their input blobs pinned until the lease resolves.
- Invariants currently live in SQLite triggers. A second backend must pass a shared conformance suite before it is trusted.
- Remote execution moves broadcast audio off the host, so the data destination is a route permission like spending.
- Results can differ by executor and device; every envelope records which executor produced it.
