# Storage placement and capacity

Reviewed: 2026-10-03. Status: primary-source research and implementation proposal. No hardware, card, NAS, ARM64 runtime or deployment profile is qualified by this record. No storage was mounted and no capacity benchmark ran.

The confirmed direction spans a Pi 4-like ARM64 home server with a stated 2 TB SD card, a gaming PC, a local server with 2 TB or 20 TB NAS media, and reproducible infrastructure deployments. These are target configurations, not support claims. Follow the [reliability and scale plan](../docs/development/reliability-and-scale.md), [scaling architecture](../docs/design/scaling-architecture.md), [architecture and data](../docs/planning/02-architecture-and-data.md), and [assurance gates](../docs/planning/04-assurance-and-validation.md). This record supplies storage evidence and alternatives; a selected implementation needs its own decision.

## Hardware capacity is not processing capacity

The current [Pi 4 specifications](https://www.raspberrypi.com/products/raspberry-pi-4-model-b/specifications/) list a quad-core 64-bit Cortex-A72 at 1.8 GHz, RAM variants of 1, 2, 3, 4 or 8 GB, Gigabit Ethernet, two USB 3.0 ports and a microSD slot. Actual board revision, available memory, power and cooling remain deployment inputs. The graphics specification does not qualify an inference backend. Capture, catalog inspection and search need their own measured profile; recognition, translation and simultaneous execution need additional profiles.

The [SD Association capacity definitions](https://www.sdcard.org/developers/sd-standard-overview/capacity-sd-sdhc-sdxc-sduc/) place capacities through 2 TB within SDXC and capacities above 2 TB through 128 TB within SDUC. This does not establish an unnamed card's authentic capacity, Pi compatibility, boot behavior, endurance or flush semantics. [Official Pi card documentation](https://www.raspberrypi.com/documentation/accessories/sd-cards.html) covers specific products and host performance. Do not transfer those results to the stated card.

Qualification should record actual usable bytes, filesystem, mount options, card or drive identity, sustained write and synchronization latency, temperature and undervoltage observations. A capacity label or speed class is not a power-loss guarantee. [Pi hardware documentation](https://www.raspberrypi.com/documentation/hardware/rf/) describes thermal management; model throughput must be measured after sustained load, not only before throttling.

Linux ARM64 qualification requires native runtime and dependency architecture checks, hashed assets, actual cgroup delegation, worker bounds, owner-death cleanup, capture independence and recovery on the chosen host. Cross-compilation alone establishes none of these. Start with capture/catalog qualification and a bounded CPU workload; select concurrency only after measurements. A small host can remain useful while processing is held or delayed.

## Current implementation

The [library owner](../crates/sigy-service/src/library.rs) canonicalizes one library root and holds its exclusive lock. Catalog, WAL/SHM, control endpoints, media and analysis scratch currently share that root. The [catalog](../crates/sigy-service/src/storage/mod.rs) selects WAL and `synchronous=FULL`. [Media paths](../crates/sigy-service/src/recordings.rs) resolve beneath `media/`, reject links and enforce the current private directory boundary. [Execution staging](../crates/sigy-service/src/execution/stage.rs) places scratch inside the library. External media-store configuration is unimplemented.

[Backup and restore](../docs/decisions/0045-library-backup-and-restore.md) use a consistent catalog snapshot and hashed retained objects. These existing operations are the baseline to extend, not a reason to create a separate backup mechanism.

SQLite states that [WAL does not work over network filesystems](https://sqlite.org/wal.html), and its [network guidance](https://sqlite.org/useovernet.html) explains locking, synchronization and performance risks. Keep the live catalog, WAL, SHM and ownership lock on qualified local storage. Serve clients through service operations. A NAS can hold bulk media or backup artifacts without becoming the catalog authority.

Keep FULL synchronization for the exact ledger. [SQLite's corruption guidance](https://www.sqlite.org/howtocorrupt.html) documents devices that misreport synchronization, flash-controller power-loss damage and fake capacity. Successful flush calls depend on the complete storage chain; software fault fixtures do not prove physical power-loss durability.

## Proposed placement and identity

| Placement | Contract |
| --- | --- |
| Local authority | Catalog, exact ledger, lock and control endpoints on a qualified local filesystem |
| Local execution | Finite capture spool, analysis scratch and qualified asset cache, with independent byte reservations |
| Bulk media | Explicitly registered local disk or later qualified NAS store, identified independently of its path |
| Backup | Verified snapshot and enumerated object manifest, with a separate restore rehearsal |

Register a persistent typed media-store identity, configuration revision, owned directory marker and filesystem/mount evidence. An object references that identity, a validated relative key, expected size/hash and lifecycle state. A path is a locator, not the identity. A marker helps detect accidental replacement; it is not authentication against an adversary able to copy it.

Revalidate the intended store before publication, reads, deletion and recovery. Distinguish unavailable storage, identity mismatch, permission refusal, stale handle, corrupt object and confirmed missing object. Missing bulk storage must leave catalog history readable and hold affected operations. Do not automatically create a replacement store or fall through to the directory underneath a vanished mount.

This matters in current code: deletion and segment release treat `NotFound` as successful absence, and their directory resolver can create missing media storage. Those rules fit the existing owned local layout. Reusing them unchanged for an external mount could commit deletion or release against the wrong underlying directory. External placement must establish store identity before absence can discharge a lifecycle obligation.

## Publication, leases and failure

Local capture spool and scratch need explicit worst-case reservations and protected capture headroom. NAS interruption holds transfer; it cannot authorize unlimited local accumulation. Analysis overload cannot consume the storage needed to seal admitted capture.

Cross-store movement is a durable operation: create a destination temporary object, copy within bounds, verify bytes/hash, establish the selected destination's durability acknowledgement, publish atomically there, and commit the verified location. Cross-filesystem rename is not a substitute. Reclaim the old copy only after the new location is committed and read leases permit it. Persist progress so a crash leaves an inspectable pending copy rather than guessed success.

Read leases survive cancellation until actual reader or worker completion. [Linux NFS semantics](https://man7.org/linux/man-pages/man5/nfs.5.html) allow hard mounts to retry indefinitely and warn of corruption risks with soft timeouts. An asynchronous deadline does not force a blocked filesystem operation to return. Keep network-file I/O off the catalog actor; bound offloaded slots and queues, retain unresolved leases, and qualify termination behavior separately. Do not advertise a hard cleanup deadline where the operating system cannot enforce it.

Backup manifests must enumerate store mappings and selected retained objects alongside a consistent catalog snapshot. A NAS snapshot alone does not establish catalog/media consistency. Restore must verify objects, preserve immutable reservations and history, and explicitly remap stores without silently resetting their identities.

## Alternatives and evidence gates

The canonical [ST/AR packages](../docs/development/reliability-and-scale.md#storage-and-memory-increments) separate the inventory, concrete local move/resolver, NAS qualification and archive retrieval work. Specify the offline move's finite manifest, exclusive ownership and recovery protocol before implementing the resolver. The combined outline below describes the required boundaries, not one large runtime change.

1. **First bounded package:** select typed store identity and a canonical resolver used by capture, playback, analysis, pruning and backup. Retain the current local layout by default. Add read-only doctor placement evidence and unavailable-store reasons. Implement a second local media root with explicit offline migration before accepting network storage.
2. **Fault evidence:** root disappearance/replacement, wrong marker, symlink or reparse substitution, readonly/full storage, partial copy, changed hash, interrupted location commit, concurrent read lease and reclamation, and reopen/restore. Absence must never be inferred from an unavailable store.
3. **Physical profile:** actual Pi/card or server/disk, sustained capture plus catalog/search, synchronization latency, power/cooling observations and bounded restart/restore evidence. Power-loss claims require a separately authorized real experiment; no destructive card or power test is implied here.
4. **NAS profile:** named protocol, server/filesystem and mount settings; disconnect, stale-handle and remount behavior; publication/durability acknowledgement; bounded service responsiveness and unresolved-I/O behavior. Capacity alone does not qualify any of these.
5. **Reproducible deployment:** pinned executable or image digest, service identity, explicit local authority volume, registered media store, scratch limits, cgroup delegation and destination/secret references. Deployment reapplication must preserve budgets, grants and history. A systemd profile precedes any claim for cluster deployment.
6. **Later scale triggers:** measured write contention or concurrent-authority requirements before changing the catalog backend; measured object lifecycle or placement requirements before object storage; qualified delegation before remote workers; measured recovery and partition behavior before source sharding. Preserve the same accounting, leases and publication contracts through each substitution.

An all-local library remains the simplest alternative for a small host. Local authority plus NAS media increases capacity but adds failure and durability obligations. Remote execution can add processing capacity later without moving the catalog; it requires the existing authenticated delegation and destination gates. No option establishes simultaneous inference or long-term endurance without measured evidence.

## Archive query work and projections

The current [archive search](../crates/sigy-service/src/storage/archive.rs) scans revision/cue rows under explicit page, row, byte and application-deadline bounds. The deadline is checked between reads; it cannot interrupt arbitrary SQL work or blocked filesystem I/O. It applies the monitor's literal comparison and reports exact revision/media state. There is no lexical archive projection. The retained [SQLite build](../vendor/libsqlite3-sys/build.rs) enables FTS5, which supplies a candidate without a new database dependency; availability alone does not select its semantics.

[SQLite progress callbacks](https://www.sqlite.org/c3ref/progress_handler.html) can interrupt cooperative virtual-machine work. A connection has one callback; registration must be scoped and cleared on every path, and the callback cannot modify that connection. This is not forced cancellation of blocked kernel I/O. [Query-plan output](https://www.sqlite.org/eqp.html) helps inspect index use but is not a stable application interface. Measure selective filters, work and tail latency before selecting an index layout.

[FTS5 documentation](https://www.sqlite.org/fts5.html) exposes important multilingual tradeoffs: default `unicode61` uses Unicode 6.1 case folding and removes Latin diacritics; Porter stemming targets English; trigram full-text queries shorter than three characters return no matches. External-content indexes require consistency management. Recommendation: preserve the existing literal path, explicitly version indexed semantics and qualify meaningful case/diacritic and short-query behavior. Full-text query grammar needs separate treatment from SQL parameterization. Ranking is relevance, not evidence confidence.

A proposed projection keys exact cue/revision/target identity and its tokenizer/generation. Canonical publication and a pending marker commit together; bounded service passes maintain watermarks. Rebuild staging keeps the last useful generation, accounts for concurrent corrections/deletions and never changes canonical history. Revalidate candidates and report lag, omissions and truncation. Use frozen growing libraries to measure rows/cues/object counts, catalog/WAL/index footprint, recall and query tails under capture, rather than interpreting a large disk quota as scale evidence.

Optional embeddings and a specialized database follow a useful lexical baseline and independent multilingual reference results. Target/model/chunk identity, filtered recall, memory, build cost, context bytes and restoration need their own profiles. Graph or vector retrieval cannot qualify claim truth or source independence. Follow the [knowledge-memory research](37-knowledge-formats-and-temporal-memory.md) for interpretation and temporal boundaries.
