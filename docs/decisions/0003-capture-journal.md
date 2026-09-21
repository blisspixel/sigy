# 0003: Durable capture intent and worker generations

Date: 2026-09-20. Status: implemented storage API with local tests. This records the original journal slice. On 2026-09-21, [recording and retention](0005-recording-and-retention.md) added finite media workers and verified publication through this journal; the original limitations below describe the earlier checkpoint.

## Contract

Schema v2 adds immutable capture intent and append-only lifecycle events to the existing SQLite catalog. A plan contains an immutable source-configuration reference, a finite UTC acquisition window and a positive byte ceiling. References are validated identifiers, not raw URLs, credentials or a promise that an adapter can resolve them. Source registration and authorization must precede future worker dispatch.

Create is idempotent only when the key and complete plan match. Replaying a terminal job returns that job and cannot restart it. Every mutation compares the exact revision and worker generation, increments the revision, and commits state and history in one immediate transaction. Starting a new attempt or declaring an old attempt lost advances the generation. An old worker's acknowledgment cannot update a recovered or replaced attempt.

The initial conservative limits are 256 pending jobs and two active lifecycle slots, checked atomically. Interrupted jobs continue occupying pending capacity. Lists and history accept pages of 1 through 64 records. These are implementation admission bounds, not measured simultaneous-stream capacity or physical disk reservations. Future source/media admission must enforce storage reserve, byte/time limits and worker resource ownership before accepting executable capture work.

Service startup, while holding the exclusive library lock and before workers exist, marks abandoned starting/running/retrying/stopping attempts interrupted. Recovery is repeatable and journaled; it does not automatically resume work or claim that missing media was captured. Only a new admitted start can resume an interrupted intent. Cancelling an interrupted intent records cancellation without deleting evidence. Scheduled and interrupted jobs can fail with a recorded cause, including a missed window.

The storage API rejects finalization because no verified segment-publication operation exists yet. Add completion only with validated media structure, checksums, timing, completeness and orphan reconciliation. Process exit alone cannot establish a completed recording. Revision fencing also does not terminate an orphan process; owned-worker cleanup belongs to the acquisition slice.

## Persistence and verification

The migration from v1 is atomic and retains existing financial liabilities. Store opening performs SQLite structural/foreign-key checks, the existing ledger audit, and capture journal replay against projected state. Capture audit uses one read snapshot and streams history rather than retaining the entire journal in memory. It rejects missing events, invalid transitions, discontinuous revisions/generations and inconsistent projected state. This detects accidental inconsistency; it is not authentication against an actor with arbitrary database write access.

Tests cover replay conflicts, stale starts and acknowledgments, competing admission, journal-write rollback during creation/transition/recovery, migration success/failure, missed/future windows, pending limits, pagination, immutable intent, corrupted projections, forged transitions, restart and client-visible recovery. The CLI status reports lifecycle counts and explicitly reports capture dispatch unavailable. No recording command exposes unresolved source intent to users.

The next slice must implement source configuration, bounded network/media workers and segment publication through these operations. It must also qualify backup-before-migration, long-history audit cost, physical disk pressure and power-loss recovery before a release. Current evidence does not establish those properties.

Primary API review on 2026-09-20: [SQLite immediate transactions and rollback behavior](https://www.sqlite.org/lang_transaction.html), [durability pragmas](https://www.sqlite.org/pragma.html#pragma_synchronous), and [rusqlite 0.40.2 transactions](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Transaction.html). No new dependency was required for this slice.
