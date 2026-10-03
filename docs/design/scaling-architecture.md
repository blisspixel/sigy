# Scaling architecture

Status: proposed design, 2026-09-24, updated 2026-10-02. Implemented: the task contract and job pool ([0043](../decisions/0043-task-contract-and-job-pool.md)), the first stage controller ([0048](../decisions/0048-monitor-processing.md)), chunked recognition ([0049](../decisions/0049-chunked-recognition.md)), the claim-order part of increment 5 ([0050](../decisions/0050-fair-claim-order.md)), a library recognition pace in doctor ([0051](../decisions/0051-recognition-pace.md)), live classification of a monitored recognition job from that pace ([0052](../decisions/0052-live-recognition-deadline.md)), the wall time of queued recognition audio at that pace ([0053](../decisions/0053-queued-recognition.md)), the empty-group snapshot for recognition decode and recognize ([0054](../decisions/0054-recognition-worker-cost.md)), and a comparison of admitted recognition audio with completed busy time ([0055](../decisions/0055-recognition-arrival.md)). [Transcript corrections](../decisions/0056-transcript-corrections.md) append one cue edit and are outside this capacity work. [Stored findings](../decisions/0057-stored-findings.md) store one citation and are outside this capacity work. [Briefings](../decisions/0058-briefings.md) store one generation over findings and are outside this capacity work. [Frozen briefing coverage](../decisions/0059-frozen-briefing-coverage.md) stores that generation's coverage snapshot and is outside this capacity work. The snapshot and the comparison are not a host budget, and the comparison is not a clock time for an empty queue. Host budgets and increments 6 and 7 are not implemented. The evidence and alternatives are in the [scaling and execution research](../../research/31-scaling-and-execution.md).

## Goal

The 2026-10-02 [reliability and scale plan](../development/reliability-and-scale.md) updates the implementation sequence and qualification gates, with [dated primary-source research](../../research/35-reliability-scale-and-interpretation.md). Aggregate host/device admission, warm workers, remote execution and distributed authority remain unimplemented. English is the current translation target; the confirmed directed-target contract belongs in future derivation identity, grants and provenance before extending execution.

The same contracts should serve one person on a laptop, a small Linux box with a GPU, a team server, and a large deployment with many sources and machines. Local-first stays the default. Scaling must not weaken the invariants that make results trustworthy: exact job generations, proof that work stopped before its result is accepted, immutable results, service-side validation, and exact money reservations before any paid work.

## Planes

The 2026-10-03 [storage and memory contract](storage-and-memory.md) separates local authority from future bulk-media placement and rebuildable knowledge views. Its [packages](../development/reliability-and-scale.md#storage-and-memory-increments) qualify second-local-volume movement, NAS faults, archive projections and bounded time-aware context independently. Current library paths remain colocated. Additional execution capacity does not require a network-mounted catalog, graph database or external memory framework.

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
- **Fenced publication, proposed remote contract:** an authenticated remote result matches the admitted attempt, generation and manifest. This can prevent stale publication but cannot itself prove execution stopped, containment held or billed liability ended. Remote routes require separately qualified platform termination and billing evidence. A late result from an expired lease is refused as stale.

A deadline for remote work must be enforced by the platform (execution timeout, Kubernetes `activeDeadlineSeconds`), so a service crash cannot extend spend. If containment cannot be shown, the job becomes interrupted with a new generation and any reservation stays an uncertain liability.

## Job queue

A `JobQueue` trait fronts the queue: enqueue (idempotent, a changed request under the same ID is refused), claim (bumps the generation, which is the fencing token), heartbeat (fenced on the generation, returns cancelling), complete and fail (each needs containment evidence and runs publication in the same transaction), cancel, and reap.

- **SQLite** is the default for one machine: the current catalog, with no broker.
- **Postgres** with `FOR UPDATE SKIP LOCKED` is a researched candidate when measured catalog contention or concurrent-writer requirements justify a backend change. A second execution worker can leave authority with the existing service and SQLite. Claim, ledger reservation, fencing checks, publication and settlement remain transactionally consistent; each substitution needs migration and backend-conformance evidence.
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

Local CPU remains the portable baseline; explicitly selected qualified GPU or container profiles can accelerate it. A configured remote or cluster route needs its own data-destination, containment, capacity and cost qualification, even when it has no metered inference fee. A paid executor is eligible only when a request or saved policy names that route and the lifetime allowance covers its full worst case. Local overload cannot grant a new route.

The worst-case cost of a serverless task is the ceiling rate multiplied by the startup timeout, execution timeout, overrun slack, billed idle time and minimum rounding. Platform budgets reset monthly or hourly, so they are only a second line of defense behind the lifetime ledger. Charges stay uncertain until reconciled against usage exports.

## Stage graph

Work is a graph of derivations over immutable facts: a sealed segment derives a pin, a pin derives recognition chunks, chunks derive one transcript revision, a transcript derives language evidence and a translation, and translations derive passage matches, findings and briefings. Each edge is a **stage**.

- **Level-triggered reconcilers, not events.** A stage controller reads catalog facts and asks one question: which derivations that policy wants are missing? It enqueues those, bounded per pass, with IDs derived from content (input hash, stage version, profile hash). A crash, restart, duplicate pass or second coordinator is harmless, because an enqueue of an existing ID is a replay. Nothing depends on an event being delivered once.
- **Policy is a filter on the graph, not a separate system.** A monitor is a set of standing requests over the graph ("these sources, these profiles, this many hours a day"). Its controller admits derivations oldest first until a cap is reached and records each step, so coverage, deferral and spend are read from the same rows.
- **Memoization by exact compatible inputs, proposed.** Sharing requires exact input revision/content, stage/template version, target language where applicable, profile/parameters, relevant context and compatible authority/privacy boundaries. Text equality alone cannot establish equivalence. Any future reuse creates explicit provenance and is labeled reused rather than a fresh run. Current canonical job sharing does not establish general cue-text caching.
- **Invalidation follows lineage.** A correction or a new revision marks downstream derivations stale through the same edges; recomputation goes through the normal queue and caps.
- **A sans-IO planning core.** Each controller is a pure function from a catalog snapshot to a bounded list of derivations, tested without a runtime, and the actor or a coordinator performs the I/O. The same functions drive one laptop or many workers.

## Throughput

Scaling out multiplies whatever each worker wastes, so per-worker efficiency comes first. Measured on the development host (see the linked research records):

- **Measure resident-worker alternatives.** llama.cpp spent 1.3 to 4.6 s loading a translation model before each cue ([Vulkan measurement](../../research/experiments/local-mt/vulkan-780m.md)). A proposed resident worker may amortize loading, but worker-lifetime drain does not satisfy today's per-job empty-group publication requirement. First define per-request completion, contamination isolation, bounded resident state, cancellation, worker incarnation/sequence and fault reconciliation; compare useful quality and full resource cost with the per-cue process. Uncertain or paid attempts cannot simply requeue.
- **Batch only where measured.** Evaluate useful throughput and tail latency for each runtime/model/device profile. Parallel slots may improve utilization or cause memory pressure and interference. Aggregate device admission and a qualified completion contract precede higher concurrency.
- **Measure speech gating and language hints.** The current recognizer hears windows of at most 30 s, with boundary phrase handling described in [chunked recognition](../decisions/0049-chunked-recognition.md). Evaluate further skipping or carried hints against mixed languages, weak speech, songs, boundary words and citation recall. Record skipped intervals and hint origin explicitly; no-speech output is not proof of silence. Preserve originals and compare quality/resource tradeoffs before selecting an optimization.
- **Keep models where the work is.** A scheduler that sends a task to a worker already holding its model avoids reloads; a model swap is a cost in the plan, not a free action.

## Capacity and admission

Every (profile, device) pair gets a measured cost: load time, real-time factor per second of speech, memory, and slots. From these the scheduler computes demand against capacity before admitting standing work:

- **Live work** (a monitored live station) has a deadline: a chunk should be recognized within a bounded delay of its seal. It is scheduled earliest deadline first within the live class.
- **Batch work** (backlog, podcasts, reprocessing) fills idle capacity, with the guaranteed older-work share from operation 28.
- **Overload is reported, not hidden.** When demand exceeds measured capacity, the monitor's coverage shows the deficit (for example "needs about 1.8 times this host's recognition capacity") and the deferred time, oldest first. That multiple needs a measured host budget, which is still open. [Queued recognition](../decisions/0053-queued-recognition.md) reports the processing time of audio already waiting and does not compute that multiple. [Recognition arrival](../decisions/0055-recognition-arrival.md) says whether admitted audio per wall millisecond was more, the same, or less than completed busy work. It does not compute that multiple. [Recognition worker cost](../decisions/0054-recognition-worker-cost.md) records the empty-group snapshot. Neither figure is the host budget the multiple still needs. Overload never triggers paid processing and never silently drops captured audio, which stays retained for later catch-up within retention.
- **Capture stays independent and measured.** Bitrate, decoder, socket, filesystem and device costs vary. Reserve capture/control headroom and measure mixed workloads; nominal low bitrate alone cannot qualify a source count. Capture does not wait on analysis.

## A spare GPU machine

A proposed next deployment adds an authenticated execution host while the existing service retains catalog and ledger authority. `sigy worker` is an illustrative future command, not an implemented interface. The worker advertises qualified devices/profiles and receives fenced leases, exact input/asset hashes and finite transfer capabilities. Moving audio off-host requires explicit destination authority. Results return as bounded envelopes for service validation and publication. Worker loss may leave execution, committed output or liability unresolved; reconcile those states and prove eligible termination before retrying. Lease expiry alone does not establish safety.

Beyond that, evaluate transactional storage, object storage, additional executors and source partitioning when measurements justify each change. Design exclusive capture ownership, partition recovery and authoritative global liability before sharding. Each selected substitution needs shared conformance evidence for job generations, exact accounting, publication, media lifecycle, migration and restore. A trait alone does not make these backends interchangeable.

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
4. **Chunked recognition.** Implemented in [0049](../decisions/0049-chunked-recognition.md): recordings longer than one window and multi-segment captures. Each process hears at most 30 seconds. A phrase that reaches the window edge while audio remains is stored in the next window, which starts at that phrase. A phrase with no earlier break is stored through the window end. One transcript revision per job. Each window still runs with `language=auto`. Fair scheduling remains increment 5.
5. **Fair scheduling and capacity (operation 28).** The claim chooser is implemented in [0050](../decisions/0050-fair-claim-order.md): sources rotate, and every fourth claim is the oldest waiting batch job. A fast live source cannot take that reserved claim in the chooser tests. [Recognition pace](../decisions/0051-recognition-pace.md) reports completed jobs on this library from doctor. [Live recognition deadline](../decisions/0052-live-recognition-deadline.md) treats a monitored recognition job as live while that pace still fits after the last seal. The deadline is computed at claim time and is not stored. [Queued recognition](../decisions/0053-queued-recognition.md) reports the processing time of audio already waiting at that pace. A running or stopping job is a count. The queue figure is not a host budget. [Recognition arrival](../decisions/0055-recognition-arrival.md) compares admitted audio with completed busy time in a separate sentence and does not give a clock time for an empty queue. A storage fixture publishes on three monitored sources while recognition is queued, and again after those monitors are paused. The doctor sentence stays on the queued retained audio. One station's disconnect gap is not audio, and a queued translation is omitted. [Recognition worker cost](../decisions/0054-recognition-worker-cost.md) records peak committed memory and total CPU time from the job-object snapshot that proved the group empty. An empty cgroup stores no numbers. The stored figure is not the host budget this exit still requires. Host budgets remain. Exit still requires those budgets.
6. **Warm workers.** A long-lived contained worker per (profile, device) with a pipe protocol, measured against per-task processes on the same inputs; adopted only if faster at equal output and equal containment.
7. **Memoized translation** of repeated cue text, labeled as reuse.

Later candidates, each requiring measured need and qualification: content-addressed transfer with bounded per-lease scratch; authenticated remote execution with revocable destination grants; platform termination separate from publication fencing; explicitly bounded paid transport; and transactional/object-storage backends with migration and shared conformance evidence. Follow the [current implementation sequence](../development/reliability-and-scale.md#next-increments-and-dependencies), not a mandatory infrastructure shopping list.

## Not in the first complete release

Postgres, object storage, container or serverless executors, sharded capture, distributed playback, a message broker, autoscaling and multi-tenant access. The first release ships increments 1 to 5 with the local executor and the in-service paid-provider executor behind the same trait; warm workers, memoization and one `sigy worker` on a second machine follow if measurement justifies them.

## Risks

- Fencing refuses late remote results but cannot prove a remote process stopped; remote routes must be labeled that way and their input blobs pinned until the lease resolves.
- Invariants currently live in SQLite triggers. A second backend must pass a shared conformance suite before it is trusted.
- Remote execution moves broadcast audio off the host, so the data destination is a route permission like spending.
- Results can differ by executor and device; every envelope records which executor produced it.
