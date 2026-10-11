# 0091: Responsive player observation

Date: 2026-10-11. Status: implemented bounded EX-02C increment; [active work](../development/progress.md#2026-10-11-responsive-player-observation) records final gates. Catalog v51 and local IPC v52 are unchanged.

## Problem

Quick player status and stop waited on the terminal thread. A delayed status exchange could prevent input, resize, stop intent and quit from progressing. Endpoint loading precedes the five-second IPC exchange timeout, so that timeout never established a whole-call filesystem bound. Metadata effects retain a separate synchronous path.

## Decision

Reuse the existing Tokio runtime and control operations. One status lane and one independent stop lane each own at most one task and a two-message channel. Poll without waiting on the terminal thread. A response does not free task capacity until the task is actually finished and its messages are consumed. Repeated status ticks coalesce. Stop closes owned client stdin immediately, then sends at most one stop at a time for the last observed exact listen ID. A repeated key observes the existing request rather than resending it. After an uncertain stop, only a fresh same-ID receipt confirming that the listen is still running enables another explicit stop key; there is no automatic retry. If identity has not been observed, close stdin and report unresolved completion without guessing a target from a later hint.

Each result carries the local generation and exact raw listen identity. Starting a replacement and requesting stop fence older observations. Observing a stop result also fences status reads begun before that result. A late response for another session cannot change the current playback projection. Receipt identity is checked before converting it to bounded display text. Uncertainty comes before error details so truncation cannot hide it. After uncertain delivery, inspect the same listen identity rather than creating a new request.

Read the mutable session hint off the terminal thread and limit its input to 4,096 bytes before JSON parsing. The hint locates an existing service receipt; it grants no new contact, registration, capture, provider call or budget. Missing/unreadable hints remain explicit. Service state, client exit and native/acoustic completion retain their separate meanings.

Keep diagnostic thread ownership through child exit. A replacement waits for the previous diagnostic drain instead of accumulating readers. Reap exited clients without waiting in the terminal loop. On quit, close stdin and hand remaining child/diagnostic joins to one reaper; the parent-pid fence remains effective. Quitting does not prove native closure or stop service-owned recordings.

Draw adopted asynchronous results on each terminal-loop pass, independently of the one-second status-request cadence. Search, linked context and finding metadata still use their existing synchronous path and need a separate bounded increment.

## Research and verification

[Tokio's channel contract](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) supplies finite capacity and backpressure. Its [task ownership contract](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html) distinguishes dropping a handle from task completion. Rolling documentation describes 1.53.2; the lock remains 1.53.1 and no dependency upgrade is selected. The repository test verifies the actual pinned implementation's slot retention, pressure and observer-drop behavior.

Regression fixtures cover stale generation/session results, pre-stop reads, repeated keys, two-message pressure, missing/oversized/disconnected hints, missing/conflicting receipts, failed output, actual child exit and diagnostic drain. A gated status task remains withheld through 100 production renders and local key handling; a real IPC stop of a nonexistent exact identity completes independently, preserving capture and budget snapshots. That integration proves independent dispatch and honest refusal, not live capture continuity or audible delivery.

The 100 ms native key-to-frame target remains unmeasured. Synthetic renders are not native terminal timing. Blocked filesystem completion and runtime shutdown are not given a new hard deadline. Omarchy and remaining TUI acoustic checks stay deferred by the user. Retained playback, interactive DVR controls, completion-settlement recovery, aggregate admission and broad platform/language qualification remain separate increments. No roadmap stage exits.
