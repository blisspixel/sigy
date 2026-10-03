# Exact task evidence and publication

Date: 2026-10-03. Status: implemented bounded increment with passing integrated local gates. See [active work](../development/progress.md) for receipts and exact-commit CI. This decision does not establish a roadmap stage exit, language qualification or general task planning.

## Problem and selected behavior

Task-owned collection and processing already identify exact occurrences, recordings, processing receipts and canonical jobs. The existing `task execute` publishes a broader monitor checkpoint. A finite task needs a publication route bound to its own collected evidence, including a useful partial result when collection or processing is incomplete.

Reuse the single catalog, service actor, scheduler, finding storage, briefing headers/membership and lifetime publication allowance. A new exact snapshot is an observation, with no collection, processing or publication authority. Explicit publication chooses that immutable observation and grants one finite literal-evidence run. Existing checkpoint execution stays available; a task can use one origin for its lifetime run, with exact replay preserving the original selection.

## Bounded observation

`task evidence <id>` is a read-only live reconciliation. `task freeze <id> <request> --expected-snapshot <ordinal>` stores its exact frozen counterpart; `task snapshot <id> <ordinal>` reads history. The selected mode is freeze now. Pending work remains pending in that observation even if it completes later. Another fresh snapshot can observe later work within the same lifetime observation allowance.

The snapshot binds the immutable task scope, inspected monitor version and complete action prefix, collection/processing grant digests, owned recording identities, processing receipts, canonical job/profile/input identities, observed job generation/state and exact published transcript/translation revisions. Today's translation target is explicitly `en`; this introduces no new target support. Capture, recognition and translation retain separate coverage and uncertainty.

Streaming reconciliation checks cue type and UTF-8 byte length before allocating comparison text. It bounds two collected entries, four stage receipts, 64 citations, 4,096 examined cue headers and 4 MiB cumulative examined original/English text. Existing 4,096-byte per-value bounds remain. Exactly 64 matches can be complete; a known additional match or exhausted scan becomes partial with an explicit remainder. No match after an exhausted scan cannot become a complete no-match claim.

The canonical catalog guard bounds cooperative SQL work to 100 ms and four million VM operations, with a 10 ms lock wait. It clears the callback and restores the previous busy timeout on exit. It does not force a blocked filesystem read to return and establishes no portable latency guarantee. Failure refuses fresh effects; an exact replay resolves an already committed operation if later client/cleanup reporting fails.

A capped writer stops before extending a snapshot past 65,536 bytes. Mandatory lineage and an explicit partial reason take precedence over optional citations. Encoding that cannot fit mandatory fields is refused. Truncated citations retain their deterministic prefix and mark the snapshot partial, including when jobs were pending.

Legacy checkpoints and exact snapshots share 128 lifetime observations per task. Their ordinal sequences remain distinct and their stored JSON/hash recipes are preserved. The catalog transaction checks the expected exact ordinal, common task clock, current scope and grants before inserting the complete observation. Replay selects the stored request before current-policy or live-evidence checks; changing replay parameters conflicts.

## Historical inspection and publication

Historical reads audit immutable lineage and coverage. Complete snapshots independently check the entire bounded citation membership at the frozen revisions and terms, including an empty complete result. Partial snapshots preserve unknown remainder. Later transcript corrections, translation completion, media expiry or segment release do not rewrite earlier evidence. Original/translation identities remain independently inspectable.

The first hosted Windows run exposed repeated audit work exceeding the unchanged cooperative guard in the 64-finding boundary fixture. Each operation now audits its immutable selected observation once inside one transaction. Post-effect checks still reread the grant, digest, scope and intents, then validate complete event history and artifact membership. Complete membership uses one prepared ordered cue stream; partial prefixes retain point checks. Finding history reads bounded citation metadata instead of repeatedly allocating passage text. Public run/briefing reads use a consistent deferred transaction. No cross-operation cache, relaxed resource bound or skipped fixture is introduced. The milestone record preserves the failed CI receipt and subsequent measurements.

Stored media byte counts describe the observation time. Segment release changes today's retained byte count, so an old count is checked against the immutable published interval envelope rather than compared with current retention. Existing release history does not independently reconstruct every historical retained-byte instant. Observed sharing counts likewise describe the snapshot; current cancellation authority comes from canonical interests.

`task publish <id> <request> --snapshot <ordinal> --max-findings <1..64> --expected-generation 0` admits one exact-origin run through the existing executor. Frozen scope and current authority are checked in the admission transaction. It cannot create jobs, restore media, refill resources, choose a new translation target or start model inference. Each finding attempt and its receipt commit together. Current retained-media truth determines a new citation's retained, expired or missing status. Unsupported original-only evidence produces an explicit skipped receipt rather than an invented translation.

The canonical briefing uses exactly the successful task-owned finding effects from that run. Its exclusive coverage origin references the immutable exact snapshot. It does not manufacture monitor-wide coverage. The legacy monitor briefing reader refuses this origin with a directing error; `task briefing <id>` returns exact coverage and bounded finding references. Repeated original scripts use the existing grouping rule; classification stays off and independent corroboration remains unmeasured.

Publication and cancellation share the existing run generations. Policy drift revokes future effects. A backward clock holds fresh work. `task cancel` stops future task publications and preserves stored findings, collection and independently authorized jobs. Processing withdrawal is a separate [versioned operation](0079-task-interest-withdrawal.md).

## Storage, compatibility and evidence

Schema v46 adds immutable exact observations and shared observation/clock enforcement. Schema v47 extends the existing run grant to disjoint checkpoint/snapshot origins and adds an exclusive exact briefing origin. Local IPC v48 exposes the typed exact publication/read results together with the preceding withdrawal increment. No new task mutation is exposed through MCP.

Migration copies the existing run, intent and event rows without changing legacy JSON, hashes, row identities or receipts, installs admission checks after historical copy, and checks foreign keys before committing. It must roll back as a whole on conflict. Tests that reconstruct older catalogs explicitly remove newer dependent objects first.

Acceptance covers normal and incomplete collection, frozen pending jobs, corrections, retention, exact membership, shared lifetime capacity, replay, backward clocks, scope drift, busy-query cleanup, insertion/effect faults, hostile rehashed edits, populated migration/reopen and canonical briefing origin separation. Native cancellation evidence belongs to decision0079. Fixture measurements do not qualify language understanding, real-world task success, a Pi/NAS profile or long unattended operation.

Source owners are [snapshot storage](../../crates/sigy-service/src/storage/tasks/snapshot.rs), [snapshot audit](../../crates/sigy-service/src/storage/tasks/snapshot/audit.rs), [bounded reconciliation](../../crates/sigy-service/src/storage/tasks/evidence.rs), [run execution](../../crates/sigy-service/src/storage/tasks/run.rs), [canonical task briefings](../../crates/sigy-service/src/storage/briefings/task.rs), and the [snapshot fixtures](../../crates/sigy-service/src/storage/tasks/processing/tests/snapshots.rs). The [near-term briefs](../development/near-term-implementation.md) and [workflow invariants](../design/workflow-invariants.md) retain the broader acceptance and qualification gates.
