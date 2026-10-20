# Atomic worker responses and exact translation replay

Date: 2026-10-11. Status: implemented first S-03B settlement foundation; the 2026-10-20 amendment adds committed attempt and claim-owner checks. Integrated verification belongs in [active work](../development/progress.md). S-03B remains open. Catalog v51, local IPC v52 and dependencies are unchanged by the original foundation.

## Problem

The [completion audit](../../research/49-completion-settlement-and-shutdown.md) found that recognition and translation read their response after committing publication. A read failure could therefore report failure despite committed output. Inspection also found the same ordering in both native claim paths: a committed claim could fail to return its work token. Translation's successful terminal replay returned history without checking whether the submitted result matched that history.

## Decision

Read the claimed or finished job inside the same immediate transaction as its state change. Return that snapshot only after commit succeeds. A response-read error rolls the transaction back; a failed commit returns no successful claim or completion. There is no database read after a successful commit in these four paths. This does not make a filesystem I/O error during commit unambiguous or implement automatic retry.

For successful translation replay, locate the artifact by its original job ID, independently of the newest translation revision. Check transcript/revision, generation, profile/hash, English target, zero-USD amount, completion timestamp, cue count and translated membership. Compare every ordered cue's ordinal, state, exact English bytes and untranslated reason. The source work has at most 256 cues; a streaming query reads at most that count plus one lookahead row. Different results refuse with an idempotency conflict. Missing or inconsistent stored membership/identity refuses as an integrity fault. Replay publishes nothing, starts no worker and creates no new charge.

Cancellation continues to win before terminal publication. Other terminal states keep their prior replay contract. Successful historical replay does not require original audio to remain retained. No language evaluation, profile qualification, schema migration or new authority is introduced.

## Evidence and limits

Disposable SQLite catalogs use connection-local authorizer/update hooks to deny response reads after an actual state update, commit hooks to refuse commit, and a commit marker to deny subsequent reads. Hooks modify only atomic test state, never run SQL inside a callback. Independent reads verify rollback, exact returned snapshots, artifact counts, zero paid requests/ledger entries and retained original protection. A rollback followed by cancellation publishes no translation. Successful replay is tested after reopen, after media deletion, after a newer result, with Unicode normalization differences, all 256 cues and changed final-cue text. Deliberately corrupted fixtures cover header identity, missing rows and extra lookahead membership.

These fixtures use synthetic drained outcomes and establish no new native execution or device evidence. [SQLite commit hooks](https://www.sqlite.org/c3ref/commit_hook.html), [authorization callbacks](https://www.sqlite.org/c3ref/set_authorizer.html) and the pinned [rusqlite transaction contract](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Transaction.html) define the exercised boundaries.

The actor still removes a completion before fallible settlement. Preserving that capability needs bounded completed slots, classified transient failures and finite exhaustion/shutdown semantics. Launch-refusal settlement, language-evidence followup and legacy restart behavior remain separate boundaries. This foundation does not close S-03B or establish a roadmap stage exit.

## 2026-10-20 amendment

Recognition and translation claims read the stored attempt and lease owner in the same transaction as the claim. The work token returned after commit carries that owner. An empty owner remains only an uncommitted admission value. Finish compares the token with the stored row before terminal replay or publication. A different attempt or owner returns `stale-worker` and writes nothing. A claim that cannot read back an owner equal to the claimant, or an attempt that disagrees with the claimed job, returns a storage integrity fault and does not commit the token. Catalog transition triggers still reject a direct SQL change of those fields. Verification keeps its existing generation check and does not use this token. This slice changes no schema, IPC version, dependency, or retry policy.

The actor still removes a completion before fallible settlement. Bounded completed slots, classified transient retry, exhaustion and finite shutdown remain unimplemented. Retry attempt count, lock wait and monotonic deadline are not selected. Uncommitted in-memory evidence is not reconstructible after process death. This amendment does not close S-03B or establish a roadmap stage exit.
