# 0016: List explorer on the Termina backend

Date: 2026-09-21. Status: implemented for the list. The globe, map, and stage 4 stay open. The client still opens no second catalog.

## Decision

`sigy tui` renders the list explorer with Ratatui 0.30.2 and one backend, Termina 0.3.3, through ratatui-termina 0.1.0. The Ratatui dependency disables default features and enables `std`, `termina`, and `layout-cache`. It does not enable Crossterm. Termina is pinned to 0.3.3 because 0.4.0 is outside the range measured in [0015](0015-terminal-stack.md).

The explorer is a client of the existing operations. When the service holds the library lock, requests go through local IPC. Otherwise each request opens the catalog and closes it before the next one. The process does not keep a second connection beside the service.

## What the screen shows

The chrome shows the service connection or the local catalog, the partial station cache, observation age, the latest refresh outcome, search, favorite marks, the selected source, directory health, directory languages, recordings, quota, and the playback line. Directory health is labeled directory health. It is not local playback and not detected speech. A fresh client has no listen receipt, so playback reads `none` until a receipt is loaded. A running recording stays labeled as a recording.

Workspaces are Explore, Live, Recordings, Monitors, Findings, and System. Monitors and Findings say they are unavailable. The globe and map say they are unavailable. Reduced motion, linear order, and monochrome are available before any animation. This operation draws no animation frames.

## Actions

Selection, Enter, and pasted text do not start audio, capture, refresh, or a click. Search maps to `radio search`. The favorites filter maps to `radio search --favorites`. Saving or clearing a favorite maps to `radio favorite` or `radio unfavorite`. Reload maps to `service` status, `radio status`, `radio search`, `dvr status`, and `record list`. A stale search or favorite response cannot replace the current page. Quit, including `q` outside the search field, detaches this client. It does not send `service stop` and does not stop a recording. `listen file`, `listen source`, and `record start` remain separate CLI commands.

`NO_COLOR` selects monochrome. `SIGY_REDUCED_MOTION=1` and `SIGY_LINEAR=1` select those modes, as do the matching flags. `--inspect` draws one frame, writes a size report, and exits.

## Evidence

State tests cover focus, ordinary search editing, paste that contains `q`, selection that starts nothing, a stale search, a stale favorite, disconnect that keeps the last snapshot and blocks a favorite, and quit while a recording is `running`. Layout tests at 80x24 keep search, selected source, directory health, directory languages, playback, and help, and they keep the partial cache, refresh outcome, observation age, favorites, recording, and quota. A 10x4 terminal renders a recovery prompt. Linear order is top to bottom. Monochrome uses no color. Color is used only when monochrome and linear order are off. Unavailable workspaces are visible.

The Windows inspection on 2026-09-21 ran `sigy tui --inspect` inside Windows Terminal 1.24.11911.0. The request was 80x24. The frame was 80x24. `WT_SESSION` was set. The console font face was Consolas. The backend was Termina. Reduced motion, linear order, and monochrome were on. The empty local catalog did not start audio. The report is [2026-09-21-explorer.json](../../research/experiments/terminal/results/2026-09-21-explorer.json).

Ratatui, ratatui-core, ratatui-termina, and ratatui-widgets are MIT. Termina is MIT OR MPL-2.0. kasuari, pulled in by layout caching, is MIT OR Apache-2.0. No Crossterm crate is in the `sigy` dependency tree.

## Limits

This is not a screen-reader test, not a globe, and not a support matrix. Idle redraw stays at zero because the client draws on input and on responses, not on a timer. Observation age therefore advances on the next input or response. Key-to-frame latency was not measured again. Hard kill still cannot restore terminal modes. Catalog schema stays v11 and local IPC stays v12. Stage 4 stays open.
