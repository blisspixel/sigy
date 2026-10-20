# 0096: Responsive metadata reads

Date: 2026-10-20. Status: implemented bounded EX-02C followup for issue #34; [active work](../development/progress.md#2026-10-20-responsive-metadata-reads) records the gates that have actually run. Catalog v51 and local IPC v53 are unchanged. This decision adds no dependency. Tokio remains locked at 1.53.1. A compatible lock update for published rustls advisories is recorded with that receipt and does not change this behavior.

## Problem

Search, linked inspection, monitor reads, finding reads and reload still ran on the terminal thread after player status and stop moved aside. A delayed catalog or service reply could stall resize, Back, stop and quit. The five-second control exchange timeout does not bound a direct local catalog open, and dropping a handle does not prove that a filesystem read has stopped.

## Decision

Keep player status, player stop, directory fetch and metadata on separate observers. The metadata observer has one read lane and one favorite lane. Each lane owns at most one task and a two-message channel. Poll adopts finished messages without waiting on the terminal thread. A slot stays occupied until its task is finished and its receiver is empty. Dropping the task handle is not cancellation and is not used to free the slot.

Discardable reads are search, random exploration, linked context, monitor list, monitor detail, finding, finding original and reload. At most one of those reads is in flight. A newer discardable read replaces one queued read and does not spawn until the in-flight slot is idle and the queued generation is still current. Favorite changes never enter that queue. A favorite that arrives while one is already in flight restores that in-flight generation, reports that the change is still settling, and is neither cancelled nor repeated.

Each discardable read takes one generation from a checked counter. Back, a workspace change, a finding-query edit and disconnect advance that generation and clear pending discardable reads. Favorite state is left alone. A late success or a late failure whose generation does not match is discarded completely: it does not replace rows, status, link, playback or findings. Finding and finding-original results also require the query captured at submission. Reload adoption restores the playback projection around the desk update. Direct test callers that still apply a desk synchronously do not use this generation.

The read and favorite tasks share one async catalog lock for that explorer, held through the catalog call and the send. The lock is not process-wide. A task waiting on it does not occupy the second worker, so player work can proceed while one worker is inside SQLite. Local `Library::open` stays inside the spawned task. The terminal loop does not call it, and the work is not moved to `spawn_blocking`.

On quit, the terminal is restored and the loop returns before the runtime is dropped. Dropping the runtime waits for the in-flight tasks. There is no shutdown timeout that abandons a favorite write. A service exchange already inside the existing request helper remains subject to the five-second control timeout. A direct local catalog open has no new deadline. Quitting, a discarded reply and a closed channel are not proof that the catalog operation stopped.

Pending, stale and error states keep the last valid snapshot. Player lanes stay independent. Capture, budgets and source registration are not mutated by these reads.

## Research and verification

[Tokio's channel contract](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) supplies finite capacity and backpressure. Its [task ownership contract](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html) distinguishes dropping a handle from task completion. Rolling documentation can describe a newer 1.53 patch; this repository stays on 1.53.1 and selects no upgrade.

Focused fixtures withhold a read while production dispatch draws the pending intent and returns, then adopt the outcome on a later poll. They cover a key burst that keeps one task and the latest queued generation, slot retention until the task finishes, a closed receiver reported once, stale success and stale failure after Back or a new view, finding, monitor list, monitor detail and random exploration, linked failure, a busy favorite, reload that preserves playback, and a service search whose budget and capture snapshots stay unchanged. A withheld metadata task stays gated through 100 production renders, Back, repeated stop and quit while a real stop completes over local IPC. Those checks use `TestBackend`. They do not measure native key-to-frame time.

The 100 ms native key-to-frame target remains unmeasured. Synthetic renders are not native terminal timing. Blocked filesystem completion is not given a new hard deadline. After the interface is gone, process exit can still wait for an in-flight local catalog read, or for the remainder of a service exchange inside the existing five-second control timeout. Omarchy and remaining TUI acoustic checks stay deferred. Retained terminal playback, interactive DVR controls, completion-settlement recovery, aggregate admission and broad platform or language qualification remain separate increments. No roadmap stage exits.
